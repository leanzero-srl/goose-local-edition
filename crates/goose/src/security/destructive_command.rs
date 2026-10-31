//! Destructive-command gate (real-use finding F17, 2026-10-31).
//!
//! In real use a local model ran `git reset --hard 1192c90` that nobody asked for and wiped four
//! of the user's commits plus an uncommitted edit; goose ran it at once because the session was in
//! the default `auto` mode, where every tool call is approved. This inspector asks the user,
//! through the existing tool-approval card, before a shell command that destroys work which goose's
//! own tool output cannot give back: uncommitted or staged changes, untracked files, branches,
//! stashes, remote history, or a directory tree that is not disposable.
//!
//! It runs in every mode that runs tools (`auto`, `approve`, `smart_approve`); the only way to turn
//! it off is the explicit config key [`DESTRUCTIVE_CONFIRM_KEY`] set to `false`.
//!
//! Disposable trees, which `rm -r` may delete without asking: a path inside the system temp dir,
//! and a DIRECTORY inside the session's working dir that BOTH the repository declares ignored
//! (`git check-ignore`) AND sits in a known regenerable build/cache output
//! ([`REGENERABLE_OUTPUT_DIRS`]). Ignored alone is not disposable: an ignored `data/` or local DB
//! is exactly what git cannot give back.
//!
//! What the parser sees: the command line as the shell would split it, quotes and escapes
//! honoured, chains (`&&`, `||`, `;`, `|`, `&`, newlines, subshells), `$( )` and backtick
//! substitutions, here-document bodies (skipped as data; an unquoted delimiter's substitutions are
//! still scanned), `sh -c` / `bash -o pipefail -c` / `eval` scripts, `cd` along the chain, and the
//! usual wrappers (`sudo`, `env`, `nice`, `timeout`, `xargs`, `command`, `exec`, `nohup`, `time`,
//! variable assignments). What it cannot see, and does not claim to: shell aliases and functions,
//! scripts and Makefiles the command runs (`./cleanup.sh`, `npm run clean`), git aliases, commands
//! assembled at run time from variables, and `find -delete`. History rewrites (`rebase`,
//! `commit --amend`) are local and kept in the reflog; they only destroy shared history when
//! force-pushed, and the force push is what this gate stops. `git reset --keep` is not gated: git
//! aborts it when a file it would touch has local changes.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::config::{Config, ConfigError, GooseMode};
use crate::conversation::message::{Message, ToolRequest};
use crate::session::SessionManager;
use crate::tool_inspection::{InspectionAction, InspectionResult, ToolInspector};

/// Config key (or env var of the same name). `false` = never ask before destructive commands.
pub const DESTRUCTIVE_CONFIRM_KEY: &str = "GOOSE_DESTRUCTIVE_CONFIRM";

/// `InspectionResult::inspector_name` of this inspector; the approval handler keys on it.
pub const DESTRUCTIVE_INSPECTOR_NAME: &str = "destructive";

/// One destructive command found in a command line.
#[derive(Debug, Clone, PartialEq)]
pub struct DestructiveFinding {
    /// The simple command as parsed (words joined by spaces).
    pub command: String,
    /// What it destroys, in words a user can act on.
    pub reason: String,
    /// For a recursive `rm` whose every target resolved to an absolute path: those paths, so the
    /// repository's ignore rules can clear it. `None` when any target is unresolvable (a variable,
    /// a glob, `~`, unknown cwd) or the finding is not an `rm`.
    pub rm_targets: Option<Vec<PathBuf>>,
}

/// Every destructive simple command in `line`, in order of appearance, with no known cwd.
pub fn find_destructive(line: &str) -> Vec<DestructiveFinding> {
    find_destructive_in(line, None)
}

/// Every destructive simple command in `line`, starting in `cwd` (the session's working dir).
pub fn find_destructive_in(line: &str, cwd: Option<&Path>) -> Vec<DestructiveFinding> {
    let mut findings = Vec::new();
    let mut cwd = cwd.map(Path::to_path_buf);
    scan_line(line, &mut cwd, &mut findings);
    findings
}

fn scan_line(line: &str, cwd: &mut Option<PathBuf>, findings: &mut Vec<DestructiveFinding>) {
    let mut nested = Vec::new();
    let segments = split_segments(line, &mut nested);
    for words in segments {
        scan_segment(&words, cwd, findings);
    }
    // Substitutions run too: `echo "$(git reset --hard)"` resets.
    for script in nested {
        scan_line(&script, &mut cwd.clone(), findings);
    }
}

// ---------------------------------------------------------------------------------------------
// Lexer: shell words grouped into simple commands.
// ---------------------------------------------------------------------------------------------

struct PendingHeredoc {
    delimiter: String,
    quoted: bool,
    strip_tabs: bool,
}

/// Split a command line into simple commands (lists of words). Separators: `;` `&` `|` (so also
/// `&&` `||`), newline, `(` `)`. Quotes and backslashes are honoured; `$( )` and backtick bodies
/// are pushed to `nested` and stand in the word as their literal text. Redirection targets are
/// dropped (`2>/dev/null` is not an argument); here-document bodies are data, not commands.
fn split_segments(line: &str, nested: &mut Vec<String>) -> Vec<Vec<String>> {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut segments: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut skip_next_word = false;
    let mut heredocs: Vec<PendingHeredoc> = Vec::new();
    let mut i = 0;

    fn flush_word(
        cur: &mut String,
        in_word: &mut bool,
        words: &mut Vec<String>,
        skip_next_word: &mut bool,
    ) {
        if *in_word {
            if *skip_next_word {
                *skip_next_word = false;
            } else {
                words.push(std::mem::take(cur));
            }
            cur.clear();
            *in_word = false;
        }
    }

    fn flush_segment(words: &mut Vec<String>, segments: &mut Vec<Vec<String>>) {
        if !words.is_empty() {
            segments.push(std::mem::take(words));
        }
    }

    while i < len {
        let c = chars[i];
        match c {
            '\'' => {
                in_word = true;
                i += 1;
                while i < len && chars[i] != '\'' {
                    cur.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                in_word = true;
                i += 1;
                while i < len && chars[i] != '"' {
                    let d = chars[i];
                    if d == '\\' && i + 1 < len && matches!(chars[i + 1], '"' | '\\' | '$' | '`') {
                        cur.push(chars[i + 1]);
                        i += 2;
                    } else if d == '$' && i + 1 < len && chars[i + 1] == '(' {
                        let (body, end) = take_paren_body(&chars, i + 2);
                        cur.extend(&chars[i..end]);
                        nested.push(body);
                        i = end;
                    } else if d == '`' {
                        let (body, end) = take_backtick_body(&chars, i + 1);
                        cur.extend(&chars[i..end]);
                        nested.push(body);
                        i = end;
                    } else {
                        cur.push(d);
                        i += 1;
                    }
                }
                i += 1;
            }
            '\\' => {
                in_word = true;
                if i + 1 < len && chars[i + 1] != '\n' {
                    cur.push(chars[i + 1]);
                }
                i += 2;
            }
            '$' if i + 1 < len && chars[i + 1] == '(' => {
                in_word = true;
                let (body, end) = take_paren_body(&chars, i + 2);
                cur.extend(&chars[i..end]);
                nested.push(body);
                i = end;
            }
            '`' => {
                in_word = true;
                let (body, end) = take_backtick_body(&chars, i + 1);
                cur.extend(&chars[i..end]);
                nested.push(body);
                i = end;
            }
            '#' if !in_word => {
                while i < len && chars[i] != '\n' {
                    i += 1;
                }
            }
            ' ' | '\t' => {
                flush_word(&mut cur, &mut in_word, &mut words, &mut skip_next_word);
                i += 1;
            }
            '\n' => {
                flush_word(&mut cur, &mut in_word, &mut words, &mut skip_next_word);
                flush_segment(&mut words, &mut segments);
                i += 1;
                for doc in heredocs.drain(..) {
                    i = skip_heredoc_body(&chars, i, &doc, nested);
                }
            }
            ';' | '&' | '|' | '(' | ')' => {
                flush_word(&mut cur, &mut in_word, &mut words, &mut skip_next_word);
                flush_segment(&mut words, &mut segments);
                i += 1;
            }
            '<' if i + 2 < len && chars[i + 1] == '<' && chars[i + 2] != '<' => {
                flush_word(&mut cur, &mut in_word, &mut words, &mut skip_next_word);
                i += 2;
                let strip_tabs = chars[i] == '-';
                if strip_tabs {
                    i += 1;
                }
                while i < len && matches!(chars[i], ' ' | '\t') {
                    i += 1;
                }
                let (delimiter, quoted, end) = read_heredoc_delimiter(&chars, i);
                i = end;
                if !delimiter.is_empty() {
                    heredocs.push(PendingHeredoc {
                        delimiter,
                        quoted,
                        strip_tabs,
                    });
                }
            }
            '<' | '>' => {
                // A file-descriptor number glued to the operator (`2>`) is not an argument.
                if in_word && !cur.is_empty() && cur.chars().all(|ch| ch.is_ascii_digit()) {
                    cur.clear();
                    in_word = false;
                } else {
                    flush_word(&mut cur, &mut in_word, &mut words, &mut skip_next_word);
                }
                let mut last = c;
                while i < len && matches!(chars[i], '<' | '>' | '&' | '|') {
                    last = chars[i];
                    i += 1;
                }
                if last == '&' && i < len && (chars[i].is_ascii_digit() || chars[i] == '-') {
                    // `2>&1`, `>&-`: a descriptor, not a file word.
                    while i < len && (chars[i].is_ascii_digit() || chars[i] == '-') {
                        i += 1;
                    }
                } else {
                    skip_next_word = true;
                }
            }
            _ => {
                in_word = true;
                cur.push(c);
                i += 1;
            }
        }
    }
    flush_word(&mut cur, &mut in_word, &mut words, &mut skip_next_word);
    flush_segment(&mut words, &mut segments);
    segments
}

/// The here-document delimiter word at `start`: (delimiter, quoted, index after it). Any quote or
/// backslash in the word makes the body literal, as in the shell.
fn read_heredoc_delimiter(chars: &[char], start: usize) -> (String, bool, usize) {
    let mut i = start;
    let mut word = String::new();
    let mut quoted = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' | '"' => {
                quoted = true;
                i += 1;
                while i < chars.len() && chars[i] != c {
                    word.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '\\' => {
                quoted = true;
                if i + 1 < chars.len() {
                    word.push(chars[i + 1]);
                }
                i += 2;
            }
            ' ' | '\t' | '\n' | ';' | '&' | '|' | '(' | ')' | '<' | '>' => break,
            _ => {
                word.push(c);
                i += 1;
            }
        }
    }
    (word, quoted, i.min(chars.len()))
}

/// Skip a here-document body starting at line start `start`; returns the index after the
/// delimiter line. An unquoted body still expands `$( )` and backticks, so those are scanned.
fn skip_heredoc_body(
    chars: &[char],
    start: usize,
    doc: &PendingHeredoc,
    nested: &mut Vec<String>,
) -> usize {
    let mut i = start;
    while i < chars.len() {
        let line_end = chars[i..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(chars.len(), |p| i + p);
        let line: String = chars[i..line_end].iter().collect();
        let candidate = if doc.strip_tabs {
            line.trim_start_matches('\t')
        } else {
            line.as_str()
        };
        let next = (line_end + 1).min(chars.len());
        if candidate == doc.delimiter {
            return next;
        }
        if !doc.quoted {
            let line_chars: Vec<char> = line.chars().collect();
            let mut j = 0;
            while j < line_chars.len() {
                if line_chars[j] == '$' && line_chars.get(j + 1) == Some(&'(') {
                    let (body, end) = take_paren_body(&line_chars, j + 2);
                    nested.push(body);
                    j = end;
                } else if line_chars[j] == '`' {
                    let (body, end) = take_backtick_body(&line_chars, j + 1);
                    nested.push(body);
                    j = end;
                } else {
                    j += 1;
                }
            }
        }
        i = next;
        if line_end == chars.len() {
            break;
        }
    }
    chars.len()
}

/// Body of `$( ... )` starting just after the `(`; returns (body, index after the closing `)`).
fn take_paren_body(chars: &[char], start: usize) -> (String, usize) {
    let mut depth = 1usize;
    let mut i = start;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) => {
                if c == '\\' && q == '"' {
                    i += 1;
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                '\\' => i += 1,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return (chars[start..i].iter().collect(), i + 1);
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    (
        chars[start.min(chars.len())..].iter().collect(),
        chars.len(),
    )
}

/// Body of a backtick substitution starting just after the opening backtick.
fn take_backtick_body(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == '`' {
            return (chars[start..i].iter().collect(), i + 1);
        }
        i += 1;
    }
    (
        chars[start.min(chars.len())..].iter().collect(),
        chars.len(),
    )
}

// ---------------------------------------------------------------------------------------------
// Classifier: one simple command.
// ---------------------------------------------------------------------------------------------

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn is_assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit())
        }
        None => false,
    }
}

/// Drop reserved words, assignments and wrapper commands from the front of a simple command, so
/// `sudo env FOO=1 nice -n 5 git reset --hard` is classified as `git reset --hard`.
fn strip_prefix(words: &[String]) -> &[String] {
    let mut w = words;
    loop {
        let Some(first) = w.first() else { return w };
        let name = basename(first);
        let skip = match name {
            "!" | "{" | "}" | "if" | "then" | "else" | "elif" | "do" | "while" | "until"
            | "time" | "exec" | "command" | "builtin" | "nohup" => 1,
            _ if is_assignment(first) => 1,
            "sudo" | "doas" => {
                1 + options_len(
                    &w[1..],
                    &["-u", "-g", "-h", "-p", "-C", "-D", "-U", "-r", "-t"],
                )
            }
            "env" => {
                let mut n = 1;
                while n < w.len() {
                    let a = w[n].as_str();
                    if a == "-u" || a == "-C" || a == "-S" {
                        n += 2;
                    } else if a.starts_with('-') || is_assignment(a) {
                        n += 1;
                    } else {
                        break;
                    }
                }
                n
            }
            "nice" => 1 + options_len(&w[1..], &["-n"]),
            "timeout" => {
                let n = 1 + options_len(&w[1..], &["-s", "-k", "--signal", "--kill-after"]);
                // the duration
                if n < w.len() {
                    n + 1
                } else {
                    n
                }
            }
            "xargs" => {
                1 + options_len(
                    &w[1..],
                    &[
                        "-I", "-n", "-P", "-L", "-s", "-E", "-d", "-a", "-J", "-R", "-S",
                    ],
                )
            }
            _ => 0,
        };
        if skip == 0 {
            return w;
        }
        w = &w[skip.min(w.len())..];
    }
}

/// Number of leading option words (and their values, for options in `with_value`).
fn options_len(words: &[String], with_value: &[&str]) -> usize {
    let mut n = 0;
    while n < words.len() {
        let a = words[n].as_str();
        if a == "--" {
            return n + 1;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        n += if with_value.contains(&a) { 2 } else { 1 };
    }
    n.min(words.len())
}

/// Option words of `args` (up to `--`), positional words, and whether `--` was present.
struct Args<'a> {
    flags: Vec<&'a str>,
    positional: Vec<&'a str>,
    after_double_dash: Vec<&'a str>,
    double_dash: bool,
}

fn split_args(args: &[String]) -> Args<'_> {
    let mut out = Args {
        flags: vec![],
        positional: vec![],
        after_double_dash: vec![],
        double_dash: false,
    };
    for a in args {
        let a = a.as_str();
        if out.double_dash {
            out.after_double_dash.push(a);
        } else if a == "--" {
            out.double_dash = true;
        } else if a.starts_with('-') && a != "-" {
            out.flags.push(a);
        } else {
            out.positional.push(a);
        }
    }
    out
}

impl Args<'_> {
    fn long(&self, name: &str) -> bool {
        self.flags
            .iter()
            .any(|f| *f == name || f.strip_prefix(name).is_some_and(|r| r.starts_with('=')))
    }
    /// A short flag, alone or in a cluster (`-fd` has `f` and `d`).
    fn short(&self, c: char) -> bool {
        self.flags.iter().any(|f| {
            !f.starts_with("--")
                && f.strip_prefix('-')
                    .is_some_and(|cluster| cluster.contains(c))
        })
    }
    fn exact(&self, flag: &str) -> usize {
        self.flags.iter().filter(|f| **f == flag).count()
    }
}

fn finding(words: &[String], reason: &str) -> DestructiveFinding {
    DestructiveFinding {
        command: words.join(" "),
        reason: reason.to_string(),
        rm_targets: None,
    }
}

/// `cd` along a chain: absolute, or relative to a known cwd; anything else makes the cwd unknown.
fn apply_cd(rest: &[String], cwd: &mut Option<PathBuf>) {
    let target = rest
        .iter()
        .find(|a| !matches!(a.as_str(), "-L" | "-P" | "-e" | "-@"));
    *cwd = match target {
        Some(t) => resolve(t, cwd.as_deref()),
        None => None,
    };
}

/// An absolute, lexically normalised path for a shell word, or `None` when the word needs the
/// shell to expand it (variables, globs, `~`) or is relative to an unknown cwd.
fn resolve(word: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    if word.is_empty()
        || word == "-"
        || word.starts_with('~')
        || word.contains(['$', '*', '?', '[', '{', '`'])
    {
        return None;
    }
    let joined = if word.starts_with('/') {
        PathBuf::from(word)
    } else {
        cwd?.join(word)
    };
    let mut out = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

fn scan_segment(
    words: &[String],
    cwd: &mut Option<PathBuf>,
    findings: &mut Vec<DestructiveFinding>,
) {
    let words = strip_prefix(words);
    let Some(first) = words.first() else { return };
    let name = basename(first);
    let rest = &words[1..];
    match name {
        "cd" | "pushd" => apply_cd(rest, cwd),
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "fish" => {
            // `bash -c '<script>'`, also `-lc`, `-ec`, `-o pipefail -c`: the script is the word
            // after the cluster that holds `c`.
            let mut idx = 0;
            while idx < rest.len() {
                let a = rest[idx].as_str();
                if matches!(a, "-o" | "+o" | "-O" | "+O") {
                    idx += 2;
                    continue;
                }
                if !a.starts_with("--")
                    && a.strip_prefix('-')
                        .is_some_and(|cluster| cluster.contains('c'))
                {
                    if let Some(script) = rest.get(idx + 1) {
                        scan_line(script, &mut cwd.clone(), findings);
                    }
                    return;
                }
                if !a.starts_with('-') && !a.starts_with('+') {
                    return; // `bash script.sh`: the script's contents are not visible here
                }
                idx += 1;
            }
        }
        "eval" => scan_line(&rest.join(" "), &mut cwd.clone(), findings),
        "git" => {
            if let Some(f) = classify_git(words, rest) {
                findings.push(f);
            }
        }
        "rm" => {
            if let Some(f) = classify_rm(words, rest, cwd.as_deref()) {
                findings.push(f);
            }
        }
        _ => {}
    }
}

fn classify_git(words: &[String], rest: &[String]) -> Option<DestructiveFinding> {
    // Global options before the subcommand.
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].as_str();
        if !a.starts_with('-') {
            break;
        }
        i += if matches!(a, "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace") {
            2
        } else {
            1
        };
    }
    let sub = rest.get(i)?.as_str();
    let args = split_args(&rest[i + 1..]);
    let reason = match sub {
        "reset" if args.long("--hard") => {
            "discards every uncommitted change in the working tree and moves the branch; commits after the target and unsaved edits are gone"
        }
        "reset" if args.long("--merge") => {
            "moves the branch and overwrites files changed since the target; staged changes to them are gone"
        }
        "checkout" if checkout_overwrites_paths(&args) => {
            "overwrites files with another version; uncommitted edits to them are gone"
        }
        "restore"
            if !(args.short('S') || args.long("--staged"))
                || args.short('W')
                || args.long("--worktree") =>
        {
            "overwrites files in the working tree; uncommitted edits to them are gone"
        }
        "clean"
            if (args.short('f') || args.long("--force"))
                && !(args.short('n') || args.long("--dry-run")) =>
        {
            "deletes untracked files, which git has no copy of"
        }
        "push"
            if args.short('f')
                || args.long("--force")
                || args.long("--force-with-lease")
                || args.long("--force-if-includes")
                || args.long("--mirror")
                || args.long("--delete")
                || args.short('d')
                || args.long("--prune")
                || args
                    .positional
                    .iter()
                    .skip(1)
                    .any(|r| r.starts_with('+') || r.starts_with(':')) =>
        {
            "rewrites or deletes history on the remote; commits others pushed can be lost"
        }
        "branch"
            if args.short('D')
                || args.short('M')
                || args.short('C')
                || args.short('f')
                || args.long("--force") =>
        {
            "force-deletes or overwrites a branch; commits only on that branch are lost"
        }
        "switch"
            if args.short('f')
                || args.long("--force")
                || args.long("--discard-changes")
                || args.short('C')
                || args.long("--force-create") =>
        {
            "throws away uncommitted changes or overwrites a branch"
        }
        "stash" if matches!(args.positional.first(), Some(&"drop") | Some(&"clear")) => {
            "deletes stashed changes, which exist nowhere else"
        }
        _ => return None,
    };
    Some(finding(words, reason))
}

/// `git checkout` that writes files rather than switching branches: forced, `-- <paths>`, `.`,
/// or `<tree-ish> <paths>` (two or more positionals once the value of `-b`/`-B`/`--orphan` is
/// set aside, since `checkout -b new start-point` also has two).
fn checkout_overwrites_paths(args: &Args<'_>) -> bool {
    let branch_values = args.exact("-b") + args.exact("-B") + args.exact("--orphan");
    args.short('f')
        || args.long("--force")
        || (args.double_dash && !args.after_double_dash.is_empty())
        || args
            .positional
            .iter()
            .any(|p| *p == "." || p.starts_with("./") || p.starts_with(":/"))
        || args.positional.len().saturating_sub(branch_values) >= 2
}

fn classify_rm(
    words: &[String],
    rest: &[String],
    cwd: Option<&Path>,
) -> Option<DestructiveFinding> {
    let args = split_args(rest);
    let recursive = args.short('r') || args.short('R') || args.long("--recursive");
    if !recursive {
        return None;
    }
    let targets: Vec<&str> = args
        .positional
        .iter()
        .chain(args.after_double_dash.iter())
        .copied()
        .collect();
    // No visible target (`xargs rm -rf`) is not proof of a temp target.
    if !targets.is_empty() && targets.iter().all(|t| is_temp_path(t)) {
        return None;
    }
    let resolved: Option<Vec<PathBuf>> = if targets.is_empty() {
        None
    } else {
        targets.iter().map(|t| resolve(t, cwd)).collect()
    };
    Some(DestructiveFinding {
        command: words.join(" "),
        reason: "deletes a directory tree that is not disposable, with no copy to restore from"
            .to_string(),
        rm_targets: resolved,
    })
}

/// Inside (not equal to) a system temp directory, with no `..` escape.
fn is_temp_path(path: &str) -> bool {
    if path.split('/').any(|seg| seg == "..") {
        return false;
    }
    let mut roots: Vec<String> = [
        "/tmp/",
        "/private/tmp/",
        "/var/folders/",
        "/private/var/folders/",
        "$TMPDIR/",
        "${TMPDIR}/",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let sys = std::env::temp_dir();
    let sys = sys.to_string_lossy();
    if sys.len() > 1 {
        roots.push(format!("{}/", sys.trim_end_matches('/')));
    }
    roots
        .iter()
        .any(|root| path.len() > root.len() && path.starts_with(root.as_str()))
}

/// Named policy (GATE 10 policy constant, receipt: F17 real-use incident 2026-10-31 and the
/// independent review of 281e3abe92, which showed a git-ignored `data/` deleted with no card):
/// the build and cache outputs that tools regenerate from tracked sources. A directory is
/// disposable only when it is git-ignored AND one of its path components (relative to the
/// working dir) is one of these; every other ignored directory still asks.
pub const REGENERABLE_OUTPUT_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".turbo",
    ".gradle",
    "coverage",
];

fn is_regenerable_output(relative: &Path) -> bool {
    relative.components().any(|c| {
        matches!(c, Component::Normal(name)
            if REGENERABLE_OUTPUT_DIRS.iter().any(|d| name == std::ffi::OsStr::new(d)))
    })
}

/// A directory strictly inside `root` that is a regenerable output AND that the repository at
/// `root` ignores. Asks git for the second half; any failure to ask counts as NOT disposable.
async fn is_ignored_dir_in(root: &Path, path: &Path) -> bool {
    if path == root || !path.starts_with(root) || !path.is_dir() {
        return false;
    }
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    if !is_regenerable_output(relative) {
        return false;
    }
    match tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "-q", "--"])
        .arg(relative)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
    {
        Ok(status) => status.success(),
        Err(e) => {
            tracing::warn!(error = %e, "git check-ignore could not run; rm -r stays gated");
            false
        }
    }
}

/// Drop the `rm -r` findings whose every target is an ignored directory inside `root`.
pub async fn drop_disposable(
    findings: Vec<DestructiveFinding>,
    root: Option<&Path>,
) -> Vec<DestructiveFinding> {
    let Some(root) = root else {
        return findings;
    };
    let mut kept = Vec::new();
    for f in findings {
        let disposable = match &f.rm_targets {
            Some(paths) => {
                let mut all = true;
                for p in paths {
                    if !is_ignored_dir_in(root, p).await {
                        all = false;
                        break;
                    }
                }
                all
            }
            None => false,
        };
        if !disposable {
            kept.push(f);
        }
    }
    kept
}

// ---------------------------------------------------------------------------------------------
// Messages and the inspector.
// ---------------------------------------------------------------------------------------------

/// Text on the approval card.
pub fn approval_prompt(findings: &[DestructiveFinding]) -> String {
    let mut s =
        String::from("This command can destroy work that goose cannot give back afterwards.\n");
    for f in findings {
        s.push_str(&format!("\n`{}`: {}.", f.command, f.reason));
    }
    s.push_str("\n\nAllow it only if you asked for this.");
    s
}

/// What the gate found, as carried in `InspectionResult::reason` and quoted back to the model.
pub fn findings_summary(findings: &[DestructiveFinding]) -> String {
    findings
        .iter()
        .map(|f| format!("`{}` ({})", f.command, f.reason))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Tool result the model gets when the user declines (replaces the generic decline text, which
/// tells the model never to call the tool again and so would cut it off from the shell).
pub fn declined_response(summary: &str) -> String {
    format!(
        "The user declined this command because it would destroy work: {summary}. Nothing in it was run. \
         Do not run it again and do not reach the same result another way (another reset, checkout, \
         restore, clean, rm or force push). Leave the repository and files as they are, tell the user \
         what you wanted to do and why, and ask how they want to proceed. Other shell commands are \
         still allowed."
    )
}

fn shell_command_text(tool_call: &rmcp::model::CallToolRequestParams) -> Option<&str> {
    let name = tool_call.name.as_ref();
    let is_shell = matches!(
        name,
        "shell" | "bash" | "execute_command" | "run_command" | "terminal"
    ) || name.ends_with("__shell")
        || name.ends_with("__bash")
        || name.ends_with("__terminal");
    if !is_shell {
        return None;
    }
    let args = tool_call.arguments.as_ref()?;
    ["command", "cmd", "script"]
        .iter()
        .find_map(|k| args.get(*k).and_then(|v| v.as_str()))
}

/// Findings for one tool request run from `cwd` (empty for non-shell tools and harmless
/// commands), before the disposable-directory check.
pub fn findings_for_request(request: &ToolRequest, cwd: Option<&Path>) -> Vec<DestructiveFinding> {
    let Ok(tool_call) = &request.tool_call else {
        return Vec::new();
    };
    match shell_command_text(tool_call) {
        Some(command) => find_destructive_in(command, cwd),
        None => Vec::new(),
    }
}

pub struct DestructiveCommandInspector {
    session_manager: Arc<SessionManager>,
}

impl DestructiveCommandInspector {
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        Self { session_manager }
    }

    /// The session's working dir; unknown means relative `rm -r` targets cannot be cleared.
    async fn working_dir(&self, session_id: &str) -> Option<PathBuf> {
        match self.session_manager.get_session(session_id, false).await {
            Ok(session) if session.working_dir.is_absolute() => Some(session.working_dir),
            Ok(_) => None,
            Err(e) => {
                tracing::debug!(error = %e, "no session working dir; rm -r targets stay gated");
                None
            }
        }
    }
}

#[async_trait]
impl ToolInspector for DestructiveCommandInspector {
    fn name(&self) -> &'static str {
        DESTRUCTIVE_INSPECTOR_NAME
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn inspect(
        &self,
        session_id: &str,
        tool_requests: &[ToolRequest],
        _messages: &[Message],
        goose_mode: GooseMode,
    ) -> Result<Vec<InspectionResult>> {
        if goose_mode == GooseMode::Chat {
            return Ok(Vec::new());
        }
        let mut results = Vec::new();
        let mut root: Option<Option<PathBuf>> = None;
        for request in tool_requests {
            if !matches!(&request.tool_call, Ok(tc) if shell_command_text(tc).is_some()) {
                continue;
            }
            if root.is_none() {
                root = Some(self.working_dir(session_id).await);
            }
            let cwd = root.as_ref().and_then(|r| r.as_deref());
            let findings = drop_disposable(findings_for_request(request, cwd), cwd).await;
            if findings.is_empty() {
                continue;
            }
            tracing::info!(
                security.event_type = "destructive_command",
                tool.request_id = %request.id,
                commands = %findings.iter().map(|f| f.command.as_str()).collect::<Vec<_>>().join(" | "),
                "destructive shell command held for user approval"
            );
            results.push(InspectionResult {
                tool_request_id: request.id.clone(),
                action: InspectionAction::RequireApproval(Some(approval_prompt(&findings))),
                reason: findings_summary(&findings),
                confidence: 1.0,
                inspector_name: DESTRUCTIVE_INSPECTOR_NAME.to_string(),
                finding_id: None,
            });
        }
        Ok(results)
    }

    /// On unless the user explicitly set [`DESTRUCTIVE_CONFIRM_KEY`] to `false`. An absent key is
    /// the default (on); an unreadable value keeps the gate on and says so in the log.
    fn is_enabled(&self) -> bool {
        match Config::global().get_param::<bool>(DESTRUCTIVE_CONFIRM_KEY) {
            Ok(enabled) => enabled,
            Err(ConfigError::NotFound(_)) => true,
            Err(e) => {
                tracing::warn!(
                    key = DESTRUCTIVE_CONFIRM_KEY,
                    error = %e,
                    "unreadable destructive-confirm setting; keeping the gate ON"
                );
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolRequestParams;
    use rmcp::object;

    fn hit(line: &str) -> bool {
        !find_destructive(line).is_empty()
    }

    /// The exact F17 call (Gabee, session 20261031_3, message 516).
    #[test]
    fn f17_exact_reset_hard_is_caught() {
        let line = "cd '/Users/mihaiperdum/goose-builds/quality/RU-2026-10-31-1-single-qwen35-9b/work' && git reset --hard 1192c90";
        let f = find_destructive(line);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].command, "git reset --hard 1192c90");
    }

    /// The same session's harmless git calls (messages 502-513) stay unasked.
    #[test]
    fn f17_session_harmless_calls_pass() {
        for line in [
            "cd '/w' && git reset --soft 8bcacbe",
            "cd '/w' && git status",
            "cd '/w' && git add -A && git commit -m \"Add skipped-by-reason summary\n\n- After plan completes\"",
            "cd '/w' && git log --oneline -5",
            "cd '/w' && git add src/a.js src/a.test.js && git commit -m \"Skip DC comments\"",
        ] {
            assert!(!hit(line), "{line}");
        }
    }

    #[test]
    fn chained_and_wrapped_forms_are_caught() {
        for line in [
            "git reset --hard",
            "git status; git reset --hard HEAD~3",
            "git status || git reset --hard",
            "git log | head && git reset --hard origin/main",
            "(cd repo && git reset --hard)",
            "cd repo&&git reset --hard",
            "sh -c 'git reset --hard 1192c90'",
            "bash -lc \"cd /w && git reset --hard\"",
            "bash -o pipefail -c 'git reset --hard'",
            "bash +o histexpand -e -c 'git clean -fd'",
            "zsh -O extglob -c 'git reset --hard'",
            "eval \"git reset --hard\"",
            "echo \"$(git reset --hard)\"",
            "echo `git clean -fd`",
            "sudo git -C /w reset --hard",
            "GIT_DIR=.git env FOO=1 git reset --hard",
            "timeout 30 git reset --hard",
            "/usr/bin/git reset --hard",
            "git -c core.x=y reset --hard",
            "git reset --hard 2>&1",
            "git reset --hard >/dev/null 2>&1",
            "git reset --merge HEAD~1",
            "git checkout -- src/a.js",
            "git checkout .",
            "git checkout -f main",
            "git checkout HEAD src/a.js",
            "git checkout main src/a.js",
            "git checkout HEAD~2 -- src/a.js",
            "git restore src/a.js",
            "git restore --staged --worktree a.js",
            "git restore -SW a.js",
            "git clean -fd",
            "git clean -xdf",
            "git clean --force",
            "git push --force origin main",
            "git push -f",
            "git push --force-with-lease",
            "git push origin +main",
            "git push origin :old-branch",
            "git push --delete origin old",
            "git branch -D feature",
            "git branch -f main HEAD~2",
            "git switch --discard-changes main",
            "git stash drop",
            "git stash clear",
            "rm -rf src",
            "rm -rf ./",
            "rm -r -f build",
            "rm -fr ~/project",
            "rm --recursive docs",
            "rm -rf /tmp/x ~/work",
            "rm -rf /tmp/../Users/me",
            "rm -rf /tmp",
            "find . -name x | xargs rm -rf",
            "cat <<EOF > out.txt\n$(git reset --hard)\nEOF",
            "cat <<'EOF' > notes.md\nhello\nEOF\ngit reset --hard",
        ] {
            assert!(hit(line), "should be caught: {line}");
        }
    }

    #[test]
    fn harmless_look_alikes_pass() {
        for line in [
            "git reset --soft HEAD~1",
            "git reset --keep HEAD~1",
            "git reset HEAD a.js",
            "git reset",
            "git log --hard",
            "git log --grep 'reset --hard'",
            "git commit -m 'git reset --hard was a mistake'",
            "echo \"rm -rf\"",
            "echo 'git reset --hard'",
            "echo git reset --hard",
            "grep -r 'rm -rf' .",
            "# git reset --hard\ngit status",
            "git checkout main",
            "git checkout -b feature",
            "git checkout -b feature origin/feature",
            "git checkout -B feature main",
            "git checkout --orphan pages main",
            "git restore --staged a.js",
            "git clean -n -d",
            "git clean -fdn",
            "git push origin main",
            "git push -u origin feature",
            "git branch -d merged",
            "git branch -a",
            "git switch main",
            "git stash",
            "git stash pop",
            "git rebase main",
            "git commit --amend --no-edit",
            "rm a.txt",
            "rm -f a.txt",
            "rm -rf /tmp/goose-scratch",
            "rm -rf /private/tmp/build-1",
            "rm -rf \"$TMPDIR/cache\"",
            "ls -la | grep reset",
            "cat notes.md",
            "cat <<'EOF' > RUNBOOK.md\nTo undo: git reset --hard HEAD~1\nrm -rf build\nEOF",
            "cat <<EOF > RUNBOOK.md\ngit push --force\nEOF\ngit status",
            "cat <<-END\n\tgit clean -fd\n\tEND\necho done",
            "git commit -F - <<'MSG'\nrevert: git reset --hard was wrong\nMSG",
        ] {
            assert!(
                !hit(line),
                "should pass: {line} -> {:?}",
                find_destructive(line)
            );
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A project that ignores node_modules/, target/, dist/ and .env, with a tracked src/ and an
    /// untracked (not ignored) notes/ dir.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        std::fs::write(
            root.join(".gitignore"),
            "node_modules/\ntarget/\ndist\n.env\ndata/\nlocal.db/\n",
        )
        .unwrap();
        for d in [
            "node_modules/x",
            "target/debug",
            "dist",
            "src",
            "notes",
            "data/raw",
            "local.db",
        ] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("src/a.js"), "a").unwrap();
        std::fs::write(root.join("notes/todo.md"), "mine").unwrap();
        std::fs::write(root.join(".env"), "SECRET=1").unwrap();
        git(root, &["add", ".gitignore", "src/a.js"]);
        git(
            root,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "init",
            ],
        );
        dir
    }

    async fn asks_in(root: &Path, line: &str) -> bool {
        !drop_disposable(find_destructive_in(line, Some(root)), Some(root))
            .await
            .is_empty()
    }

    #[tokio::test]
    async fn rm_of_ignored_build_dirs_inside_the_project_is_not_asked() {
        let dir = project();
        let root = dir.path();
        for line in [
            "rm -rf node_modules",
            "rm -rf ./target",
            "rm -rf dist target/debug",
            "rm -rf node_modules/",
            "rm -rf node_modules/x",
        ] {
            assert!(!asks_in(root, line).await, "should pass: {line}");
        }
        let cd_line = format!("cd '{}' && rm -rf node_modules dist", root.display());
        assert!(!asks_in(root, &cd_line).await, "{cd_line}");
    }

    #[tokio::test]
    async fn rm_of_tracked_untracked_outside_or_unresolved_still_asks() {
        let dir = project();
        let root = dir.path();
        for line in [
            "rm -rf src",
            "rm -rf notes",
            "rm -rf data",
            "rm -rf data/raw",
            "rm -rf local.db",
            "rm -rf node_modules data",
            "rm -rf node_modules notes",
            "rm -rf node_modules ../elsewhere",
            "rm -rf .",
            "rm -r .env",
            "rm -rf $DIR",
            "rm -rf node_*",
            "cd .. && rm -rf node_modules",
            "cd \"$HOME\" && rm -rf node_modules",
        ] {
            assert!(asks_in(root, line).await, "should ask: {line}");
        }
        // No known working dir: nothing can be cleared.
        assert!(
            !drop_disposable(find_destructive("rm -rf node_modules"), None)
                .await
                .is_empty()
        );
        // A different project's root does not clear this one's dirs.
        let other = project();
        let findings = find_destructive_in("rm -rf node_modules", Some(root));
        assert!(!drop_disposable(findings, Some(other.path()))
            .await
            .is_empty());
    }

    #[test]
    fn decline_text_names_the_command_and_keeps_the_shell_open() {
        let f = find_destructive("git reset --hard 1192c90");
        let text = declined_response(&findings_summary(&f));
        assert!(text.contains("`git reset --hard 1192c90`"));
        assert!(text.contains("Nothing in it was run"));
        assert!(text.contains("Other shell commands are still allowed"));
        assert!(approval_prompt(&f).contains("git reset --hard 1192c90"));
    }

    fn shell_request(id: &str, command: &str) -> ToolRequest {
        ToolRequest {
            id: id.to_string(),
            tool_call: Ok(
                CallToolRequestParams::new("shell").with_arguments(object!({ "command": command }))
            ),
            metadata: None,
            tool_meta: None,
        }
    }

    #[tokio::test]
    async fn inspector_asks_in_auto_mode_and_ignores_harmless() {
        let sessions = Arc::new(SessionManager::new(tempfile::tempdir().unwrap().keep()));
        let inspector = DestructiveCommandInspector::new(sessions);
        let reqs = vec![
            shell_request("bad", "cd '/w' && git reset --hard 1192c90"),
            shell_request("ok", "cd '/w' && git reset --soft 8bcacbe"),
        ];
        for mode in [GooseMode::Auto, GooseMode::SmartApprove, GooseMode::Approve] {
            let results = inspector.inspect("s", &reqs, &[], mode).await.unwrap();
            assert_eq!(results.len(), 1, "{mode:?}");
            assert_eq!(results[0].tool_request_id, "bad");
            assert!(matches!(
                &results[0].action,
                InspectionAction::RequireApproval(Some(m)) if m.contains("git reset --hard 1192c90")
            ));
            assert!(results[0].reason.contains("`git reset --hard 1192c90`"));
        }
        let chat = inspector
            .inspect("s", &reqs, &[], GooseMode::Chat)
            .await
            .unwrap();
        assert!(chat.is_empty());
    }

    #[test]
    fn non_shell_tools_are_not_inspected() {
        let req = ToolRequest {
            id: "w".into(),
            tool_call: Ok(CallToolRequestParams::new("write")
                .with_arguments(object!({ "command": "git reset --hard" }))),
            metadata: None,
            tool_meta: None,
        };
        assert!(findings_for_request(&req, None).is_empty());
        assert!(!findings_for_request(&shell_request("s", "rm -rf src"), None).is_empty());
        let prefixed = ToolRequest {
            id: "p".into(),
            tool_call: Ok(CallToolRequestParams::new("developer__shell")
                .with_arguments(object!({ "command": "git clean -fd" }))),
            metadata: None,
            tool_meta: None,
        };
        assert!(!findings_for_request(&prefixed, None).is_empty());
    }
}
