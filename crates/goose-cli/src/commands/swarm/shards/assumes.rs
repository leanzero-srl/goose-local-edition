//! ASSUMES RESOLVED AGAINST THE SIBLINGS, PER NAME (VA-108) — the dossier's `assumptions_unmet`
//! rule, extended from "did any candidate meet a definition" to "which NAME does no sibling
//! define, and what is the nearest name a sibling DOES define".
//!
//! MEASURED (r6h, module `viz-engine`, 07:11): two shards done and verified `undefined_refs: []`
//! each. camera-labels-brush's README ASSUMES "GL context variable is named gl (module scope,
//! written by initViz)" and "sets a per-frame uniform uBrushActive = brushSet.size > 0"; its
//! `brush.js` uses a bare `gl` (typeof-guarded at :56, so nothing throws). data-stream-render-pick
//! WRITES `vizGL` (`render-core.js:10 let vizGL = null`), aliases `const gl = vizGL` INSIDE each
//! function, and names the uniform `uBrush` (a GLSL string literal at :46, `getUniformLocation(
//! mainProg, 'uBrush')` at :167). After assembly there is no module-scope `gl`, `brushFlagGL` is
//! never created and brushed instances never dim — a graded feature, no exception, nothing in
//! run.jsonl. The old rule (`candidate_names`: backticked spans, else tokens carrying `(` or `.`)
//! never made `gl` or `uBrushActive` a candidate at all, while it flagged data-stream's "boot wires
//! `new EventSource(…)`" (candidate `new`, a keyword) and debug-api's "assembly places the facade
//! before Boot" (candidate `window.vs7dbg`, a global's member) — two false statements handed to
//! the merger beside the one true one (`invalidatePick()`).
//!
//! THE IDENTIFIER RULE — a clause names a code-shaped identifier when the token is
//!   (a) a FREE REFERENCE of the shard's own pieces (`shard_verify::undefined_references` with no
//!       exemptions: used by its code, defined by none of its pieces, not a runtime global) — this
//!       is how a plain word like `gl` qualifies: the code leans on it; or
//!   (b) lexically code-shaped and at least 4 characters: a call (`invalidatePick()`), camelCase
//!       with a lowercase start and an inner capital followed by a lowercase letter
//!       (`uBrushActive`, not `rAF`), snake_case with a lowercase start (`amount_minor`, not
//!       `UNSIGNED_BYTE`), or a dotted path (`records.count`, any length); or
//!   (c) backticked (kept for notes that arrive un-stripped; the README parser strips backticks).
//! Skipped: JS keywords and runtime globals (`new`, `EventSource`, `Math`, `window`, `frames`), a
//! member with no head (`.onmessage`), file names (`streaming.js`, `index.html`), and any token
//! the shard's OWN non-comment lines carry that is NOT a free reference — its own definitions and
//! locals, API members it calls (`bufferSubData`, `round`), names inside its own string literals
//! (data-stream's `uBrush` uniform). A clause with no code-shaped token at all is PROSE, listed
//! once as `assumptions_prose`, never an error.
//!
//! RESOLUTION — a name is FOUND when some shard's piece DEFINES it (`extract_symbols`' one rule,
//! `same_symbol` for dotted names: `viz3d.toggleBrush` is met by `toggleBrush`, `brushSet.size` by
//! its head `brushSet`) or when its root is a declared shared-state root (the merger initialises
//! that itself). A README's PROVIDES/WRITES claim meets nothing (DESIGN-SPLIT-V2 §3: a promise
//! provides nothing — an unbacked PROVIDES is already a GAP). Module scope is what
//! `extract_symbols` reads for state — an indented `const gl = vizGL` is a local and never a
//! definition, so a "module-scope" ASSUMES resolves only against module-scope state.
//!
//! NEAREST — for an unbacked name, the closest sibling name from three pools in order: the
//! siblings' DEFINITIONS, then their PROVIDES/WRITES claims, then every identifier token in their
//! non-comment lines (where the GLSL `uBrush` lives — `extract_symbols` cannot see inside a string
//! literal, and no GLSL regex is added; the token list is the fact). Closeness by camelCase/snake
//! SEGMENTS: the name is the head or the tail of the candidate (`gl` → `vizGL`), or the candidate
//! is the head of the name (`uBrush` → `uBrushActive`; a bare tail like `pick` for
//! `invalidatePick` is NOT close), else edit distance ≤ 2 for names of 4+ characters. Nothing close
//! → `nearest: null`, never invented.
//!
//! WHAT THIS CANNOT SEE, said: an ASSUMES about who CALLS a name ("applyBatch calls
//! ensureBrushFlag()") — `ensureBrushFlag` is camera's own definition, so it is found; that
//! data-stream's `applyBatch` never calls it is a call-graph fact outside this scan. A trailing
//! comment on a code line counts as the shard's own vocabulary (line-level comment stripping).

use std::collections::BTreeSet;

use goose_swarm::ModuleInterface;

use super::super::shard_verify::{ident_tokens, is_runtime_global, undefined_references};
use super::super::TargetLang;
use super::assembly::{assembly_lang, is_comment_line};
use super::{
    ident_at, is_ident_char, is_ident_start, same_symbol, ShardDossier, ShardNote, SymbolKind,
    JS_KEYWORDS,
};

/// The closest sibling name to an unbacked one, with the pool it came from and the rule that
/// made it close — facts the merger can check, never a guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Nearest {
    pub(super) name: String,
    pub(super) shard: String,
    /// "state defined by" / "function defined by" / "PROVIDES of" / "WRITES of" / "a token in the
    /// pieces of" — read as `{source} \`{shard}\``.
    pub(super) source: String,
    pub(super) rule: String,
}

/// One identifier an ASSUMES clause names that no shard defines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssumeUnbacked {
    pub(super) shard: String,
    pub(super) name: String,
    pub(super) clause: String,
    pub(super) nearest: Option<Nearest>,
}

impl AssumeUnbacked {
    pub(super) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "nearest": self.nearest.as_ref().map(|n| serde_json::json!({
                "name": n.name,
                "shard": n.shard,
                "source": n.source,
                "rule": n.rule,
            })),
        })
    }

    /// The merger's glue line for this name — the two names, the two shards, the rule, and the
    /// decision left to the merger (MILD: code names the gap, the merger writes the glue).
    pub(super) fn glue(&self) -> String {
        match &self.nearest {
            Some(n) => format!(
                "`{name}` is defined by no shard; the nearest sibling name is `{near}` ({source} `{shard}`; {rule}) — alias `{name}` to `{near}` where `{near}` is written, or rename the `{name}` uses to `{near}`; you decide, say which under KEPT.",
                name = self.name,
                near = n.name,
                source = n.source,
                shard = n.shard,
                rule = n.rule,
            ),
            None => format!(
                "`{}` is defined by no shard and no sibling name is close — write it, or remove the use of it, and say which under FILLED.",
                self.name
            ),
        }
    }
}

/// What the resolution established over every shard's ASSUMES.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Resolution {
    /// One per (shard, name), the first clause naming it.
    pub(super) unbacked: Vec<AssumeUnbacked>,
    /// (shard, clause) — clauses with at least one unbacked name, in README order.
    pub(super) unmet: Vec<(String, String)>,
    /// (shard, clause) — clauses naming no code-shaped identifier at all.
    pub(super) prose: Vec<(String, String)>,
}

struct ShardNames<'a> {
    id: &'a str,
    note: Option<&'a ShardNote>,
    defs: Vec<(String, SymbolKind)>,
    claims: Vec<(String, &'static str)>,
    /// Every identifier in the shard's non-comment lines (string literals included).
    tokens: BTreeSet<String>,
    /// Referenced by its pieces, defined by none, not a runtime global.
    free: BTreeSet<String>,
}

impl<'a> ShardNames<'a> {
    fn of(sh: &'a ShardDossier, sources: &[(String, String)]) -> Self {
        let defs = sh
            .pieces
            .iter()
            .flat_map(|(_, _, syms)| syms.iter())
            .filter(|s| !s.shorthand)
            .map(|s| (s.name.clone(), s.kind))
            .collect();
        let mut claims: Vec<(String, &'static str)> = Vec::new();
        if let Some(n) = &sh.note {
            for p in &n.provides {
                if let Some(i) = ident_at(p) {
                    claims.push((i.trim_end_matches('.').to_string(), "PROVIDES of"));
                }
            }
            for w in &n.writes {
                if let Some(i) = ident_at(w) {
                    claims.push((i.trim_end_matches('.').to_string(), "WRITES of"));
                }
            }
        }
        let mut tokens = BTreeSet::new();
        for (name, src) in sources {
            let lang = std::path::Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .and_then(assembly_lang);
            for line in src.lines() {
                let t = line.trim_start();
                let comment = match lang {
                    Some(l) => is_comment_line(t, l),
                    None => is_comment_line(t, TargetLang::TypeScript) || t.starts_with('#'),
                };
                if !comment {
                    tokens.extend(ident_tokens(line));
                }
            }
        }
        let free: BTreeSet<String> =
            undefined_references(sources, &ModuleInterface::default(), &[])
                .undefined
                .into_iter()
                .collect();
        ShardNames {
            id: &sh.id,
            note: sh.note.as_ref(),
            defs,
            claims,
            tokens,
            free,
        }
    }
}

fn is_camel(w: &str) -> bool {
    let cs: Vec<char> = w.chars().collect();
    cs.first().is_some_and(|c| c.is_lowercase())
        && cs
            .windows(2)
            .any(|p| p[0].is_uppercase() && p[1].is_lowercase())
}

fn is_snake(w: &str) -> bool {
    w.chars().next().is_some_and(|c| c.is_lowercase()) && w.trim_matches('_').contains('_')
}

/// A dotted token whose tail is a source or asset extension names a FILE, not a symbol
/// (`streaming.js`, `index.html`) — the kinds `lang_for_path` and the split's static assets know.
fn is_file_name(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && matches!(
                ext,
                "js" | "mjs"
                    | "cjs"
                    | "ts"
                    | "tsx"
                    | "jsx"
                    | "py"
                    | "rs"
                    | "go"
                    | "html"
                    | "css"
                    | "md"
                    | "json"
                    | "txt"
                    | "svg"
                    | "yaml"
                    | "yml"
                    | "toml"
            )
    })
}

/// The code-shaped identifiers of one ASSUMES clause (module doc: the identifier rule), in order,
/// deduplicated, with the shard's own vocabulary removed. The bool says whether ANY code-shaped
/// token was present before that removal — false means the clause is prose.
pub(super) fn assumed_identifiers(
    clause: &str,
    free: &BTreeSet<String>,
    vocab: &BTreeSet<String>,
) -> (bool, Vec<String>) {
    let chars: Vec<char> = clause.chars().collect();
    let mut shaped_any = false;
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if !(is_ident_start(chars[i]) || chars[i] == '.') {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && (is_ident_char(chars[i]) || chars[i] == '.') {
            i += 1;
        }
        let run: String = chars[start..i].iter().collect();
        if run.starts_with('.') {
            continue;
        }
        let path = run.trim_end_matches('.');
        if path.is_empty() || is_file_name(path) {
            continue;
        }
        let head = path.split('.').next().unwrap_or(path);
        if path.chars().count() < 2 || JS_KEYWORDS.contains(&head) || is_runtime_global(head) {
            continue;
        }
        let backticked = start > 0 && chars[start - 1] == '`';
        let call = chars.get(i) == Some(&'(');
        let long = head.chars().count() >= 4;
        let shaped = backticked
            || path.contains('.')
            || (long && (call || is_camel(head) || is_snake(head)))
            || free.contains(head);
        if !shaped {
            continue;
        }
        shaped_any = true;
        if vocab.contains(head) && !free.contains(head) {
            continue;
        }
        if !out.iter().any(|o| o == path) {
            out.push(path.to_string());
        }
    }
    (shaped_any, out)
}

/// camelCase / snake_case / dotted segments, lowercased: `vizGL` → [viz, gl]; `uBrushActive` →
/// [u, brush, active]; `GLContext` → [gl, context].
fn segments_of(name: &str) -> Vec<String> {
    let cs: Vec<char> = name.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (k, &c) in cs.iter().enumerate() {
        if c == '_' || c == '.' || c == '$' {
            if !cur.is_empty() {
                out.push(cur.to_lowercase());
                cur.clear();
            }
            continue;
        }
        let boundary = !cur.is_empty()
            && c.is_uppercase()
            && (cs[k - 1].is_lowercase()
                || cs[k - 1].is_ascii_digit()
                || (cs[k - 1].is_uppercase() && cs.get(k + 1).is_some_and(|n| n.is_lowercase())));
        if boundary {
            out.push(cur.to_lowercase());
            cur.clear();
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur.to_lowercase());
    }
    out
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Some((rank, rule)) when `cand` is a plausible sibling spelling of `name`; lower rank is closer.
pub(super) fn closeness(name: &str, cand: &str) -> Option<(usize, String)> {
    if name == cand {
        return None;
    }
    let n = segments_of(name);
    let c = segments_of(cand);
    if n.is_empty()
        || c.is_empty()
        || n.concat().chars().count() < 2
        || c.concat().chars().count() < 2
    {
        return None;
    }
    if n == c {
        return Some((0, format!("`{cand}` is `{name}` in another case")));
    }
    if n.len() < c.len() {
        if c.starts_with(&n) {
            return Some((
                c.len() - n.len(),
                format!("`{name}` is the head of `{cand}`"),
            ));
        }
        if c.ends_with(&n) {
            return Some((
                c.len() - n.len(),
                format!("`{name}` is the tail of `{cand}`"),
            ));
        }
    } else if c.len() < n.len() && n.starts_with(&c) {
        return Some((
            n.len() - c.len(),
            format!("`{cand}` is the head of `{name}`"),
        ));
    }
    let (ln, lc) = (name.to_lowercase(), cand.to_lowercase());
    if ln.chars().count() >= 4 && lc.chars().count() >= 4 {
        let d = edit_distance(&ln, &lc);
        if d <= 2 {
            return Some((10 + d, format!("edit distance {d} from `{cand}`")));
        }
    }
    None
}

fn kind_word(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Function => "function",
        SymbolKind::Class => "class",
        SymbolKind::Constant => "constant",
        SymbolKind::State => "state",
        SymbolKind::Reference => "name",
    }
}

fn consider(
    best: &mut Option<(usize, Nearest)>,
    name: &str,
    cand: &str,
    shard: &str,
    source: &str,
) {
    let Some((rank, rule)) = closeness(name, cand) else {
        return;
    };
    let better = best
        .as_ref()
        .is_none_or(|(r, b)| rank < *r || (rank == *r && cand < b.name.as_str()));
    if better {
        *best = Some((
            rank,
            Nearest {
                name: cand.to_string(),
                shard: shard.to_string(),
                source: source.to_string(),
                rule,
            },
        ));
    }
}

fn nearest(name: &str, me: &str, all: &[ShardNames<'_>]) -> Option<Nearest> {
    let siblings = || all.iter().filter(|s| s.id != me);
    let mut best: Option<(usize, Nearest)> = None;
    for s in siblings() {
        for (d, kind) in &s.defs {
            consider(
                &mut best,
                name,
                d,
                s.id,
                &format!("{} defined by", kind_word(*kind)),
            );
        }
    }
    if best.is_none() {
        for s in siblings() {
            for (c, source) in &s.claims {
                consider(&mut best, name, c, s.id, source);
            }
        }
    }
    if best.is_none() {
        for s in siblings() {
            for t in &s.tokens {
                consider(&mut best, name, t, s.id, "a token in the pieces of");
            }
        }
    }
    best.map(|(_, n)| n)
}

/// Resolve every shard's ASSUMES against the module (module doc). `sources` are each shard's
/// pieces as `(file name, source)`, keyed by shard id; a shard with no entry has no vocabulary
/// and no free references (every lexical token is then checked).
pub(super) fn resolve(
    shards: &[ShardDossier],
    sources: &[(String, Vec<(String, String)>)],
    shared_state: &str,
) -> Resolution {
    let roots: Vec<String> = shared_state
        .split(|c: char| !(is_ident_char(c) || c == '.'))
        .filter(|w| !w.is_empty())
        .map(|w| w.split('.').next().unwrap_or(w).to_string())
        .collect();
    let names: Vec<ShardNames<'_>> = shards
        .iter()
        .map(|sh| {
            let srcs: &[(String, String)] = sources
                .iter()
                .find(|(id, _)| *id == sh.id)
                .map(|(_, s)| s.as_slice())
                .unwrap_or(&[]);
            ShardNames::of(sh, srcs)
        })
        .collect();
    let all_defs: Vec<&str> = names
        .iter()
        .flat_map(|n| n.defs.iter().map(|(d, _)| d.as_str()))
        .collect();
    let found = |name: &str| -> bool {
        let head = name.split('.').next().unwrap_or(name);
        all_defs
            .iter()
            .any(|d| same_symbol(name, d) || (head != name && same_symbol(head, d)))
            || roots.iter().any(|r| r == head)
    };
    let mut out = Resolution::default();
    for me in &names {
        let Some(note) = me.note else {
            continue;
        };
        for clause in &note.assumes {
            let (shaped, idents) = assumed_identifiers(clause, &me.free, &me.tokens);
            if !shaped {
                out.prose.push((me.id.to_string(), clause.clone()));
                continue;
            }
            let mut any = false;
            for name in idents {
                if found(name.as_str()) {
                    continue;
                }
                any = true;
                if out
                    .unbacked
                    .iter()
                    .any(|u| u.shard == me.id && u.name == name)
                {
                    continue;
                }
                let nearest = nearest(&name, me.id, &names);
                out.unbacked.push(AssumeUnbacked {
                    shard: me.id.to_string(),
                    name,
                    clause: clause.clone(),
                    nearest,
                });
            }
            if any {
                out.unmet.push((me.id.to_string(), clause.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::swarm::shards::{extract_symbols, parse_shard_note, SHARDS_DIR};

    const CAMERA_README: &str =
        include_str!("fixtures/r6h_viz_engine/camera-labels-brush/README.md");
    const CAMERA_PIECES: [(&str, &str); 5] = [
        (
            "brush.js",
            include_str!("fixtures/r6h_viz_engine/camera-labels-brush/brush.js"),
        ),
        (
            "camera-input.js",
            include_str!("fixtures/r6h_viz_engine/camera-labels-brush/camera-input.js"),
        ),
        (
            "camera-state.js",
            include_str!("fixtures/r6h_viz_engine/camera-labels-brush/camera-state.js"),
        ),
        (
            "labels.js",
            include_str!("fixtures/r6h_viz_engine/camera-labels-brush/labels.js"),
        ),
        (
            "project.js",
            include_str!("fixtures/r6h_viz_engine/camera-labels-brush/project.js"),
        ),
    ];
    const DATA_README: &str =
        include_str!("fixtures/r6h_viz_engine/data-stream-render-pick/README.md");
    const DATA_PIECES: [(&str, &str); 4] = [
        (
            "data-stream.js",
            include_str!("fixtures/r6h_viz_engine/data-stream-render-pick/data-stream.js"),
        ),
        (
            "pick-buffer.js",
            include_str!("fixtures/r6h_viz_engine/data-stream-render-pick/pick-buffer.js"),
        ),
        (
            "render-core.js",
            include_str!("fixtures/r6h_viz_engine/data-stream-render-pick/render-core.js"),
        ),
        (
            "streaming.js",
            include_str!("fixtures/r6h_viz_engine/data-stream-render-pick/streaming.js"),
        ),
    ];
    const DEBUG_README: &str = include_str!("fixtures/r6h_viz_engine/debug-api/README.md");
    const DEBUG_PIECES: [(&str, &str); 2] = [
        (
            "boot.js",
            include_str!("fixtures/r6h_viz_engine/debug-api/boot.js"),
        ),
        (
            "vs7dbg-facade.js",
            include_str!("fixtures/r6h_viz_engine/debug-api/vs7dbg-facade.js"),
        ),
    ];

    type Sources = (String, Vec<(String, String)>);

    /// A shard as `build_merge_dossier` records it (symbols per piece, the parsed README) plus its
    /// sources as the dossier builder hands them to `resolve`.
    fn shard(
        module: &str,
        name: &str,
        readme: &str,
        pieces: &[(&str, &str)],
    ) -> (ShardDossier, Sources) {
        let id = format!("{module}-{name}");
        let folder = format!("{SHARDS_DIR}/{module}/{name}");
        let dossier = ShardDossier {
            id: id.clone(),
            folder: folder.clone(),
            readme_present: true,
            note: parse_shard_note(readme),
            pieces: pieces
                .iter()
                .map(|(n, src)| {
                    (
                        format!("{folder}/{n}"),
                        None,
                        extract_symbols(src, TargetLang::TypeScript),
                    )
                })
                .collect(),
            ..Default::default()
        };
        let sources = (
            id,
            pieces
                .iter()
                .map(|(n, s)| (n.to_string(), s.to_string()))
                .collect(),
        );
        (dossier, sources)
    }

    fn set(words: &[&str]) -> BTreeSet<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// r6h's three viz-engine shards, verbatim. camera's `gl` (a free reference of brush.js and
    /// labels.js, plain word) is unbacked with `vizGL` nearest — data-stream's module-scope state,
    /// `gl` its tail; `uBrushActive` (camelCase, only in camera's comments) is unbacked with
    /// `uBrush` nearest — a token of render-core.js's GLSL literal, no definition sees it;
    /// `invalidatePick()` is unbacked for camera AND debug-api with nothing close (`pick` is a bare
    /// tail, not a head). `ensureBrushFlag` is camera's own definition — found; that data-stream's
    /// `applyBatch` never calls it is a call-graph fact this scan cannot see. Every name a sibling
    /// defines is silent; data-stream's own `uBrush` and the old rule's two false clauses (the
    /// keyword `new`, the global's member `window.vs7dbg`) are silent; camera's index.html clause
    /// and debug-api's facade-order clause are prose.
    #[test]
    fn r6h_viz_engine_assumes_resolve_gl_to_vizgl_and_ubrushactive_to_ubrush() {
        let m = "viz-engine";
        let (camera, cs) = shard(m, "camera-labels-brush", CAMERA_README, &CAMERA_PIECES);
        let (data, ds) = shard(m, "data-stream-render-pick", DATA_README, &DATA_PIECES);
        let (debug, es) = shard(m, "debug-api", DEBUG_README, &DEBUG_PIECES);
        assert_eq!(
            camera.note.as_ref().map(|n| n.assumes.len()),
            Some(10),
            "the fixture README's ten ASSUMES parse"
        );
        let r = resolve(&[camera, data, debug], &[cs, ds, es], "");
        let cam = "viz-engine-camera-labels-brush";
        let dat = "viz-engine-data-stream-render-pick";
        let dbg = "viz-engine-debug-api";
        let of = |shard: &str, name: &str| {
            r.unbacked
                .iter()
                .find(|u| u.shard == shard && u.name == name)
        };
        let gl = of(cam, "gl").unwrap_or_else(|| panic!("`gl` is unbacked: {:?}", r.unbacked));
        assert!(
            gl.clause
                .starts_with("GL context variable is named gl (module scope"),
            "{gl:?}"
        );
        assert_eq!(
            gl.nearest,
            Some(Nearest {
                name: "vizGL".into(),
                shard: dat.into(),
                source: "state defined by".into(),
                rule: "`gl` is the tail of `vizGL`".into(),
            })
        );
        let u = of(cam, "uBrushActive")
            .unwrap_or_else(|| panic!("`uBrushActive` is unbacked: {:?}", r.unbacked));
        assert_eq!(
            u.nearest,
            Some(Nearest {
                name: "uBrush".into(),
                shard: dat.into(),
                source: "a token in the pieces of".into(),
                rule: "`uBrush` is the head of `uBrushActive`".into(),
            })
        );
        let inv = of(cam, "invalidatePick").expect("camera's invalidatePick() is unbacked");
        assert_eq!(inv.nearest, None, "{inv:?}");
        assert!(
            of(dbg, "invalidatePick").is_some(),
            "debug-api repeats the phantom name: {:?}",
            r.unbacked
        );
        for name in [
            "ensureBrushFlag",
            "pickCore",
            "requestRender",
            "initViz",
            "applyBatch",
            "renderFrame",
            "records",
            "instanceGeom",
            "brushFlagGL",
            "brushSet",
            "updateLabels",
            "setCameraCore",
        ] {
            assert!(
                of(cam, name).is_none(),
                "`{name}` is defined: {:?}",
                r.unbacked
            );
        }
        for name in [
            "camera",
            "brushSet",
            "brushFlag",
            "uBrush",
            "bindCameraInput",
            "updateLabels",
            "vizGL",
            "flagBuf",
        ] {
            assert!(of(dat, name).is_none(), "`{name}`: {:?}", r.unbacked);
        }
        for name in [
            "layoutBasis",
            "digestSums",
            "brushSet",
            "getCamera",
            "setCameraCore",
            "pickPixelCore",
            "loadRecords",
            "onStreamMessage",
            "bindClickInput",
            "viz3d",
            "toggleBrush",
            "initBrushFlagBuffer",
        ] {
            assert!(of(dbg, name).is_none(), "`{name}`: {:?}", r.unbacked);
        }
        let unmet = |shard: &str, prefix: &str| {
            r.unmet
                .iter()
                .any(|(s, c)| s == shard && c.starts_with(prefix))
        };
        assert!(unmet(cam, "render-pick marks the pick buffer stale"));
        assert!(unmet(cam, "GL context variable is named gl"));
        assert!(unmet(
            cam,
            "render-pick's instanced program binds brushFlagGL"
        ));
        assert!(
            !unmet(dat, "boot (debug-api) wires"),
            "the keyword `new` is not a candidate: {:?}",
            r.unmet
        );
        assert!(
            !unmet(dbg, "assembly places the vs7dbg-facade"),
            "a global's member names nothing to define: {:?}",
            r.unmet
        );
        let prose = |shard: &str, prefix: &str| {
            r.prose
                .iter()
                .any(|(s, c)| s == shard && c.starts_with(prefix))
        };
        assert!(prose(cam, "index.html provides #viz3d"), "{:?}", r.prose);
        assert!(
            prose(dbg, "assembly places the vs7dbg-facade"),
            "{:?}",
            r.prose
        );
        assert!(
            gl.glue().starts_with("`gl` is defined by no shard; the nearest sibling name is `vizGL` (state defined by `viz-engine-data-stream-render-pick`; `gl` is the tail of `vizGL`) — alias `gl` to `vizGL` where `vizGL` is written, or rename the `gl` uses to `vizGL`; you decide, say which under KEPT."),
            "{}",
            gl.glue()
        );
        assert!(
            inv.glue().starts_with(
                "`invalidatePick` is defined by no shard and no sibling name is close — write it"
            ),
            "{}",
            inv.glue()
        );
        let j = gl.to_json();
        assert_eq!(j["name"], "gl");
        assert_eq!(j["nearest"]["name"], "vizGL");
        assert_eq!(j["nearest"]["shard"], dat);
        assert_eq!(inv.to_json()["nearest"], serde_json::Value::Null);
    }

    /// The identifier rule on r6h's own clauses: a free reference qualifies a plain word (`gl`);
    /// camelCase qualifies a prose-only name (`uBrushActive`, `uActive`); the shard's own
    /// vocabulary (`brushSet`, `round` — a member it calls) and a 3-letter call (`mix(`) do not;
    /// a keyword, a runtime global, a headless member and a file name never do; a clause of words
    /// is prose.
    #[test]
    fn the_identifier_rule_reads_free_references_and_lexical_shape_never_prose() {
        let free = set(&["gl"]);
        let vocab = set(&["gl", "brushSet", "round", "brushFlagGL"]);
        assert_eq!(
            assumed_identifiers(
                "GL context variable is named gl (module scope, written by initViz); updateLabels no-op safely",
                &free,
                &vocab
            ),
            (true, vec!["gl".to_string(), "initViz".to_string(), "updateLabels".to_string()])
        );
        assert_eq!(
            assumed_identifiers(
                "binds brushFlagGL as a per-instance UNSIGNED_BYTE attribute and sets a per-frame uniform uBrushActive = brushSet.size > 0; base c' = uActive ? mix(c, round(0.30·c), flag) : c",
                &free,
                &vocab
            ),
            (true, vec!["uBrushActive".to_string(), "uActive".to_string()])
        );
        assert_eq!(
            assumed_identifiers(
                "boot wires new EventSource('/api/stream') with .onmessage = onStreamMessage after loadRecords",
                &BTreeSet::new(),
                &BTreeSet::new()
            ),
            (true, vec!["onStreamMessage".to_string(), "loadRecords".to_string()])
        );
        assert_eq!(
            assumed_identifiers(
                "assembly concatenates pieces in order data-stream.js → streaming.js → render-core.js",
                &BTreeSet::new(),
                &BTreeSet::new()
            ),
            (false, vec![])
        );
        assert_eq!(
            assumed_identifiers(
                "index.html provides #viz3d (canvas) and #viz-labels; styles.css carries .viz-label typography",
                &BTreeSet::new(),
                &BTreeSet::new()
            ),
            (false, vec![])
        );
        assert_eq!(
            assumed_identifiers(
                "records.count and brushSet.size are read",
                &set(&["records"]),
                &set(&["records", "brushSet"])
            ),
            (true, vec!["records.count".to_string()])
        );
    }

    /// Closeness: head/tail by segments, the candidate as the name's head, edit distance for 4+
    /// characters — and the shapes that are NOT close (a bare tail, a one-letter head, unrelated).
    #[test]
    fn closeness_is_head_or_tail_segments_then_edit_distance_never_a_bare_tail() {
        assert_eq!(segments_of("vizGL"), vec!["viz", "gl"]);
        assert_eq!(segments_of("uBrushActive"), vec!["u", "brush", "active"]);
        assert_eq!(segments_of("GLContext"), vec!["gl", "context"]);
        assert_eq!(
            segments_of("VIZ_CAM_DEFAULT"),
            vec!["viz", "cam", "default"]
        );
        assert_eq!(
            closeness("gl", "vizGL"),
            Some((1, "`gl` is the tail of `vizGL`".to_string()))
        );
        assert_eq!(
            closeness("uBrushActive", "uBrush"),
            Some((1, "`uBrush` is the head of `uBrushActive`".to_string()))
        );
        assert_eq!(
            closeness("pick", "pickCore"),
            Some((1, "`pick` is the head of `pickCore`".to_string()))
        );
        assert_eq!(closeness("invalidatePick", "pick"), None, "a bare tail");
        assert_eq!(closeness("uActive", "u"), None, "one letter is not a name");
        assert_eq!(closeness("gl", "toggleBrush"), None, "`gl` inside a word");
        assert_eq!(closeness("gl", "gl"), None, "the name itself");
        assert_eq!(
            closeness("requestRender", "requestRenders"),
            Some((11, "edit distance 1 from `requestRenders`".to_string()))
        );
        assert_eq!(closeness("uActive", "uBrush"), None);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }
}
