//! Swarm-coherence primitives (Phase 1, deterministic — no model in the path).
//!
//! One pure helper that makes decomposed work fit together by construction, fully deterministic
//! so it is unit-testable here without a fleet:
//!
//! [`extract_signatures`] — TIER-A backward extraction. Given a built dependency's source, return
//! ONLY its declaration surface (function/method signatures with bodies removed; type/const/var
//! declarations kept verbatim). This replaces injecting a dependency's WHOLE body into a consumer's
//! prompt with the exact, smaller, body-noise-free signatures it actually needs to call.
//! (`scope_contract_bundle`, the frozen-bundle scoper, died with the CONTRACTS phase — P1-4.)
//!
//! HEURISTIC, NOT A FULL AST (Phase 1, by design): the extractor is a brace/indentation-aware line
//! scanner, not a language parser. It assumes the common K&R style (a block's opening `{` sits on the
//! declaration line) and ignores braces inside string/char literals and `//` line comments. It never
//! panics and, when it extracts nothing useful, the caller falls back to the original body — so a
//! mis-parse degrades to today's behavior, never to a crash or an empty API.

/// The languages the extractor knows. Maps 1:1 from the CLI's `TargetLang`; `Other` is a passthrough.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigLang {
    Python,
    Rust,
    Go,
    TypeScript,
    Other,
}

/// Extract the declaration surface of `source` for `lang`. Function/method BODIES are removed; type,
/// const and var declarations are kept as-is. Returns a possibly-empty string; the caller decides the
/// fallback (an empty result means "nothing recognizable was found").
pub fn extract_signatures(source: &str, lang: SigLang) -> String {
    match lang {
        SigLang::Python => extract_python(source),
        SigLang::Rust | SigLang::Go | SigLang::TypeScript => extract_brace(source, lang),
        // Unknown language: no deterministic extractor, so signal "nothing extracted" and let the
        // caller keep the original body.
        SigLang::Other => String::new(),
    }
}

/// Count `{` / `}` on a line, ignoring braces inside `"…"`, `'…'`, `` `…` `` literals and after a `//`
/// line comment. Single-line only (multi-line strings are a known Phase-1 limitation). Returns
/// `(opens, closes)`.
fn brace_counts(line: &str) -> (usize, usize) {
    let mut opens = 0usize;
    let mut closes = 0usize;
    let mut chars = line.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == '\\' {
                    chars.next(); // skip the escaped char
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' | '`' => quote = Some(c),
                '/' if chars.peek() == Some(&'/') => break, // line comment: rest is inert
                '{' => opens += 1,
                '}' => closes += 1,
                _ => {}
            },
        }
    }
    (opens, closes)
}

/// The signature portion of a declaration line: everything up to (not including) the body-opening `{`,
/// trimmed of trailing whitespace and terminated with `;`. When the line has no `{` (e.g. a trait/interface
/// method declaration that already ends in `;`, or a continuation), it is returned trimmed-end unchanged.
/// Leading indentation is preserved so nested method signatures stay indented under their container.
fn fn_signature(line: &str) -> String {
    match line.find('{') {
        Some(idx) => format!("{};", line[..idx].trim_end()),
        None => line.trim_end().to_string(),
    }
}

/// Rust/Go/TypeScript extractor. Emits every line UNLESS it is inside a function/method body; a function
/// declaration line is reduced to its signature. Type/container declarations (struct/enum/interface/trait/
/// impl/class) and their contents are kept, so shared data shapes and method signatures survive; only
/// executable bodies are dropped.
fn extract_brace(source: &str, lang: SigLang) -> String {
    // Stack of open braces; `true` = this brace opened a function body (drop its contents).
    let mut stack: Vec<bool> = Vec::new();
    let mut out = String::new();
    for line in source.lines() {
        let in_drop = stack.iter().any(|&d| d);
        let trimmed = line.trim_start();
        let is_fn = !in_drop && is_fn_decl(trimmed, lang);
        if !in_drop {
            if is_fn {
                out.push_str(&fn_signature(line));
            } else {
                out.push_str(line.trim_end());
            }
            out.push('\n');
        }
        // Update the brace stack from THIS line. The first body brace on a function line is a drop
        // frame; every other brace inherits "keep" — while any drop frame is open, `in_drop` holds until
        // it closes, so nested braces inside a dropped body never resurface.
        let (opens, closes) = brace_counts(line);
        for _ in 0..opens {
            stack.push(is_fn);
        }
        for _ in 0..closes {
            stack.pop();
        }
    }
    out
}

/// Is `trimmed` (already left-trimmed) the start of a function/method declaration whose body we drop?
fn is_fn_decl(trimmed: &str, lang: SigLang) -> bool {
    match lang {
        SigLang::Rust => {
            let core = strip_rust_fn_modifiers(trimmed);
            core.starts_with("fn ")
        }
        SigLang::Go => trimmed.starts_with("func ") || trimmed.starts_with("func("),
        SigLang::TypeScript => is_ts_fn(trimmed),
        _ => false,
    }
}

/// Strip leading Rust visibility/qualifier tokens so `pub(crate) async unsafe fn` reduces to `fn …`.
fn strip_rust_fn_modifiers(t: &str) -> &str {
    let mut s = t;
    loop {
        let mut advanced = false;
        for pfx in [
            "pub(crate) ",
            "pub(super) ",
            "pub(self) ",
            "pub ",
            "default ",
            "async ",
            "unsafe ",
            "const ",
            "extern \"C\" ",
            "extern ",
        ] {
            if let Some(rest) = s.strip_prefix(pfx) {
                s = rest;
                advanced = true;
                break;
            }
        }
        if !advanced {
            return s;
        }
    }
}

/// TypeScript function / method detection. Handles `function` declarations, exported arrow-function
/// consts, and class methods (`name(args)… {`), while excluding control-flow keywords so a `if (…) {`
/// inside a kept class body is never mistaken for a method.
fn is_ts_fn(t: &str) -> bool {
    let core = strip_ts_export(t);
    if core.starts_with("function ")
        || core.starts_with("async function ")
        || core.starts_with("function*")
    {
        return true;
    }
    // Exported arrow function: `const f = (…) => {` / `const f = async (…) => {`.
    if (core.starts_with("const ") || core.starts_with("let ") || core.starts_with("var "))
        && core.contains("=>")
        && core.contains('{')
    {
        return true;
    }
    // Class method: `name(` … `{`, but not a control-flow statement or a plain call.
    if core.contains('{') {
        let name: String = core
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        if !name.is_empty() {
            let after = core[name.len()..].trim_start();
            let is_keyword = matches!(
                name.as_str(),
                "if" | "for"
                    | "while"
                    | "switch"
                    | "catch"
                    | "return"
                    | "do"
                    | "try"
                    | "else"
                    | "with"
            );
            // A method-shaped line strips its leading `public/private/…/async/get/set/static` markers
            // first (handled by strip_ts_export not covering these — so also allow those as the name).
            if !is_keyword && (after.starts_with('(') || is_ts_method_marker(&name)) {
                return true;
            }
        }
    }
    false
}

fn is_ts_method_marker(name: &str) -> bool {
    matches!(
        name,
        "public" | "private" | "protected" | "static" | "async" | "get" | "set" | "readonly"
    )
}

fn strip_ts_export(t: &str) -> &str {
    t.strip_prefix("export default ")
        .or_else(|| t.strip_prefix("export "))
        .unwrap_or(t)
}

/// Python extractor: keep decorators, `class` headers and `def`/`async def` signatures (bodies dropped),
/// preserving indentation so methods stay nested under their class. Multi-line signatures are joined until
/// the `:` that closes them at paren-depth 0.
fn extract_python(source: &str) -> String {
    let mut out = String::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        if t.starts_with('@') {
            out.push_str(line.trim_end());
            out.push('\n');
            i += 1;
            continue;
        }
        let is_class = t.starts_with("class ");
        let is_def = t.starts_with("def ") || t.starts_with("async def ");
        if is_class || is_def {
            // Join continuation lines until the signature-terminating `:` at paren depth 0.
            let mut sig = String::from(line.trim_end());
            let mut depth = paren_depth(line);
            let mut ended = depth <= 0 && ends_python_sig(line);
            while !ended && i + 1 < lines.len() {
                i += 1;
                let cont = lines[i];
                sig.push('\n');
                sig.push_str(cont.trim_end());
                depth += paren_depth(cont);
                ended = depth <= 0 && ends_python_sig(cont);
            }
            if is_def {
                sig.push_str(" ...");
            }
            out.push_str(&sig);
            out.push('\n');
        }
        i += 1;
    }
    out
}

/// Net paren/bracket/brace depth contributed by a line, ignoring string literals and `#` comments.
fn paren_depth(line: &str) -> i32 {
    let mut depth = 0i32;
    let mut chars = line.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == '\\' {
                    chars.next();
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '#' => break,
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            },
        }
    }
    depth
}

/// True if the (trailing-comment-stripped) line ends the Python signature, i.e. its last significant char
/// is `:`.
fn ends_python_sig(line: &str) -> bool {
    let code = match line.find('#') {
        Some(idx) => &line[..idx],
        None => line,
    };
    code.trim_end().ends_with(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_keeps_signatures_drops_bodies() {
        let src = "\
import os

class Store:
    def __init__(self, path: str) -> None:
        self.path = path
        self.rows = []

    def add(self, x: int) -> int:
        total = x + 1
        return total

def top_level(
    a: int,
    b: int,
) -> int:
    return a + b
";
        let out = extract_signatures(src, SigLang::Python);
        assert!(out.contains("class Store:"));
        assert!(out.contains("def __init__(self, path: str) -> None: ..."));
        assert!(out.contains("def add(self, x: int) -> int: ..."));
        // multi-line signature joined and terminated
        assert!(out.contains("def top_level("));
        assert!(out.contains(") -> int: ..."));
        // bodies gone
        assert!(!out.contains("self.path = path"));
        assert!(!out.contains("return total"));
        assert!(!out.contains("return a + b"));
        assert!(!out.contains("import os"));
    }

    #[test]
    fn rust_keeps_types_and_fn_sigs_drops_bodies() {
        let src = "\
use std::collections::HashMap;

pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub enum Dir { North, South }

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Point { x, y }
    }
    fn secret(&self) -> i32 {
        self.x + self.y
    }
}

pub fn add(a: i32, b: i32) -> i32 {
    let s = a + b;
    s
}
";
        let out = extract_signatures(src, SigLang::Rust);
        // type shapes kept verbatim (fields survive)
        assert!(out.contains("pub struct Point {"));
        assert!(out.contains("pub x: i32,"));
        assert!(out.contains("pub enum Dir { North, South }"));
        // impl header + method signatures kept, bodies dropped
        assert!(out.contains("impl Point {"));
        assert!(out.contains("pub fn new(x: i32, y: i32) -> Self;"));
        assert!(out.contains("fn secret(&self) -> i32;"));
        assert!(out.contains("pub fn add(a: i32, b: i32) -> i32;"));
        assert!(!out.contains("Point { x, y }"));
        assert!(!out.contains("let s = a + b;"));
        assert!(!out.contains("self.x + self.y"));
    }

    #[test]
    fn go_keeps_types_and_func_sigs_drops_bodies() {
        let src = "\
package store

type Point struct {
    X int
    Y int
}

func Add(a int, b int) int {
    s := a + b
    return s
}
";
        let out = extract_signatures(src, SigLang::Go);
        assert!(out.contains("type Point struct {"));
        assert!(out.contains("X int"));
        assert!(out.contains("func Add(a int, b int) int;"));
        assert!(!out.contains("s := a + b"));
        assert!(!out.contains("return s"));
    }

    #[test]
    fn ts_keeps_types_and_fn_sigs_drops_bodies() {
        let src = "\
export interface Point {
    x: number;
    y: number;
}

export function add(a: number, b: number): number {
    const s = a + b;
    return s;
}

export class Store {
    private rows: number[] = [];
    add(x: number): void {
        this.rows.push(x);
    }
}
";
        let out = extract_signatures(src, SigLang::TypeScript);
        assert!(out.contains("export interface Point {"));
        assert!(out.contains("x: number;"));
        assert!(out.contains("export function add(a: number, b: number): number;"));
        assert!(out.contains("export class Store {"));
        assert!(out.contains("add(x: number): void;"));
        assert!(!out.contains("const s = a + b;"));
        assert!(!out.contains("this.rows.push(x);"));
    }

    #[test]
    fn brace_in_string_does_not_unbalance() {
        let src = "\
pub const OPEN: &str = \"{\";
pub fn f() -> i32 {
    let x = \"}\";
    1
}
pub const CLOSE: &str = \"}\";
";
        let out = extract_signatures(src, SigLang::Rust);
        // Both consts (before and after the fn) must survive — proves the fn body did not swallow them.
        assert!(out.contains("pub const OPEN: &str = \"{\";"));
        assert!(out.contains("pub const CLOSE: &str = \"}\";"));
        assert!(out.contains("pub fn f() -> i32;"));
        assert!(!out.contains("let x ="));
    }

    #[test]
    fn other_language_extracts_nothing() {
        assert_eq!(extract_signatures("puts 'hi'", SigLang::Other), "");
    }
}
