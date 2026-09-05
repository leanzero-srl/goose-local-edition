//! THE CROSS-TASK IMPORT VERDICT at a completion (VA-110): what one unresolved import MEANS once the
//! PLAN's ownership has been read — the owner's file is still coming, the owner finished without it,
//! or nobody was ever given the file. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases).
//!
//! THE MECHANISM IT REPLACES (imports.rs `attribute_import_gap_with_owner`): the attribution never
//! resolved a RELATIVE module to a file — its own comment: "Relative modules (leading dot) resolve
//! against the importing file's package and are left to the importing-file-owner case below" — so
//! every `from .x import` / `from .. import x` fell through to "which no task owns — the import line
//! is <importer's owner>'s to fix", and an ABSOLUTE module owned by a still-running task was routed to
//! that owner as a DEFECT ("unresolved; app/stream.py is owned by sse-endpoint, state: running").
//! MEASURED on r6h (swarm-3node-r0, run.jsonl 04:10:45 and 04:22:40 UTC): `app/api.py imports
//! `..webhooks`, which no task owns — the import line is ledgerd-core's to fix (state: done)`, ×3
//! (`..drafts`, `..auth`), repeated verbatim at two unrelated shard completions, while the plan gave
//! `app/webhooks.py`, `app/drafts.py`, `app/auth.py` to webhooks-workflow (dispatched 04:10:45, no
//! terminal row). The plan owner was never consulted because the module started with a dot.
//!
//! What the words held (gate 7, ledgerd-core.think.log:268): "My impl.py is at app/ledgerd/impl.py →
//! `from .. import webhooks` … relative imports work" — right for impl.py, one directory too far in
//! app/api.py, where `..` climbs above `app/` (Python: attempted relative import beyond top-level
//! package). So the r6h line was WRONG in its reason ("no task owns" — the plan owns app/webhooks.py)
//! and only accidentally right in its address. This module resolves the module the way
//! `tree_import_gaps` does (the importer's own directory, one level up per extra dot), looks the file
//! up in the plan, reads the owner's STATE before calling anything a defect, and names the plan's
//! nearest file when the resolved path is unowned — the fact a fixer needs. A fact fires ONCE per
//! (importer, module, verdict) across completions; the `seen` set lives on the dispatcher.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::imports::task_state_label;

/// The plan file sharing an unowned module's basename — how a reader sees "one directory off".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NearMiss {
    pub(super) file: String,
    pub(super) owner: String,
    pub(super) owner_state: String,
}

/// One unresolved import, read against the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GapVerdict {
    /// The plan's owner of the file reached a terminal state and the file is not in the tree.
    OwnerDefect {
        file: String,
        owner: String,
        owner_state: String,
    },
    /// The plan's owner has not finished: the import resolves when its file lands. Not a defect.
    Pending {
        file: String,
        owner: String,
        owner_state: String,
    },
    /// No task owns any file the module could resolve to — a plan gap, or an import line that
    /// points where the plan put nothing.
    Unowned {
        /// The tree-relative `.py` path the module resolves to.
        file: String,
        /// The importing file's planned owner and its state, when the plan has one.
        importer_owner: Option<(String, String)>,
        near_miss: Option<NearMiss>,
    },
}

/// The tree-relative paths a module resolves to, mirroring `tree_import_gaps`: an absolute module at
/// the root or under `src/`; a relative one against the importer's directory, one level up per extra
/// dot. Err when the dots climb above the tree — the one lookup the plan cannot answer.
// string_slice: `dots` counts leading ASCII `.` characters, one byte each.
#[allow(clippy::string_slice)]
fn candidate_files(importer: &str, module: &str) -> Result<Vec<String>, String> {
    let dots = module.chars().take_while(|c| *c == '.').count();
    let tail = &module[dots..];
    let parts: Vec<&str> = tail.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err(format!("`{module}` names no module"));
    }
    let joined = parts.join("/");
    if dots == 0 {
        return Ok(vec![
            format!("{joined}.py"),
            format!("{joined}/__init__.py"),
            format!("src/{joined}.py"),
            format!("src/{joined}/__init__.py"),
        ]);
    }
    let mut base = Path::new(importer)
        .parent()
        .ok_or_else(|| format!("{importer} has no parent directory"))?
        .to_path_buf();
    for _ in 1..dots {
        base = base
            .parent()
            .ok_or_else(|| format!("`{module}` from {importer} climbs above the tree root"))?
            .to_path_buf();
    }
    let resolved = base.join(joined).display().to_string();
    Ok(vec![
        format!("{resolved}.py"),
        format!("{resolved}/__init__.py"),
    ])
}

/// The plan's owner of one tree-relative path; sorted so two claimants (never, post-repair) read
/// the same on every completion.
fn owner_of<'a>(file: &str, ownership: &'a HashMap<String, Vec<String>>) -> Option<&'a str> {
    let mut owners: Vec<&str> = ownership
        .iter()
        .filter(|(_, files)| files.iter().any(|f| f == file))
        .map(|(task, _)| task.as_str())
        .collect();
    owners.sort_unstable();
    owners.first().copied()
}

/// A plan file with the resolved module's basename (`webhooks.py` or `webhooks/__init__.py`) at
/// another path — r6h's `..webhooks` from app/api.py resolves to `webhooks.py`; the plan's is
/// `app/webhooks.py`.
fn nearest_plan_file(
    candidate: &str,
    ownership: &HashMap<String, Vec<String>>,
) -> Option<(String, String)> {
    let stem = Path::new(candidate).file_stem()?.to_str()?.to_string();
    let module_file = format!("{stem}.py");
    let package_init = format!("{stem}/__init__.py");
    let package_suffix = format!("/{package_init}");
    let mut hits: Vec<(String, String)> = ownership
        .iter()
        .flat_map(|(task, files)| files.iter().map(move |f| (f.clone(), task.clone())))
        .filter(|(f, _)| {
            f.as_str() != candidate
                && (Path::new(f).file_name().and_then(|n| n.to_str()) == Some(module_file.as_str())
                    || f.as_str() == package_init.as_str()
                    || f.ends_with(package_suffix.as_str()))
        })
        .collect();
    hits.sort();
    hits.into_iter().next()
}

/// Read one unresolved import against the plan. Pure over its inputs: the ownership map is the
/// plan's (`owned_files_by_task`, published before the first dispatch), `states` the ledger minis'
/// terminal rows, `dispatched` the tasks that have started — the same three the lane view reads.
pub(super) fn classify_import_gap(
    importer: &str,
    module: &str,
    ownership: &HashMap<String, Vec<String>>,
    states: &HashMap<String, String>,
    dispatched: &HashSet<String>,
) -> Result<GapVerdict, String> {
    let candidates = candidate_files(importer, module)?;
    let state_of = |task: &str| task_state_label(task, states, dispatched);
    for file in &candidates {
        if let Some(owner) = owner_of(file, ownership) {
            let owner_state = state_of(owner);
            let terminal = matches!(owner_state.as_str(), "done" | "failed");
            let (file, owner) = (file.clone(), owner.to_string());
            return Ok(if terminal {
                GapVerdict::OwnerDefect {
                    file,
                    owner,
                    owner_state,
                }
            } else {
                GapVerdict::Pending {
                    file,
                    owner,
                    owner_state,
                }
            });
        }
    }
    let importer_owner = owner_of(importer, ownership).map(|t| (t.to_string(), state_of(t)));
    let near_miss = nearest_plan_file(&candidates[0], ownership).map(|(file, owner)| {
        let owner_state = state_of(&owner);
        NearMiss {
            file,
            owner,
            owner_state,
        }
    });
    Ok(GapVerdict::Unowned {
        file: candidates[0].clone(),
        importer_owner,
        near_miss,
    })
}

impl GapVerdict {
    /// The line a reader gets — every clause a plan or ledger fact, and for a defect the owner,
    /// the file and the importing line: a handoff, not a verdict about "the tree".
    pub(super) fn line(&self, importer: &str, module: &str) -> String {
        match self {
            GapVerdict::OwnerDefect {
                file,
                owner,
                owner_state,
            } => format!(
                "{owner} (state: {owner_state}) did not write {file}, which {importer} imports as `{module}`"
            ),
            GapVerdict::Pending {
                file,
                owner,
                owner_state,
            } => format!(
                "{importer} imports `{module}` — {file} is {owner}'s (state: {owner_state}); the import resolves when it lands"
            ),
            GapVerdict::Unowned {
                file,
                importer_owner,
                near_miss,
            } => {
                let mut s = format!("{importer} imports `{module}` → {file}, which no task owns");
                if importer_owner.is_none() {
                    s.push_str(" or has written");
                }
                if let Some(nm) = near_miss {
                    s.push_str(&format!(
                        "; the plan's nearest file is {} ({}, state: {})",
                        nm.file, nm.owner, nm.owner_state
                    ));
                }
                if let Some((task, state)) = importer_owner {
                    s.push_str(&format!(
                        " — the import line is {task}'s to fix (state: {state})"
                    ));
                }
                s
            }
        }
    }
}

/// Where a verdict's line goes at THIS completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Routed {
    /// Into the completing task's own `defects`.
    Own(String),
    /// Into `cross_task`, addressed to the owner named.
    Cross { owner: String, line: String },
}

/// One unresolved import at one completion: the event it earns (once per fact run-wide, via `seen`)
/// and the defect line, if the verdict is one. `Pending` and a failed lookup are never defects; the
/// completing task's OWN gaps ride its `defects` at every one of its completions, another task's
/// ride `cross_task` once.
pub(super) fn route_import_gap(
    completing: &str,
    importer: &str,
    module: &str,
    verdict: Result<GapVerdict, String>,
    seen: &mut HashSet<String>,
    emit: &mut dyn FnMut(serde_json::Value),
    notice: &mut dyn FnMut(String),
) -> Option<Routed> {
    let kind = match &verdict {
        Err(_) => "lookup_failed",
        Ok(GapVerdict::OwnerDefect { .. }) => "owner_defect",
        Ok(GapVerdict::Pending { .. }) => "pending",
        Ok(GapVerdict::Unowned { .. }) => "unowned",
    };
    let first = seen.insert(format!("{importer}\u{1}{module}\u{1}{kind}"));
    let verdict = match verdict {
        Err(error) => {
            if first {
                emit(serde_json::json!({
                    "event": "cross_task_owner_lookup_failed",
                    "importer": importer,
                    "module": module,
                    "error": error,
                    "at_completion_of": completing,
                }));
                notice(format!(
                    "{importer} imports `{module}` — owner lookup failed: {error}"
                ));
            }
            return None;
        }
        Ok(v) => v,
    };
    let line = verdict.line(importer, module);
    match &verdict {
        GapVerdict::Pending {
            file,
            owner,
            owner_state,
        } => {
            if first {
                emit(serde_json::json!({
                    "event": "cross_task_pending",
                    "importer": importer,
                    "module": module,
                    "file": file,
                    "owner": owner,
                    "owner_state": owner_state,
                    "at_completion_of": completing,
                }));
                notice(line);
            }
            None
        }
        GapVerdict::Unowned {
            file,
            importer_owner,
            near_miss,
        } => {
            if first {
                let mut ev = serde_json::json!({
                    "event": "cross_task_unowned",
                    "importer": importer,
                    "module": module,
                    "file": file,
                    "importer_owner": importer_owner.as_ref().map(|(t, _)| t.clone()),
                    "importer_owner_state": importer_owner.as_ref().map(|(_, s)| s.clone()),
                    "at_completion_of": completing,
                });
                if let Some(nm) = near_miss {
                    ev["near_miss"] = serde_json::json!({
                        "file": nm.file,
                        "owner": nm.owner,
                        "owner_state": nm.owner_state,
                    });
                }
                emit(ev);
            }
            match importer_owner {
                Some((task, _)) if task.as_str() != completing => first.then(|| Routed::Cross {
                    owner: task.clone(),
                    line,
                }),
                _ => Some(Routed::Own(line)),
            }
        }
        GapVerdict::OwnerDefect { owner, .. } => {
            if owner.as_str() == completing {
                Some(Routed::Own(line))
            } else {
                first.then(|| Routed::Cross {
                    owner: owner.clone(),
                    line,
                })
            }
        }
    }
}

/// The completion's stderr lines — the same words the `delivery_defects` event carries, so the
/// terminal and run.jsonl tell one story. Moved here from `emit_delivery_defects` with the routing.
pub(super) fn print_delivery_lines(task_id: &str, defects: &[String], cross: &[serde_json::Value]) {
    for d in defects {
        eprintln!(
            "  {} {} delivered a defect: {d}",
            console::style("!").red().bold(),
            console::style(task_id).bold()
        );
    }
    for c in cross {
        eprintln!(
            "  {} {} owns a defect surfaced at {}'s completion: {}",
            console::style("!").red().bold(),
            console::style(c["owner_task"].as_str().unwrap_or("?")).bold(),
            task_id,
            c["defect"].as_str().unwrap_or("?")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route_once(
        completing: &str,
        importer: &str,
        module: &str,
        verdict: Result<GapVerdict, String>,
        seen: &mut HashSet<String>,
    ) -> (Option<Routed>, Vec<serde_json::Value>, Vec<String>) {
        let mut events: Vec<serde_json::Value> = Vec::new();
        let mut notices: Vec<String> = Vec::new();
        let routed = route_import_gap(
            completing,
            importer,
            module,
            verdict,
            seen,
            &mut |v: serde_json::Value| events.push(v),
            &mut |s: String| notices.push(s),
        );
        (routed, events, notices)
    }

    /// (ownership: task → files, states: task → state, dispatched tasks)
    type PlanAndLedger = (
        HashMap<String, Vec<String>>,
        HashMap<String, String>,
        HashSet<String>,
    );

    /// r6h's plan (plan-loaded.json, `files` per subtask) and ledger at 04:22:40: ledgerd-core and
    /// skeleton done, webhooks-workflow dispatched with no terminal row.
    fn r6h() -> PlanAndLedger {
        let mut ownership: HashMap<String, Vec<String>> = HashMap::new();
        ownership.insert(
            "ledgerd-core".into(),
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
            .into_iter()
            .map(String::from)
            .collect(),
        );
        ownership.insert(
            "webhooks-workflow".into(),
            ["app/webhooks.py", "app/drafts.py", "app/auth.py"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        ownership.insert(
            "skeleton".into(),
            ["app/__main__.py", "app/ledgerd/__init__.py"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        let mut states: HashMap<String, String> = HashMap::new();
        states.insert("ledgerd-core".into(), "done".into());
        states.insert("skeleton".into(), "done".into());
        let dispatched: HashSet<String> = ["ledgerd-core", "skeleton", "webhooks-workflow"]
            .into_iter()
            .map(String::from)
            .collect();
        (ownership, states, dispatched)
    }

    /// The r6h shape with the import as it should have been written (`from .webhooks`): the plan's
    /// owner is running, so the gap is PENDING — one event, zero defects, silent at the next
    /// completion. Under the replaced attribution this was "which no task owns — the import line is
    /// ledgerd-core's to fix (state: done)", a false defect repeated at every completion.
    #[test]
    fn r6h_a_relative_import_of_a_running_owners_planned_file_is_pending_not_a_defect() {
        let (ownership, states, dispatched) = r6h();
        let mut seen = HashSet::new();
        for module in [".webhooks", ".drafts", ".auth"] {
            let verdict =
                classify_import_gap("app/api.py", module, &ownership, &states, &dispatched);
            let file = format!("app/{}.py", module.trim_start_matches('.'));
            assert_eq!(
                verdict,
                Ok(GapVerdict::Pending {
                    file: file.clone(),
                    owner: "webhooks-workflow".into(),
                    owner_state: "running".into(),
                })
            );
            let (routed, events, notices) = route_once(
                "viz-engine-data-stream-render-pick",
                "app/api.py",
                module,
                verdict.clone(),
                &mut seen,
            );
            assert_eq!(routed, None, "a pending import is never a defect");
            assert_eq!(events.len(), 1, "{events:?}");
            assert_eq!(events[0]["event"], "cross_task_pending");
            assert_eq!(events[0]["file"], file);
            assert_eq!(events[0]["owner"], "webhooks-workflow");
            assert_eq!(events[0]["owner_state"], "running");
            assert!(
                notices[0].contains("resolves when it lands") && !notices[0].contains("to fix"),
                "{notices:?}"
            );
            // The next completion (viz-engine-debug-api, 04:22:40) restates nothing.
            let (again, events2, notices2) = route_once(
                "viz-engine-debug-api",
                "app/api.py",
                module,
                verdict,
                &mut seen,
            );
            assert_eq!(again, None);
            assert!(events2.is_empty() && notices2.is_empty(), "{events2:?}");
        }
    }

    /// r6h AS WRITTEN: `..webhooks` from app/api.py climbs to the tree root — `webhooks.py`, which
    /// the plan never assigned. The verdict is UNOWNED with the plan's nearest file named
    /// (app/webhooks.py, webhooks-workflow, running), addressed to the importer's owner because the
    /// import line really is the thing to fix — and never again "which no task owns" as the whole
    /// story. Once across completions.
    #[test]
    fn r6h_as_written_two_dots_from_app_api_resolve_to_the_root_and_name_the_plans_nearest_file() {
        let (ownership, states, dispatched) = r6h();
        let verdict =
            classify_import_gap("app/api.py", "..webhooks", &ownership, &states, &dispatched);
        assert_eq!(
            verdict,
            Ok(GapVerdict::Unowned {
                file: "webhooks.py".into(),
                importer_owner: Some(("ledgerd-core".into(), "done".into())),
                near_miss: Some(NearMiss {
                    file: "app/webhooks.py".into(),
                    owner: "webhooks-workflow".into(),
                    owner_state: "running".into(),
                }),
            })
        );
        let mut seen = HashSet::new();
        let (routed, events, _) = route_once(
            "viz-engine-data-stream-render-pick",
            "app/api.py",
            "..webhooks",
            verdict.clone(),
            &mut seen,
        );
        let Some(Routed::Cross { owner, line }) = &routed else {
            panic!("the importer's owner is the route: {routed:?}");
        };
        assert_eq!(owner, "ledgerd-core");
        assert!(line.contains("→ webhooks.py, which no task owns"), "{line}");
        assert!(
            line.contains(
                "the plan's nearest file is app/webhooks.py (webhooks-workflow, state: running)"
            ),
            "{line}"
        );
        assert!(
            line.contains("the import line is ledgerd-core's to fix (state: done)"),
            "{line}"
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["event"], "cross_task_unowned");
        assert_eq!(events[0]["near_miss"]["file"], "app/webhooks.py");
        assert_eq!(events[0]["importer_owner"], "ledgerd-core");
        let (again, events2, _) = route_once(
            "viz-engine-debug-api",
            "app/api.py",
            "..webhooks",
            verdict,
            &mut seen,
        );
        assert_eq!(
            again, None,
            "the same fact is not restated at the next completion"
        );
        assert!(events2.is_empty());
    }

    /// The owner finished (or failed) and its file is not in the tree: the defect is the OWNER's,
    /// never the importer's — and the same (importer, module) that was pending earlier fires again,
    /// because the verdict changed.
    #[test]
    fn a_done_owner_that_never_wrote_the_file_owns_the_defect() {
        let (ownership, mut states, dispatched) = r6h();
        let mut seen = HashSet::new();
        let pending =
            classify_import_gap("app/api.py", ".webhooks", &ownership, &states, &dispatched);
        route_once("skeleton", "app/api.py", ".webhooks", pending, &mut seen);
        states.insert("webhooks-workflow".into(), "done".into());
        let verdict =
            classify_import_gap("app/api.py", ".webhooks", &ownership, &states, &dispatched);
        assert_eq!(
            verdict,
            Ok(GapVerdict::OwnerDefect {
                file: "app/webhooks.py".into(),
                owner: "webhooks-workflow".into(),
                owner_state: "done".into(),
            })
        );
        let (routed, events, _) =
            route_once("notifierd", "app/api.py", ".webhooks", verdict, &mut seen);
        let Some(Routed::Cross { owner, line }) = &routed else {
            panic!("{routed:?}");
        };
        assert_eq!(owner, "webhooks-workflow");
        assert!(
            line.contains("webhooks-workflow (state: done) did not write app/webhooks.py")
                && line.contains("which app/api.py imports as `.webhooks`"),
            "{line}"
        );
        assert!(!line.contains("ledgerd-core"), "{line}");
        assert!(
            events.is_empty(),
            "a defect rides the completion event, not its own: {events:?}"
        );

        // The owner completing with the gap keeps it on its OWN list — at every one of its completions.
        let verdict =
            classify_import_gap("app/api.py", ".webhooks", &ownership, &states, &dispatched);
        let (own, _, _) = route_once(
            "webhooks-workflow",
            "app/api.py",
            ".webhooks",
            verdict,
            &mut seen,
        );
        assert!(matches!(own, Some(Routed::Own(_))), "{own:?}");

        states.insert("webhooks-workflow".into(), "failed".into());
        let failed =
            classify_import_gap("app/api.py", ".webhooks", &ownership, &states, &dispatched);
        assert!(
            matches!(failed, Ok(GapVerdict::OwnerDefect { ref owner_state, .. }) if owner_state == "failed"),
            "{failed:?}"
        );
    }

    /// An import of a module the plan never assigned anywhere: unowned, no near miss, the importer's
    /// owner addressed — and when the importer is unowned too, the completing task keeps the line.
    #[test]
    fn an_import_nobody_planned_is_unowned_and_says_so() {
        let (ownership, states, dispatched) = r6h();
        let verdict =
            classify_import_gap("app/api.py", ".nothing", &ownership, &states, &dispatched);
        assert_eq!(
            verdict,
            Ok(GapVerdict::Unowned {
                file: "app/nothing.py".into(),
                importer_owner: Some(("ledgerd-core".into(), "done".into())),
                near_miss: None,
            })
        );
        let mut seen = HashSet::new();
        let (routed, events, _) =
            route_once("skeleton", "app/api.py", ".nothing", verdict, &mut seen);
        assert!(
            matches!(&routed, Some(Routed::Cross { owner, line }) if owner == "ledgerd-core" && !line.contains("nearest")),
            "{routed:?}"
        );
        assert_eq!(events[0]["event"], "cross_task_unowned");
        assert!(events[0].get("near_miss").is_none(), "{:?}", events[0]);

        let bare = classify_import_gap(
            "scripts/tool.py",
            "app.ghost",
            &ownership,
            &states,
            &dispatched,
        );
        let (own, _, _) = route_once("skeleton", "scripts/tool.py", "app.ghost", bare, &mut seen);
        let Some(Routed::Own(line)) = &own else {
            panic!("{own:?}");
        };
        assert_eq!(
            line,
            "scripts/tool.py imports `app.ghost` → app/ghost.py, which no task owns or has written"
        );
    }

    /// Three dots from a top-level package file cannot be resolved against the tree: a loud
    /// `cross_task_owner_lookup_failed`, no defect invented.
    #[test]
    fn a_relative_import_above_the_tree_root_is_a_loud_lookup_failure_not_a_defect() {
        let (ownership, states, dispatched) = r6h();
        let verdict = classify_import_gap("app/api.py", "...x", &ownership, &states, &dispatched);
        assert!(
            matches!(&verdict, Err(e) if e.contains("climbs above the tree root")),
            "{verdict:?}"
        );
        let mut seen = HashSet::new();
        let (routed, events, notices) =
            route_once("skeleton", "app/api.py", "...x", verdict, &mut seen);
        assert_eq!(routed, None);
        assert_eq!(events[0]["event"], "cross_task_owner_lookup_failed");
        assert!(notices[0].contains("owner lookup failed"), "{notices:?}");
    }

    /// THE r2 SHAPE (II-9, seq 293): `app/ledgerd/server.py` imports `app.stream` and the scan fires
    /// because `documentation` completed last. The owner named is sse-endpoint, running — PENDING,
    /// never a defect on documentation and (since VA-110) never a defect on sse-endpoint either
    /// while it runs. A package owner (`app/stream/__init__.py`) is the same fact.
    #[test]
    fn r2_an_absolute_import_of_a_running_owners_file_is_pending_for_that_owner() {
        let mut ownership: HashMap<String, Vec<String>> = HashMap::new();
        ownership.insert("sse-endpoint".into(), vec!["app/stream.py".into()]);
        ownership.insert("documentation".into(), vec!["README.md".into()]);
        ownership.insert(
            "ledgerd-server".into(),
            vec!["app/ledgerd/server.py".into()],
        );
        let mut states: HashMap<String, String> = HashMap::new();
        states.insert("documentation".into(), "done".into());
        let dispatched: HashSet<String> = ["sse-endpoint", "documentation", "ledgerd-server"]
            .into_iter()
            .map(String::from)
            .collect();
        let verdict = classify_import_gap(
            "app/ledgerd/server.py",
            "app.stream",
            &ownership,
            &states,
            &dispatched,
        );
        assert_eq!(
            verdict,
            Ok(GapVerdict::Pending {
                file: "app/stream.py".into(),
                owner: "sse-endpoint".into(),
                owner_state: "running".into(),
            })
        );
        let mut seen = HashSet::new();
        let (routed, events, _) = route_once(
            "documentation",
            "app/ledgerd/server.py",
            "app.stream",
            verdict,
            &mut seen,
        );
        assert_eq!(routed, None);
        assert_eq!(events[0]["owner"], "sse-endpoint");
        assert_eq!(
            events[0]["at_completion_of"], "documentation",
            "the completing task appears only as the completion, never as an owner: {:?}",
            events[0]
        );

        ownership.insert("sse-endpoint".into(), vec!["app/stream/__init__.py".into()]);
        let pkg = classify_import_gap(
            "app/ledgerd/server.py",
            "app.stream",
            &ownership,
            &states,
            &dispatched,
        );
        assert!(
            matches!(pkg, Ok(GapVerdict::Pending { ref owner, .. }) if owner == "sse-endpoint"),
            "{pkg:?}"
        );
    }

    /// THE r5 SHAPE (swarm-20260830-083847650 seq 141): `app/httpapi.py` planned to ledgerd-service,
    /// not yet dispatched; the scan fired on skeleton's completion. Pending with the honest state
    /// "pending (not yet dispatched)" — never "no task owns", never "skeleton's to fix".
    #[test]
    fn r5_a_planned_owner_not_yet_dispatched_is_pending_never_no_task_owns() {
        let mut ownership: HashMap<String, Vec<String>> = HashMap::new();
        ownership.insert(
            "ledgerd-service".into(),
            vec!["app/httpapi.py".into(), "app/webhooks.py".into()],
        );
        ownership.insert(
            "skeleton".into(),
            vec!["app/__main__.py".into(), "app/ledgerd/__init__.py".into()],
        );
        let states: HashMap<String, String> = HashMap::new();
        let dispatched: HashSet<String> = [String::from("skeleton")].into_iter().collect();
        let verdict = classify_import_gap(
            "app/ledgerd/__init__.py",
            "app.httpapi",
            &ownership,
            &states,
            &dispatched,
        );
        let Ok(GapVerdict::Pending {
            owner, owner_state, ..
        }) = &verdict
        else {
            panic!("{verdict:?}");
        };
        assert_eq!(owner, "ledgerd-service");
        assert_eq!(owner_state, "pending (not yet dispatched)");
        let line = verdict
            .as_ref()
            .unwrap()
            .line("app/ledgerd/__init__.py", "app.httpapi");
        assert!(
            !line.contains("no task owns") && !line.contains("skeleton's to fix"),
            "{line}"
        );
    }

    /// THE r5 ROUTING RECEIPTS (item 5, seq 172): the module's owner is the route, the importer's
    /// owner is the route when nobody owns the module, and a gap owned at neither end stays on the
    /// completing task's own list.
    #[test]
    fn the_route_is_the_module_owner_then_the_importers_owner_then_the_completing_task() {
        let mut ownership: HashMap<String, Vec<String>> = HashMap::new();
        ownership.insert("decisions".into(), vec!["DECISIONS.md".into()]);
        ownership.insert(
            "notifierd-service".into(),
            vec!["app/notifierapi.py".into()],
        );
        ownership.insert(
            "skeleton".into(),
            vec![
                "app/notifierd/__init__.py".into(),
                "app/ledgerd/__init__.py".into(),
            ],
        );
        let mut states: HashMap<String, String> = HashMap::new();
        states.insert("notifierd-service".into(), "done".into());
        let dispatched: HashSet<String> = ["decisions", "skeleton", "notifierd-service"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut seen = HashSet::new();

        let v = classify_import_gap(
            "app/notifierd/__init__.py",
            "app.notifierapi",
            &ownership,
            &states,
            &dispatched,
        );
        let (r, _, _) = route_once(
            "decisions",
            "app/notifierd/__init__.py",
            "app.notifierapi",
            v,
            &mut seen,
        );
        assert!(
            matches!(&r, Some(Routed::Cross { owner, .. }) if owner == "notifierd-service"),
            "{r:?}"
        );

        let v = classify_import_gap(
            "app/ledgerd/__init__.py",
            "app.ghost",
            &ownership,
            &states,
            &dispatched,
        );
        let (r, _, _) = route_once(
            "decisions",
            "app/ledgerd/__init__.py",
            "app.ghost",
            v,
            &mut seen,
        );
        assert!(
            matches!(&r, Some(Routed::Cross { owner, .. }) if owner == "skeleton"),
            "{r:?}"
        );

        let v = classify_import_gap(
            "scripts/tool.py",
            "app.ghost",
            &ownership,
            &states,
            &dispatched,
        );
        let (r, _, _) = route_once("decisions", "scripts/tool.py", "app.ghost", v, &mut seen);
        assert!(
            matches!(&r, Some(Routed::Own(line)) if line.contains("no task owns or has written")),
            "{r:?}"
        );
    }

    #[test]
    fn candidate_files_mirror_the_scans_resolution() {
        assert_eq!(
            candidate_files("app/ledgerd/impl.py", "..webhooks").unwrap(),
            vec!["app/webhooks.py", "app/webhooks/__init__.py"]
        );
        assert_eq!(
            candidate_files("app/api.py", ".relay").unwrap(),
            vec!["app/relay.py", "app/relay/__init__.py"]
        );
        assert_eq!(
            candidate_files("app/api.py", "app.sub.mod").unwrap(),
            vec![
                "app/sub/mod.py",
                "app/sub/mod/__init__.py",
                "src/app/sub/mod.py",
                "src/app/sub/mod/__init__.py"
            ]
        );
        assert!(candidate_files("app/api.py", "..").is_err());
    }
}
