//! THE WEB-RUNTIME REFERENCE SCAN: defined-vs-referenced identifiers for browser JS, the check
//! `node --check` structurally cannot be.
//!
//! WHY (r5, reader 4, the single most score-costly finding): `web/viz.js` shipped spec-literate,
//! 46.7 KB, "Syntax verified (`node --check` → SYNTAX_OK)", ledger `delivery_defects: []` — and it
//! HAD NEVER EXECUTED. Line 1124 registered `onBrushChangeTracked` in `addEventListener` while the
//! only handler defined was `onBrushChange` (line 887); under the IIFE's `"use strict"` boot()
//! died at that line before `loadRecords()`, so four of the five graded mechanisms were dead on
//! arrival. A syntax check cannot see a runtime ReferenceError. The worker even SAW the
//! duplication mid-write and resolved it the wrong way round.
//!
//! MILD BY CONSTRUCTION: every finding becomes one delivery-defect FACT line ("references `X`
//! which is not defined in this file or its script-tag siblings") riding the existing
//! `verify_owned_files` seam — the judge prompt, the defect steer, the completion event, the
//! ledger row, the roll-up. Measured, never blocking; nothing here can end a task or bound work.
//!
//! FALSE POSITIVES ARE THE REAL DANGER (same law as the import scan): a wrong line costs a model
//! turn arguing with a measurement. So the scan checks only two reference positions where a bare
//! identifier MUST resolve at runtime — call position (`name(...)` not preceded by `.`) and bare
//! argument position (`f(a, name)` / `addEventListener("x", name)`) — and the DECLARED side is a
//! deliberate over-approximation (params, destructuring targets, assignment targets, method
//! shorthand names all count as defined). Every ambiguity resolves toward silence.

use std::collections::HashSet;
use std::path::Path;

/// Browser and JS globals a bare identifier may legitimately resolve to. Honest and extensible:
/// every name here is a REAL global in a browser page context (window properties included, since
/// `open(...)` and `addEventListener(...)` do resolve bare). `gl` is NOT here on purpose — it is
/// a `var gl` declaration in every real WebGL file, and listing it would hide a genuinely
/// undeclared one.
const BROWSER_GLOBALS: &[&str] = &[
    // document/window and friends
    "document",
    "window",
    "self",
    "globalThis",
    "navigator",
    "location",
    "history",
    "screen",
    "console",
    "parent",
    "top",
    "opener",
    "origin",
    "isSecureContext",
    "devicePixelRatio",
    "innerWidth",
    "innerHeight",
    "outerWidth",
    "outerHeight",
    "scrollX",
    "scrollY",
    // bare window methods
    "alert",
    "confirm",
    "prompt",
    "fetch",
    "open",
    "close",
    "focus",
    "blur",
    "print",
    "postMessage",
    "addEventListener",
    "removeEventListener",
    "dispatchEvent",
    "getSelection",
    "getComputedStyle",
    "matchMedia",
    "scroll",
    "scrollTo",
    "scrollBy",
    "requestAnimationFrame",
    "cancelAnimationFrame",
    "requestIdleCallback",
    "cancelIdleCallback",
    "setTimeout",
    "clearTimeout",
    "setInterval",
    "clearInterval",
    "queueMicrotask",
    "structuredClone",
    "createImageBitmap",
    "atob",
    "btoa",
    // storage and platform objects
    "localStorage",
    "sessionStorage",
    "indexedDB",
    "caches",
    "crypto",
    "performance",
    // constructors a page reaches bare
    "XMLHttpRequest",
    "WebSocket",
    "EventSource",
    "AbortController",
    "Headers",
    "Request",
    "Response",
    "FormData",
    "Blob",
    "File",
    "FileReader",
    "URL",
    "URLSearchParams",
    "TextEncoder",
    "TextDecoder",
    "DOMParser",
    "Image",
    "Audio",
    "Option",
    "Worker",
    "SharedWorker",
    "BroadcastChannel",
    "MessageChannel",
    "Notification",
    "CustomEvent",
    "Event",
    "EventTarget",
    "KeyboardEvent",
    "MouseEvent",
    "WheelEvent",
    "PointerEvent",
    "TouchEvent",
    "DragEvent",
    "FocusEvent",
    "InputEvent",
    "MessageEvent",
    "CloseEvent",
    "ErrorEvent",
    "ProgressEvent",
    "StorageEvent",
    "PopStateEvent",
    "HashChangeEvent",
    "ResizeObserver",
    "MutationObserver",
    "IntersectionObserver",
    "PerformanceObserver",
    "Node",
    "Element",
    "HTMLElement",
    "HTMLCanvasElement",
    "HTMLInputElement",
    "SVGElement",
    "DocumentFragment",
    "Range",
    "DOMRect",
    "DOMMatrix",
    "Path2D",
    "ImageData",
    "ImageBitmap",
    "OffscreenCanvas",
    "CanvasRenderingContext2D",
    "WebGLRenderingContext",
    "WebGL2RenderingContext",
    "AudioContext",
    "DataTransfer",
    // JS builtins
    "Object",
    "Function",
    "Array",
    "String",
    "Number",
    "Boolean",
    "Symbol",
    "BigInt",
    "Math",
    "JSON",
    "Date",
    "RegExp",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
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
    "Infinity",
    "NaN",
    "undefined",
    "eval",
    "isNaN",
    "isFinite",
    "parseInt",
    "parseFloat",
    "encodeURI",
    "decodeURI",
    "encodeURIComponent",
    "decodeURIComponent",
    "escape",
    "unescape",
    "arguments",
];

const KEYWORDS: &[&str] = &[
    "var",
    "let",
    "const",
    "function",
    "class",
    "return",
    "if",
    "else",
    "for",
    "while",
    "do",
    "switch",
    "case",
    "default",
    "break",
    "continue",
    "new",
    "delete",
    "typeof",
    "instanceof",
    "in",
    "of",
    "this",
    "true",
    "false",
    "null",
    "void",
    "throw",
    "try",
    "catch",
    "finally",
    "yield",
    "await",
    "async",
    "static",
    "get",
    "set",
    "import",
    "export",
    "extends",
    "super",
    "debugger",
    "with",
];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String, u32),
    Punct(&'static str),
    /// A numeric literal, whole. It exists ONLY so `regex_can_follow` can say no after one —
    /// see the lexer's doc. Its value is never read.
    Num,
}

/// A minimal JS lexer: identifiers and the punctuation this scan reasons about, with comments,
/// strings, template literals and regex literals skipped. The division-vs-regex ambiguity is
/// resolved with the standard previous-token heuristic; a template literal is skipped whole,
/// interpolations included — a reference missed inside `${...}` is a safe false negative.
///
/// A NUMBER IS ONE TOKEN, AND A REGEX MAY NOT FOLLOW IT (r6c web-viz, look 16). Digits used to
/// fall through to the punctuation arm as `Punct("op")` each, so `regex_can_follow` said yes after
/// one and `const dOff = (D0 - 1) / 2` opened a PHANTOM regex that ran to the next `/` in the
/// file — which is normally the first slash of a `//` comment, leaving the second slash to open
/// another. MEASURED on the delivered web/viz.js: that chain swallowed lines 496-654 whole,
/// deleting `function ensureSized` (499), `updateLabels` (558), `uploadSlotFloat` (599),
/// `clearBrush` (622) and `buildScene` (636) from the DECLARED side while their call sites
/// survived — and the judge was handed six "runtime ReferenceError" defects that were all false.
/// The lane then spent a tool call (19:26:24Z, `sed -n '108,118p' web/viz.js; ...`) reading its
/// own lines to refute them. A false line costs a model turn arguing with a measurement, which is
/// the one thing this scan's own module doc says it must never do.
fn lex(body: &str) -> Vec<Tok> {
    let mut toks: Vec<Tok> = Vec::new();
    let b: Vec<char> = body.chars().collect();
    let mut i = 0usize;
    let mut line = 1u32;
    let regex_can_follow = |toks: &[Tok]| -> bool {
        match toks.last() {
            None => true,
            Some(Tok::Punct(p)) => !matches!(*p, ")" | "]"),
            Some(Tok::Ident(w, _)) => KEYWORDS.contains(&w.as_str()),
            // `1 / 2` is division. Nothing else in this lexer can tell it from a regex.
            Some(Tok::Num) => false,
        }
    };
    while i < b.len() {
        let c = b[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < b.len() && b[i + 1] == '/' {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < b.len() && b[i + 1] == '*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                if b[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i = (i + 2).min(b.len());
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            while i < b.len() && b[i] != quote {
                if b[i] == '\\' {
                    i += 1;
                }
                if i < b.len() && b[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if c == '`' {
            i += 1;
            while i < b.len() && b[i] != '`' {
                if b[i] == '\\' {
                    i += 1;
                }
                if i < b.len() && b[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if c == '/' && regex_can_follow(&toks) {
            i += 1;
            let mut in_class = false;
            while i < b.len() && (b[i] != '/' || in_class) {
                match b[i] {
                    '\\' => i += 1,
                    '[' => in_class = true,
                    ']' => in_class = false,
                    '\n' => line += 1,
                    _ => {}
                }
                i += 1;
            }
            i += 1;
            // THE FLAGS BELONG TO THE LITERAL. Without this the `g` of
            // `String(...).replace(/\B(?=(\d{3})+(?!\d))/g, ',')` was lexed as a bare identifier
            // sitting between `(` and `,` — bare-argument position — and shipped to the judge as
            // "web/viz.js references `g` (line 113) ... a runtime ReferenceError" (r6c web-viz,
            // look 16). A regex literal is never followed directly by an identifier character in
            // valid JS, so consuming the run is unambiguous.
            while i < b.len() && b[i].is_ascii_alphabetic() {
                i += 1;
            }
            continue;
        }
        if c.is_ascii_digit() {
            i += 1;
            while i < b.len() {
                let d = b[i];
                if (d == 'e' || d == 'E')
                    && b.get(i + 1).is_some_and(|n| *n == '+' || *n == '-')
                    && b.get(i + 2).is_some_and(char::is_ascii_digit)
                {
                    i += 2; // the exponent's sign, then its digits on the next turns
                } else if d.is_ascii_alphanumeric() || d == '_' {
                    i += 1; // 0x1f, 1n, 1e3, 1_000 — every suffix form, none of them read
                } else if d == '.' && b.get(i + 1).is_some_and(char::is_ascii_digit) {
                    i += 1; // 0.5, but never the `.` of `(0.5).toFixed(2)`
                } else {
                    break;
                }
            }
            toks.push(Tok::Num);
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_' || b[i] == '$') {
                i += 1;
            }
            toks.push(Tok::Ident(b[start..i].iter().collect(), line));
            continue;
        }
        if c == '=' && i + 1 < b.len() && b[i + 1] == '>' {
            toks.push(Tok::Punct("=>"));
            i += 2;
            continue;
        }
        if (c == '=' || c == '!' || c == '<' || c == '>') && i + 1 < b.len() && b[i + 1] == '=' {
            toks.push(Tok::Punct("cmp"));
            i += 2;
            // swallow a third '=' (=== / !==)
            if i < b.len() && b[i] == '=' {
                i += 1;
            }
            continue;
        }
        let p: &'static str = match c {
            '(' => "(",
            ')' => ")",
            '{' => "{",
            '}' => "}",
            '[' => "[",
            ']' => "]",
            ';' => ";",
            ',' => ",",
            '.' => ".",
            ':' => ":",
            '=' => "=",
            _ => "op",
        };
        toks.push(Tok::Punct(p));
        i += 1;
    }
    toks
}

fn is_word(t: Option<&Tok>, w: &str) -> bool {
    matches!(t, Some(Tok::Ident(n, _)) if n == w)
}

/// The DECLARED side: every name this file (or a sibling) binds. Deliberately generous — see the
/// module doc. One pass over the token stream:
///   * `function NAME`, `class NAME`, and every identifier inside a `function`/`catch`/arrow
///     param group (default values included — over-approximation toward silence);
///   * `var`/`let`/`const` declarator lists up to `=`/`;` (comma lists, `{..}`/`[..]` patterns);
///   * any identifier directly followed by `=` (assignment target) or `=>` (bare arrow param);
///   * a method-shorthand name (identifier whose paren group closes into `{`).
fn declared_names(toks: &[Tok]) -> HashSet<String> {
    let mut declared: HashSet<String> = HashSet::new();
    // paren-group stack: (is_param_group, names seen inside)
    let mut groups: Vec<(bool, Vec<String>)> = Vec::new();
    let mut in_decl_list = false;
    let mut decl_depth = 0usize;
    for (idx, t) in toks.iter().enumerate() {
        match t {
            Tok::Ident(name, _) => {
                if KEYWORDS.contains(&name.as_str()) {
                    if matches!(name.as_str(), "var" | "let" | "const") {
                        in_decl_list = true;
                        decl_depth = groups.len();
                    }
                    continue;
                }
                if is_word(toks.get(idx.wrapping_sub(1)), "function")
                    || is_word(toks.get(idx.wrapping_sub(1)), "class")
                {
                    declared.insert(name.clone());
                }
                if in_decl_list {
                    declared.insert(name.clone());
                }
                // Collected unconditionally; whether the group was a param list is only knowable
                // at its close (`) =>` and `) {` shapes), so the decision lives there.
                if let Some((_, names)) = groups.last_mut() {
                    names.push(name.clone());
                }
                match toks.get(idx + 1) {
                    Some(Tok::Punct("=")) | Some(Tok::Punct("=>")) => {
                        declared.insert(name.clone());
                    }
                    Some(Tok::Punct("(")) => {
                        // Method shorthand / function-ish definition: the group closes into `{`.
                        if let Some(close) = matching_close(toks, idx + 1) {
                            if matches!(toks.get(close + 1), Some(Tok::Punct("{"))) {
                                declared.insert(name.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Tok::Punct("(") => {
                let prev = toks.get(idx.wrapping_sub(1));
                let prev2 = toks.get(idx.wrapping_sub(2));
                let is_param = is_word(prev, "function")
                    || is_word(prev, "catch")
                    || (matches!(prev, Some(Tok::Ident(_, _))) && is_word(prev2, "function"));
                groups.push((is_param, Vec::new()));
            }
            Tok::Punct(")") => {
                if let Some((is_param, names)) = groups.pop() {
                    let arrow_next = matches!(toks.get(idx + 1), Some(Tok::Punct("=>")));
                    // `) {` also declares its group: method shorthand (`toggle(id) {`) and any
                    // function-ish definition. It over-reaches to `if (x) {` and friends — a
                    // condition's identifiers becoming "declared" is a false NEGATIVE, the safe
                    // direction this whole scan resolves ambiguity toward.
                    let brace_next = matches!(toks.get(idx + 1), Some(Tok::Punct("{")));
                    if is_param || arrow_next || brace_next {
                        declared.extend(names);
                    }
                }
                if in_decl_list && groups.len() < decl_depth {
                    in_decl_list = false;
                }
            }
            Tok::Punct("=") | Tok::Punct(";") => {
                if in_decl_list && groups.len() == decl_depth {
                    in_decl_list = false;
                }
            }
            _ => {}
        }
    }
    declared
}

/// Index of the `)` matching the `(` at `open`, if the stream is balanced.
fn matching_close(toks: &[Tok], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (j, t) in toks.iter().enumerate().skip(open) {
        match t {
            Tok::Punct("(") => depth += 1,
            Tok::Punct(")") => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

/// The REFERENCED side: bare identifiers in the two positions where an unresolved name is a
/// runtime ReferenceError — call position and bare-argument position. Everything ambiguous is
/// skipped (see module doc).
fn referenced_names(toks: &[Tok]) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    for (idx, t) in toks.iter().enumerate() {
        let Tok::Ident(name, ln) = t else { continue };
        if KEYWORDS.contains(&name.as_str()) {
            continue;
        }
        let prev = toks.get(idx.wrapping_sub(1));
        if matches!(prev, Some(Tok::Punct("."))) {
            continue;
        }
        let next = toks.get(idx + 1);
        // Call position: `name(...)` whose group does NOT close into `{` (that shape is a
        // method-shorthand definition, counted as declared instead). Skip when the previous
        // token is an identifier (`get foo()`, `async foo()`, `function foo()` handled above).
        if matches!(next, Some(Tok::Punct("("))) {
            if matches!(prev, Some(Tok::Ident(_, _))) {
                continue;
            }
            if let Some(close) = matching_close(toks, idx + 1) {
                if matches!(toks.get(close + 1), Some(Tok::Punct("{"))) {
                    continue;
                }
            }
            out.push((name.clone(), *ln));
            continue;
        }
        // Bare argument position: `(name)` `, name)` `(name,` `, name,` — the
        // addEventListener-callback shape that shipped r5's boot-killer.
        let prev_arg = matches!(prev, Some(Tok::Punct("(")) | Some(Tok::Punct(",")));
        let next_arg = matches!(next, Some(Tok::Punct(")")) | Some(Tok::Punct(",")));
        if prev_arg && next_arg {
            out.push((name.clone(), *ln));
        }
    }
    out
}

/// The sibling set a browser file's references may legitimately resolve into: the OTHER scripts
/// an html in the same directory loads beside it (script-tag siblings), else every other .js in
/// the directory when no html names it yet — mid-run, viz.js completed eleven minutes before
/// index.html existed, and brush.js's `vs7dbg` surface was already its legitimate resolver.
fn sibling_declared(working_dir: &Path, rel: &str) -> HashSet<String> {
    let path = working_dir.join(rel);
    let Some(dir) = path.parent() else {
        return HashSet::new();
    };
    let Some(base) = path.file_name().and_then(|n| n.to_str()) else {
        return HashSet::new();
    };
    let mut sibs: Vec<std::path::PathBuf> = Vec::new();
    let mut from_html = false;
    if let Ok(rd) = std::fs::read_dir(dir) {
        let entries: Vec<_> = rd.flatten().collect();
        for e in &entries {
            let name = e.file_name();
            let name = name.to_string_lossy().to_string();
            if !name.ends_with(".html") {
                continue;
            }
            let Ok(html) = std::fs::read_to_string(e.path()) else {
                continue;
            };
            let srcs: Vec<String> = html
                .split("src=")
                .skip(1)
                .filter_map(|s| {
                    let q = s.chars().next()?;
                    if q != '"' && q != '\'' {
                        return None;
                    }
                    s.split(q).nth(1).map(String::from)
                })
                .filter(|s| s.ends_with(".js") && !s.starts_with("http"))
                .collect();
            if srcs.iter().any(|s| s.rsplit('/').next() == Some(base)) {
                from_html = true;
                sibs = srcs
                    .iter()
                    .filter(|s| s.rsplit('/').next() != Some(base))
                    .map(|s| dir.join(s))
                    .collect();
                break;
            }
        }
        if !from_html {
            sibs = entries
                .iter()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().and_then(|x| x.to_str()) == Some("js")
                        && p.file_name().and_then(|n| n.to_str()) != Some(base)
                })
                .collect();
        }
    }
    let mut declared = HashSet::new();
    for s in sibs {
        if let Ok(body) = std::fs::read_to_string(&s) {
            declared.extend(declared_names(&lex(&body)));
        }
    }
    declared
}

/// Is this a browser-runtime JS file this scan should judge? An html sibling in its directory, or
/// a conventional web-root first path segment. Node-flavored files (CommonJS markers) are skipped
/// entirely — this scan only knows the browser's global surface.
fn is_browser_js(working_dir: &Path, rel: &str, body: &str) -> bool {
    if body.contains("module.exports") || body.contains("require(") {
        return false;
    }
    let first_seg = rel.split('/').next().unwrap_or("");
    if matches!(first_seg, "web" | "static" | "public" | "frontend") {
        return true;
    }
    working_dir
        .join(rel)
        .parent()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| {
            rd.flatten()
                .any(|e| e.file_name().to_string_lossy().ends_with(".html"))
        })
        .unwrap_or(false)
}

/// One fact line per unresolved referenced identifier in a browser JS file, empty when clean.
/// Rides `verify_owned_files`' return — the same seam as every other delivery defect.
pub(in crate::commands::swarm) fn browser_js_undefined_refs(
    working_dir: &Path,
    rel: &str,
) -> Vec<String> {
    let Ok(body) = std::fs::read_to_string(working_dir.join(rel)) else {
        // Absence is already a distinct defect from the existence check; nothing to scan IS
        // nothing to report here.
        return Vec::new();
    };
    if !is_browser_js(working_dir, rel, &body) {
        return Vec::new();
    }
    let toks = lex(&body);
    let mut declared = declared_names(&toks);
    declared.extend(sibling_declared(working_dir, rel));
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for (name, ln) in referenced_names(&toks) {
        if declared.contains(&name)
            || BROWSER_GLOBALS.contains(&name.as_str())
            || !seen.insert(name.clone())
        {
            continue;
        }
        out.push(format!(
            "{rel} references `{name}` (line {ln}), which is not defined in this file or its \
             script-tag siblings — a runtime ReferenceError `node --check` cannot see"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "goose-webrefs-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// THE r5 KILLER, VERBATIM SHAPE: viz.js:887 defines `onBrushChange`, viz.js:1124 registers
    /// `onBrushChangeTracked` — the single defect that left the graded 3D field dead on arrival
    /// behind a green `node --check` and `delivery_defects: []`.
    #[test]
    fn the_viz_boot_killer_is_one_fact_line() {
        let dir = tmp("viz");
        std::fs::create_dir_all(dir.join("web")).unwrap();
        std::fs::write(
            dir.join("web/viz.js"),
            r#"(function () {
  "use strict";
  function vs7dbgRef() { return window.vs7dbg || {}; }
  function onBrushChange(ev) { render(ev); }
  function render(ev) { console.log(ev); }
  function boot() {
    document.addEventListener(vs7dbgRef().CHANGE_EVENT || "vs7dbg:brush-change", onBrushChangeTracked);
    render(null);
  }
  boot();
})();
"#,
        )
        .unwrap();
        let found = browser_js_undefined_refs(&dir, "web/viz.js");
        assert_eq!(found.len(), 1, "exactly the killer: {found:?}");
        assert!(found[0].contains("onBrushChangeTracked"), "{found:?}");
        assert!(found[0].contains("line 7"), "{found:?}");
        assert!(
            found[0].contains("not defined in this file or its script-tag siblings"),
            "{found:?}"
        );

        // The wrong-way-round resolution the worker shipped, inverted (registration fixed):
        // clean.
        std::fs::write(
            dir.join("web/viz.js"),
            r#"function onBrushChange(ev) { console.log(ev); }
document.addEventListener("vs7dbg:brush-change", onBrushChange);
"#,
        )
        .unwrap();
        assert!(
            browser_js_undefined_refs(&dir, "web/viz.js").is_empty(),
            "a registered handler that exists is silence"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// r6c web-viz, look 16 (19:00:48Z), DEFECT 1 OF 3 VERBATIM: the delivered web/viz.js line
    /// 113 is `String(Math.floor(minor / p)).replace(/\B(?=(\d{3})+(?!\d))/g, ',')`, and the scan
    /// shipped "web/viz.js references `g` (line 113), which is not defined ... a runtime
    /// ReferenceError". The regex literal was skipped but its FLAGS were not, so `g` was lexed as
    /// a bare identifier between `(` and `,` — bare-argument position, the addEventListener shape.
    #[test]
    fn a_regex_literals_flags_are_never_a_bare_identifier() {
        let dir = tmp("regexflags");
        std::fs::create_dir_all(dir.join("web")).unwrap();
        std::fs::write(
            dir.join("web/viz.js"),
            r#"(function () {
  "use strict";
  function expOf(cur) { return cur === 'JPY' ? 0 : 2; }
  function fmtMoney(minor, cur) { // integer-based: no float drift in display
    const e = expOf(cur);
    const p = Math.pow(10, e);
    let s = String(Math.floor(minor / p)).replace(/\B(?=(\d{3})+(?!\d))/g, ',');
    if (e > 0) s += '.' + String(minor % p).padStart(e, '0');
    return cur + ' ' + s;
  }
  document.addEventListener('DOMContentLoaded', function () { fmtMoney(1234567, 'USD'); });
})();
"#,
        )
        .unwrap();
        assert!(
            browser_js_undefined_refs(&dir, "web/viz.js").is_empty(),
            "a regex flag is part of the literal, not a reference: {:?}",
            browser_js_undefined_refs(&dir, "web/viz.js")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// r6c web-viz, look 16, DEFECTS 2-6: `ensureSized` (line 421), `updateLabels` (451),
    /// `uploadSlotFloat` (751), `buildScene` (762), `clearBrush` (897) reported undefined while
    /// every one of them is a plain `function NAME(...)` declaration in the same file (lines 499,
    /// 558, 599, 636, 622).
    ///
    /// THE MECHANISM WAS NOT HOISTING — the declared-side pass already reads the whole token
    /// stream, so position never mattered. It was the LEXER losing the declarations: digits fell
    /// through to the punctuation arm, so `regex_can_follow` said yes after a number and
    /// `Math.exp(-(now - coast.t0) / 1000 / TAU_S)` (viz.js:471) opened a phantom regex that ran
    /// to the first slash of the next `//` comment — whose second slash opened another, and so on
    /// until a real division closed the chain at viz.js:654. Lines 496-654 of the delivered file
    /// were swallowed whole, taking all five declarations with them while their call sites, past
    /// the chain's end, survived. This fixture is that cascade in miniature: the opener, a line
    /// comment, the declarations, the division that closes it, then the uses.
    #[test]
    fn a_division_after_a_number_does_not_swallow_the_declarations_that_follow() {
        let dir = tmp("phantomregex");
        std::fs::create_dir_all(dir.join("web")).unwrap();
        std::fs::write(
            dir.join("web/viz.js"),
            r#"(function () {
  "use strict";
  const TAU_S = 0.18, SPAN = 96;
  function stepCoast(now, coast) {
    const ey = Math.exp(-(now - coast.t0) / 1000 / TAU_S);
    return ey;
  }
  // Demand rendering: draw only when dirty or coasting — 0 draws at rest.
  function ensureSized() { return true; }
  function updateLabels() { return true; }
  const half = SPAN / 2;
  function boot() {
    stepCoast(0, { t0: half });
    ensureSized();
    updateLabels();
    window.addEventListener('resize', updateLabels);
  }
  boot();
})();
"#,
        )
        .unwrap();
        assert!(
            browser_js_undefined_refs(&dir, "web/viz.js").is_empty(),
            "declarations after a division must survive the lexer: {:?}",
            browser_js_undefined_refs(&dir, "web/viz.js")
        );
        // And the scan still SEES a real one in the same shape — the fix widened nothing.
        std::fs::write(
            dir.join("web/viz.js"),
            r#"(function () {
  "use strict";
  const SPAN = 96;
  const half = SPAN / 2;
  function updateLabels() { return half; }
  window.addEventListener('resize', updateLabelsTracked);
})();
"#,
        )
        .unwrap();
        let found = browser_js_undefined_refs(&dir, "web/viz.js");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("updateLabelsTracked"), "{found:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The brief's other suspect, pinned so it stays covered: a `function NAME` declared AFTER its
    /// call site is DEFINED (JS hoists it, and `declared_names` is a whole-file pass, so position
    /// never decided). It was never the r6c cause — the lexer was — but a future lexer change that
    /// made the declared side positional would resurface the same six false defects.
    #[test]
    fn a_function_declared_after_its_call_site_is_defined() {
        let dir = tmp("hoisting");
        std::fs::create_dir_all(dir.join("web")).unwrap();
        std::fs::write(
            dir.join("web/viz.js"),
            r#"(function () {
  "use strict";
  boot();
  function boot() { render(); }
  function render() { console.log('drawn'); }
})();
"#,
        )
        .unwrap();
        assert!(
            browser_js_undefined_refs(&dir, "web/viz.js").is_empty(),
            "hoisted declarations are defined at their call site: {:?}",
            browser_js_undefined_refs(&dir, "web/viz.js")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// FALSE-POSITIVE SUITE: the shapes generated browser code actually writes must all stay
    /// silent — params, arrow params, destructuring, comma declarator lists, method shorthand,
    /// callbacks that ARE defined, browser globals, strings/comments/regex/templates.
    #[test]
    fn real_browser_shapes_stay_silent() {
        let dir = tmp("clean");
        std::fs::create_dir_all(dir.join("web")).unwrap();
        std::fs::write(
            dir.join("web/app.js"),
            r#"(function () {
  "use strict";
  var PAGE_SIZE = 50, offset = 0;
  const { limit, total } = { limit: 50, total: 0 };
  let [first, second] = [1, 2];
  function fmt(amount, currency) { return currency + amount; }
  var draw = function inner(gl) { gl.clear(0); };
  const go = (a, b) => fmt(a, b);
  const single = x => x + offset;
  var api = {
    toggle(id) { return id; },
    clear: function () { return null; },
  };
  function onTick(ev) {
    // a comment mentioning ghostFn( should not count
    var re = /ghostRegex\(/;
    var s = "ghostString(nope)";
    var t = 'a template mentioning ghostTemplate(x)';
    fmt(limit, total);
    go(first, second);
    single(offset);
    api.toggle(ev);
    draw({ clear: function (n) { return n; } });
    return re.test(s) && t;
  }
  document.addEventListener("tick", onTick);
  setTimeout(onTick, 100);
  requestAnimationFrame(onTick);
  fetch("/api/payments").then(function (r) { return r.json(); });
  try { onTick(null); } catch (err) { console.error(err); }
  for (var i = 0, n = 3; i < n; i++) { single(i); }
  window.vs7dbg = api;
})();
"#,
        )
        .unwrap();
        let found = browser_js_undefined_refs(&dir, "web/app.js");
        assert!(found.is_empty(), "every shape here is defined: {found:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SCRIPT-TAG SIBLINGS RESOLVE: viz.js consuming brush.js's surface is the r5 contract
    /// (`vs7dbg`), and the sibling set comes from the html's own script tags when one names the
    /// file — the same reference chain `verify_owned_files` already walks for html.
    #[test]
    fn a_sibling_defined_name_resolves_and_a_missing_one_does_not() {
        let dir = tmp("sib");
        std::fs::create_dir_all(dir.join("web")).unwrap();
        std::fs::write(
            dir.join("web/index.html"),
            "<script src=\"brush.js\"></script><script src=\"viz.js\"></script>",
        )
        .unwrap();
        std::fs::write(
            dir.join("web/brush.js"),
            "function installBrush() { return 1; }\nwindow.vs7dbg = { brush: installBrush };\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("web/viz.js"),
            "document.addEventListener(\"x\", installBrush);\nsetTimeout(missingHandler, 5);\n",
        )
        .unwrap();
        let found = browser_js_undefined_refs(&dir, "web/viz.js");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("missingHandler"), "{found:?}");
        assert!(
            !found.iter().any(|f| f.contains("installBrush")),
            "a script-tag sibling's definition resolves: {found:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Node-flavored files and non-web files are none of this scan's business: it knows only the
    /// browser's global surface, and judging a node script against it would cry wolf.
    #[test]
    fn node_files_and_non_web_dirs_are_skipped() {
        let dir = tmp("node");
        std::fs::create_dir_all(dir.join("web")).unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(
            dir.join("web/tool.js"),
            "const fs = require(\"fs\");\nmodule.exports = { run: run };\nfunction run() { ghost(); }\n",
        )
        .unwrap();
        assert!(
            browser_js_undefined_refs(&dir, "web/tool.js").is_empty(),
            "CommonJS markers mean not-browser, whatever the directory"
        );
        std::fs::write(dir.join("scripts/x.js"), "setTimeout(ghostFn, 1);\n").unwrap();
        assert!(
            browser_js_undefined_refs(&dir, "scripts/x.js").is_empty(),
            "no html sibling and no web-root segment: not judged"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
