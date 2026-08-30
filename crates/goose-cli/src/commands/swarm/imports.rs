//! THE CROSS-TASK IMPORT CHECK: the deterministic scan that finds `app/ledgerd.py` importing
//! `app.store` when no task wrote `app/store.py`, plus the DAG attribution that names the owner.
//! Fifth sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). The scan/attribution cluster moved
//! here verbatim from swarm.rs — behavior unchanged, each item keeps its own WHY — paying for the
//! r5 ownership-timing fix landing in the same commit (see `attribute_import_gap`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// THE CROSS-TASK CHECK: a file that imports something nobody wrote.
///
/// `verify_owned_files` asks whether ONE task delivered its own files. It cannot see the defect that
/// actually sinks a build — `app/ledgerd.py` importing `app.store`, which the task that owned `app/store.py`
/// never wrote. Each task looks fine alone; the tree does not run.
///
/// Deterministic and node-free: read the imports, check the paths. No model is asked whether the code
/// "looks right", because that question is what costs 46% of the fleet and is answered wrongly.
///
/// Scoped to LOCAL imports only — a missing stdlib or third-party module is a different problem and this
/// must not cry wolf about `json` or `sqlite3`.
pub(super) fn verify_tree_imports(working_dir: &Path) -> Vec<String> {
    tree_import_gaps(working_dir)
        .into_iter()
        .map(|(rel, module)| format!("{rel} imports `{module}`, which no task has written"))
        .collect()
}

/// The same scan, returning (importing file, unresolved module) PAIRS instead of formatted lines.
///
/// Split out for II-9: `emit_delivery_defects` must attribute a cross-import gap to the task that
/// OWNS the missing module — which means it needs the module as DATA, not re-parsed out of a
/// message string. `verify_tree_imports` stays as the formatter every existing caller reads.
pub(super) fn tree_import_gaps(working_dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut py: Vec<PathBuf> = Vec::new();
    fn walk(dir: &Path, py: &mut Vec<PathBuf>, depth: usize) {
        if depth > 6 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "__pycache__" || name == "node_modules" {
                continue;
            }
            if p.is_dir() {
                walk(&p, py, depth + 1);
            } else if name.ends_with(".py") {
                py.push(p);
            }
        }
    }
    walk(working_dir, &mut py, 0);
    // THE IMPORT ROOTS. `src/app/store.py` resolves as `app.store`, and a tree laid out that way was
    // skipped WHOLESALE by the old single-root test — the package directory `app` does not exist beside
    // the working dir, so every import in a src-layout project was "none of our business".
    let mut roots: Vec<PathBuf> = vec![working_dir.to_path_buf()];
    if working_dir.join("src").is_dir() {
        roots.push(working_dir.join("src"));
    }
    // A module resolves either as `<name>.py` or as a package directory holding `__init__.py`.
    let resolves = |dir: &Path, parts: &[&str]| -> bool {
        let joined = parts.join("/");
        dir.join(format!("{joined}.py")).is_file()
            || dir.join(&joined).join("__init__.py").is_file()
    };
    // A NAME IMPORTED FROM A PACKAGE IS NOT NECESSARILY A SUBMODULE. `from app import Ledger` is
    // perfectly good when `app/__init__.py` defines or re-exports `Ledger`, and reporting it would be a
    // false positive on a finding class the engine acts on automatically.
    let reexported = |pkg_dir: &Path, name: &str| -> bool {
        std::fs::read_to_string(pkg_dir.join("__init__.py")).is_ok_and(|b| {
            b.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|t| t == name)
        })
    };
    for f in &py {
        let Ok(body) = std::fs::read_to_string(f) else {
            continue;
        };
        let rel = f
            .strip_prefix(working_dir)
            .unwrap_or(f)
            .display()
            .to_string();
        let mut report = |module: &str| {
            let pair = (rel.clone(), module.to_string());
            if !out.contains(&pair) {
                out.push(pair);
            }
        };
        for line in body.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("from ") {
                let Some((module_tok, names_tok)) = rest.split_once(" import ") else {
                    continue;
                };
                let module_tok = module_tok.trim();
                let names: Vec<&str> = names_tok
                    .trim()
                    .trim_start_matches('(')
                    .trim_end_matches('\\')
                    .trim_end_matches(')')
                    .split(',')
                    .map(|n| n.split_whitespace().next().unwrap_or("").trim())
                    .filter(|n| !n.is_empty() && *n != "*")
                    .collect();
                let dots = module_tok.chars().take_while(|c| *c == '.').count();
                let tail = module_tok.get(dots..).unwrap_or("");
                let tail_parts: Vec<&str> = tail.split('.').filter(|p| !p.is_empty()).collect();
                if dots > 0 {
                    // A RELATIVE IMPORT RESOLVES AGAINST THE IMPORTING FILE'S OWN PACKAGE, which is why
                    // the old `trim_start_matches('.')` could never check one: it turned `.store` into
                    // "store", found no dot, and skipped. One leading dot is the file's own directory,
                    // each further dot is one level up.
                    let Some(mut base) = f.parent().map(|p| p.to_path_buf()) else {
                        continue;
                    };
                    let mut ok = true;
                    for _ in 1..dots {
                        match base.parent() {
                            Some(p) if p.starts_with(working_dir) => base = p.to_path_buf(),
                            _ => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if !ok {
                        continue;
                    }
                    if tail_parts.is_empty() {
                        // `from . import store` — every NAME is a sibling module (or an `__init__` symbol).
                        for n in &names {
                            if !resolves(&base, &[n]) && !reexported(&base, n) {
                                report(&format!("{module_tok}{n}"));
                            }
                        }
                    } else if !resolves(&base, &tail_parts) {
                        report(module_tok);
                    }
                    continue;
                }
                if tail_parts.is_empty() {
                    continue;
                }
                // Only judge imports rooted at a package that EXISTS in this tree; anything else is
                // stdlib or a dependency and none of our business.
                let Some(root) = roots.iter().find(|r| r.join(tail_parts[0]).is_dir()) else {
                    continue;
                };
                if tail_parts.len() > 1 && !resolves(root, &tail_parts) {
                    report(tail);
                    continue;
                }
                // `from app import store` — the shape generated code writes most, and the one the old
                // `!module.contains('.')` skip made structurally invisible. Only checked when the module
                // is a DIRECTORY: if it resolved as `app.py` the imported names are attributes of that
                // module, and deciding whether one exists needs an AST this function deliberately avoids.
                let pkg_dir = root.join(tail_parts.join("/"));
                if !pkg_dir.is_dir() {
                    continue;
                }
                // A DIRECTORY WITH NO PYTHON IN IT IS NOT A PACKAGE, whatever it is called. `static/`,
                // `templates/` and `docs/` sit beside the code in most of these trees, and a name
                // collision with a third-party import would otherwise manufacture a finding — which is
                // the one failure this check cannot afford, because its findings are acted on
                // automatically.
                let has_python = std::fs::read_dir(&pkg_dir)
                    .map(|rd| {
                        rd.flatten()
                            .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("py"))
                    })
                    .unwrap_or(false);
                if !has_python {
                    continue;
                }
                for n in &names {
                    if !resolves(&pkg_dir, &[n]) && !reexported(&pkg_dir, n) {
                        report(&format!("{tail}.{n}"));
                    }
                }
            } else if let Some(rest) = line.strip_prefix("import ") {
                let module = rest.split([' ', ',']).next().unwrap_or("");
                let parts: Vec<&str> = module.split('.').filter(|p| !p.is_empty()).collect();
                if parts.len() < 2 {
                    continue;
                }
                let Some(root) = roots.iter().find(|r| r.join(parts[0]).is_dir()) else {
                    continue;
                };
                if !resolves(root, &parts) {
                    report(module);
                }
            }
        }
    }
    out
}

/// II-9: charge a cross-import gap to the task the DAG says is responsible, not to whichever task
/// happened to complete last.
///
/// `emit_delivery_defects` runs the whole-tree import scan on every completion, and its event
/// carries the COMPLETING task's id — so on r2, 5 of 6 `delivery_defects` events blamed the wrong
/// task (seq 293 charged `ledger-core` with `app/notifierd/*` and `app/ledgerd/*` gaps), and the
/// mis-attribution buried exactly the signal the ledger now delivers to repair. The DAG already
/// knows the owner: an unresolved `app.stream` is a missing `app/stream.py`, and some task owns
/// that path.
///
/// Three cases, in order of usefulness:
///   * a task owns the file that would RESOLVE the module — name it, with its state, because
///     "owned by sse-endpoint, state: running" means the import is an in-flight dependency, not a
///     defect anyone should act on yet;
///   * nobody owns the module but a task owns the IMPORTING file — the import references something
///     the plan never assigned, so fixing the import line belongs to that owner;
///   * neither is owned — keep the unattributed line; there is no DAG fact to add.
///
/// THE OWNERSHIP MAP MUST BE THE PLAN'S, NOT THE DISPATCH LOG'S. MEASURED on r5
/// (swarm-20260830-083847650, seqs 141/172/183): `app/ledgerd/__init__.py imports `app.httpapi``
/// was reported "which no task owns — the import line is skeleton's to fix" THREE times while the
/// plan had assigned `app/httpapi.py` to ledgerd-service all along — the map was populated only at
/// dispatch, and ledgerd-service dispatched at 12:36:09, after all three scans. The same-shaped
/// `app.notifierapi` gap attributed correctly at seq 172 because its owner happened to have
/// dispatched already. The caller now publishes the WHOLE plan's ownership before the first
/// dispatch; `dispatched` tells this function which owners have actually started, so a
/// planned-but-unstarted owner reads "pending (not yet dispatched)" instead of impersonating a
/// running one — or, worse, vanishing into "no task owns".
///
/// Pure over its inputs so the r2 and r5 shapes are testable without a dispatcher.
pub(super) fn attribute_import_gap(
    rel: &str,
    module: &str,
    ownership: &HashMap<String, Vec<String>>,
    states: &HashMap<String, String>,
    dispatched: &HashSet<String>,
) -> String {
    let state_of = |task: &str| -> String {
        states.get(task).cloned().unwrap_or_else(|| {
            if dispatched.contains(task) {
                // Dispatched but no ledger row yet: the attempt has not reached a terminal
                // transition, so the honest state is "still running".
                "running".to_string()
            } else {
                // In the plan's ownership map but never dispatched: the import is a PLANNED
                // dependency whose owner has not started. "running" here would claim work that
                // is not happening (the r5 mechanism, from the other direction).
                "pending (not yet dispatched)".to_string()
            }
        })
    };
    // A module resolves as `<m>.py` or `<m>/__init__.py`, at the tree root or under src/ — the
    // same two roots `tree_import_gaps` checks. Relative modules (leading dot) resolve against
    // the importing file's package and are left to the importing-file-owner case below.
    if !module.starts_with('.') {
        let joined = module.replace('.', "/");
        let candidates = [
            format!("{joined}.py"),
            format!("{joined}/__init__.py"),
            format!("src/{joined}.py"),
            format!("src/{joined}/__init__.py"),
        ];
        for cand in &candidates {
            if let Some(task) = ownership
                .iter()
                .find(|(_, files)| files.iter().any(|f| f == cand))
                .map(|(t, _)| t)
            {
                return format!(
                    "{rel} imports `{module}` — unresolved; {cand} is owned by {task}, state: {}",
                    state_of(task)
                );
            }
        }
    }
    if let Some(task) = ownership
        .iter()
        .find(|(_, files)| files.iter().any(|f| f == rel))
        .map(|(t, _)| t)
    {
        return format!(
            "{rel} imports `{module}`, which no task owns — the import line is {task}'s to fix (state: {})",
            state_of(task)
        );
    }
    format!("{rel} imports `{module}`, which no task has written")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "goose-imports-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The defect that sinks a build and that no per-task check can see: each task delivered its own file,
    /// and the tree still does not run.
    #[test]
    fn the_tree_check_finds_an_import_nobody_wrote() {
        let dir = std::env::temp_dir().join(format!("goose-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("app")).unwrap();
        std::fs::write(dir.join("app/__init__.py"), "").unwrap();
        // ledgerd imports a sibling that exists, a sibling that does NOT, and stdlib.
        std::fs::write(
            dir.join("app/ledgerd.py"),
            "import json\nimport sqlite3\nfrom app.common import x\nfrom app.store import y\n",
        )
        .unwrap();
        std::fs::write(dir.join("app/common.py"), "x = 1\n").unwrap();

        let found = verify_tree_imports(&dir);
        assert_eq!(
            found.len(),
            1,
            "exactly the missing local import: {found:?}"
        );
        assert!(found[0].contains("app.store"), "{found:?}");
        assert!(
            !found.iter().any(|f| f.contains("json") || f.contains("sqlite3")),
            "stdlib must never be reported -- crying wolf here would make the whole check ignorable: {found:?}"
        );
        assert!(
            !found.iter().any(|f| f.contains("app.common")),
            "a sibling that EXISTS is not a defect: {found:?}"
        );

        // Once the missing module lands, the tree is clean.
        std::fs::write(dir.join("app/store.py"), "y = 2\n").unwrap();
        assert!(
            verify_tree_imports(&dir).is_empty(),
            "the finding must clear when the file appears"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// II-9, the r2 shape: `app/ledgerd/server.py` imports `app.stream`, the scan fires because
    /// `documentation` completed last — and the defect must name `sse-endpoint`, the task the DAG
    /// says owns `app/stream.py`, never the task that happened to trigger the scan. On r2, 5 of 6
    /// `delivery_defects` events charged the wrong task exactly this way (seq 293).
    #[test]
    fn a_cross_import_gap_charges_the_owner_not_the_last_task_through_the_door() {
        let mut ownership: HashMap<String, Vec<String>> = HashMap::new();
        ownership.insert("sse-endpoint".into(), vec!["app/stream.py".into()]);
        ownership.insert("documentation".into(), vec!["README.md".into()]);
        ownership.insert(
            "ledgerd-server".into(),
            vec!["app/ledgerd/server.py".into()],
        );
        // sse-endpoint has no terminal ledger row: it is dispatched and still running.
        let mut states: HashMap<String, String> = HashMap::new();
        states.insert("documentation".into(), "done".into());
        let dispatched: HashSet<String> = ["sse-endpoint", "documentation", "ledgerd-server"]
            .into_iter()
            .map(String::from)
            .collect();

        let line = attribute_import_gap(
            "app/ledgerd/server.py",
            "app.stream",
            &ownership,
            &states,
            &dispatched,
        );
        assert!(line.contains("sse-endpoint"), "{line}");
        assert!(
            line.contains("state: running"),
            "a dispatched task with no terminal row is running, and the reader must see the import may yet resolve: {line}"
        );
        assert!(
            !line.contains("documentation"),
            "the completing task must never be charged with a gap it does not own: {line}"
        );

        // A module resolving as a package: app/stream/__init__.py is the same ownership fact.
        ownership.insert("sse-endpoint".into(), vec!["app/stream/__init__.py".into()]);
        let pkg = attribute_import_gap(
            "app/ledgerd/server.py",
            "app.stream",
            &ownership,
            &states,
            &dispatched,
        );
        assert!(pkg.contains("sse-endpoint"), "{pkg}");

        // Nobody owns the module: the import line itself is the importer's owner's to fix.
        let unowned = attribute_import_gap(
            "app/ledgerd/server.py",
            "app.ghost",
            &ownership,
            &states,
            &dispatched,
        );
        assert!(unowned.contains("ledgerd-server"), "{unowned}");
        assert!(unowned.contains("no task owns"), "{unowned}");

        // Neither end is owned: no DAG fact to add, keep the unattributed line.
        let bare = attribute_import_gap(
            "scripts/tool.py",
            "app.ghost",
            &ownership,
            &states,
            &dispatched,
        );
        assert_eq!(
            bare,
            "scripts/tool.py imports `app.ghost`, which no task has written"
        );

        // A terminal state is reported as the ledger recorded it.
        states.insert("sse-endpoint".into(), "failed".into());
        let failed = attribute_import_gap(
            "app/ledgerd/server.py",
            "app.stream",
            &ownership,
            &states,
            &dispatched,
        );
        assert!(failed.contains("state: failed"), "{failed}");
    }

    /// THE r5 SHAPE, VERBATIM (swarm-20260830-083847650 seq 141, 11:52:01): the plan assigned
    /// `app/httpapi.py` to ledgerd-service, ledgerd-service had not yet dispatched (its task_owns
    /// row landed at seq 191, 12:36:09), and the scan fired on skeleton's completion. The event
    /// said "which no task owns — the import line is skeleton's to fix (state: running)" — a
    /// planned dependency misread as an unassigned import, three events in a row. With the plan's
    /// ownership published up front, the dotted module resolves to its owned .py file and the
    /// owner's honest state is pending.
    #[test]
    fn a_planned_owner_not_yet_dispatched_is_named_pending_never_no_task_owns() {
        let mut ownership: HashMap<String, Vec<String>> = HashMap::new();
        ownership.insert(
            "ledgerd-service".into(),
            vec![
                "app/vendor_client.py".into(),
                "app/sync.py".into(),
                "app/ledgerdb.py".into(),
                "app/events.py".into(),
                "app/outbox.py".into(),
                "app/webhooks.py".into(),
                "app/drafts.py".into(),
                "app/httpapi.py".into(),
            ],
        );
        ownership.insert(
            "skeleton".into(),
            vec![
                "app/__main__.py".into(),
                "app/ledgerd/__main__.py".into(),
                "app/ledgerd/__init__.py".into(),
                "app/notifierd/__main__.py".into(),
                "app/notifierd/__init__.py".into(),
            ],
        );
        let states: HashMap<String, String> = HashMap::new();
        let dispatched: HashSet<String> = [String::from("skeleton")].into_iter().collect();

        let line = attribute_import_gap(
            "app/ledgerd/__init__.py",
            "app.httpapi",
            &ownership,
            &states,
            &dispatched,
        );
        assert!(
            line.contains("ledgerd-service"),
            "the planned owner must be named: {line}"
        );
        assert!(
            line.contains("pending (not yet dispatched)"),
            "an unstarted owner must not impersonate a running one: {line}"
        );
        assert!(
            !line.contains("no task owns") && !line.contains("skeleton's to fix"),
            "the r5 misattribution must be impossible: {line}"
        );
    }

    /// `from app import store` is the shape generated code writes most, and `!module.contains('.')`
    /// made it structurally invisible — along with every relative import.
    #[test]
    fn the_tree_check_sees_the_import_shapes_it_was_blind_to() {
        let dir = tmp("imports");
        std::fs::create_dir_all(dir.join("app")).unwrap();
        std::fs::write(dir.join("app/__init__.py"), "").unwrap();
        std::fs::write(dir.join("app/common.py"), "x = 1\n").unwrap();

        std::fs::write(
            dir.join("app/ledgerd.py"),
            "from app import common\nfrom app import store\n",
        )
        .unwrap();
        let found = verify_tree_imports(&dir);
        assert_eq!(found.len(), 1, "only the missing one: {found:?}");
        assert!(found[0].contains("app.store"), "{found:?}");

        std::fs::write(
            dir.join("app/ledgerd.py"),
            "from .common import x\nfrom .store import y\n",
        )
        .unwrap();
        let found = verify_tree_imports(&dir);
        assert_eq!(found.len(), 1, "a relative import resolves too: {found:?}");
        assert!(found[0].contains(".store"), "{found:?}");

        std::fs::write(dir.join("app/ledgerd.py"), "from . import common, store\n").unwrap();
        let found = verify_tree_imports(&dir);
        assert_eq!(found.len(), 1, "`from . import x` too: {found:?}");
        assert!(found[0].contains("store"), "{found:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// FALSE POSITIVES ARE THE REAL DANGER: these findings are acted on automatically, and a name
    /// re-exported from a package's `__init__.py` is NOT a submodule.
    #[test]
    fn a_reexported_name_and_a_plain_module_attribute_are_never_flagged() {
        let dir = tmp("reexport");
        std::fs::create_dir_all(dir.join("app")).unwrap();
        std::fs::write(dir.join("app/__init__.py"), "Ledger = object()\n").unwrap();
        std::fs::write(dir.join("app/main.py"), "from app import Ledger\n").unwrap();
        assert!(
            verify_tree_imports(&dir).is_empty(),
            "a symbol defined in __init__.py is a legitimate import, not a missing module"
        );

        // `helpers` is a MODULE, not a package, so `thing` is one of its attributes and this check
        // deliberately has no AST with which to judge it.
        std::fs::write(dir.join("helpers.py"), "thing = 1\n").unwrap();
        std::fs::write(dir.join("app/main.py"), "from helpers import thing\n").unwrap();
        assert!(
            verify_tree_imports(&dir).is_empty(),
            "an attribute of a module is not a submodule of a package"
        );

        std::fs::write(
            dir.join("app/main.py"),
            "from typing import List\nimport os\n",
        )
        .unwrap();
        assert!(
            verify_tree_imports(&dir).is_empty(),
            "stdlib must never be reported — crying wolf makes the whole check ignorable"
        );

        // A DIRECTORY IS NOT A PACKAGE just because it shares a name with an import. `static/` sits
        // beside the code in most of these trees and holds no Python at all.
        std::fs::create_dir_all(dir.join("static")).unwrap();
        std::fs::write(dir.join("static/app.css"), "body{}").unwrap();
        std::fs::write(dir.join("app/main.py"), "from static import files\n").unwrap();
        assert!(
            verify_tree_imports(&dir).is_empty(),
            "an asset directory must never manufacture an import finding"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A src-layout tree was skipped WHOLESALE: the package root test looked beside the working dir only.
    #[test]
    fn a_src_layout_tree_is_checked_rather_than_skipped() {
        let dir = tmp("srclayout");
        std::fs::create_dir_all(dir.join("src/app")).unwrap();
        std::fs::write(dir.join("src/app/__init__.py"), "").unwrap();
        std::fs::write(dir.join("src/app/common.py"), "x = 1\n").unwrap();
        std::fs::write(
            dir.join("src/app/ledgerd.py"),
            "from app.common import x\nfrom app.store import y\n",
        )
        .unwrap();
        let found = verify_tree_imports(&dir);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("app.store"), "{found:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
