//! The worker brief's "API of <file>" blocks — the REAL source of every plan file that exists on
//! disk and the task does not own (AGENTS.md: "workers read real dependency sources (dep_block)").
//! Sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases); extracted from `run_task_inner` for VA-103.
//!
//! `all_files` is the plan's owned files SORTED and DEDUPED (scheduler.rs, the dispatch's
//! `all_files.sort(); all_files.dedup();`), not plan order — so the budget is spent alphabetically:
//! in r6h `app/__main__.py` came before `app/ledgerd/__init__.py`, which came before the notifierd
//! skeleton.
//!
//! Two ceilings live here and both are literals older than any measurement: `DEP_SOURCES_BUDGET_CHARS`
//! (14,000, 350fca46b 2026-06-27, "bound context on slow local models") and `DEP_SOURCE_FILE_CHARS`
//! (3,500, same origin; the line-boundary cut and its marker came with afc90a2cd 2026-08-03). They are
//! kept, not because the numbers are right, but because the measured direction is the other way:
//! r6e's briefs went 6k → 21k chars while BUILD went 325m → 608m (gate 9's brief-diet receipt), so
//! an unbounded file-count × file-size product is not the fix. What VA-103 adds is what a ceiling
//! needs to be honest under gate 1:
//!
//! 1. A dependency that IS THE TASK'S CONTRACT is never cut. r6h (2026-09-02 02:36): `ledgerd-core`
//!    owned `app/ledgerd/impl.py`; the skeleton's `app/ledgerd/__init__.py` (6,104 bytes) holds
//!    `def run(...)` at byte 5,128 with `from . import impl` and `return impl.run(db_dir=..., port=...,
//!    notifier_url=..., vendor_url=..., tokens_file=...)` — the exact signature the task had to
//!    deliver — and the 3,500-char cut kept 3,481 bytes (97 lines, inside `SkeletonHandler`'s body
//!    read loop). The worker's first words were "let me check the remaining part of
//!    `app/ledgerd/__init__.py` (it was truncated)" and its second call was `sed -n '120,260p'`.
//!    The independent tracer measured that recovery at ~1.5 lane-minutes (the docstring's one-line
//!    contract was inside the old cut), so this is a LOUDNESS and CORRECTNESS fix, not a time win.
//!    A file is the contract when it REFERENCES the owned module in code syntax — an import
//!    (`from . import impl`, `from .impl import`, `import app.db`, `from app import db`,
//!    `from app.db import`, `mod impl;`), a qualified attribute (`app.db.Store`), or a path literal of
//!    the owned file (`app/ledgerd/impl.py`, `ledgerd::impl`, `./impl`) — never a bare word: the
//!    first cut of this rule matched `--db-dir` in `app/__main__.py`'s usage text and labelled that
//!    file the contract for `app/db.py`. Bare forms (`from . import x`, `import x`, `mod x;`) count
//!    only from the module's own directory; every other file must name it qualified. The contract is
//!    carried WHOLE and debits the budget so the rest stays bounded.
//! 2. Every cut and every omission is LOUD: a `DepSourceCut` the caller emits as
//!    `dep_source_truncated{task_id, file, bytes, kept, reason}`, and a marker in the brief at the cut
//!    point that carries the exact `sed -n 'A,Bp' <file>` recovering what is missing — a handoff, not
//!    a hint. The old loop `break`-ed silently once the budget was spent, so every later file was
//!    ABSENT from the block with no trace; now it is named with `kept: 0`.
//! 3. VA-126: the two ceilings below are REFERENCE values on the 262,144 window; the live pair
//!    arrives as `budget_chars` / `file_chars` from `budgets::ShownBudgets`, scaled from the
//!    fleet's probed window (byte-identical on this fleet, proportional on a 1M-window model).

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use super::{shape_excerpt, TargetLang};

/// Reference (262,144-window) value of the whole "API of" block — the live budget is
/// `ShownBudgets::dep_sources_chars`.
pub(super) const DEP_SOURCES_BUDGET_CHARS: usize = 14_000; // measured: r6h value on the 262,144 reference window (r6h-golden-0.4616)
/// Reference (262,144-window) value of one dependency source inside the block — the live budget
/// is `ShownBudgets::dep_source_file_chars`.
pub(super) const DEP_SOURCE_FILE_CHARS: usize = 3_500; // measured: r6h value on the 262,144 reference window (r6h-golden-0.4616)

pub(super) const CUT_PER_FILE_CAP: &str = "per_file_cap";
pub(super) const CUT_BUDGET_EXHAUSTED: &str = "dep_budget_exhausted";
/// The one reason a dependency rides whole past the per-file ceiling (module doc, point 1).
pub(super) const CARRIED_NAMES_OWNED_MODULE: &str = "names_owned_module";

/// One dependency source the block did not carry whole. `kept == 0` means the file was named but
/// not shown at all (the budget was spent before it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DepSourceCut {
    pub(super) file: String,
    pub(super) bytes: usize,
    pub(super) kept: usize,
    pub(super) reason: &'static str,
}

/// One dependency carried WHOLE because it names the task's own module (module doc, point 1).
/// VA-115: the carry debits the block's budget exactly like a shown file and was the one arm
/// with no event — a 30k importer riding whole spent the budget of every file after it unseen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DepSourceCarried {
    pub(super) file: String,
    pub(super) chars: usize,
    /// The owned file it references, and the reference form that matched.
    pub(super) named: String,
    pub(super) form: String,
    /// The block budget left AFTER this file debited it.
    pub(super) budget_left: usize,
}

#[derive(Debug, Default)]
pub(super) struct DepSourcesBlock {
    pub(super) text: String,
    pub(super) cuts: Vec<DepSourceCut>,
    pub(super) carried: Vec<DepSourceCarried>,
    /// The pair IN FORCE for this block (`ShownBudgets`, VA-126) — what the events report, never
    /// the reference literals.
    pub(super) budget_chars: usize,
    pub(super) file_chars: usize,
}

pub(super) fn sig_lang(lang: TargetLang) -> goose_swarm::SigLang {
    match lang {
        TargetLang::Python => goose_swarm::SigLang::Python,
        TargetLang::Rust => goose_swarm::SigLang::Rust,
        TargetLang::Go => goose_swarm::SigLang::Go,
        TargetLang::TypeScript => goose_swarm::SigLang::TypeScript,
        TargetLang::Other => goose_swarm::SigLang::Other,
    }
}

fn comment_leader(lang: TargetLang) -> &'static str {
    match lang {
        TargetLang::Rust | TargetLang::Go | TargetLang::TypeScript => "//",
        TargetLang::Python | TargetLang::Other => "#",
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn word_end(c: Option<char>) -> bool {
    c.is_none_or(|c| !is_ident_char(c))
}

fn ident_start(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphabetic() || c == '_')
}

/// `needle` occurs in `text` with a non-identifier character (or the start) before it and a
/// character satisfying `after` following it (`None` = end of text).
// string_slice: `i` is a `match_indices` hit and `i + needle.len()` its end.
#[allow(clippy::string_slice)]
fn occurs(text: &str, needle: &str, after: impl Fn(Option<char>) -> bool) -> bool {
    text.match_indices(needle).any(|(i, _)| {
        text[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !is_ident_char(c))
            && after(text[i + needle.len()..].chars().next())
    })
}

/// A `<from_prefix> a, b as c` line whose imported names include `stem` as a whole word.
fn imports_name(text: &str, from_prefix: &str, stem: &str) -> bool {
    text.lines().any(|line| {
        line.trim_start()
            .strip_prefix(from_prefix)
            .is_some_and(|names| occurs(names, stem, word_end))
    })
}

/// The module a caller names for an owned file. A package marker (`__init__.py`, `mod.rs`,
/// `index.ts`) is named by its directory, from the directory above.
struct OwnedModule<'a> {
    stem: &'a str,
    dir: Option<&'a Path>,
    /// The directory's last component (`ledgerd` for `app/ledgerd/impl.py`); `None` at the root.
    pkg: Option<&'a str>,
    /// `app.ledgerd` for `app/ledgerd/impl.py`; empty at the root.
    parent_dotted: String,
    /// `app.ledgerd.impl` for `app/ledgerd/impl.py`.
    dotted: String,
}

fn owned_module(owned: &str) -> Option<OwnedModule<'_>> {
    let path = Path::new(owned);
    let mut stem = path.file_stem()?.to_str()?;
    let mut dir = path.parent();
    if matches!(stem, "__init__" | "mod" | "index") {
        if let Some(pkg) = dir.and_then(|d| d.file_name()).and_then(|n| n.to_str()) {
            stem = pkg;
            dir = dir.and_then(|d| d.parent());
        }
    }
    let components: Vec<&str> = dir
        .into_iter()
        .flat_map(|d| d.components())
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let parent_dotted = components.join(".");
    let dotted = if parent_dotted.is_empty() {
        stem.to_string()
    } else {
        format!("{parent_dotted}.{stem}")
    };
    Some(OwnedModule {
        stem,
        dir,
        pkg: components.last().copied(),
        parent_dotted,
        dotted,
    })
}

/// The reference syntax by which `content` names `m`, if any — returned verbatim so the brief can
/// quote what matched. Qualified forms are valid from any directory; bare forms only from the
/// module's own (`same_dir`), because a bare `impl` elsewhere is somebody else's `impl`.
fn reference_form(content: &str, m: &OwnedModule<'_>, same_dir: bool) -> Option<String> {
    let stem = m.stem;
    let dotted = &m.dotted;
    // A root-level module has nothing to qualify with (`dotted == stem`): its import forms are the
    // bare ones below, gated on the directory like every other bare form.
    if !m.parent_dotted.is_empty() {
        let import_dotted = format!("import {dotted}");
        if occurs(content, &import_dotted, word_end) {
            return Some(import_dotted);
        }
        let from_dotted = format!("from {dotted} import");
        if occurs(content, &from_dotted, |_| true) {
            return Some(from_dotted);
        }
        let from_parent = format!("from {} import", m.parent_dotted);
        if imports_name(content, &from_parent, stem) {
            return Some(format!("{from_parent} {stem}"));
        }
        let attribute = format!("{dotted}.");
        if occurs(content, &attribute, ident_start) {
            return Some(attribute);
        }
    }
    if let Some(pkg) = m.pkg {
        for sep in ["/", "::"] {
            let path_literal = format!("{pkg}{sep}{stem}");
            if occurs(content, &path_literal, word_end) {
                return Some(path_literal);
            }
        }
    }
    if !same_dir {
        return None;
    }
    if imports_name(content, "from . import", stem) {
        return Some(format!("from . import {stem}"));
    }
    let relative = format!("from .{stem}");
    if occurs(content, &relative, |c| matches!(c, Some(' ') | Some('.'))) {
        return Some(relative);
    }
    let import_bare = format!("import {stem}");
    if occurs(content, &import_bare, word_end) {
        return Some(import_bare);
    }
    let from_bare = format!("from {stem} import");
    if occurs(content, &from_bare, |_| true) {
        return Some(from_bare);
    }
    let mod_decl = format!("mod {stem}");
    if occurs(content, &mod_decl, |c| {
        matches!(c, Some(';') | Some(' ') | Some('{'))
    }) {
        return Some(mod_decl);
    }
    let ts_relative = format!("./{stem}");
    if occurs(content, &ts_relative, word_end) {
        return Some(ts_relative);
    }
    None
}

/// Is the dependency at `dep_path` (with `content`) a CALLER of one of `owned_files` — a file that
/// names, in reference syntax, the module this task must deliver? Returns the owned file it names
/// and the form that matched. Such a file is the task's contract and is carried whole.
pub(super) fn names_owned_module<'a>(
    dep_path: &str,
    content: &str,
    owned_files: &'a [String],
) -> Option<(&'a String, String)> {
    let dep_dir = Path::new(dep_path).parent();
    owned_files.iter().find_map(|owned| {
        let m = owned_module(owned)?;
        let form = reference_form(content, &m, dep_dir == m.dir)?;
        Some((owned, form))
    })
}

fn targeted_read_hint(f: &str) -> String {
    format!("`grep -n '<name>' {f}` then `sed -n 'A,Bp' {f}` — never a whole-file cat")
}

/// Every plan file on disk the task does not own, as fenced "## API of <file>" sections, plus the
/// list of cuts the caller must emit. Test files, non-source files, missing and empty files are
/// skipped exactly as before; `signatures_on` (GOOSE_SWARM_DEP_SIGNATURES, ships OFF) swaps the body
/// for `shape_excerpt` and falls back to the body when the excerpt is empty.
// string_slice (the recovery line): `leading` is where `content.trim_start()` begins and `kept` is
// the byte length of `whole` — a prefix of `trimmed` (a slice of `content`) cut at a `\n` — so the
// sum is a char boundary of `content` by construction.
#[allow(clippy::string_slice)]
pub(super) fn dependency_sources_block(
    root: &Path,
    owned_files: &[String],
    all_files: &[String],
    lang: TargetLang,
    signatures_on: bool,
    budget_chars: usize,
    file_chars: usize,
) -> DepSourcesBlock {
    let owned_set: std::collections::HashSet<&String> = owned_files.iter().collect();
    let leader = comment_leader(lang);
    let mut out = DepSourcesBlock {
        budget_chars,
        file_chars,
        ..Default::default()
    };
    let mut budget = budget_chars;
    for f in all_files {
        if owned_set.contains(f) || !lang.is_source_file(f) {
            continue;
        }
        let base = Path::new(f)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if lang.is_test_file(base) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        let trimmed = content.trim();
        if trimmed.is_empty() {
            continue;
        }
        let excerpt = if signatures_on {
            Some(shape_excerpt(trimmed, sig_lang(lang))).filter(|e| !e.trim().is_empty())
        } else {
            None
        };
        let is_excerpt = excerpt.is_some();
        let api_source: Cow<str> = match excerpt {
            Some(e) => Cow::Owned(e),
            None => Cow::Borrowed(trimmed),
        };
        let total_chars = api_source.chars().count();
        let bytes = api_source.len();
        let hint = targeted_read_hint(f);

        if let Some((named, form)) = names_owned_module(f, &api_source, owned_files) {
            budget = budget.saturating_sub(total_chars);
            out.carried.push(DepSourceCarried {
                file: f.clone(),
                chars: total_chars,
                named: named.clone(),
                form: form.clone(),
                budget_left: budget,
            });
            out.text.push_str(&format!(
                "## API of {f} — CARRIED WHOLE: this file references your module `{named}` \
                 (`{form}`), so it is a caller of what you own; build to the signatures it \
                 uses:\n```\n{api_source}\n```\n\n"
            ));
            continue;
        }
        if budget == 0 {
            out.cuts.push(DepSourceCut {
                file: f.clone(),
                bytes,
                kept: 0,
                reason: CUT_BUDGET_EXHAUSTED,
            });
            out.text.push_str(&format!(
                "## API of {f} — NOT SHOWN ({bytes} bytes): the {budget_chars}-char \
                 dependency-source budget was spent on the files above. Read it TARGETED before \
                 calling into it: {hint}.\n\n"
            ));
            continue;
        }
        let cap = budget.min(file_chars);
        if total_chars <= cap {
            budget -= total_chars;
            out.text.push_str(&format!(
                "## API of {f} (a dependency you import — build against THIS; for any symbol, key or \
                 route it does not show, read the real file TARGETED: {hint}):\n```\n{api_source}\n```\n\n"
            ));
            continue;
        }
        // CUT ON A LINE BOUNDARY AND SAY SO. The raw `.take(n)` sliced mid-identifier and the fence
        // closed unconditionally, so the worker received a file that stopped in the middle of a `def`
        // formatted exactly like a complete one (F196: 3 of 4 blocks ended mid-token, `meridian.py`
        // at `    def _up`, and none said so). The marker now carries the exact recovery command.
        let head: String = api_source.chars().take(cap).collect();
        let reason = if cap < file_chars {
            CUT_BUDGET_EXHAUSTED
        } else {
            CUT_PER_FILE_CAP
        };
        // No line boundary inside the room left (the budget's last few chars, or a one-line
        // minified file): a fragment of one line is not a view of anything — name the file, show
        // nothing, and say why.
        let Some((whole, _)) = head.rsplit_once('\n') else {
            out.cuts.push(DepSourceCut {
                file: f.clone(),
                bytes,
                kept: 0,
                reason,
            });
            out.text.push_str(&format!(
                "## API of {f} — NOT SHOWN ({bytes} bytes): its first line alone is longer than the \
                 {cap} chars left for it. Read it TARGETED before calling into it: {hint}.\n\n"
            ));
            continue;
        };
        let kept = whole.len();
        out.cuts.push(DepSourceCut {
            file: f.clone(),
            bytes,
            kept,
            reason,
        });
        budget = budget.saturating_sub(whole.chars().count());
        let recovery = if is_excerpt {
            format!("read the file itself: {hint}")
        } else {
            // `trimmed` is a slice of `content`, so line numbers are the FILE's, not the excerpt's.
            let leading = content.len() - content.trim_start().len();
            let next_line = content[..leading + kept].lines().count() + 1;
            let last_line = content.lines().count();
            format!(
                "lines {next_line}-{last_line} of {f} are NOT shown; read them before building \
                 against this file: `sed -n '{next_line},{last_line}p' {f}`"
            )
        };
        out.text.push_str(&format!(
            "## API of {f} (a dependency you import — build against THIS; for any symbol, key or \
             route it does not show, read the real file TARGETED: {hint}):\n```\n{whole}\n\
             {leader} … [dep source TRUNCATED at {kept} of {bytes} bytes — {recovery}]\n```\n\n"
        ));
    }
    out
}

impl DepSourcesBlock {
    /// One `dep_source_truncated{task_id, file, bytes, kept, reason}` per cut — the loud half of
    /// the budget (module doc, point 2); the caller writes each to the run's event stream.
    pub(super) fn cut_events(&self, task_id: &str) -> Vec<serde_json::Value> {
        self.cuts
            .iter()
            .map(|cut| {
                serde_json::json!({
                    "event": "dep_source_truncated",
                    "task_id": task_id,
                    "file": cut.file,
                    "bytes": cut.bytes,
                    "kept": cut.kept,
                    "reason": cut.reason,
                })
            })
            .collect()
    }

    /// VA-115: one `dep_source_carried_whole{task, dep_task, file, chars, reason, named, form,
    /// budget_chars, file_chars, budget_left}` per contract file — the carry as loud as the cut
    /// (point 2 of the module doc made only the cuts visible). `budget_chars` / `file_chars` are
    /// the pair in force, never the reference literals; `dep_task` is the task the caller's
    /// ownership map (task id -> owned files) names for the file, null when the map does not
    /// hold it — an absence stated, never a guessed owner.
    pub(super) fn carried_events(
        &self,
        task_id: &str,
        owners: &HashMap<String, Vec<String>>,
    ) -> Vec<serde_json::Value> {
        self.carried
            .iter()
            .map(|c| {
                let dep_task = owners
                    .iter()
                    .find(|(_, files)| files.iter().any(|f| *f == c.file))
                    .map(|(task, _)| task.as_str());
                serde_json::json!({
                    "event": "dep_source_carried_whole",
                    "task": task_id,
                    "dep_task": dep_task,
                    "file": c.file,
                    "chars": c.chars,
                    "reason": CARRIED_NAMES_OWNED_MODULE,
                    "named": c.named,
                    "form": c.form,
                    "budget_chars": self.budget_chars,
                    "file_chars": self.file_chars,
                    "budget_left": c.budget_left,
                })
            })
            .collect()
    }

    /// The brief's dependency section: the "API of …" blocks, or — D3, a FIRST-WAVE task with no
    /// dependency file on disk yet — the same heading with a redirect. The worker prompts point at
    /// "'API of …'" as the authoritative surface; an empty block would point the worker at a
    /// section that is not there, and emitting the heading WITH the redirect makes every pointer
    /// true without touching the prompts that carry them. (The redirect once pointed at the FROZEN
    /// MODULE INTERFACES bundle; that died with CONTRACTS, P1-4 — the plan manifest is the naming
    /// authority.)
    pub(super) fn text_or_none_on_disk(self) -> String {
        if self.text.is_empty() {
            NONE_ON_DISK_YET.to_string()
        } else {
            self.text
        }
    }
}

const NONE_ON_DISK_YET: &str = "## API of dependencies — NONE ON DISK YET\n\
    No dependency source exists on disk yet (your siblings are still building). The \
    PROJECT FILE LAYOUT above is the naming authority: import your dependencies from \
    EXACTLY those paths, and once a dependency lands on disk read its real source \
    (`grep -n`/`sed -n`) before writing calls against it.\n\n";

#[cfg(test)]
mod tests {
    use super::*;

    /// r6h's files as the engine saw them at ledgerd-core's dispatch (2026-09-02 02:36:52Z), copied
    /// from the run tree byte for byte.
    const R6H_LEDGERD_INIT: &str = include_str!("testdata/va103/ledgerd__init__.py");
    const R6H_NOTIFIERD_INIT: &str = include_str!("testdata/va103/notifierd__init__.py");
    const R6H_APP_MAIN: &str = include_str!("testdata/va103/app__main__.py");

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dep-sources-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("app/ledgerd")).unwrap();
        std::fs::create_dir_all(dir.join("app/notifierd")).unwrap();
        dir
    }

    fn r6h_owned() -> Vec<String> {
        [
            "app/__init__.py",
            "app/ledgerd/impl.py",
            "app/db.py",
            "app/sync.py",
            "app/ledger.py",
            "app/relay.py",
            "app/api.py",
            "README.md",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    /// r6h, 02:36:52: ledgerd-core's brief cut `app/ledgerd/__init__.py` at 3,481 of 6,104 bytes and
    /// the `def run(...)` / `impl.run(...)` contract at byte 5,128 fell off the end. The file names
    /// the owned module by path (`app/ledgerd/impl.py`, docstring line 7) and by relative import
    /// (`from . import impl`, line 155): the contract, carried whole, no cut. The notifierd skeleton
    /// (3,746 trimmed bytes, names ITS OWN impl) is not this task's contract: cut on a line boundary
    /// at 3,470 bytes, loudly, with `sed -n '108,115p'`. `app/__main__.py` mentions `--db-dir` in
    /// prose and is NOT the contract for `app/db.py`; it fits under the cap and is carried plain.
    // string_slice: every index is a `find`/`rfind` hit moved past ASCII fence text (the byte before
    // the marker is the `\n` the cut leaves), or the length of `kept_text`, asserted a prefix of
    // `trimmed` before it is used.
    #[allow(clippy::string_slice)]
    #[test]
    fn r6h_the_contract_caller_is_carried_whole_and_the_other_cut_is_loud() {
        assert_eq!(R6H_LEDGERD_INIT.len(), 6104);
        assert_eq!(R6H_LEDGERD_INIT.find("\ndef run(").unwrap() + 1, 5128);
        let root = scratch("r6h");
        std::fs::write(root.join("app/ledgerd/__init__.py"), R6H_LEDGERD_INIT).unwrap();
        std::fs::write(root.join("app/notifierd/__init__.py"), R6H_NOTIFIERD_INIT).unwrap();
        std::fs::write(root.join("app/__main__.py"), R6H_APP_MAIN).unwrap();
        let owned = r6h_owned();
        let mut all = owned.clone();
        all.extend(
            [
                "app/__main__.py",
                "app/ledgerd/__init__.py",
                "app/ledgerd/__main__.py",
                "app/notifierd/__init__.py",
                "app/notifierd/__main__.py",
            ]
            .iter()
            .map(|s| s.to_string()),
        );

        let block = dependency_sources_block(
            &root,
            &owned,
            &all,
            TargetLang::Python,
            false,
            DEP_SOURCES_BUDGET_CHARS,
            DEP_SOURCE_FILE_CHARS,
        );

        // The contract, whole: the signature the task must implement is IN the brief.
        assert!(
            block.text.contains(
                "def run(db_dir, port, notifier_url=None, vendor_url=None, tokens_file=None):"
            ),
            "run() must reach the worker:\n{}",
            block.text
        );
        assert!(block.text.contains("return impl.run("));
        assert!(block.text.contains(
            "## API of app/ledgerd/__init__.py — CARRIED WHOLE: this file references your module \
             `app/ledgerd/impl.py` (`ledgerd/impl`)"
        ));
        assert!(
            !block
                .cuts
                .iter()
                .any(|c| c.file == "app/ledgerd/__init__.py"),
            "the contract is never cut: {:?}",
            block.cuts
        );

        // `--db-dir` in usage prose is not a reference to app/db.py: plain, whole, unlabelled.
        assert_eq!(
            names_owned_module("app/__main__.py", R6H_APP_MAIN, &owned),
            None
        );
        assert!(block
            .text
            .contains("## API of app/__main__.py (a dependency you import — build against THIS"));
        assert!(!block
            .text
            .contains("## API of app/__main__.py — CARRIED WHOLE"));
        assert!(block
            .text
            .contains("from .ledgerd import run as ledgerd_run"));

        // The non-contract sibling: cut on a line boundary, named, with the recovery command.
        let cut = block
            .cuts
            .iter()
            .find(|c| c.file == "app/notifierd/__init__.py")
            .expect("the notifierd skeleton is over the per-file cap and is cut loudly");
        assert_eq!(
            (cut.bytes, cut.kept, cut.reason),
            (3746, 3470, CUT_PER_FILE_CAP),
            "{cut:?}"
        );
        assert!(block.text.contains(
            "# … [dep source TRUNCATED at 3470 of 3746 bytes — lines 108-115 of \
             app/notifierd/__init__.py are NOT shown; read them before building against this file: \
             `sed -n '108,115p' app/notifierd/__init__.py`]"
        ));
        let marker = "# … [dep source TRUNCATED at 3470 of 3746 bytes";
        let at = block.text.find(marker).unwrap();
        let section_start = block
            .text
            .find("## API of app/notifierd/__init__.py")
            .unwrap();
        let fence = block.text[section_start..at].rfind("```\n").unwrap() + section_start + 4;
        let kept_text = &block.text[fence..at - 1];
        assert_eq!(kept_text.len(), 3470);
        let trimmed = R6H_NOTIFIERD_INIT.trim();
        assert!(
            trimmed.starts_with(kept_text),
            "the kept text is a prefix of the source"
        );
        assert!(
            trimmed[kept_text.len()..].starts_with('\n'),
            "the kept prefix ends on a line boundary"
        );
        assert_eq!(block.cuts.len(), 1, "{:?}", block.cuts);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The old loop `break`-ed once the budget was spent, so later files were absent with no
    /// trace. Now the file is named in the brief and reported with `kept: 0`.
    #[test]
    fn budget_exhaustion_names_the_omitted_file_instead_of_dropping_it() {
        let root = scratch("budget");
        let line = format!("{}\n", "x".repeat(49));
        let big = line.repeat(72); // 3,600 bytes, over the per-file cap
        let owned = vec!["app/ledgerd/impl.py".to_string()];
        let mut all = owned.clone();
        for i in 0..6 {
            let f = format!("app/dep{i}.py");
            std::fs::write(root.join(&f), &big).unwrap();
            all.push(f);
        }
        let block = dependency_sources_block(
            &root,
            &owned,
            &all,
            TargetLang::Python,
            false,
            DEP_SOURCES_BUDGET_CHARS,
            DEP_SOURCE_FILE_CHARS,
        );
        let first = block.cuts.iter().find(|c| c.file == "app/dep0.py").unwrap();
        assert_eq!(first.reason, CUT_PER_FILE_CAP);
        assert_eq!(
            first.kept, 3_499,
            "70 whole lines of 50 bytes, minus the final newline"
        );
        // 4 × 3,499 kept leaves 4 chars: dep4 gets no line boundary in that room and dep5 finds the
        // same 4 — both are NAMED with nothing shown, never silently absent.
        for f in ["app/dep4.py", "app/dep5.py"] {
            let cut = block.cuts.iter().find(|c| c.file == f).unwrap();
            assert_eq!(
                (cut.kept, cut.reason, cut.bytes),
                (0, CUT_BUDGET_EXHAUSTED, big.trim().len()),
                "{f}"
            );
            assert!(block
                .text
                .contains(&format!("## API of {f} — NOT SHOWN (3599 bytes)")));
            assert!(block.text.contains(&format!("`sed -n 'A,Bp' {f}`")));
        }
        assert_eq!(block.cuts.len(), 6, "{:?}", block.cuts);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// VA-115: the carry is as loud as the cut. r6h's contract file rides whole, and the event
    /// names the owner, the chars debited and the pair of budgets IN FORCE — the ones the caller
    /// passed (a 1M-window fleet scales them; the reference literals would then be a lie).
    #[test]
    fn the_contract_carry_is_reported_with_the_budget_in_force() {
        let root = scratch("carried");
        std::fs::write(root.join("app/ledgerd/__init__.py"), R6H_LEDGERD_INIT).unwrap();
        let owned = vec!["app/ledgerd/impl.py".to_string()];
        let all = vec![owned[0].clone(), "app/ledgerd/__init__.py".to_string()];
        let (budget, per_file) = (DEP_SOURCES_BUDGET_CHARS * 4, DEP_SOURCE_FILE_CHARS * 4);
        let block = dependency_sources_block(
            &root,
            &owned,
            &all,
            TargetLang::Python,
            false,
            budget,
            per_file,
        );
        let chars = R6H_LEDGERD_INIT.trim().chars().count();
        assert_eq!(
            block.carried,
            vec![DepSourceCarried {
                file: "app/ledgerd/__init__.py".to_string(),
                chars,
                named: "app/ledgerd/impl.py".to_string(),
                form: "ledgerd/impl".to_string(),
                budget_left: budget - chars,
            }]
        );
        assert!(block.cuts.is_empty());
        let mut owners = HashMap::new();
        owners.insert(
            "skeleton".to_string(),
            vec!["app/ledgerd/__init__.py".to_string()],
        );
        owners.insert("ledgerd-core".to_string(), owned.clone());
        let events = block.carried_events("ledgerd-core", &owners);
        assert_eq!(events.len(), 1, "{events:?}");
        let ev = &events[0];
        assert_eq!(ev["event"], "dep_source_carried_whole");
        assert_eq!(ev["task"], "ledgerd-core");
        assert_eq!(ev["dep_task"], "skeleton");
        assert_eq!(ev["file"], "app/ledgerd/__init__.py");
        assert_eq!(ev["chars"], chars);
        assert_eq!(ev["reason"], CARRIED_NAMES_OWNED_MODULE);
        assert_eq!(ev["form"], "ledgerd/impl");
        assert_eq!(ev["budget_chars"], budget);
        assert_eq!(ev["file_chars"], per_file);
        assert_eq!(ev["budget_left"], budget - chars);
        // A file the map does not hold is a stated absence, never a guessed owner.
        let unowned = block.carried_events("ledgerd-core", &HashMap::new());
        assert!(unowned[0]["dep_task"].is_null(), "{unowned:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A file under the cap, not a contract, is carried whole with no cut — byte-identical to the
    /// pre-VA-103 happy path.
    #[test]
    fn a_small_non_contract_dependency_is_whole_with_no_cut() {
        let root = scratch("small");
        std::fs::write(root.join("app/util.py"), "def helper():\n    return 1\n").unwrap();
        let owned = vec!["app/ledgerd/impl.py".to_string()];
        let all = vec![owned[0].clone(), "app/util.py".to_string()];
        let block = dependency_sources_block(
            &root,
            &owned,
            &all,
            TargetLang::Python,
            false,
            DEP_SOURCES_BUDGET_CHARS,
            DEP_SOURCE_FILE_CHARS,
        );
        assert!(block.cuts.is_empty());
        assert!(block
            .text
            .contains("## API of app/util.py (a dependency you import"));
        assert!(block.text.contains("def helper():\n    return 1\n```"));
        assert!(!block.text.contains("TRUNCATED"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The first cut of this rule was a prose word-match: `--db-dir` in `app/__main__.py`'s usage
    /// text made it "the contract for app/db.py". Only reference SYNTAX names a module.
    #[test]
    fn a_bare_word_in_prose_is_not_a_reference_but_an_import_of_the_same_name_is() {
        let db = vec!["app/db.py".to_string()];
        assert!(R6H_APP_MAIN.contains("--db-dir"));
        assert_eq!(
            names_owned_module("app/__main__.py", R6H_APP_MAIN, &db),
            None
        );
        // Positive control: the same file with one relative import of the module IS its caller.
        let with_import = format!("{R6H_APP_MAIN}\nfrom . import db\n");
        assert_eq!(
            names_owned_module("app/__main__.py", &with_import, &db),
            Some((&db[0], "from . import db".to_string()))
        );
        // `ledger.db` (a filename in a help string) is not a reference to app/ledger.py either.
        let ledger = vec!["app/ledger.py".to_string()];
        assert!(R6H_APP_MAIN.contains("ledger.db"));
        assert_eq!(
            names_owned_module("app/__main__.py", R6H_APP_MAIN, &ledger),
            None
        );
        // Nor is `from .ledgerd import` a reference to `app/ledger.py`.
        assert!(R6H_APP_MAIN.contains("from .ledgerd import run"));
    }

    #[test]
    fn names_owned_module_matches_reference_syntax_only() {
        let owned = vec!["app/ledgerd/impl.py".to_string()];
        let form =
            |dep: &str, text: &str| names_owned_module(dep, text, &owned).map(|(_, form)| form);
        // Same directory, bare import forms.
        assert_eq!(
            form("app/ledgerd/__init__.py", "from . import impl\n"),
            Some("from . import impl".into())
        );
        assert_eq!(
            form(
                "app/ledgerd/__init__.py",
                "from . import run, impl  # comment\n"
            ),
            Some("from . import impl".into())
        );
        assert_eq!(
            form("app/ledgerd/__main__.py", "from .impl import run\n"),
            Some("from .impl".into())
        );
        assert_eq!(
            form("app/ledgerd/__main__.py", "import impl\n"),
            Some("import impl".into())
        );
        // A bare call with no import is not enough — and prose never is.
        assert_eq!(form("app/ledgerd/__main__.py", "impl.run(1)\n"), None);
        assert_eq!(form("app/ledgerd/__main__.py", "the impl is late\n"), None);
        assert_eq!(
            form("app/ledgerd/__init__.py", "from . import impl_helpers\n"),
            None
        );
        assert_eq!(
            form("app/ledgerd/__init__.py", "from .impl_helpers import x\n"),
            None
        );
        // Another directory naming ITS OWN impl: not this task's contract.
        assert_eq!(
            form(
                "app/notifierd/__init__.py",
                "from . import impl\nimpl.run()\n"
            ),
            None
        );
        // Another directory, qualified.
        assert_eq!(
            form("app/__main__.py", "from app.ledgerd import impl\n"),
            Some("from app.ledgerd import impl".into())
        );
        assert_eq!(
            form(
                "app/__main__.py",
                "import app.ledgerd.impl as ledger_impl\n"
            ),
            Some("import app.ledgerd.impl".into())
        );
        assert_eq!(
            form("app/__main__.py", "from app.ledgerd.impl import run\n"),
            Some("from app.ledgerd.impl import".into())
        );
        assert_eq!(
            form("app/__main__.py", "app.ledgerd.impl.run()\n"),
            Some("app.ledgerd.impl.".into())
        );
        assert_eq!(
            form("tools/x.py", "open('app/ledgerd/impl.py')\n"),
            Some("ledgerd/impl".into())
        );
        assert_eq!(
            form("app/__main__.py", "from app.ledgerd import impl_helpers\n"),
            None
        );
        assert_eq!(form("app/__main__.py", "import app.ledgerd.implx\n"), None);
        // A root-level owned file has no parent to qualify with: imports only, from the root.
        let root_db = vec!["db.py".to_string()];
        assert!(names_owned_module("main.py", "import db\n", &root_db).is_some());
        assert!(names_owned_module("main.py", "see db.py for the schema\n", &root_db).is_none());
        assert!(names_owned_module("app/x.py", "import db\n", &root_db).is_none());
        // Rust: a package marker is named by its directory from the directory above.
        let rs = vec!["src/ledgerd/mod.rs".to_string()];
        assert_eq!(
            names_owned_module("src/main.rs", "mod ledgerd;\nuse ledgerd::run;\n", &rs),
            Some((&rs[0], "mod ledgerd".to_string()))
        );
        assert!(names_owned_module("src/lib.rs", "use ledgerd_ext::run;\n", &rs).is_none());
        let rs_impl = vec!["src/ledgerd/impl.rs".to_string()];
        assert_eq!(
            names_owned_module("src/main.rs", "use ledgerd::impl::run;\n", &rs_impl),
            Some((&rs_impl[0], "ledgerd::impl".to_string()))
        );
        // TypeScript: a relative path literal from the same directory.
        let ts = vec!["src/ledger/impl.ts".to_string()];
        assert_eq!(
            names_owned_module(
                "src/ledger/index.ts",
                "import { run } from './impl';\n",
                &ts
            ),
            Some((&ts[0], "./impl".to_string()))
        );
        assert_eq!(
            names_owned_module(
                "src/main.ts",
                "import { run } from '../ledger/impl';\n",
                &ts
            ),
            Some((&ts[0], "ledger/impl".to_string()))
        );
    }
}
