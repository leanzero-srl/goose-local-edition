//! SHARD VERIFICATION at completion — DESIGN-SPLIT-V2 mechanism 2 (HIGH): "verify a shard at
//! completion, against stubs of its ASSUMES. Parse every piece; scan free identifiers; names not
//! defined in the folder, not in the declared interface, not language globals →
//! `shard_undefined_ref{shard, names}` (MILD: feeds the dossier and the merger's gap list, never a
//! retry). JS: `node --check` + the scan; Python: `py_compile` + the scan; other languages:
//! 'unchecked' said, never green."
//!
//! WHY (the works-prover, VA-079/VA-065): a shard is Done on its README alone; a piece that
//! exports `{ pick, drawBrush }` with `drawBrush` defined nowhere is a `ReferenceError` at load
//! that nobody names until the merger has retyped the module and the app fails to boot (the r5
//! brush ReferenceError shipped exactly so). CODE can see it at the shard's completion: the
//! folder's pieces are text, and a free identifier — used, defined by no piece, promised by no
//! declaration or ASSUMES line, and not a runtime global — is a fact about the tree.
//!
//! WHAT THIS IS NOT: a parser. The scans are conservative token walks — every declaration form
//! they recognise ADDS to the defined set, and a name they cannot classify is treated as
//! defined, so the failure mode is a missed reference (the merger finds it as `check_merge`
//! always did), never a phantom gap (a false "undefined" would send a real symbol out as
//! work). Known misses, on purpose: template-literal and f-string expressions are skipped;
//! labels and a bare `[a, b] = …` destructuring assignment read as references; TS/JSX pieces
//! (type positions, tag names) are UNSCANNED and said so; a `match`/`case` soft keyword is
//! recognised only at statement start with a trailing colon.
//!
//! GATES: no caps, clocks or counts — the only terminator is the folder's file list; MILD — the
//! shard stays Done, everything here is an event and a row the merger's brief reads; general —
//! the globals are per-language DATA (one list each, below), nothing here knows the spec;
//! fallback gate — an absent `node`/`python3` is `shard_check_unavailable{tool}`, an extension
//! with no checker or no scan is `shard_check_unavailable{tool: null, check}`; a piece is never
//! called parsed or clean because nothing looked at it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use goose_swarm::{ModuleInterface, ShardOf};

use super::parse_checks::{check_piece_with, PieceCheck, ToolNames};
use super::shards::extract_symbols;
use super::TargetLang;

// ---- the per-language data: one list each, marked as such ------------------------------------

/// The languages the free-identifier scan covers, chosen by a piece's extension. `.ts`/`.tsx`/
/// `.jsx` are NOT scanned (type positions and JSX tag names would read as references) — said as
/// `shard_check_unavailable{check: free_identifier_scan}`, never scanned badly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ScanLang {
    Js,
    Python,
}

pub(super) fn scan_lang_of(piece: &str) -> Option<ScanLang> {
    match Path::new(piece).extension().and_then(|e| e.to_str()) {
        Some("js" | "mjs" | "cjs") => Some(ScanLang::Js),
        Some("py") => Some(ScanLang::Python),
        _ => None,
    }
}

/// JavaScript runtime globals — ECMAScript, the browser (DOM, canvas, fetch, storage, observers,
/// events) and Node's CommonJS scope. A free identifier in this list resolves at run time, so it
/// is never an undefined reference. THE ONE PLACE this list lives; extend it here.
pub(super) const JS_GLOBALS: &[&str] = &[
    // ECMAScript
    "globalThis",
    "undefined",
    "NaN",
    "Infinity",
    "Object",
    "Function",
    "Array",
    "Number",
    "Boolean",
    "String",
    "Symbol",
    "BigInt",
    "Math",
    "JSON",
    "Date",
    "RegExp",
    "Error",
    "EvalError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "TypeError",
    "URIError",
    "AggregateError",
    "Promise",
    "Proxy",
    "Reflect",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "WeakRef",
    "FinalizationRegistry",
    "ArrayBuffer",
    "SharedArrayBuffer",
    "DataView",
    "Atomics",
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
    "Intl",
    "Iterator",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "encodeURI",
    "encodeURIComponent",
    "decodeURI",
    "decodeURIComponent",
    "escape",
    "unescape",
    "eval",
    "structuredClone",
    "queueMicrotask",
    "setTimeout",
    "clearTimeout",
    "setInterval",
    "clearInterval",
    "setImmediate",
    "clearImmediate",
    "arguments",
    // browser
    "window",
    "self",
    "document",
    "navigator",
    "location",
    "history",
    "screen",
    "console",
    "alert",
    "confirm",
    "prompt",
    "fetch",
    "Headers",
    "Request",
    "Response",
    "FormData",
    "Blob",
    "File",
    "FileReader",
    "FileList",
    "URL",
    "URLSearchParams",
    "TextEncoder",
    "TextDecoder",
    "AbortController",
    "AbortSignal",
    "EventSource",
    "WebSocket",
    "XMLHttpRequest",
    "Worker",
    "SharedWorker",
    "BroadcastChannel",
    "MessageChannel",
    "MessagePort",
    "Event",
    "CustomEvent",
    "EventTarget",
    "ErrorEvent",
    "ProgressEvent",
    "MessageEvent",
    "CloseEvent",
    "Node",
    "NodeList",
    "Element",
    "Text",
    "Comment",
    "Attr",
    "Document",
    "DocumentFragment",
    "Range",
    "Selection",
    "Image",
    "Audio",
    "Option",
    "ImageData",
    "ImageBitmap",
    "createImageBitmap",
    "Path2D",
    "DOMParser",
    "XMLSerializer",
    "CanvasRenderingContext2D",
    "CanvasGradient",
    "CanvasPattern",
    "OffscreenCanvas",
    "requestAnimationFrame",
    "cancelAnimationFrame",
    "requestIdleCallback",
    "cancelIdleCallback",
    "performance",
    "crypto",
    "localStorage",
    "sessionStorage",
    "indexedDB",
    "caches",
    "matchMedia",
    "getComputedStyle",
    "getSelection",
    "devicePixelRatio",
    "innerWidth",
    "innerHeight",
    "outerWidth",
    "outerHeight",
    "scrollX",
    "scrollY",
    "pageXOffset",
    "pageYOffset",
    "scrollTo",
    "scrollBy",
    "addEventListener",
    "removeEventListener",
    "dispatchEvent",
    "atob",
    "btoa",
    "ResizeObserver",
    "IntersectionObserver",
    "MutationObserver",
    "PerformanceObserver",
    "KeyboardEvent",
    "MouseEvent",
    "PointerEvent",
    "WheelEvent",
    "TouchEvent",
    "Touch",
    "TouchList",
    "InputEvent",
    "FocusEvent",
    "DragEvent",
    "DataTransfer",
    "ClipboardEvent",
    "AnimationEvent",
    "TransitionEvent",
    "UIEvent",
    "Notification",
    "MediaQueryList",
    "FontFace",
    "ReadableStream",
    "WritableStream",
    "TransformStream",
    "CompressionStream",
    "DecompressionStream",
    "Worklet",
    "frames",
    "parent",
    "top",
    "opener",
    "origin",
    "name",
    "status",
    "closed",
    "length",
    "print",
    "open",
    "close",
    "focus",
    "blur",
    "stop",
    "postMessage",
    "reportError",
    // Node / CommonJS
    "require",
    "module",
    "exports",
    "process",
    "Buffer",
    "__dirname",
    "__filename",
    "global",
];

/// Web-platform interface families named by prefix (`WebGL2RenderingContext`, `HTMLCanvasElement`,
/// `SVGPathElement`, `RTCPeerConnection`, `IDBDatabase`, `GPUDevice`, `CSSStyleSheet`, `DOMRect`,
/// `AudioContext`, `MediaStream`): a name that starts with one of these AND continues with an
/// upper-case letter or a digit is a platform global. Kept beside `JS_GLOBALS` as the same data.
pub(super) const JS_GLOBAL_PREFIXES: &[&str] = &[
    "WebGL", "HTML", "SVG", "RTC", "IDB", "GPU", "CSS", "DOM", "Audio", "Media",
];

/// Reserved words plus the contextual ones (`get`/`set`/`static`/`async`/`of`/`as`/`from`/`type`)
/// and TS's type keywords, so a `.js` piece that leans on TS syntax does not read its keywords as
/// references. A variable that shadows one of these is missed, never flagged (conservative).
const JS_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "let",
    "static",
    "await",
    "async",
    "of",
    "as",
    "from",
    "get",
    "set",
    "type",
    "interface",
    "namespace",
    "declare",
    "implements",
    "readonly",
    "private",
    "public",
    "protected",
    "abstract",
    "override",
    "keyof",
    "infer",
    "is",
    "satisfies",
    "string",
    "number",
    "boolean",
    "any",
    "unknown",
    "never",
    "object",
    "symbol",
    "bigint",
];

/// Python builtins (CPython 3.12 `dir(builtins)`: functions, types, constants, exceptions) and the
/// module dunders every file has. THE ONE PLACE this list lives; extend it here.
pub(super) const PY_BUILTINS: &[&str] = &[
    "abs",
    "aiter",
    "all",
    "anext",
    "any",
    "ascii",
    "bin",
    "bool",
    "breakpoint",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "compile",
    "complex",
    "copyright",
    "credits",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "exit",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "license",
    "list",
    "locals",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "quit",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
    "Ellipsis",
    "NotImplemented",
    "__import__",
    "__build_class__",
    "__name__",
    "__doc__",
    "__file__",
    "__spec__",
    "__loader__",
    "__package__",
    "__builtins__",
    "__debug__",
    "__all__",
    "__dict__",
    "__class__",
    "__annotations__",
    "__path__",
    "__version__",
    "__author__",
    "ArithmeticError",
    "AssertionError",
    "AttributeError",
    "BaseException",
    "BaseExceptionGroup",
    "BlockingIOError",
    "BrokenPipeError",
    "BufferError",
    "BytesWarning",
    "ChildProcessError",
    "ConnectionAbortedError",
    "ConnectionError",
    "ConnectionRefusedError",
    "ConnectionResetError",
    "DeprecationWarning",
    "EOFError",
    "EncodingWarning",
    "EnvironmentError",
    "Exception",
    "ExceptionGroup",
    "FileExistsError",
    "FileNotFoundError",
    "FloatingPointError",
    "FutureWarning",
    "GeneratorExit",
    "IOError",
    "ImportError",
    "ImportWarning",
    "IndentationError",
    "IndexError",
    "InterruptedError",
    "IsADirectoryError",
    "KeyError",
    "KeyboardInterrupt",
    "LookupError",
    "MemoryError",
    "ModuleNotFoundError",
    "NameError",
    "NotADirectoryError",
    "NotImplementedError",
    "OSError",
    "OverflowError",
    "PendingDeprecationWarning",
    "PermissionError",
    "ProcessLookupError",
    "RecursionError",
    "ReferenceError",
    "ResourceWarning",
    "RuntimeError",
    "RuntimeWarning",
    "StopAsyncIteration",
    "StopIteration",
    "SyntaxError",
    "SyntaxWarning",
    "SystemError",
    "SystemExit",
    "TabError",
    "TimeoutError",
    "TypeError",
    "UnboundLocalError",
    "UnicodeDecodeError",
    "UnicodeEncodeError",
    "UnicodeError",
    "UnicodeTranslateError",
    "UnicodeWarning",
    "UserWarning",
    "ValueError",
    "Warning",
    "WindowsError",
    "ZeroDivisionError",
];

const PY_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

fn is_js_global(name: &str) -> bool {
    JS_GLOBALS.contains(&name)
        || JS_GLOBAL_PREFIXES.iter().any(|p| {
            name.strip_prefix(p)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}

fn is_global(lang: ScanLang, name: &str) -> bool {
    match lang {
        ScanLang::Js => is_js_global(name),
        ScanLang::Python => PY_BUILTINS.contains(&name),
    }
}

// ---- tokens ------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Punct(String),
    /// A statement boundary: every JS newline; a Python newline outside brackets.
    Newline,
}

impl Tok {
    fn is(&self, p: &str) -> bool {
        matches!(self, Tok::Punct(s) if s == p)
    }
    fn is_word(&self, w: &str) -> bool {
        matches!(self, Tok::Ident(s) if s == w)
    }
    fn ident(&self) -> Option<&str> {
        match self {
            Tok::Ident(s) => Some(s),
            _ => None,
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn matches_at(c: &[char], i: usize, op: &str) -> bool {
    op.chars()
        .enumerate()
        .all(|(k, ch)| c.get(i + k) == Some(&ch))
}

/// Longest first, so `>>>=` is not read as `>` `>` `>=`.
const JS_OPS: &[&str] = &[
    ">>>=", "===", "!==", "**=", "<<=", ">>=", ">>>", "...", "&&=", "||=", "??=", "==", "!=", "<=",
    ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "%=", "&=", "|=", "^=", "**", "<<",
    ">>", "=>",
];

const PY_OPS: &[&str] = &[
    "**=", "//=", ">>=", "<<=", "...", ":=", "->", "**", "//", "==", "!=", "<=", ">=", "+=", "-=",
    "*=", "/=", "%=", "@=", "&=", "|=", "^=", "<<", ">>",
];

/// Skip a `'…'`/`"…"` literal; an unterminated one ends at the line (the walk is a heuristic and
/// must never eat the rest of the file).
fn skip_quoted(c: &[char], i: &mut usize, q: char) {
    *i += 1;
    while *i < c.len() {
        match c[*i] {
            '\\' => *i += 2,
            '\n' => return,
            ch if ch == q => {
                *i += 1;
                return;
            }
            _ => *i += 1,
        }
    }
}

fn skip_triple_quoted(c: &[char], i: &mut usize, q: char) {
    *i += 3;
    while *i < c.len() {
        if c[*i] == '\\' {
            *i += 2;
        } else if c[*i] == q && c.get(*i + 1) == Some(&q) && c.get(*i + 2) == Some(&q) {
            *i += 3;
            return;
        } else {
            *i += 1;
        }
    }
}

fn skip_number(c: &[char], i: &mut usize) {
    *i += 1;
    while *i < c.len() && (c[*i].is_ascii_alphanumeric() || c[*i] == '_' || c[*i] == '.') {
        *i += 1;
    }
}

fn read_ident(c: &[char], i: &mut usize) -> String {
    let start = *i;
    while *i < c.len() && is_ident_char(c[*i]) {
        *i += 1;
    }
    c[start..*i].iter().collect()
}

/// After which token may a `/` start a regex literal rather than divide? After nothing, an
/// operator or opening bracket, or a keyword that takes an expression (`return /x/`); never after
/// an identifier, a number-ish token or a closing bracket.
fn js_regex_allowed(out: &[Tok]) -> bool {
    match out.iter().rev().find(|t| **t != Tok::Newline) {
        None => true,
        Some(Tok::Ident(w)) => matches!(
            w.as_str(),
            "return"
                | "typeof"
                | "instanceof"
                | "in"
                | "of"
                | "new"
                | "delete"
                | "void"
                | "throw"
                | "case"
                | "do"
                | "else"
                | "await"
                | "yield"
        ),
        Some(Tok::Punct(p)) => !matches!(p.as_str(), ")" | "]" | "}" | "++" | "--"),
        Some(Tok::Newline) => true,
    }
}

fn skip_js_regex(c: &[char], i: &mut usize) {
    *i += 1;
    let mut in_class = false;
    while *i < c.len() {
        match c[*i] {
            '\\' => *i += 2,
            '\n' => return,
            '[' => {
                in_class = true;
                *i += 1;
            }
            ']' => {
                in_class = false;
                *i += 1;
            }
            '/' if !in_class => {
                *i += 1;
                while *i < c.len() && c[*i].is_ascii_alphabetic() {
                    *i += 1;
                }
                return;
            }
            _ => *i += 1,
        }
    }
}

fn tokenize_js(src: &str) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    js_code(&chars, &mut i, &mut out, false);
    out
}

/// One code frame: the file, or the expression inside a template literal's `${ }` (which ends at
/// the `}` closing it, so a template can nest code that nests templates).
fn js_code(c: &[char], i: &mut usize, out: &mut Vec<Tok>, in_template_expr: bool) {
    let mut depth = 0i32;
    while *i < c.len() {
        let ch = c[*i];
        if ch == '\n' {
            out.push(Tok::Newline);
            *i += 1;
            continue;
        }
        if ch.is_whitespace() {
            *i += 1;
            continue;
        }
        if ch == '/' {
            let next = c.get(*i + 1).copied();
            if next == Some('/') {
                while *i < c.len() && c[*i] != '\n' {
                    *i += 1;
                }
                continue;
            }
            if next == Some('*') {
                *i += 2;
                while *i + 1 < c.len() && !(c[*i] == '*' && c[*i + 1] == '/') {
                    *i += 1;
                }
                *i = (*i + 2).min(c.len());
                continue;
            }
            if js_regex_allowed(out) {
                skip_js_regex(c, i);
                continue;
            }
            if next == Some('=') {
                out.push(Tok::Punct("/=".to_string()));
                *i += 2;
            } else {
                out.push(Tok::Punct("/".to_string()));
                *i += 1;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            skip_quoted(c, i, ch);
            continue;
        }
        if ch == '`' {
            *i += 1;
            js_template(c, i, out);
            continue;
        }
        if ch.is_ascii_digit() {
            skip_number(c, i);
            continue;
        }
        if is_ident_start(ch) {
            let word = read_ident(c, i);
            out.push(Tok::Ident(word));
            continue;
        }
        if ch == '{' {
            depth += 1;
            out.push(Tok::Punct("{".to_string()));
            *i += 1;
            continue;
        }
        if ch == '}' {
            if in_template_expr && depth == 0 {
                *i += 1;
                return;
            }
            depth -= 1;
            out.push(Tok::Punct("}".to_string()));
            *i += 1;
            continue;
        }
        if let Some(op) = JS_OPS.iter().find(|op| matches_at(c, *i, op)) {
            out.push(Tok::Punct(op.to_string()));
            *i += op.chars().count();
            continue;
        }
        out.push(Tok::Punct(ch.to_string()));
        *i += 1;
    }
}

fn js_template(c: &[char], i: &mut usize, out: &mut Vec<Tok>) {
    while *i < c.len() {
        match c[*i] {
            '\\' => *i += 2,
            '`' => {
                *i += 1;
                return;
            }
            '$' if c.get(*i + 1) == Some(&'{') => {
                *i += 2;
                js_code(c, i, out, true);
            }
            _ => *i += 1,
        }
    }
}

fn tokenize_py(src: &str) -> Vec<Tok> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut depth = 0i32;
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            if depth == 0 {
                out.push(Tok::Newline);
            }
            i += 1;
            continue;
        }
        if ch == '\\' && c.get(i + 1) == Some(&'\n') {
            i += 2;
            continue;
        }
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '#' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            if c.get(i + 1) == Some(&ch) && c.get(i + 2) == Some(&ch) {
                skip_triple_quoted(&c, &mut i, ch);
            } else {
                skip_quoted(&c, &mut i, ch);
            }
            continue;
        }
        if ch.is_ascii_digit() {
            skip_number(&c, &mut i);
            continue;
        }
        if is_ident_start(ch) {
            let word = read_ident(&c, &mut i);
            // A string prefix (`r'…'`, `f"…"`, `rb'''…'''`) is not a name.
            let is_prefix = word.len() <= 2
                && word.chars().all(|w| "rRbBuUfF".contains(w))
                && matches!(c.get(i), Some('\'') | Some('"'));
            if is_prefix {
                let q = c[i];
                if c.get(i + 1) == Some(&q) && c.get(i + 2) == Some(&q) {
                    skip_triple_quoted(&c, &mut i, q);
                } else {
                    skip_quoted(&c, &mut i, q);
                }
                continue;
            }
            out.push(Tok::Ident(word));
            continue;
        }
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
        if let Some(op) = PY_OPS.iter().find(|op| matches_at(&c, i, op)) {
            out.push(Tok::Punct(op.to_string()));
            i += op.chars().count();
            continue;
        }
        out.push(Tok::Punct(ch.to_string()));
        i += 1;
    }
    out
}

// ---- the scans ---------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct ScanSets {
    defined: BTreeSet<String>,
    referenced: BTreeSet<String>,
}

fn is_open(t: &Tok) -> bool {
    t.is("(") || t.is("[") || t.is("{")
}
fn is_close(t: &Tok) -> bool {
    t.is(")") || t.is("]") || t.is("}")
}

/// Index of the bracket closing the one at `open` (any kind), or the end of the slice.
fn matching_close(toks: &[Tok], open: usize) -> usize {
    let mut depth = 0i32;
    for (k, t) in toks.iter().enumerate().skip(open) {
        if is_open(t) {
            depth += 1;
        } else if is_close(t) {
            depth -= 1;
            if depth == 0 {
                return k;
            }
        }
    }
    toks.len().saturating_sub(1)
}

fn idents_in(toks: &[Tok], from: usize, to: usize, into: &mut BTreeSet<String>) {
    let end = to.min(toks.len().saturating_sub(1));
    for (k, t) in toks.iter().enumerate().take(end + 1).skip(from) {
        if let Tok::Ident(w) = t {
            let after_dot = k > 0 && (toks[k - 1].is(".") || toks[k - 1].is("?."));
            if !after_dot {
                into.insert(w.clone());
            }
        }
    }
}

fn prev_sig(toks: &[Tok], i: usize) -> Option<&Tok> {
    toks[..i].iter().rev().find(|t| **t != Tok::Newline)
}
fn next_sig(toks: &[Tok], i: usize) -> Option<&Tok> {
    toks[i + 1..].iter().find(|t| **t != Tok::Newline)
}

/// `const`/`let`/`var` declaration list: every name right after the keyword or after a depth-0
/// comma is declared; a `{…}`/`[…]` pattern declares everything inside it (keys and defaults
/// included — over-approximation on the defined side). Ends at `;`, `)`, `of`/`in`, another
/// statement keyword, or a newline that does not follow a comma.
///
/// This is the undefined-reference scan's SCOPE set (locals at any depth, destructuring), not
/// the definition rule: what a piece DEFINES for the module is `shards::extract_symbols` (one
/// rule — VA-097), which `undefined_references` unions into `defined` below.
fn js_declaration_list(toks: &[Tok], kw: usize, defined: &mut BTreeSet<String>) {
    let mut k = kw + 1;
    let mut expect_name = true;
    let mut depth = 0i32;
    while k < toks.len() {
        let t = &toks[k];
        if depth == 0 {
            if t.is(";") || t.is(")") || t.is_word("of") || t.is_word("in") {
                return;
            }
            if t.ident().is_some_and(|w| {
                matches!(
                    w,
                    "let"
                        | "const"
                        | "var"
                        | "function"
                        | "class"
                        | "if"
                        | "for"
                        | "while"
                        | "return"
                        | "export"
                        | "import"
                        | "switch"
                        | "try"
                )
            }) {
                return;
            }
            if *t == Tok::Newline {
                if !prev_sig(toks, k).is_some_and(|p| p.is(",") || p.is("=")) {
                    return;
                }
                k += 1;
                continue;
            }
            if expect_name {
                if is_open(t) {
                    let close = matching_close(toks, k);
                    idents_in(toks, k, close, defined);
                    k = close + 1;
                    expect_name = false;
                    continue;
                }
                if let Some(w) = t.ident() {
                    defined.insert(w.to_string());
                    expect_name = false;
                    k += 1;
                    continue;
                }
            }
            if t.is(",") {
                expect_name = true;
                k += 1;
                continue;
            }
        }
        if is_open(t) {
            depth += 1;
        } else if is_close(t) {
            depth -= 1;
        }
        k += 1;
    }
}

fn scan_js(toks: &[Tok]) -> ScanSets {
    let mut s = ScanSets::default();
    let kw = |w: &str| JS_KEYWORDS.contains(&w);
    // Pass A — every declaration form adds to `defined`.
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Ident(w) if w == "const" || w == "let" || w == "var" => {
                js_declaration_list(toks, i, &mut s.defined);
            }
            Tok::Ident(w) if w == "import" => {
                // `import a, { b as c } from '…'` / `import * as ns from '…'`: names until `from`.
                let mut k = i + 1;
                while k < toks.len() {
                    match &toks[k] {
                        Tok::Newline => break,
                        Tok::Punct(p) if p == ";" => break,
                        Tok::Ident(w) if w == "from" => break,
                        Tok::Ident(w) if !kw(w) => {
                            s.defined.insert(w.clone());
                        }
                        _ => {}
                    }
                    k += 1;
                }
            }
            Tok::Ident(w) if !kw(w) => {
                let prev = prev_sig(toks, i);
                let next = next_sig(toks, i);
                let after_decl_kw = prev.is_some_and(|p| {
                    p.is_word("function")
                        || p.is_word("class")
                        || p.is_word("type")
                        || p.is_word("interface")
                        || p.is_word("enum")
                        || p.is_word("namespace")
                        || (p.is("*")
                            && prev_sig(toks, i - 1).is_some_and(|q| q.is_word("function")))
                });
                if after_decl_kw || next.is_some_and(|n| n.is("=>") || n.is("=")) {
                    s.defined.insert(w.clone());
                }
            }
            Tok::Punct(p) if p == "(" => {
                let close = matching_close(toks, i);
                let after = next_sig(toks, close);
                let before = prev_sig(toks, i);
                let arrow = after.is_some_and(|a| a.is("=>"));
                let body = after.is_some_and(|a| a.is("{"));
                let head_defines_params = match before {
                    Some(Tok::Ident(w)) => !kw(w) || w == "function" || w == "catch",
                    _ => false,
                };
                if arrow || (body && head_defines_params) {
                    idents_in(toks, i + 1, close.saturating_sub(1), &mut s.defined);
                    let head_name = match before {
                        Some(Tok::Ident(w)) if !kw(w) => Some(w),
                        _ => None,
                    };
                    if let Some(w) = head_name.filter(|_| body) {
                        s.defined.insert(w.clone());
                    }
                }
            }
            _ => {}
        }
    }
    // Pass B — every other identifier use is a reference.
    for (i, t) in toks.iter().enumerate() {
        let Tok::Ident(w) = t else { continue };
        if kw(w) {
            continue;
        }
        let prev = prev_sig(toks, i);
        if prev.is_some_and(|p| p.is(".") || p.is("?.") || p.is("#")) {
            continue;
        }
        let object_key = prev.is_some_and(|p| p.is("{") || p.is(","))
            && next_sig(toks, i).is_some_and(|n| n.is(":"));
        if object_key {
            continue;
        }
        s.referenced.insert(w.clone());
    }
    s
}

/// Bracket depth of every token, computed once per statement.
fn depths(toks: &[Tok]) -> Vec<i32> {
    let mut d = 0i32;
    toks.iter()
        .map(|t| {
            if is_close(t) {
                d -= 1;
            }
            let here = d;
            if is_open(t) {
                d += 1;
            }
            here
        })
        .collect()
}

fn scan_py_statement(st: &[Tok], s: &mut ScanSets) {
    if st.is_empty() {
        return;
    }
    let kw = |w: &str| PY_KEYWORDS.contains(&w);
    let d = depths(st);
    let first = st[0].ident();
    let last_is_colon = st.last().is_some_and(|t| t.is(":"));
    let second = st.get(1);
    let soft_kw = matches!(first, Some("match" | "case"))
        && last_is_colon
        && st.len() > 2
        && !second.is_some_and(|t| {
            t.is("=") || t.is(".") || t.is("(") || t.is("[") || t.is(",") || t.is(")")
        });
    let is_import = st.iter().any(|t| t.is_word("import"));
    let is_case = soft_kw && first == Some("case");
    // Pass A — declarations.
    let mut def_at: Option<usize> = None;
    for (i, t) in st.iter().enumerate() {
        let Tok::Ident(w) = t else { continue };
        let after_dot = i > 0 && st[i - 1].is(".");
        match w.as_str() {
            "def" => def_at = Some(i),
            "class" => {
                if let Some(Tok::Ident(n)) = st.get(i + 1) {
                    s.defined.insert(n.clone());
                }
            }
            "global" | "nonlocal" => {
                idents_in(st, i + 1, st.len().saturating_sub(1), &mut s.defined);
            }
            "for" => {
                let end = st[i + 1..]
                    .iter()
                    .position(|t| t.is_word("in"))
                    .map(|p| i + p)
                    .unwrap_or(st.len().saturating_sub(1));
                idents_in(st, i + 1, end, &mut s.defined);
            }
            "as" => {
                if let Some(Tok::Ident(n)) = st.get(i + 1) {
                    s.defined.insert(n.clone());
                }
            }
            "lambda" => {
                let end = st[i + 1..]
                    .iter()
                    .position(|t| t.is(":"))
                    .map(|p| i + p)
                    .unwrap_or(st.len().saturating_sub(1));
                idents_in(st, i + 1, end, &mut s.defined);
            }
            _ => {
                if !after_dot && st.get(i + 1).is_some_and(|n| n.is(":=")) {
                    s.defined.insert(w.clone());
                }
                if is_import && !kw(w) && !after_dot {
                    s.defined.insert(w.clone());
                }
                if is_case && !kw(w) && !after_dot {
                    s.defined.insert(w.clone());
                }
            }
        }
    }
    if let Some(di) = def_at {
        if let Some(Tok::Ident(n)) = st.get(di + 1) {
            s.defined.insert(n.clone());
        }
        if st.get(di + 2).is_some_and(|t| t.is("(")) {
            let close = matching_close(st, di + 2);
            idents_in(st, di + 3, close.saturating_sub(1), &mut s.defined);
        }
    }
    // Statement-level assignment: every depth-0 name before the first depth-0 `=` / augmented op.
    let assign_ops = [
        "=", "+=", "-=", "*=", "/=", "//=", "%=", "**=", "@=", "&=", "|=", "^=", ">>=", "<<=",
    ];
    if def_at.is_none() && first != Some("class") {
        if let Some(eq) = st.iter().enumerate().position(|(k, t)| {
            d[k] == 0 && matches!(t, Tok::Punct(p) if assign_ops.contains(&p.as_str()))
        }) {
            for (k, t) in st.iter().enumerate().take(eq) {
                if let Tok::Ident(w) = t {
                    let after_dot = k > 0 && st[k - 1].is(".");
                    if d[k] == 0 && !after_dot && !kw(w) {
                        s.defined.insert(w.clone());
                    }
                }
            }
        } else if let (Some(f), Some(sec)) = (first, second) {
            // `x: int` — an annotation with no value declares the name.
            if !kw(f) && sec.is(":") && !soft_kw {
                s.defined.insert(f.to_string());
            }
        }
    }
    // Pass B — references.
    for (i, t) in st.iter().enumerate() {
        let Tok::Ident(w) = t else { continue };
        if kw(w) || (soft_kw && i == 0) {
            continue;
        }
        if i > 0 && st[i - 1].is(".") {
            continue;
        }
        let kwarg_key = d[i] > 0
            && st.get(i + 1).is_some_and(|n| n.is("="))
            && (i == 0 || st[i - 1].is("(") || st[i - 1].is(","));
        if kwarg_key {
            continue;
        }
        s.referenced.insert(w.clone());
    }
}

fn scan_py(toks: &[Tok]) -> ScanSets {
    let mut s = ScanSets::default();
    for statement in toks.split(|t| *t == Tok::Newline || t.is(";")) {
        scan_py_statement(statement, &mut s);
    }
    s
}

fn ident_tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !is_ident_char(c))
        .filter(|w| !w.is_empty() && w.chars().next().is_some_and(is_ident_start))
        .map(str::to_string)
}

/// Names the declared interface accounts for: every export (exact and its last dotted segment —
/// `window.vs7dbg.pick` is met by `pick`) and every identifier in the shared-state text (`S.dirty`
/// is met by `S` and `dirty`; the merger reconciles the shape, as `build_merge_dossier` already
/// rules).
fn interface_names(interface: &ModuleInterface) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for e in &interface.exports {
        out.extend(ident_tokens(&e.name));
        if let Some(last) = e.name.rsplit('.').next() {
            out.insert(last.to_string());
        }
    }
    out.extend(ident_tokens(&interface.shared_state));
    out
}

/// The README's ASSUMES lines are the shard's own declaration of what it leans on: every
/// identifier in them is a stub the merger will reconcile, never an undefined reference here.
fn assumes_names(assumes: &[String]) -> BTreeSet<String> {
    assumes.iter().flat_map(|a| ident_tokens(a)).collect()
}

/// The free-identifier scan over one shard folder's pieces `(file name, source)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ScanOutcome {
    /// Sorted, deduplicated: used by a piece, defined by none, promised by neither the interface
    /// nor ASSUMES, and not a runtime global of any scanned language.
    pub(super) undefined: Vec<String>,
    pub(super) scanned: Vec<String>,
    pub(super) unscanned: Vec<String>,
}

pub(super) fn undefined_references(
    pieces: &[(String, String)],
    interface: &ModuleInterface,
    assumes: &[String],
) -> ScanOutcome {
    let mut defined: BTreeSet<String> = BTreeSet::new();
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    let mut langs: BTreeSet<ScanLang> = BTreeSet::new();
    let mut out = ScanOutcome::default();
    for (name, src) in pieces {
        let Some(lang) = scan_lang_of(name) else {
            out.unscanned.push(name.clone());
            continue;
        };
        let sets = match lang {
            ScanLang::Js => scan_js(&tokenize_js(src)),
            ScanLang::Python => scan_py(&tokenize_py(src)),
        };
        defined.extend(sets.defined);
        referenced.extend(sets.referenced);
        // The dossier's own extractor agrees on what a DEFINITION is (S14-1: shorthand
        // properties are mentions, never definitions).
        let target = match lang {
            ScanLang::Js => TargetLang::TypeScript,
            ScanLang::Python => TargetLang::Python,
        };
        for sym in extract_symbols(src, target) {
            if !sym.shorthand {
                if let Some(last) = sym.name.rsplit('.').next() {
                    defined.insert(last.to_string());
                }
                defined.insert(sym.name);
            }
        }
        langs.insert(lang);
        out.scanned.push(name.clone());
    }
    let mut known = interface_names(interface);
    known.extend(assumes_names(assumes));
    out.undefined = referenced
        .difference(&defined)
        .filter(|n| !known.contains(*n))
        .filter(|n| !langs.iter().any(|l| is_global(*l, n.as_str())))
        .cloned()
        .collect();
    out
}

// ---- the verification at completion ------------------------------------------------------------

/// What CODE established about one shard's folder at its completion. Every list is a fact about
/// a file; nothing here is a verdict on the shard.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ShardVerification {
    /// Every file in the folder that is not the README, sorted.
    pub(super) pieces: Vec<String>,
    pub(super) parsed: Vec<String>,
    /// (piece, the tool's error line)
    pub(super) pieces_unparsed: Vec<(String, String)>,
    /// (tool, piece) — the checker could not start.
    pub(super) checks_unavailable: Vec<(String, String)>,
    /// (extension, piece) — no per-file checker exists.
    pub(super) unchecked: Vec<(String, String)>,
    /// (piece, io error) — the free-identifier scan could not read it.
    pub(super) unreadable: Vec<(String, String)>,
    pub(super) scanned: Vec<String>,
    pub(super) unscanned: Vec<String>,
    pub(super) undefined_refs: Vec<String>,
}

/// The ASSUMES list out of the row `record_shard_note` returned (`shard_note.assumes`); an
/// absent note is an empty list — the absence is already `merge_note_missing`, said there.
pub(super) fn assumes_of(note_row: &serde_json::Value) -> Vec<String> {
    note_row["shard_note"]["assumes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

pub(super) async fn verify_shard(
    root: &Path,
    shard: &ShardOf,
    note_row: &serde_json::Value,
) -> ShardVerification {
    verify_shard_with(root, shard, note_row, &ToolNames::default()).await
}

pub(super) async fn verify_shard_with(
    root: &Path,
    shard: &ShardOf,
    note_row: &serde_json::Value,
    tools: &ToolNames,
) -> ShardVerification {
    let dir = root.join(&shard.folder);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| n != "README.md")
        .collect();
    names.sort();
    let mut v = ShardVerification::default();
    let mut sources: Vec<(String, String)> = Vec::new();
    for n in &names {
        let path = dir.join(n);
        match check_piece_with(&path, tools).await {
            PieceCheck::Parsed => v.parsed.push(n.clone()),
            PieceCheck::Failed(err) => v.pieces_unparsed.push((n.clone(), err)),
            PieceCheck::ToolUnavailable { tool, .. } => {
                v.checks_unavailable.push((tool, n.clone()))
            }
            PieceCheck::NoChecker { ext } => v.unchecked.push((ext, n.clone())),
        }
        match std::fs::read_to_string(&path) {
            Ok(src) => sources.push((n.clone(), src)),
            Err(e) => v.unreadable.push((n.clone(), e.to_string())),
        }
    }
    let scan = undefined_references(&sources, &shard.interface, &assumes_of(note_row));
    v.pieces = names;
    v.scanned = scan.scanned;
    v.unscanned = scan.unscanned;
    v.undefined_refs = scan.undefined;
    v
}

fn group<'a, I>(pairs: I) -> BTreeMap<String, Vec<String>>
where
    I: IntoIterator<Item = &'a (String, String)>,
{
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (key, piece) in pairs {
        m.entry(key.clone()).or_default().push(piece.clone());
    }
    m
}

impl ShardVerification {
    /// The events for run.jsonl — one per unparsed piece, one per absent tool, one per
    /// checker-less extension, one for the unscanned pieces, one for the undefined names. Nothing
    /// when the folder is clean and every piece was checked and scanned.
    pub(super) fn events(&self, shard: &ShardOf, task_id: &str) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        for (piece, error) in &self.pieces_unparsed {
            out.push(serde_json::json!({
                "event": "shard_piece_unparsed",
                "module": shard.module,
                "shard": shard.shard,
                "task_id": task_id,
                "folder": shard.folder,
                "piece": piece,
                "error": error,
            }));
        }
        for (tool, pieces) in group(&self.checks_unavailable) {
            out.push(serde_json::json!({
                "event": "shard_check_unavailable",
                "module": shard.module,
                "shard": shard.shard,
                "task_id": task_id,
                "check": "parse",
                "tool": tool,
                "pieces": pieces,
            }));
        }
        for (ext, pieces) in group(&self.unchecked) {
            out.push(serde_json::json!({
                "event": "shard_check_unavailable",
                "module": shard.module,
                "shard": shard.shard,
                "task_id": task_id,
                "check": "parse",
                "tool": serde_json::Value::Null,
                "ext": ext,
                "pieces": pieces,
                "reason": "no per-file parser for this extension",
            }));
        }
        for (piece, error) in &self.unreadable {
            out.push(serde_json::json!({
                "event": "shard_check_unavailable",
                "module": shard.module,
                "shard": shard.shard,
                "task_id": task_id,
                "check": "free_identifier_scan",
                "tool": serde_json::Value::Null,
                "pieces": [piece],
                "reason": format!("unreadable: {error}"),
            }));
        }
        if !self.unscanned.is_empty() {
            out.push(serde_json::json!({
                "event": "shard_check_unavailable",
                "module": shard.module,
                "shard": shard.shard,
                "task_id": task_id,
                "check": "free_identifier_scan",
                "tool": serde_json::Value::Null,
                "pieces": self.unscanned,
                "reason": "no free-identifier scan for this extension (js/mjs/cjs and py only)",
            }));
        }
        if !self.undefined_refs.is_empty() {
            out.push(serde_json::json!({
                "event": "shard_undefined_ref",
                "module": shard.module,
                "shard": shard.shard,
                "task_id": task_id,
                "folder": shard.folder,
                "names": self.undefined_refs,
                "pieces_scanned": self.scanned,
            }));
        }
        out
    }

    /// The `verify` object merged into the shard's ledger row, where the merger's dispatch
    /// (`merge_holes`) reads it back into the GAP paragraph.
    pub(super) fn row_extra(&self) -> serde_json::Value {
        serde_json::json!({
            "pieces": self.pieces,
            "parsed": self.parsed,
            "pieces_unparsed": self.pieces_unparsed.iter().map(|(p, e)| serde_json::json!({"piece": p, "error": e})).collect::<Vec<_>>(),
            "checks_unavailable": self.checks_unavailable.iter().map(|(t, p)| serde_json::json!({"tool": t, "piece": p})).collect::<Vec<_>>(),
            "unchecked": self.unchecked.iter().map(|(x, p)| serde_json::json!({"ext": x, "piece": p})).collect::<Vec<_>>(),
            "unreadable": self.unreadable.iter().map(|(p, e)| serde_json::json!({"piece": p, "error": e})).collect::<Vec<_>>(),
            "scanned": self.scanned,
            "unscanned": self.unscanned,
            "undefined_refs": self.undefined_refs,
        })
    }
}

/// Add the verification to the row `record_shard_note` built, under one key, so every other row
/// stays byte-identical and the dossier's readers find it by name.
pub(super) fn merge_into_row(row: &mut serde_json::Value, v: &ShardVerification) {
    if let Some(o) = row.as_object_mut() {
        o.insert("verify".to_string(), v.row_extra());
    }
}

/// The `verify` object of a shard task's ledger row (`write_task_ledger`'s file,
/// `<LEDGER_DIR>/<activity_digest_key(task_id)>.json`), or None when the row or the key is absent
/// — an unverified shard has nothing to say, as `shard_pieces_absent_event` reads its own key.
pub(super) fn ledger_verify_row(root: &Path, task_id: &str) -> Option<serde_json::Value> {
    let path = root
        .join(super::LEDGER_DIR)
        .join(format!("{}.json", super::activity_digest_key(task_id)));
    let row: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    row.get("verify").cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_swarm::DeclaredExport;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "goose-shard-verify-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn interface(exports: &[&str], shared_state: &str) -> ModuleInterface {
        ModuleInterface {
            exports: exports
                .iter()
                .map(|n| DeclaredExport {
                    name: n.to_string(),
                    ..Default::default()
                })
                .collect(),
            shared_state: shared_state.to_string(),
            layout: Vec::new(),
        }
    }

    fn shard(folder: &str, exports: &[&str]) -> ShardOf {
        ShardOf {
            module: "web-viz".into(),
            shard: "render".into(),
            folder: folder.into(),
            interface: interface(exports, "S = { points: Float32Array, dirty: bool }"),
            ..Default::default()
        }
    }

    /// The works-prover's first failure, on a JS piece: it USES the interface's `pick`, its
    /// ASSUMES' `S.brush`, a dozen runtime globals (window, document, console, Math,
    /// Float32Array, requestAnimationFrame, fetch, EventSource, WebGL2RenderingContext,
    /// HTMLCanvasElement), everything it defines itself (functions, consts, params, destructured
    /// bindings, a class, imports, arrow params, catch bindings, object keys) — and ONE name
    /// nothing accounts for: `drawBrush`, the shorthand export of the r5 ReferenceError. Exactly
    /// that one name is undefined.
    #[test]
    fn js_free_identifiers_leave_exactly_the_truly_undefined_name() {
        let src = r#"
// render shard — programs and geometry
import { compile } from './gl.js';
import * as vec from './vec.js';
const canvas = document.getElementById('viz');
const gl = canvas.getContext('webgl2');
let frame = 0, pending;
const { width, height } = canvas;
const [first, ...rest] = [1, 2, 3];
const re = /[a-z]+\/x/gi;
const url = `${location.origin}/api/${pick(1, 2)}`;
class Scene extends EventTarget {
  #hidden = 1;
  count = 0;
  static create(opts = {}) { return new Scene(opts); }
  constructor(opts) { super(); this.opts = opts; this.#hidden = 2; }
  get size() { return this.count; }
  draw(points, { alpha: a = 1 } = {}) {
    const buf = new Float32Array(points.length);
    for (const p of points) buf[frame] = p * a;
    return buf;
  }
}
function initGL(w, h) {
  if (!(gl instanceof WebGL2RenderingContext)) console.warn("no gl");
  const el = new HTMLCanvasElement();
  return { w, h, el, dirty: S.dirty, brush: S.brush };
}
const render = async (dt) => {
  try { await fetch(url); } catch (err) { console.error(err); }
  const es = new EventSource('/events');
  es.onmessage = (ev) => { frame = Math.max(frame, ev.data.length); };
  requestAnimationFrame(render);
  window.viz = { initGL, render, pick, drawBrush };
  return vec.add(first, rest.length) + width * height + compile(pending);
};
export { initGL, render, Scene };
"#;
        let out = undefined_references(
            &[("render.js".to_string(), src.to_string())],
            &interface(&["window.vs7dbg.pick", "initGL(w, h)", "render(dt)"], "S"),
            &["S.brush is a Set<id>".to_string()],
        );
        assert_eq!(out.undefined, vec!["drawBrush".to_string()], "{out:?}");
        assert_eq!(out.scanned, vec!["render.js".to_string()]);
        assert!(out.unscanned.is_empty());
    }

    /// A piece that names its sibling's export defines nothing of it — the interface accounts for
    /// the export by exact name and by last dotted segment; a piece that defines the name itself
    /// (any piece in the folder) also clears it.
    #[test]
    fn a_sibling_piece_in_the_same_folder_defines_the_name() {
        let a = "export function pick(sx, sy) { return readPickAt(sx, sy); }\n";
        let b = "export function readPickAt(x, y) { return x + y; }\n";
        let out = undefined_references(
            &[
                ("pick.js".to_string(), a.to_string()),
                ("read.js".to_string(), b.to_string()),
            ],
            &interface(&[], ""),
            &[],
        );
        assert!(out.undefined.is_empty(), "{out:?}");
        let alone = undefined_references(
            &[("pick.js".to_string(), a.to_string())],
            &interface(&[], ""),
            &[],
        );
        assert_eq!(alone.undefined, vec!["readPickAt".to_string()]);
    }

    /// The Python twin: builtins, imports, def/class names, params, for-targets, with/except
    /// bindings, comprehension variables, lambda params, keyword-argument keys, augmented and
    /// tuple assignments, a walrus, f-string prefixes, decorators over a defined name — and one
    /// undefined `make_ledger`, which the ASSUMES line does NOT name (it names `Ledger`).
    #[test]
    fn python_free_identifiers_leave_exactly_the_truly_undefined_name() {
        let src = r#"
"""ledger core — the shard's piece."""
import os, json as js
from pathlib import Path
from .util import (helper,
                   other as alias)

RATE: float = 0.5
count, total = 0, 0.0
count += 1

def make_entry(payer: str, amount: float = 1.0, *args, **kw) -> dict:
    label = f"{payer}: {amount}"
    entries = [e for e in kw.values() if e]
    squares = {k: v ** 2 for k, v in enumerate(entries)}
    with open(os.getcwd()) as fh, Path(".") as p:
        data = js.loads(fh.read())
    try:
        pass
    except (ValueError, KeyError) as exc:
        raise RuntimeError(str(exc))
    if (n := len(entries)) > 2:
        print(n, label, squares, data, p, alias, helper)
    f = lambda x, y=2: x + y
    return dict(payer=payer, amount=amount, total=f(1), rest=list(args), r=RATE)

class Store(object):
    kind = "memory"
    def __init__(self, ledger: Ledger):
        self.ledger = ledger
    @staticmethod
    def build():
        return make_ledger(Store.kind)
"#;
        let out = undefined_references(
            &[("core.py".to_string(), src.to_string())],
            &interface(&["make_entry(payer, amount)"], ""),
            &["Ledger is the sibling's class".to_string()],
        );
        assert_eq!(out.undefined, vec!["make_ledger".to_string()], "{out:?}");
    }

    /// TS/JSX and every other extension are UNSCANNED — said, never scanned badly or called clean.
    #[test]
    fn other_extensions_are_unscanned_and_said() {
        let out = undefined_references(
            &[
                ("types.ts".to_string(), "export type X = Foo;\n".to_string()),
                (
                    "main.rs".to_string(),
                    "fn main() { helper(); }\n".to_string(),
                ),
                ("ok.js".to_string(), "const a = 1;\n".to_string()),
            ],
            &interface(&[], ""),
            &[],
        );
        assert_eq!(
            out.unscanned,
            vec!["types.ts".to_string(), "main.rs".to_string()]
        );
        assert_eq!(out.scanned, vec!["ok.js".to_string()]);
        assert!(out.undefined.is_empty());
    }

    /// The per-language globals are data with a prefix rule for the platform's interface
    /// families; a user type that merely shares a prefix's letters is not one.
    #[test]
    fn the_globals_lists_are_data_with_a_prefix_rule() {
        assert!(is_js_global("WebGL2RenderingContext"));
        assert!(is_js_global("HTMLCanvasElement"));
        assert!(is_js_global("DOMRect"));
        assert!(
            !is_js_global("Domain"),
            "`Domain` is not `DOM` + upper-case"
        );
        assert!(!is_js_global("drawBrush"));
        assert!(is_global(ScanLang::Python, "isinstance"));
        assert!(!is_global(ScanLang::Python, "make_ledger"));
    }

    /// The seam's verification on disk: a folder with a clean JS piece, a piece with an
    /// undefined name, a `.rs` piece nobody can check per-file, and a Python piece that does not
    /// parse. With the real tools present, the events are one `shard_piece_unparsed`, one
    /// `shard_check_unavailable{tool: null, ext: rs}`, one free-identifier-scan unavailability
    /// for the `.rs`, and one `shard_undefined_ref{names: [drawBrush]}`; the row carries the same
    /// under `verify`. When a tool is absent on this machine the arm is REPORTED, not faked.
    #[tokio::test]
    async fn verify_shard_names_unparsed_unchecked_and_undefined_and_writes_the_row() {
        let root = tmp("verify");
        let folder = ".swarm/shards/web-viz/render";
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("README.md"),
            "PROVIDES: render()\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: none\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("render.js"),
            "export function render() { return drawBrush([]); }\n",
        )
        .unwrap();
        std::fs::write(dir.join("util.rs"), "pub fn helper() {}\n").unwrap();
        std::fs::write(dir.join("broken.py"), "def f(:\n    pass\n").unwrap();
        let sh = shard(folder, &["render()"]);
        let row = serde_json::json!({"shard_note": {"assumes": []}, "pieces": ["render.js", "util.rs", "broken.py"]});
        let v = verify_shard(&root, &sh, &row).await;
        assert_eq!(
            v.pieces,
            vec![
                "broken.py".to_string(),
                "render.js".to_string(),
                "util.rs".to_string()
            ]
        );
        assert_eq!(v.unchecked, vec![("rs".to_string(), "util.rs".to_string())]);
        assert_eq!(v.unscanned, vec!["util.rs".to_string()]);
        assert_eq!(v.undefined_refs, vec!["drawBrush".to_string()], "{v:?}");
        let node_present = v.parsed.iter().any(|p| p == "render.js");
        let python_present = v.pieces_unparsed.iter().any(|(p, _)| p == "broken.py");
        if node_present {
            assert!(!v.pieces_unparsed.iter().any(|(p, _)| p == "render.js"));
        } else {
            eprintln!(
                "node absent on this machine — the parsed arm for render.js is unproven here"
            );
            assert!(v
                .checks_unavailable
                .iter()
                .any(|(t, p)| t == "node" && p == "render.js"));
        }
        if python_present {
            let (_, err) = v
                .pieces_unparsed
                .iter()
                .find(|(p, _)| p == "broken.py")
                .unwrap();
            assert!(err.contains("SyntaxError"), "{err}");
        } else {
            eprintln!(
                "python3 absent on this machine — the unparsed arm for broken.py is unproven here"
            );
            assert!(v
                .checks_unavailable
                .iter()
                .any(|(t, p)| t == "python3" && p == "broken.py"));
        }
        let events = v.events(&sh, "web-viz-render");
        let names: Vec<&str> = events
            .iter()
            .map(|e| e["event"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"shard_undefined_ref"), "{names:?}");
        let undefined = events
            .iter()
            .find(|e| e["event"] == "shard_undefined_ref")
            .unwrap();
        assert_eq!(undefined["names"], serde_json::json!(["drawBrush"]));
        assert_eq!(undefined["module"], "web-viz");
        assert_eq!(undefined["shard"], "render");
        assert_eq!(undefined["task_id"], "web-viz-render");
        let rs_unchecked = events
            .iter()
            .find(|e| e["event"] == "shard_check_unavailable" && e["ext"] == "rs")
            .expect("the .rs piece has no per-file parser — said");
        assert!(rs_unchecked["tool"].is_null());
        assert_eq!(rs_unchecked["pieces"], serde_json::json!(["util.rs"]));
        if python_present {
            let unparsed = events
                .iter()
                .find(|e| e["event"] == "shard_piece_unparsed")
                .unwrap();
            assert_eq!(unparsed["piece"], "broken.py");
        }
        let mut row = row;
        merge_into_row(&mut row, &v);
        assert_eq!(
            row["verify"]["undefined_refs"],
            serde_json::json!(["drawBrush"])
        );
        assert_eq!(row["verify"]["unchecked"][0]["ext"], "rs");
        assert_eq!(
            row["pieces"].as_array().unwrap().len(),
            3,
            "the note row's own keys stay"
        );
    }

    /// `node` absent is `shard_check_unavailable{tool}` — never a parse verdict. Proven by
    /// pointing the checker at a binary that does not exist, so PATH is never touched.
    #[tokio::test]
    async fn an_absent_node_is_said_as_check_unavailable_never_green() {
        let root = tmp("no-node");
        let folder = ".swarm/shards/web-viz/pick";
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pick.js"),
            "export function pick() { return 1; }\n",
        )
        .unwrap();
        let tools = ToolNames {
            node: "goose-no-such-node-binary-7f3a".to_string(),
            python: "goose-no-such-python-binary-7f3a".to_string(),
        };
        let sh = shard(folder, &["pick()"]);
        let v = verify_shard_with(&root, &sh, &serde_json::json!({}), &tools).await;
        assert!(v.parsed.is_empty(), "nothing looked at the file: {v:?}");
        assert!(v.pieces_unparsed.is_empty());
        assert_eq!(
            v.checks_unavailable,
            vec![(
                "goose-no-such-node-binary-7f3a".to_string(),
                "pick.js".to_string()
            )]
        );
        let events = v.events(&sh, "web-viz-pick");
        let ev = events
            .iter()
            .find(|e| e["event"] == "shard_check_unavailable")
            .expect("the absent tool is an event");
        assert_eq!(ev["tool"], "goose-no-such-node-binary-7f3a");
        assert_eq!(ev["check"], "parse");
        assert_eq!(ev["pieces"], serde_json::json!(["pick.js"]));
        assert!(
            !events.iter().any(|e| e["event"] == "shard_undefined_ref"),
            "the scan still ran and found nothing undefined: {events:?}"
        );
        // Direct: the tri-state, not a string.
        match check_piece_with(&dir.join("pick.js"), &tools).await {
            PieceCheck::ToolUnavailable { tool, .. } => assert_eq!(tool, tools.node),
            other => panic!("expected ToolUnavailable, got {other:?}"),
        }
        assert_eq!(
            check_piece_with(&dir.join("x.rs"), &tools).await,
            PieceCheck::NoChecker {
                ext: "rs".to_string()
            }
        );
    }

    /// The row → merger seam: `ledger_verify_row` reads back exactly what `merge_into_row` put in
    /// the ledger file `write_task_ledger` names; no row, or a row without the key, is None.
    #[test]
    fn the_verify_object_round_trips_through_the_ledger_row() {
        let root = tmp("row");
        let ledger = root.join(super::super::LEDGER_DIR);
        std::fs::create_dir_all(&ledger).unwrap();
        let v = ShardVerification {
            undefined_refs: vec!["drawBrush".into()],
            pieces_unparsed: vec![("a.js".into(), "SyntaxError: x".into())],
            ..Default::default()
        };
        let mut row = serde_json::json!({"task_id": "web-viz-render"});
        merge_into_row(&mut row, &v);
        std::fs::write(
            ledger.join(format!(
                "{}.json",
                super::super::activity_digest_key("web-viz-render")
            )),
            row.to_string(),
        )
        .unwrap();
        let back = ledger_verify_row(&root, "web-viz-render").expect("the key is there");
        assert_eq!(back["undefined_refs"], serde_json::json!(["drawBrush"]));
        assert_eq!(back["pieces_unparsed"][0]["piece"], "a.js");
        assert!(ledger_verify_row(&root, "never-written").is_none());
        std::fs::write(
            ledger.join(format!(
                "{}.json",
                super::super::activity_digest_key("plain")
            )),
            serde_json::json!({"task_id": "plain"}).to_string(),
        )
        .unwrap();
        assert!(
            ledger_verify_row(&root, "plain").is_none(),
            "no key → nothing to say"
        );
    }
}
