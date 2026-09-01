//! ASSEMBLE, THEN GLUE — the merger's mechanical work done by CODE (DESIGN-SPLIT-V2 §1, HIGH).
//!
//! MEASURED: the v1 merger was told "ASSEMBLE, DON'T RETYPE — `cat` the pieces in the declared
//! order, then edit the glue", and a 27B model at ~19 tok/s retyping a 40 KB module is ~30 minutes
//! of emit in which `merge_piece_dropped` and `merge_signature_mismatch` are manufactured (r6c's
//! viz.js carried five names defined twice; r6e's merger was dispatched over eight shards and zero
//! pieces). The research the design cites (Mergiraf; the ASE'24 merge-tool evaluation; ATM/SpecDB's
//! NEUTRAL party applying the writes) says: merge at the DECLARATION level, by code, then check.
//!
//! WHAT THIS DOES at the merger's dispatch — after the dossier, before the model runs:
//! 1. SEGMENT every piece whose extension is the module's into top-level blocks. A block starts at
//!    a column-0 line that is not a closer (`}` `)` `]`), not a continuation (`.` `,` `else` …) and
//!    not inside a multi-line string/comment; leading comments and Python decorators belong to the
//!    block below them. A block DEFINES the non-shorthand names `extract_symbols` (the dossier's own
//!    extractor) finds in it; a block defining nothing is an IMPORT or a top-level STATEMENT (state,
//!    wiring, boot) — the glue the merger owns.
//! 2. ORDER the defining blocks by the declared interface: each export, in `exports` order, pulls
//!    the block(s) defining it (`same_symbol`, the dossier's MILD rule); blocks no export names are
//!    appended after in shard order. A name defined by TWO shards is
//!    `merge_duplicate_definition{module, name, shards}` and BOTH blocks stay, each under a
//!    `MERGE_DUPLICATE` marker the merger resolves — never a silent drop.
//! 3. WRITE `.swarm/shards/<module>/ASSEMBLED.<ext>` — a provenance line (`// shard: <id>`) before
//!    every block, imports first, statements collected last — and SAY what was built:
//!    `merge_assembled{module, pieces, definitions, order_source, glue_needed: [...]}`.
//! The merger's brief (`MergeDossier::merger_brief`) then names the GLUE as the job — imports and
//! exports, the shared state's one initialisation, wiring, every UNFINISHED item and every GAP —
//! and forbids retyping a definition ("the pieces are already in place; a retyped definition is a
//! defect").
//!
//! Every non-blank line of every assembled piece appears in the file exactly once (imports whose
//! text is byte-identical are written once) — the invariant the tests pin; a misjudged block
//! boundary moves text between blocks, it never loses it.
//!
//! LANGUAGES: the per-file parsers THE SPLIT already has (`py`, `js`/`mjs`/`cjs`), each piece read
//! in ITS extension's language (never the run's — r6e's module was `web/viz.js` in a Python-target
//! run). Any other final-file extension, mixed extensions, or no piece of the module's language is
//! `merge_assembly_unavailable{module, ext, reason}` and the v1 brief — said, never faked. MILD:
//! nothing here gates, retries or aborts; `check_merge` runs unchanged on the file the merger writes.

use std::path::Path;

use super::super::TargetLang;
use super::{extract_symbols, same_symbol, MergeDossier, SHARDS_DIR};

/// The engine's file beside the shard folders: `.swarm/shards/<module>/ASSEMBLED.<ext>`.
pub(super) const ASSEMBLED_STEM: &str = "ASSEMBLED";
/// The marker line over a definition another shard also defines.
pub(super) const DUPLICATE_MARKER: &str = "MERGE_DUPLICATE";

/// One top-level block of a piece, verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Segment {
    /// The non-shorthand names `extract_symbols` finds in the block — empty for an import or a
    /// top-level statement (state, wiring, boot).
    pub(super) names: Vec<String>,
    pub(super) import: bool,
    pub(super) text: String,
    /// 1-based line in the piece where the block starts.
    pub(super) line: usize,
}

impl Segment {
    fn defines(&self) -> bool {
        !self.names.is_empty()
    }
}

/// The language a piece is segmented as, from ITS extension. None = no per-file parser THE SPLIT
/// has for it (assembly is then unavailable, said).
pub(super) fn assembly_lang(ext: &str) -> Option<TargetLang> {
    match ext {
        "py" => Some(TargetLang::Python),
        "js" | "mjs" | "cjs" => Some(TargetLang::TypeScript),
        _ => None,
    }
}

fn comment_prefix(lang: TargetLang) -> &'static str {
    match lang {
        TargetLang::Python => "#",
        _ => "//",
    }
}

fn is_comment_line(t: &str, lang: TargetLang) -> bool {
    match lang {
        TargetLang::Python => t.starts_with('#'),
        _ => t.starts_with("//") || t.starts_with("/*") || t.starts_with('*'),
    }
}

fn starts_with_word(t: &str, word: &str) -> bool {
    t.strip_prefix(word)
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

/// A column-0 line that CONTINUES the block above rather than opening a new one: a closer, a
/// chained/continued expression, or the second half of a top-level `if`/`try`.
fn continues_block(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('}')
        || t.starts_with(')')
        || t.starts_with(']')
        || t.starts_with('.')
        || t.starts_with(',')
        || t == ";"
        || t.starts_with("&&")
        || t.starts_with("||")
        || starts_with_word(t, "else")
        || starts_with_word(t, "elif")
        || starts_with_word(t, "except")
        || starts_with_word(t, "finally")
        || starts_with_word(t, "catch")
}

fn is_import(t: &str, lang: TargetLang) -> bool {
    match lang {
        TargetLang::Python => {
            starts_with_word(t, "import") || (starts_with_word(t, "from") && t.contains(" import "))
        }
        _ => {
            starts_with_word(t, "import")
                || (t.contains("require(")
                    && (t.starts_with("const ") || t.starts_with("let ") || t.starts_with("var ")))
                || (t.starts_with("export ") && t.contains(" from "))
        }
    }
}

/// Multi-line constructs whose inner lines may sit at column 0 (a shader source in a template
/// literal, a module docstring, a block comment) — inside one, every line continues the block.
/// Best effort by per-line parity; a miss moves text between blocks, never out of the file.
#[derive(Default)]
struct Open {
    triple_dq: bool,
    triple_sq: bool,
    backtick: bool,
    block_comment: bool,
}

impl Open {
    fn any(&self) -> bool {
        self.triple_dq || self.triple_sq || self.backtick || self.block_comment
    }

    fn feed(&mut self, line: &str, lang: TargetLang) {
        match lang {
            TargetLang::Python => {
                if line.matches("\"\"\"").count() % 2 == 1 {
                    self.triple_dq = !self.triple_dq;
                }
                if line.matches("'''").count() % 2 == 1 {
                    self.triple_sq = !self.triple_sq;
                }
            }
            _ => {
                let ticks = line
                    .match_indices('`')
                    // `match_indices`/`rfind` offsets are char boundaries; `split_at` indexes nothing.
                    .filter(|(i, _)| !line.split_at(*i).0.ends_with('\\'))
                    .count();
                if ticks % 2 == 1 {
                    self.backtick = !self.backtick;
                }
                if !self.backtick {
                    if self.block_comment {
                        self.block_comment = !line.contains("*/");
                    } else if let Some(p) = line.rfind("/*") {
                        self.block_comment = !line.split_at(p).1.contains("*/");
                    }
                }
            }
        }
    }
}

/// Split a piece into its top-level blocks (see the module doc). Blank lines attach to the block
/// above; a block of only comments/decorators attaches to the block below (or above, at the end).
pub(super) fn segments(source: &str, lang: TargetLang) -> Vec<Segment> {
    let mut raw: Vec<(usize, Vec<&str>)> = Vec::new();
    let mut open = Open::default();
    for (i, line) in source.lines().enumerate() {
        let inside = open.any();
        open.feed(line, lang);
        let blank = line.trim().is_empty();
        if raw.is_empty() {
            if blank {
                continue;
            }
            raw.push((i + 1, vec![line]));
            continue;
        }
        let starts_new = !inside
            && !blank
            && !line.starts_with(|c: char| c.is_whitespace())
            && !continues_block(line);
        if starts_new {
            raw.push((i + 1, vec![line]));
        } else if let Some(last) = raw.last_mut() {
            last.1.push(line);
        }
    }
    let lead_only = |lines: &[&str]| {
        lines.iter().all(|l| {
            let t = l.trim();
            t.is_empty()
                || is_comment_line(t, lang)
                || (lang == TargetLang::Python && t.starts_with('@'))
        })
    };
    let mut merged: Vec<(usize, Vec<&str>)> = Vec::new();
    let mut pending: Option<(usize, Vec<&str>)> = None;
    for (line, lines) in raw {
        let lead = lead_only(&lines);
        match pending.take() {
            Some((pl, mut plines)) => {
                plines.extend(lines);
                if lead {
                    pending = Some((pl, plines));
                } else {
                    merged.push((pl, plines));
                }
            }
            None => {
                if lead {
                    pending = Some((line, lines));
                } else {
                    merged.push((line, lines));
                }
            }
        }
    }
    if let Some((pl, plines)) = pending {
        match merged.last_mut() {
            Some(last) => last.1.extend(plines),
            None => merged.push((pl, plines)),
        }
    }
    merged
        .into_iter()
        .filter_map(|(line, lines)| {
            let mut end = lines.len();
            while end > 0 && lines[end - 1].trim().is_empty() {
                end -= 1;
            }
            if end == 0 {
                return None;
            }
            let body = &lines[..end];
            let text = format!("{}\n", body.join("\n"));
            let first_code = body.iter().map(|l| l.trim()).find(|t| {
                !t.is_empty()
                    && !is_comment_line(t, lang)
                    && (lang != TargetLang::Python || !t.starts_with('@'))
            });
            let import = first_code.is_some_and(|t| is_import(t, lang));
            let names: Vec<String> = if import {
                Vec::new()
            } else {
                extract_symbols(&text, lang)
                    .into_iter()
                    .filter(|s| !s.shorthand)
                    .map(|s| s.name)
                    .collect()
            };
            Some(Segment {
                names,
                import,
                text,
                line,
            })
        })
        .collect()
}

/// What CODE assembled — the facts the event and the merger's brief carry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Assembly {
    /// Relative to the run root: `.swarm/shards/<module>/ASSEMBLED.<ext>`.
    pub(super) path: String,
    pub(super) ext: String,
    pub(super) pieces: usize,
    /// (piece path, why it was not assembled) — another extension, unreadable.
    pub(super) pieces_skipped: Vec<(String, String)>,
    /// Defining blocks written.
    pub(super) definitions: usize,
    /// Of those, placed by an export's position in the declared interface.
    pub(super) ordered_by_interface: usize,
    /// (shard, names) — defining blocks no export names, appended after in shard order.
    pub(super) appended_unknown: Vec<(String, String)>,
    /// (name, shards) — a name defined by more than one shard; every block kept, marked.
    pub(super) duplicates: Vec<(String, Vec<String>)>,
    pub(super) imports: usize,
    /// (shard, top-level statement blocks collected at the end).
    pub(super) statements: Vec<(String, usize)>,
    /// Declared exports no assembled block defines — the merger's GAPS.
    pub(super) declared_missing: Vec<String>,
    pub(super) order_source: String,
    /// The glue classes the merger must write, measured: imports, shared_state_init, wiring,
    /// duplicates, gaps, unfinished.
    pub(super) glue_needed: Vec<String>,
    pub(super) bytes: usize,
    pub(super) lines: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AssemblyOutcome {
    Assembled(Box<Assembly>),
    /// Said, never faked: the reason names what is missing (a parser for `ext`, a piece of the
    /// module's language, a writable folder).
    Unavailable {
        ext: String,
        reason: String,
    },
}

fn ext_of(path: &str) -> Option<&str> {
    Path::new(path).extension().and_then(|e| e.to_str())
}

struct Block {
    shard: String,
    piece: String,
    seg: Segment,
    parse_error: Option<String>,
}

/// Assemble the module's pieces into `ASSEMBLED.<ext>` under the shard area and describe it. The
/// dossier supplies the shards, their piece paths and parse verdicts, and the declared interface;
/// the piece SOURCE is read here (the dossier keeps symbols, not text).
pub(super) fn assemble(root: &Path, dossier: &MergeDossier) -> AssemblyOutcome {
    let Some(first) = dossier.files.first() else {
        return AssemblyOutcome::Unavailable {
            ext: String::new(),
            reason: "the merger owns no final file".to_string(),
        };
    };
    let Some(ext) = ext_of(first).map(String::from) else {
        return AssemblyOutcome::Unavailable {
            ext: String::new(),
            reason: format!("the final file `{first}` has no extension"),
        };
    };
    if let Some(other) = dossier
        .files
        .iter()
        .find(|f| ext_of(f) != Some(ext.as_str()))
    {
        return AssemblyOutcome::Unavailable {
            ext,
            reason: format!(
                "the final files mix extensions (`{first}`, `{other}`) — assembly targets one language"
            ),
        };
    }
    let Some(lang) = assembly_lang(&ext) else {
        return AssemblyOutcome::Unavailable {
            ext,
            reason: "no per-file parser for this extension (py, js, mjs, cjs are assembled)"
                .to_string(),
        };
    };
    let prefix = comment_prefix(lang);

    let mut blocks: Vec<Block> = Vec::new();
    let mut pieces = 0usize;
    let mut pieces_skipped: Vec<(String, String)> = Vec::new();
    for sh in &dossier.shards {
        for (path, verdict, _) in &sh.pieces {
            match ext_of(path) {
                Some(e) if e == ext => {}
                Some(e) => {
                    pieces_skipped
                        .push((path.clone(), format!("`.{e}` is not the module's `.{ext}`")));
                    continue;
                }
                None => {
                    pieces_skipped.push((path.clone(), "no extension".to_string()));
                    continue;
                }
            }
            let src = match std::fs::read_to_string(root.join(path)) {
                Ok(s) => s,
                Err(e) => {
                    pieces_skipped.push((path.clone(), format!("unreadable: {e}")));
                    continue;
                }
            };
            pieces += 1;
            let parse_error = verdict
                .as_ref()
                .filter(|v| !v.contains("unchecked"))
                .cloned();
            for seg in segments(&src, lang) {
                blocks.push(Block {
                    shard: sh.id.clone(),
                    piece: path.clone(),
                    seg,
                    parse_error: parse_error.clone(),
                });
            }
        }
    }
    if pieces == 0 {
        return AssemblyOutcome::Unavailable {
            ext,
            reason: format!(
                "no piece with the module's `.{}` extension exists in any shard folder ({} other file(s) skipped)",
                ext_of(first).unwrap_or(""),
                pieces_skipped.len()
            ),
        };
    }

    // ORDER: each export, in declared order, pulls the blocks defining it; the rest follow in
    // shard order. A block is written once, at the first export that names it.
    let mut emitted = vec![false; blocks.len()];
    let mut via_interface = vec![false; blocks.len()];
    let mut ordered: Vec<usize> = Vec::new();
    let mut declared_missing: Vec<String> = Vec::new();
    for e in &dossier.interface.exports {
        let mut any = false;
        for (i, b) in blocks.iter().enumerate() {
            if b.seg.names.iter().any(|n| same_symbol(&e.name, n)) {
                any = true;
                if !emitted[i] {
                    emitted[i] = true;
                    via_interface[i] = true;
                    ordered.push(i);
                }
            }
        }
        if !any {
            declared_missing.push(e.name.clone());
        }
    }
    let ordered_by_interface = ordered.len();
    let mut appended_unknown: Vec<(String, String)> = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        if b.seg.defines() && !emitted[i] {
            emitted[i] = true;
            ordered.push(i);
            appended_unknown.push((b.shard.clone(), b.seg.names.join(", ")));
        }
    }
    let mut by_name: Vec<(String, Vec<String>)> = Vec::new();
    for b in &blocks {
        for n in &b.seg.names {
            match by_name.iter_mut().find(|(name, _)| name == n) {
                Some((_, shards)) => {
                    if !shards.contains(&b.shard) {
                        shards.push(b.shard.clone());
                    }
                }
                None => by_name.push((n.clone(), vec![b.shard.clone()])),
            }
        }
    }
    let duplicates: Vec<(String, Vec<String>)> =
        by_name.into_iter().filter(|(_, s)| s.len() > 1).collect();

    // RENDER: header, imports, definitions in order, statements last.
    let final_files = dossier.files.join(", ");
    let mut out = format!(
        "{prefix} ASSEMBLED BY THE ENGINE — module `{module}` ({final_files}): {defs} definition block(s) from {pieces} piece(s), {by_if} in the declared interface's order, {unk} appended after it (no export names them).\n\
         {prefix} Every block below is a shard's piece VERBATIM under its `{prefix} shard:` line. The MERGER writes the GLUE — imports/exports, the shared state's one initialisation, the wiring — into the final file and never retypes a block.\n",
        module = dossier.module,
        defs = ordered.len(),
        by_if = ordered_by_interface,
        unk = appended_unknown.len(),
    );
    let mut seen_imports: Vec<String> = Vec::new();
    let mut imports = 0usize;
    for b in blocks.iter().filter(|b| b.seg.import) {
        let key = b.seg.text.trim().to_string();
        if seen_imports.contains(&key) {
            continue;
        }
        seen_imports.push(key);
        imports += 1;
        out.push_str(&format!(
            "\n{prefix} shard: {} ({}:{}) — import\n{}",
            b.shard, b.piece, b.seg.line, b.seg.text
        ));
    }
    for &i in &ordered {
        let b = &blocks[i];
        let mut others: Vec<&str> = Vec::new();
        for (n, shards) in &duplicates {
            if b.seg.names.contains(n) {
                for s in shards {
                    if *s != b.shard && !others.contains(&s.as_str()) {
                        others.push(s.as_str());
                    }
                }
            }
        }
        out.push_str(&format!(
            "\n{prefix} shard: {} ({}:{}) — {}{}\n",
            b.shard,
            b.piece,
            b.seg.line,
            b.seg.names.join(", "),
            if via_interface[i] {
                ""
            } else {
                " — not in the declared interface"
            }
        ));
        if let Some(e) = &b.parse_error {
            out.push_str(&format!("{prefix} PARSE ERROR in this piece: {e}\n"));
        }
        if !others.is_empty() {
            out.push_str(&format!(
                "{prefix} {DUPLICATE_MARKER}: also defined by shard(s) {} — keep ONE definition per name and say which under KEPT/DROPPED\n",
                others.join(", ")
            ));
        }
        out.push_str(&b.seg.text);
    }
    let mut statements: Vec<(String, usize)> = Vec::new();
    let stmt_blocks: Vec<&Block> = blocks
        .iter()
        .filter(|b| !b.seg.import && !b.seg.defines())
        .collect();
    if !stmt_blocks.is_empty() {
        out.push_str(&format!(
            "\n{prefix} ---- TOP-LEVEL STATEMENTS from the pieces (state, wiring, boot), collected here in shard order — the merger places each where the declared layout puts it and removes duplicates ----\n"
        ));
        for b in stmt_blocks {
            match statements.iter_mut().find(|(s, _)| *s == b.shard) {
                Some((_, n)) => *n += 1,
                None => statements.push((b.shard.clone(), 1)),
            }
            out.push_str(&format!(
                "\n{prefix} shard: {} ({}:{}) — top-level statement\n{}",
                b.shard, b.piece, b.seg.line, b.seg.text
            ));
        }
    }

    let mut glue_needed: Vec<String> = Vec::new();
    if imports > 0 {
        glue_needed.push("imports".to_string());
    }
    if !dossier.interface.shared_state.trim().is_empty() {
        glue_needed.push("shared_state_init".to_string());
    }
    if !statements.is_empty()
        || dossier
            .interface
            .exports
            .iter()
            .any(|e| e.name.contains('.'))
    {
        glue_needed.push("wiring".to_string());
    }
    if !duplicates.is_empty() {
        glue_needed.push("duplicates".to_string());
    }
    if !declared_missing.is_empty() {
        glue_needed.push("gaps".to_string());
    }
    if dossier
        .shards
        .iter()
        .any(|s| !s.provides_unbacked.is_empty())
    {
        glue_needed.push("unbacked_provides".to_string());
    }
    if !dossier.unfinished.is_empty() {
        glue_needed.push("unfinished".to_string());
    }
    let order_source = if dossier.interface.exports.is_empty() {
        "shard order (synthesis declared no exports)".to_string()
    } else {
        "interface.exports".to_string()
    };

    let rel = format!("{SHARDS_DIR}/{}/{ASSEMBLED_STEM}.{ext}", dossier.module);
    let abs = root.join(&rel);
    if let Some(parent) = abs.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return AssemblyOutcome::Unavailable {
                ext,
                reason: format!("cannot create `{}`: {e}", parent.display()),
            };
        }
    }
    if let Err(e) = std::fs::write(&abs, &out) {
        return AssemblyOutcome::Unavailable {
            ext,
            reason: format!("cannot write `{rel}`: {e}"),
        };
    }
    AssemblyOutcome::Assembled(Box::new(Assembly {
        path: rel,
        ext,
        pieces,
        pieces_skipped,
        definitions: ordered.len(),
        ordered_by_interface,
        appended_unknown,
        duplicates,
        imports,
        statements,
        declared_missing,
        order_source,
        glue_needed,
        bytes: out.len(),
        lines: out.lines().count(),
    }))
}

pub(super) fn assembled_event(module: &str, task_id: &str, a: &Assembly) -> serde_json::Value {
    serde_json::json!({
        "event": "merge_assembled",
        "module": module,
        "task_id": task_id,
        "path": a.path,
        "ext": a.ext,
        "pieces": a.pieces,
        "pieces_skipped": a.pieces_skipped.iter().map(|(p, why)| serde_json::json!({"path": p, "why": why})).collect::<Vec<_>>(),
        "definitions": a.definitions,
        "ordered_by_interface": a.ordered_by_interface,
        "appended_unknown": a.appended_unknown.iter().map(|(s, n)| serde_json::json!({"shard": s, "names": n})).collect::<Vec<_>>(),
        "duplicates": a.duplicates.iter().map(|(n, s)| serde_json::json!({"name": n, "shards": s})).collect::<Vec<_>>(),
        "imports": a.imports,
        "statements": a.statements.iter().map(|(s, n)| serde_json::json!({"shard": s, "blocks": n})).collect::<Vec<_>>(),
        "declared_missing": a.declared_missing,
        "order_source": a.order_source,
        "glue_needed": a.glue_needed,
        "bytes": a.bytes,
        "lines": a.lines,
    })
}

/// One event per name two shards both define — BOTH definitions are in the assembled file.
pub(super) fn duplicate_events(
    module: &str,
    task_id: &str,
    a: &Assembly,
) -> Vec<serde_json::Value> {
    a.duplicates
        .iter()
        .map(|(name, shards)| {
            serde_json::json!({
                "event": "merge_duplicate_definition",
                "module": module,
                "task_id": task_id,
                "name": name,
                "shards": shards,
                "kept": "both, under a MERGE_DUPLICATE marker — the merger resolves",
            })
        })
        .collect()
}

pub(super) fn unavailable_event(
    module: &str,
    task_id: &str,
    ext: &str,
    reason: &str,
) -> serde_json::Value {
    serde_json::json!({
        "event": "merge_assembly_unavailable",
        "module": module,
        "task_id": task_id,
        "ext": ext,
        "reason": reason,
    })
}

#[cfg(test)]
mod tests {
    use super::super::ShardDossier;
    use super::*;
    use goose_swarm::{DeclaredExport, ModuleInterface};

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "goose-assembly-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A shard folder on disk and its dossier row — pieces sorted by path, as
    /// `build_merge_dossier` lists them; verdict None = parses.
    fn shard(root: &Path, module: &str, name: &str, files: &[(&str, &str)]) -> ShardDossier {
        let folder = format!("{SHARDS_DIR}/{module}/{name}");
        let dir = root.join(&folder);
        std::fs::create_dir_all(&dir).unwrap();
        let mut pieces = Vec::new();
        for (n, c) in files {
            std::fs::write(dir.join(n), c).unwrap();
            if *n != "README.md" {
                pieces.push((format!("{folder}/{n}"), None, Vec::new()));
            }
        }
        pieces.sort_by(|a, b| a.0.cmp(&b.0));
        ShardDossier {
            id: format!("{module}-{name}"),
            folder,
            readme_present: true,
            pieces,
            ..Default::default()
        }
    }

    fn export(name: &str, sig: &str) -> DeclaredExport {
        DeclaredExport {
            name: name.into(),
            kind: "function".into(),
            signature: sig.into(),
            purpose: name.into(),
        }
    }

    fn position(hay: &str, needle: &str) -> usize {
        hay.find(needle)
            .unwrap_or_else(|| panic!("`{needle}` not in:\n{hay}"))
    }

    /// The order follows the declared interface (pick-camera's `pick` is exports[0], so it leads
    /// although its shard is second); a defining block no export names (`clamp`) is appended after
    /// with the reason on its provenance line; imports come first; top-level statements (the
    /// state object, the boot listener) are collected last; the declared export nobody defines
    /// (`drawBrush`) is a GAP; every non-blank piece line is in the file exactly once.
    #[test]
    fn assembly_orders_definitions_by_the_interface_and_appends_unknowns() {
        let root = tmp("order");
        let render = "import { mat4 } from './gl.js';\n\nconst S = { yaw: 0, brush: new Set() };\n\n/**\n * Fill the instance buffers.\n */\nfunction buildScene(data) {\n  return data.ids.length;\n}\n\nconst clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));\n";
        let pick = "import { mat4 } from './gl.js';\nwindow.vs7dbg = window.vs7dbg || {};\nwindow.vs7dbg.pick = function (sx, sy) {\n  return null;\n};\nwindow.addEventListener('load', () => {\n  buildScene(S);\n});\n";
        let shards = vec![
            shard(&root, "web-viz", "render", &[("render.js", render)]),
            shard(&root, "web-viz", "pick-camera", &[("pick.js", pick)]),
        ];
        let dossier = MergeDossier {
            module: "web-viz".into(),
            files: vec!["web/viz.js".into()],
            interface: ModuleInterface {
                exports: vec![
                    export("window.vs7dbg.pick", "pick(sx, sy) -> {id} | null"),
                    export("buildScene", "buildScene(data) -> void"),
                    export("drawBrush", "drawBrush(ids) -> void"),
                ],
                shared_state: "S = {yaw, brush: Set<id>}".into(),
                layout: vec!["state".into(), "pick".into(), "render".into()],
            },
            shards,
            ..Default::default()
        };
        let AssemblyOutcome::Assembled(a) = assemble(&root, &dossier) else {
            panic!("two js pieces assemble");
        };
        assert_eq!(a.path, ".swarm/shards/web-viz/ASSEMBLED.js");
        assert_eq!(a.ext, "js");
        assert_eq!(a.pieces, 2);
        assert_eq!(a.definitions, 3, "{a:?}");
        assert_eq!(a.ordered_by_interface, 2);
        assert_eq!(
            a.appended_unknown,
            vec![("web-viz-render".to_string(), "clamp".to_string())]
        );
        assert_eq!(a.imports, 1, "the byte-identical import is written once");
        assert_eq!(
            a.statements,
            vec![
                ("web-viz-render".to_string(), 1),
                ("web-viz-pick-camera".to_string(), 2)
            ]
        );
        assert_eq!(a.declared_missing, vec!["drawBrush".to_string()]);
        assert!(a.duplicates.is_empty());
        assert_eq!(a.order_source, "interface.exports");
        assert_eq!(
            a.glue_needed,
            vec!["imports", "shared_state_init", "wiring", "gaps"]
        );
        let text = std::fs::read_to_string(root.join(&a.path)).unwrap();
        assert!(text.starts_with("// ASSEMBLED BY THE ENGINE — module `web-viz` (web/viz.js): 3 definition block(s) from 2 piece(s), 2 in the declared interface's order, 1 appended after it"), "{text}");
        let import = position(&text, "// shard: web-viz-render (.swarm/shards/web-viz/render/render.js:1) — import\nimport { mat4 }");
        let pick_def = position(&text, "// shard: web-viz-pick-camera (.swarm/shards/web-viz/pick-camera/pick.js:3) — window.vs7dbg.pick\nwindow.vs7dbg.pick = function (sx, sy) {");
        let scene_def = position(&text, "// shard: web-viz-render (.swarm/shards/web-viz/render/render.js:5) — buildScene\n/**\n * Fill the instance buffers.\n */\nfunction buildScene(data) {");
        let clamp_def = position(
            &text,
            "— clamp — not in the declared interface\nconst clamp = ",
        );
        let statements = position(&text, "// ---- TOP-LEVEL STATEMENTS");
        let state = position(
            &text,
            "— top-level statement\nconst S = { yaw: 0, brush: new Set() };",
        );
        let boot = position(
            &text,
            "— top-level statement\nwindow.addEventListener('load', () => {\n  buildScene(S);\n});",
        );
        assert!(
            import < pick_def && pick_def < scene_def,
            "interface order: {text}"
        );
        assert!(
            scene_def < clamp_def,
            "unknowns after the interface: {text}"
        );
        assert!(
            clamp_def < statements && statements < state && state < boot,
            "{text}"
        );
        assert!(
            !text.contains(DUPLICATE_MARKER),
            "no duplicate, no marker: {text}"
        );
        for piece in [render, pick] {
            for line in piece.lines().filter(|l| !l.trim().is_empty()) {
                assert!(text.contains(line), "piece line lost: `{line}`\n{text}");
            }
        }
        let occurrences = text.matches("function buildScene(data) {").count();
        assert_eq!(occurrences, 1, "a block is written once: {text}");
        let ev = assembled_event("web-viz", "web-viz", &a);
        assert_eq!(ev["event"], "merge_assembled");
        assert_eq!(ev["definitions"], 3);
        assert_eq!(ev["ordered_by_interface"], 2);
        assert_eq!(ev["declared_missing"], serde_json::json!(["drawBrush"]));
        assert_eq!(ev["glue_needed"][0], "imports");
        assert_eq!(ev["statements"][1]["blocks"], 2);
        assert_eq!(ev["lines"], text.lines().count());
    }

    /// r6c's archived viz.js carried five names defined twice; here `buildScene` is defined by two
    /// shards. BOTH blocks stay in the assembled file, each under a `MERGE_DUPLICATE` marker naming
    /// the other shard, the event names the name and the shards, and nothing is dropped.
    #[test]
    fn a_definition_in_two_shards_is_kept_twice_with_a_marker_and_said() {
        let root = tmp("dup");
        let shards = vec![
            shard(
                &root,
                "web-viz",
                "render",
                &[("render.js", "function buildScene(data) { real(data); }\n")],
            ),
            shard(
                &root,
                "web-viz",
                "pick",
                &[(
                    "pick.js",
                    "function buildScene(data) { /* stub */ }\nfunction readPickAt(sx, sy) {}\n",
                )],
            ),
        ];
        let dossier = MergeDossier {
            module: "web-viz".into(),
            files: vec!["web/viz.js".into()],
            interface: ModuleInterface {
                exports: vec![export("buildScene", "buildScene(data) -> void")],
                ..Default::default()
            },
            shards,
            ..Default::default()
        };
        let AssemblyOutcome::Assembled(a) = assemble(&root, &dossier) else {
            panic!("assembles");
        };
        assert_eq!(
            a.duplicates,
            vec![(
                "buildScene".to_string(),
                vec!["web-viz-render".to_string(), "web-viz-pick".to_string()]
            )]
        );
        assert_eq!(a.definitions, 3, "both buildScene blocks and readPickAt");
        assert_eq!(
            a.ordered_by_interface, 2,
            "both placed at buildScene's position"
        );
        assert_eq!(a.glue_needed, vec!["duplicates"]);
        let text = std::fs::read_to_string(root.join(&a.path)).unwrap();
        assert_eq!(
            text.matches("function buildScene(data)").count(),
            2,
            "{text}"
        );
        assert!(
            text.contains("// MERGE_DUPLICATE: also defined by shard(s) web-viz-pick — keep ONE definition per name"),
            "{text}"
        );
        assert!(
            text.contains("// MERGE_DUPLICATE: also defined by shard(s) web-viz-render"),
            "{text}"
        );
        let events = duplicate_events("web-viz", "web-viz", &a);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["event"], "merge_duplicate_definition");
        assert_eq!(events[0]["name"], "buildScene");
        assert_eq!(
            events[0]["shards"],
            serde_json::json!(["web-viz-render", "web-viz-pick"])
        );
    }

    /// Python: decorators and lead comments belong to the def below them; the module docstring,
    /// imports and the `if __name__` block are statements/imports; a class block carries its
    /// methods' names, so an export naming a method places the class.
    #[test]
    fn python_pieces_segment_by_column_zero_with_decorators_and_lead_comments() {
        let src = "\"\"\"Store module.\n\nSpans two lines at column 0.\n\"\"\"\nimport os\nfrom pathlib import Path\n\nDB = Path(\"x.db\")\n\n# the store\n@dataclass\nclass Store:\n    path: str\n\n    def load(self):\n        return 1\n\n\ndef helper(a, b=DB):\n    return a\n\nif __name__ == \"__main__\":\n    helper(1)\n";
        let segs = segments(src, TargetLang::Python);
        let kinds: Vec<(usize, bool, Vec<String>)> = segs
            .iter()
            .map(|s| (s.line, s.import, s.names.clone()))
            .collect();
        let expected: Vec<(usize, bool, Vec<String>)> = vec![
            (1, false, vec![]),
            (5, true, vec![]),
            (6, true, vec![]),
            (8, false, vec![]),
            (10, false, vec!["Store".to_string(), "load".to_string()]),
            (19, false, vec!["helper".to_string()]),
            (22, false, vec![]),
        ];
        assert_eq!(kinds, expected, "{segs:?}");
        assert!(segs[4]
            .text
            .starts_with("# the store\n@dataclass\nclass Store:"));
        assert!(segs[0].text.ends_with("\"\"\"\n"), "{:?}", segs[0].text);
        let root = tmp("py");
        let shards = vec![shard(&root, "store", "core", &[("core.py", src)])];
        let dossier = MergeDossier {
            module: "store".into(),
            files: vec!["app/store.py".into()],
            interface: ModuleInterface {
                exports: vec![export("Store.load", "load(self) -> int")],
                ..Default::default()
            },
            shards,
            ..Default::default()
        };
        let AssemblyOutcome::Assembled(a) = assemble(&root, &dossier) else {
            panic!("py assembles");
        };
        assert_eq!(a.path, ".swarm/shards/store/ASSEMBLED.py");
        assert_eq!(a.imports, 2);
        assert_eq!(a.ordered_by_interface, 1, "the class block, via its method");
        assert_eq!(
            a.appended_unknown,
            vec![("store-core".to_string(), "helper".to_string())]
        );
        assert_eq!(a.statements, vec![("store-core".to_string(), 3)]);
        let text = std::fs::read_to_string(root.join(&a.path)).unwrap();
        assert!(text.starts_with("# ASSEMBLED BY THE ENGINE"), "{text}");
        assert!(text.contains("# shard: store-core (.swarm/shards/store/core/core.py:10) — Store, load\n# the store\n@dataclass\nclass Store:"), "{text}");
    }

    /// Said, never faked: a `.rs` module, mixed final-file extensions, and a split whose shards left
    /// no piece of the module's language are each `Unavailable` with the reason.
    #[test]
    fn an_unknown_extension_mixed_files_or_no_piece_is_unavailable_and_said() {
        let root = tmp("unavailable");
        let rs = MergeDossier {
            module: "lib".into(),
            files: vec!["src/lib.rs".into()],
            ..Default::default()
        };
        assert_eq!(
            assemble(&root, &rs),
            AssemblyOutcome::Unavailable {
                ext: "rs".into(),
                reason: "no per-file parser for this extension (py, js, mjs, cjs are assembled)"
                    .into()
            }
        );
        let mixed = MergeDossier {
            module: "web".into(),
            files: vec!["web/app.js".into(), "web/index.html".into()],
            ..Default::default()
        };
        let AssemblyOutcome::Unavailable { ext, reason } = assemble(&root, &mixed) else {
            panic!("mixed extensions are unavailable");
        };
        assert_eq!(ext, "js");
        assert!(reason.contains("mix extensions"), "{reason}");
        // r6e's shape: eight shards, every folder README-only or empty — nothing to assemble.
        let shards = vec![shard(
            &root,
            "viz",
            "a",
            &[("README.md", "PROVIDES: x\n"), ("notes.txt", "n")],
        )];
        let empty = MergeDossier {
            module: "viz".into(),
            files: vec!["web/viz.js".into()],
            shards,
            ..Default::default()
        };
        let AssemblyOutcome::Unavailable { ext, reason } = assemble(&root, &empty) else {
            panic!("no js piece is unavailable");
        };
        assert_eq!(ext, "js");
        assert!(
            reason.contains("no piece with the module's `.js` extension")
                && reason.contains("1 other file(s) skipped"),
            "{reason}"
        );
        assert!(
            !root.join(".swarm/shards/viz/ASSEMBLED.js").exists(),
            "nothing written when unavailable"
        );
        let ev = unavailable_event("viz", "viz", &ext, &reason);
        assert_eq!(ev["event"], "merge_assembly_unavailable");
        assert_eq!(ev["ext"], "js");
    }

    /// The r6e declaration (run.jsonl seq 522/548: `viz3d-engine`, 8 shards, 23 exports — the
    /// names verbatim from `merge_dossier.declared_missing`, which listed every one because no
    /// piece existed). With the pieces the eight shards were briefed to write, assembly places the
    /// `vs7dbg` object first (exports[1] `vs7dbg.layout` names a method in it), then the `viz3d`
    /// object, then the eleven functions in export order; the object itself (`vs7dbg`, kind
    /// object, no function defines it) is the one declared name left as a GAP; the streaming
    /// shard's helpers and `boot` are appended after; the load listener is glue at the end.
    #[test]
    fn the_r6e_declaration_orders_eight_shards_pieces_by_its_twenty_three_exports() {
        let root = tmp("r6e");
        let m = "viz3d-engine";
        let shards = vec![
            shard(&root, m, "data-scene", &[("scene.js", "function buildInstance(rec, i) {}\nfunction computeSceneDigest() { return 'd'; }\n")]),
            shard(&root, m, "rendering-core", &[("render.js", "const VS = `\nattribute vec3 aPos;\nvoid main() {}\n`;\nfunction project(x, y, z) {}\nfunction renderFrame() {}\nfunction requestRender() {}\n")]),
            shard(&root, m, "pick-buffer", &[("pick.js", "function refreshPick() {}\nfunction pickAt(sx, sy) {}\nfunction pickPixelAt(sx, sy) {}\n")]),
            shard(&root, m, "camera-inertia", &[("camera.js", "function maybeClick(e) {}\n")]),
            shard(&root, m, "labels-culling", &[("labels.js", "function updateLabels() {}\n")]),
            shard(&root, m, "linked-brush", &[("brush.js", "function applyBrushDim(ids) {}\nwindow.viz3d = {\n  toggle(id) {},\n  clear() {},\n  setBrush(ids) {},\n};\n")]),
            shard(&root, m, "streaming-diffs", &[("stream.js", "function applyDiff(d) {}\nfunction streamLoop() {}\n")]),
            shard(&root, m, "vs7dbg-boot", &[("boot.js", "window.vs7dbg = {\n  layout() { return L; },\n  sceneDigest() { return computeSceneDigest(); },\n  camera() { return S; },\n  setCamera(yaw, pitch, dist) {},\n  pick(sx, sy) { return pickAt(sx, sy); },\n  pickPixel(sx, sy) { return pickPixelAt(sx, sy); },\n  brush(ids) {},\n  frames() { return F; },\n};\nfunction boot() {}\nwindow.addEventListener('load', boot);\n")]),
        ];
        let names = [
            "vs7dbg",
            "vs7dbg.layout",
            "vs7dbg.sceneDigest",
            "vs7dbg.camera",
            "vs7dbg.setCamera",
            "vs7dbg.pick",
            "vs7dbg.pickPixel",
            "vs7dbg.brush",
            "vs7dbg.frames",
            "viz3d.toggle",
            "viz3d.clear",
            "viz3d.setBrush",
            "project",
            "renderFrame",
            "requestRender",
            "refreshPick",
            "pickAt",
            "pickPixelAt",
            "maybeClick",
            "applyBrushDim",
            "buildInstance",
            "computeSceneDigest",
            "updateLabels",
        ];
        assert_eq!(names.len(), 23, "the archived exports_declared");
        let dossier = MergeDossier {
            module: m.into(),
            files: vec!["web/viz.js".into()],
            interface: ModuleInterface {
                exports: names.iter().map(|n| export(n, &format!("{n}()"))).collect(),
                shared_state: "S = {yaw, pitch, distance, brush: Set<id>, count, dirty}".into(),
                layout: vec!["constants".into(), "state S".into(), "boot".into()],
            },
            shards,
            ..Default::default()
        };
        let AssemblyOutcome::Assembled(a) = assemble(&root, &dossier) else {
            panic!("eight js shards assemble");
        };
        assert_eq!(a.pieces, 8);
        assert_eq!(a.definitions, 16, "{a:?}");
        assert_eq!(
            a.ordered_by_interface, 13,
            "2 objects + 11 functions: {a:?}"
        );
        assert_eq!(
            a.appended_unknown,
            vec![
                (
                    "viz3d-engine-streaming-diffs".to_string(),
                    "applyDiff".to_string()
                ),
                (
                    "viz3d-engine-streaming-diffs".to_string(),
                    "streamLoop".to_string()
                ),
                ("viz3d-engine-vs7dbg-boot".to_string(), "boot".to_string()),
            ]
        );
        assert_eq!(a.declared_missing, vec!["vs7dbg".to_string()]);
        assert_eq!(
            a.statements,
            vec![
                ("viz3d-engine-rendering-core".to_string(), 1),
                ("viz3d-engine-vs7dbg-boot".to_string(), 1)
            ]
        );
        assert!(a.duplicates.is_empty());
        let text = std::fs::read_to_string(root.join(&a.path)).unwrap();
        let first_def = position(&text, "\n// shard: ");
        assert!(
            text.split_at(first_def).1.starts_with("\n// shard: viz3d-engine-vs7dbg-boot (.swarm/shards/viz3d-engine/vs7dbg-boot/boot.js:1) — layout, sceneDigest, camera, setCamera, pick, pickPixel, brush, frames\nwindow.vs7dbg = {"),
            "the vs7dbg object leads, via exports[1]: {text}"
        );
        let viz3d = position(&text, "window.viz3d = {");
        let project = position(&text, "function project(x, y, z) {}");
        let render_frame = position(&text, "function renderFrame() {}");
        let update_labels = position(&text, "function updateLabels() {}");
        let apply_diff = position(&text, "function applyDiff(d) {}");
        let boot = position(&text, "function boot() {}");
        let shader = position(
            &text,
            "— top-level statement\nconst VS = `\nattribute vec3 aPos;",
        );
        let listener = position(&text, "window.addEventListener('load', boot);");
        assert!(
            first_def < viz3d && viz3d < project && project < render_frame,
            "{text}"
        );
        assert!(
            render_frame < update_labels && update_labels < apply_diff && apply_diff < boot,
            "{text}"
        );
        assert!(
            boot < shader && shader < listener,
            "statements last, the template literal whole: {text}"
        );
        for sh in &dossier.shards {
            for (path, _, _) in &sh.pieces {
                let src = std::fs::read_to_string(root.join(path)).unwrap();
                for line in src.lines().filter(|l| !l.trim().is_empty()) {
                    assert!(text.contains(line), "piece line lost: `{line}`");
                }
            }
        }
    }
}
