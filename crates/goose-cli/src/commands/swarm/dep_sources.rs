//! The worker brief's "API of <file>" blocks — the REAL source of every plan file that exists on
//! disk and the task does not own (AGENTS.md: "workers read real dependency sources (dep_block)").
//! Sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases); extracted from `run_task_inner` for VA-103.
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
//!    `app/ledgerd/__init__.py` (it was truncated)" and its second call was `sed -n '120,260p'`. A
//!    file that names the module the task owns (bare from the same directory — `from . import impl`,
//!    `impl.run(`, `mod impl;` — or qualified from elsewhere — `ledgerd.impl`, `ledgerd/impl`,
//!    `ledgerd::impl`, `ledgerd import impl`) is where the call sites are; it is carried WHOLE and
//!    debits the budget so the rest is bounded.
//! 2. Every cut and every omission is LOUD: a `DepSourceCut` the caller emits as
//!    `dep_source_truncated{task_id, file, bytes, kept, reason}`, and a marker in the brief at the cut
//!    point that carries the exact `sed -n 'A,Bp' <file>` recovering what is missing — a handoff, not
//!    a hint. The old loop `break`-ed silently once the budget was spent, so every later file was
//!    ABSENT from the block with no trace; now it is named with `kept: 0`.

use std::borrow::Cow;
use std::path::Path;

use super::{shape_excerpt, TargetLang};

pub(super) const DEP_SOURCES_BUDGET_CHARS: usize = 14_000;
pub(super) const DEP_SOURCE_FILE_CHARS: usize = 3_500;

pub(super) const CUT_PER_FILE_CAP: &str = "per_file_cap";
pub(super) const CUT_BUDGET_EXHAUSTED: &str = "dep_budget_exhausted";

/// One dependency source the block did not carry whole. `kept == 0` means the file was named but
/// not shown at all (the budget was spent before it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DepSourceCut {
    pub(super) file: String,
    pub(super) bytes: usize,
    pub(super) kept: usize,
    pub(super) reason: &'static str,
}

#[derive(Debug, Default)]
pub(super) struct DepSourcesBlock {
    pub(super) text: String,
    pub(super) cuts: Vec<DepSourceCut>,
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

/// `needle` occurs in `text` bounded on both sides by a non-identifier character (or the ends).
fn contains_word(text: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    text.match_indices(needle).any(|(i, _)| {
        let before_ok = text[..i].chars().next_back().is_none_or(|c| !is_ident_char(c));
        let after_ok = text[i + needle.len()..]
            .chars()
            .next()
            .is_none_or(|c| !is_ident_char(c));
        before_ok && after_ok
    })
}

/// The module a caller names for `owned`: (stem, the directory it is imported from). A package
/// marker (`__init__.py`, `mod.rs`, `index.ts`) is named by its directory, from the directory above.
fn module_of(owned: &str) -> Option<(&str, Option<&Path>)> {
    let path = Path::new(owned);
    let stem = path.file_stem()?.to_str()?;
    let dir = path.parent();
    if matches!(stem, "__init__" | "mod" | "index") {
        if let Some(pkg) = dir.and_then(|d| d.file_name()).and_then(|n| n.to_str()) {
            return Some((pkg, dir.and_then(|d| d.parent())));
        }
    }
    Some((stem, dir))
}

/// Is the dependency at `dep_path` (with `content`) a CALLER of one of `owned_files` — the file that
/// names the module this task must deliver? Returns the owned file it names. A sibling in the same
/// directory names it bare (`from . import impl`, `impl.run(`, `mod impl;`); a file elsewhere names
/// it qualified by its package (`ledgerd.impl`, `ledgerd/impl`, `ledgerd::impl`,
/// `ledgerd import impl`). Such a file is the task's contract and is carried whole.
pub(super) fn names_owned_module<'a>(
    dep_path: &str,
    content: &str,
    owned_files: &'a [String],
) -> Option<&'a String> {
    let dep_dir = Path::new(dep_path).parent();
    owned_files.iter().find(|owned| {
        let Some((stem, dir)) = module_of(owned) else {
            return false;
        };
        if dep_dir == dir {
            return contains_word(content, stem);
        }
        let Some(pkg) = dir.and_then(|d| d.file_name()).and_then(|n| n.to_str()) else {
            return false;
        };
        [".", "/", "::", " import "]
            .iter()
            .any(|sep| contains_word(content, &format!("{pkg}{sep}{stem}")))
    })
}

fn targeted_read_hint(f: &str) -> String {
    format!("`grep -n '<name>' {f}` then `sed -n 'A,Bp' {f}` — never a whole-file cat")
}

/// Every plan file on disk the task does not own, as fenced "## API of <file>" sections, plus the
/// list of cuts the caller must emit. Test files, non-source files, missing and empty files are
/// skipped exactly as before; `signatures_on` (GOOSE_SWARM_DEP_SIGNATURES, ships OFF) swaps the body
/// for `shape_excerpt` and falls back to the body when the excerpt is empty.
pub(super) fn dependency_sources_block(
    root: &Path,
    owned_files: &[String],
    all_files: &[String],
    lang: TargetLang,
    signatures_on: bool,
) -> DepSourcesBlock {
    let owned_set: std::collections::HashSet<&String> = owned_files.iter().collect();
    let leader = comment_leader(lang);
    let mut out = DepSourcesBlock::default();
    let mut budget = DEP_SOURCES_BUDGET_CHARS;
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

        if let Some(named) = names_owned_module(f, &api_source, owned_files) {
            budget = budget.saturating_sub(total_chars);
            out.text.push_str(&format!(
                "## API of {f} — CARRIED WHOLE: this file names your module `{named}`, so it is the \
                 contract you implement (every call into what you own is here; build to THESE \
                 signatures):\n```\n{api_source}\n```\n\n"
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
                "## API of {f} — NOT SHOWN ({bytes} bytes): the {DEP_SOURCES_BUDGET_CHARS}-char \
                 dependency-source budget was spent on the files above. Read it TARGETED before \
                 calling into it: {hint}.\n\n"
            ));
            continue;
        }
        let cap = budget.min(DEP_SOURCE_FILE_CHARS);
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
        let reason = if cap < DEP_SOURCE_FILE_CHARS {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dep-sources-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("app/ledgerd")).unwrap();
        std::fs::create_dir_all(dir.join("app/notifierd")).unwrap();
        dir
    }

    /// Pad `s` with comment lines to EXACTLY `target` bytes (the last line absorbs the remainder).
    fn pad_to(s: &mut String, target: usize) {
        while s.len() + 40 <= target {
            s.push_str("# skeleton filler line\n");
        }
        let rest = target - s.len();
        assert!(rest >= 2, "padding remainder too small: {rest}");
        s.push('#');
        s.push_str(&"-".repeat(rest - 2));
        s.push('\n');
    }

    /// r6h's `app/ledgerd/__init__.py`, shape-faithful: 6,104 bytes, `def run(` at byte 5,128,
    /// `from . import impl` and `return impl.run(` inside it, the ROUTES table before.
    fn r6h_ledgerd_init() -> String {
        let mut s = String::from(
            "\"\"\"Walking skeleton for the ledgerd service.\n\n\
             Contract for the real implementation (``app/ledgerd/impl.py``, owned by the\n\
             ledgerd-core task): ``impl.run(db_dir, port, notifier_url, vendor_url, tokens_file)``.\n\
             \"\"\"\n\n\
             import json\nimport os\nimport sys\n\
             from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer\n\n\
             HOST = \"127.0.0.1\"\n\n\
             ROUTES = (\n    (\"GET\", \"/health\"),\n    (\"GET\", \"/ledger\"),\n    (\"POST\", \"/sync\"),\n)\n\n\
             class SkeletonHandler(BaseHTTPRequestHandler):\n    def do_GET(self):\n        pass\n\n",
        );
        pad_to(&mut s, 5128);
        s.push_str(
            "def run(db_dir, port, notifier_url=None, vendor_url=None, tokens_file=None):\n\
             \x20   \"\"\"Bind 127.0.0.1:<port> and serve; blocks until the process exits.\"\"\"\n\
             \x20   os.makedirs(db_dir, exist_ok=True)\n\
             \x20   try:\n\
             \x20       from . import impl  # real service lands here (ledgerd-core task)\n\
             \x20   except ImportError as exc:\n\
             \x20       impl = None\n\
             \x20   if impl is not None and hasattr(impl, \"run\"):\n\
             \x20       return impl.run(\n\
             \x20           db_dir=db_dir,\n\
             \x20           port=port,\n\
             \x20           notifier_url=notifier_url,\n\
             \x20           vendor_url=vendor_url,\n\
             \x20           tokens_file=tokens_file,\n\
             \x20       )\n\
             \x20   server = ThreadingHTTPServer((HOST, int(port)), SkeletonHandler)\n\
             \x20   server.serve_forever()\n\n",
        );
        pad_to(&mut s, 6104);
        assert_eq!(s.len(), 6104);
        assert_eq!(s.find("\ndef run(").unwrap() + 1, 5128);
        s
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
    /// the owned module `impl` from the same directory, so it is the contract: carried whole, no cut.
    /// The notifierd skeleton (same size, names ITS OWN impl, not ledgerd's) is not this task's
    /// contract: cut on a line boundary, loudly, with the exact recovery command.
    #[test]
    fn r6h_the_contract_caller_is_carried_whole_and_the_other_cut_is_loud() {
        let root = scratch("r6h");
        let ledgerd = r6h_ledgerd_init();
        std::fs::write(root.join("app/ledgerd/__init__.py"), &ledgerd).unwrap();
        let notifierd = ledgerd.replace("ledgerd", "notifierd");
        std::fs::write(root.join("app/notifierd/__init__.py"), &notifierd).unwrap();
        let owned = r6h_owned();
        let mut all = owned.clone();
        all.extend(
            [
                "app/__main__.py",
                "app/notifierd/__init__.py",
                "app/notifierd/__main__.py",
                "app/ledgerd/__init__.py",
                "app/ledgerd/__main__.py",
            ]
            .iter()
            .map(|s| s.to_string()),
        );

        let block = dependency_sources_block(&root, &owned, &all, TargetLang::Python, false);

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
            "## API of app/ledgerd/__init__.py — CARRIED WHOLE: this file names your module `app/ledgerd/impl.py`"
        ));
        assert!(
            !block.cuts.iter().any(|c| c.file == "app/ledgerd/__init__.py"),
            "the contract is never cut: {:?}",
            block.cuts
        );

        // The non-contract sibling: cut on a line boundary, named, with the recovery command.
        let cut = block
            .cuts
            .iter()
            .find(|c| c.file == "app/notifierd/__init__.py")
            .expect("the notifierd skeleton is over the per-file cap and is cut loudly");
        assert_eq!(cut.bytes, notifierd.trim().len());
        assert_eq!(cut.reason, CUT_PER_FILE_CAP);
        assert!(cut.kept < DEP_SOURCE_FILE_CHARS && cut.kept > 0, "{cut:?}");
        let marker = format!(
            "# … [dep source TRUNCATED at {} of {} bytes — lines ",
            cut.kept, cut.bytes
        );
        let at = block.text.find(&marker).expect("the marker sits at the cut");
        let section_start = block
            .text
            .find("## API of app/notifierd/__init__.py")
            .unwrap();
        let fence = block.text[section_start..at].rfind("```\n").unwrap() + section_start + 4;
        let kept_text = &block.text[fence..at - 1];
        assert_eq!(kept_text.len(), cut.kept);
        assert!(
            notifierd.trim().starts_with(kept_text) && notifierd.trim()[kept_text.len()..].starts_with('\n'),
            "the kept prefix ends on a line boundary"
        );
        let next_line = notifierd[..kept_text.len()].lines().count() + 1;
        let last_line = notifierd.lines().count();
        assert!(block.text.contains(&format!(
            "`sed -n '{next_line},{last_line}p' app/notifierd/__init__.py`"
        )));
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
        let block = dependency_sources_block(&root, &owned, &all, TargetLang::Python, false);
        let first = block.cuts.iter().find(|c| c.file == "app/dep0.py").unwrap();
        assert_eq!(first.reason, CUT_PER_FILE_CAP);
        assert_eq!(first.kept, 3_499, "70 whole lines of 50 bytes, minus the final newline");
        // 4 × 3,499 kept leaves 4 chars: dep4 gets no line boundary in that room and dep5 finds the
        // same 4 — both are NAMED with nothing shown, never silently absent.
        for f in ["app/dep4.py", "app/dep5.py"] {
            let cut = block.cuts.iter().find(|c| c.file == f).unwrap();
            assert_eq!(
                (cut.kept, cut.reason, cut.bytes),
                (0, CUT_BUDGET_EXHAUSTED, big.trim().len()),
                "{f}"
            );
            assert!(block.text.contains(&format!("## API of {f} — NOT SHOWN (3599 bytes)")));
            assert!(block.text.contains(&format!("`sed -n 'A,Bp' {f}`")));
        }
        assert_eq!(block.cuts.len(), 6, "{:?}", block.cuts);
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
        let block = dependency_sources_block(&root, &owned, &all, TargetLang::Python, false);
        assert!(block.cuts.is_empty());
        assert!(block.text.contains("## API of app/util.py (a dependency you import"));
        assert!(block.text.contains("def helper():\n    return 1\n```"));
        assert!(!block.text.contains("TRUNCATED"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn names_owned_module_matches_a_bare_sibling_or_a_qualified_reference_only() {
        let owned = vec!["app/ledgerd/impl.py".to_string()];
        // Same directory, bare name.
        assert_eq!(
            names_owned_module("app/ledgerd/__init__.py", "from . import impl\n", &owned),
            Some(&owned[0])
        );
        assert!(names_owned_module("app/ledgerd/__main__.py", "impl.run(1)\n", &owned).is_some());
        // Another directory naming ITS OWN impl: not this task's contract.
        assert_eq!(
            names_owned_module("app/notifierd/__init__.py", "from . import impl\nimpl.run()", &owned),
            None
        );
        // Another directory, qualified.
        assert!(names_owned_module("app/__main__.py", "from app.ledgerd import impl\n", &owned).is_some());
        assert!(names_owned_module("app/__main__.py", "import app.ledgerd.impl\n", &owned).is_some());
        assert!(names_owned_module("tools/x.py", "open('app/ledgerd/impl.py')", &owned).is_some());
        // An identifier that merely starts with the stem is not a reference.
        assert_eq!(
            names_owned_module("app/ledgerd/__init__.py", "from . import impl_helpers\n", &owned),
            None
        );
        assert_eq!(
            names_owned_module("app/__main__.py", "from app.ledgerd import impl_helpers\n", &owned),
            None
        );
        // Rust: a package marker owned file is named by its directory from the directory above.
        let rs = vec!["src/ledgerd/mod.rs".to_string()];
        assert!(names_owned_module("src/main.rs", "mod ledgerd;\nuse ledgerd::run;\n", &rs).is_some());
        assert!(names_owned_module("src/lib.rs", "use ledgerd_ext::run;\n", &rs).is_none());
        let rs_impl = vec!["src/ledgerd/impl.rs".to_string()];
        assert!(names_owned_module("src/main.rs", "use ledgerd::impl::run;\n", &rs_impl).is_some());
    }
}
