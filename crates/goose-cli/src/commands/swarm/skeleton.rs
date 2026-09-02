//! The WALKING SKELETON: the engine-authored first task and its brief. Extracted from swarm.rs
//! under the incremental-split law in the same commit that added `refresh_skeleton_description`
//! (r6c: the brief baked pre-repair paths); every function moved mechanically, tests included.
//!
//! VA-023 (2026-09-01): which tasks WAIT on the skeleton is derived here too — from the plan's own
//! `files` against the entry files the skeleton writes — so a task that needs no entry module
//! starts at BUILD minute 0 instead of idling behind a one-node barrier (see
//! `skeleton_dependency_verdict`).

use super::{
    decomposition_of, spec_get_endpoints, spec_python_invocations, spec_surface_rows, string_list,
    SpecSurface,
};

/// PART III — THE WALKING SKELETON. The plan's first task, prepended by CODE after the review:
/// entry point(s) + boot + config + route registration, with EVERY advertised route pre-registered
/// serving a 501 stub and `GET /` serving the static shell, so the app BOOTS and answers its whole
/// advertised surface BEFORE the parallel fan writes a single module.
///
/// WHY (r2, measured): wiring was nobody's job until the sink, INTEGRATE ran ~130 minutes, fixed one
/// boot defect, never reached the wiring, and both scored criticals (`GET /` 404, dead sync) are
/// exactly the classes a route-pre-registered skeleton kills at the source. The skeleton's content
/// is assembled from DETERMINISTIC inputs only — the spec's own boot invocations, the spec's own
/// endpoint tables, the plan's own module list — the same way the semantic sink description is:
/// a model executes it, but no model decides what it says.
pub(super) const SKELETON_ID: &str = "skeleton";

/// The wiring files the skeleton owns: for every advertised `python -m X`, the entry `X/__main__.py`
/// that invocation boots through, plus `X/__init__.py` when no planned task puts any other file in
/// the package (someone must create the package marker or the entry cannot import). This is
/// `require_advertised_entry_files`' own mapping, computed for the skeleton instead of for a module
/// task — after the skeleton exists, the injection finds every entry owned and no-ops.
fn skeleton_invocation_files(
    subtasks: &[serde_json::Value],
    invocations: &[String],
) -> Vec<String> {
    let owned: Vec<String> = subtasks
        .iter()
        .filter_map(|s| s.get("files").and_then(|f| f.as_array()))
        .flatten()
        .filter_map(|f| f.as_str().map(str::to_string))
        .collect();
    let mut files = Vec::new();
    for inv in invocations {
        let dir = inv.replace('.', "/");
        let entry = format!("{dir}/__main__.py");
        if !files.contains(&entry) {
            files.push(entry);
        }
        let prefix = format!("{dir}/");
        let init = format!("{dir}/__init__.py");
        let fresh_package = !owned.iter().any(|f| f.starts_with(&prefix));
        if fresh_package && !files.contains(&init) {
            files.push(init);
        }
    }
    files
}

/// The skeleton's brief, assembled from the spec's tables and the plan — every clause traceable to a
/// deterministic parser (`spec_python_invocations`, `spec_surface_rows`/`spec_get_endpoints`, the
/// plan's own `files`). The advertised rows are quoted VERBATIM so `brief_mentions_path` — the same
/// predicate rule (d) uses — finds every advertised path here and the endpoint-append targets this
/// task (or, with nothing missing, no-ops: the skeleton IS rule (d)'s guarantee, delivered up front).
fn skeleton_description(
    subtasks: &[serde_json::Value],
    spec: &str,
    invocations: &[String],
    owned: &[String],
) -> String {
    let mut s = String::from(
        "WALKING SKELETON — assembled by the engine from the spec's own tables and the plan.\n\
         Write the app's entry point(s), boot, config and route registration FIRST, so the app \
         BOOTS and answers EVERY advertised route BEFORE any module is built. The tasks after \
         you fill their modules in behind these routes; your job is the frame they land in.\n\n\
         BOOT — the spec's own invocations; each must start, bind its documented port, and serve:\n",
    );
    for inv in invocations {
        s.push_str(&format!(
            "- `python3 -m {inv}` with exactly the flags the spec documents for it\n"
        ));
    }
    s.push_str(
        "\nYOU OWN EXACTLY THESE FILES — module tasks own everything else, never write theirs:\n",
    );
    for f in owned {
        s.push_str(&format!("- `{f}`\n"));
    }
    s.push_str(
        "\nPLANNED MODULES — they DO NOT EXIST YET. Import each one lazily or behind a guarded \
         import so a missing module NEVER stops boot; register its routes now and keep them \
         answering until the module lands:\n",
    );
    for t in subtasks {
        let id = t.get("id").and_then(|i| i.as_str()).unwrap_or_default();
        // A shard owns only its `.swarm/shards/…/README.md` — nothing importable; its module is
        // listed through the merger, which owns the final file (2c S1).
        if id == goose_swarm::SINK_ID || id == SKELETON_ID || t.get("shard_of").is_some() {
            continue;
        }
        let files: Vec<String> = t
            .get("files")
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|f| f.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if files.is_empty() {
            continue;
        }
        s.push_str(&format!("- {id}: {}\n", files.join(", ")));
    }
    // The endpoint tables, grouped by the service whose section carries each row — the same
    // service→invocation rule the plan repair's rule (d) applies (last invocation segment names
    // the service; an unmatched service falls to the first invocation).
    let SpecSurface { rows, .. } = spec_surface_rows(spec);
    let invocation_for = |service: Option<&str>| -> String {
        service
            .and_then(|name| {
                invocations
                    .iter()
                    .find(|inv| inv.rsplit('.').next() == Some(name))
            })
            .or(invocations.first())
            .cloned()
            .unwrap_or_default()
    };
    s.push_str(
        "\nADVERTISED ROUTES — pre-register EVERY row below NOW. Until its module fills it in, a \
         route answers `501 Not Implemented` with a JSON error envelope — a 501 is a promise kept; \
         a 404 is the run's oldest scored critical. `GET /` serves the static shell page (the \
         frontend entry), never a 501.\n",
    );
    if rows.is_empty() {
        for path in spec_get_endpoints(spec) {
            s.push_str(&format!("- `GET {path}`\n"));
        }
    } else {
        let mut last_service: Option<String> = None;
        for (service, row) in rows {
            let inv = invocation_for(service.as_deref());
            let label = service.unwrap_or_else(|| inv.clone());
            if last_service.as_deref() != Some(label.as_str()) {
                s.push_str(&format!("[service `{label}` — `python3 -m {inv}`]\n"));
                last_service = Some(label);
            }
            s.push_str(&format!("- `{row}`\n"));
        }
    }
    // VA-023's consequence: the frontend task no longer waits behind the skeleton, so its shell
    // page may not exist when the skeleton proves itself — `GET /` is the frontend's proof.
    s.push_str(
        "\nDONE means: every boot command above starts and binds, and every route above answers \
         with the 501/JSON envelope from THIS task's own handlers (a 501 counts, a 404 does not). \
         `GET /` serving the shell page is the frontend task's proof, not yours — it runs \
         concurrently and its files may not exist yet. Prove it yourself before finishing: run \
         each boot command, curl the routes, then KILL the server you started.\n",
    );
    s
}

/// The packages the skeleton creates — every directory whose `__init__.py` or `__main__.py` it
/// writes, with those entry files — longest path first so a nested file (`app/ledgerd/impl.py`)
/// is reported against `app/ledgerd/`, the package it actually lives in, and ties are by name so
/// the order is deterministic.
fn skeleton_packages(skeleton_files: &[String]) -> Vec<(String, Vec<String>)> {
    let mut packages: Vec<(String, Vec<String>)> = Vec::new();
    for f in skeleton_files {
        let Some((dir, name)) = f.rsplit_once('/') else {
            continue;
        };
        if name != "__init__.py" && name != "__main__.py" {
            continue;
        }
        match packages.iter_mut().find(|(d, _)| d == dir) {
            Some((_, entries)) => entries.push(f.clone()),
            None => packages.push((dir.to_string(), vec![f.clone()])),
        }
    }
    packages.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    packages
}

/// Does this task WAIT on the skeleton? Decided from the plan's own `files` against the packages
/// the skeleton creates — never from prose, never as a blanket rule.
///
/// WHY (VA-023, measured r5/r6c): with every task depending on the skeleton, BUILD's first 56 (r5)
/// and 64 (r6c) minutes ran on one node while two idled (112 / 128 idle node-minutes); r6c's
/// `web-console` (web/index.html, web/styles.css, web/app.js) and `web-viz` (web/viz.js) import no
/// entry module and touched no skeleton file, and waited 64 minutes for nothing. Mihai,
/// 2026-09-01: "We have too many sequential tasks that are done only by one node… Why must it be
/// done sequentially?"
///
/// THE RULE: KEPT when one of the task's files lies inside a package whose `__init__.py` /
/// `__main__.py` the skeleton writes (`app/db.py` inside `app/`), or is the `X.py` module shadow
/// of such a package `X/` — the very module `decomposition_of` names in
/// `module_package_collisions` and `repair_module_package_collisions` rewrites INTO the package,
/// one derivation reused so this pre-repair verdict matches the post-repair layout. The join is
/// KEPT unconditionally (it waits on every task). Everything else is RELAXED: it keeps only its
/// planner-given deps and is Ready the moment those are. Prose is NOT evidence here — r6c's
/// `decisions-doc` brief names `app/drafts.py` and every frontend brief says `app.js`; a mention
/// rule would have kept them all. A Python file OUTSIDE every skeleton package (a test, a script)
/// relaxes too: its imports are unknowable before it is written, and the skeleton's entry stubs
/// are not what such a file needs — the module it exercises is, and that is a planner dep.
///
/// Loud both ways: the row names the file and package the verdict rests on. Rows are
/// self-describing (`event` + `task` + `reason`) so a sink can fan them out as per-task
/// `skeleton_dep_kept` / `skeleton_dep_relaxed` events unchanged.
fn skeleton_dependency_verdict(
    task: &serde_json::Value,
    packages: &[(String, Vec<String>)],
    shadowed_modules: &[String],
) -> (bool, serde_json::Value) {
    // A task without an id cannot load as a DAG at all (`from_planner_json` requires one), so
    // the row states the absence instead of standing in a default for it.
    let id = task
        .get("id")
        .and_then(|i| i.as_str())
        .unwrap_or("<id missing>");
    let quoted = |items: &[String]| {
        items
            .iter()
            .map(|s| format!("`{s}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let row = |event: &str, reason: String| serde_json::json!({ "event": event, "task": id, "reason": reason });
    if id == goose_swarm::SINK_ID {
        return (
            true,
            row(
                "skeleton_dep_kept",
                "the join waits on every task".to_string(),
            ),
        );
    }
    let files = string_list(&task["files"]);
    for f in &files {
        if let Some((dir, entries)) = packages
            .iter()
            .find(|(d, _)| f.starts_with(&format!("{d}/")))
        {
            return (
                true,
                row(
                    "skeleton_dep_kept",
                    format!(
                        "owns `{f}` inside `{dir}/` — the skeleton writes its {}",
                        quoted(entries)
                    ),
                ),
            );
        }
        if shadowed_modules.iter().any(|m| m == f) {
            let stem = f.trim_end_matches(".py");
            if let Some((dir, entries)) = packages.iter().find(|(d, _)| d == stem) {
                return (
                    true,
                    row(
                        "skeleton_dep_kept",
                        format!(
                            "owns `{f}`, the module shadow of `{dir}/` (the collision repair \
                             rewrites it into the package) — the skeleton writes its {}",
                            quoted(entries)
                        ),
                    ),
                );
            }
        }
    }
    let mut package_dirs: Vec<String> = packages.iter().map(|(d, _)| format!("{d}/")).collect();
    package_dirs.sort();
    let owned = if files.is_empty() {
        "owns no files".to_string()
    } else {
        format!(
            "none of its files ({}) lies inside a package the skeleton writes an entry for ({})",
            quoted(&files),
            quoted(&package_dirs)
        )
    };
    let deps = string_list(&task["depends_on"]);
    let planned = if deps.is_empty() {
        "no planned deps — Ready at BUILD start".to_string()
    } else {
        format!("planned deps kept: {}", quoted(&deps))
    };
    (
        false,
        row("skeleton_dep_relaxed", format!("{owned}; {planned}")),
    )
}

/// Prepend the skeleton as the plan's first task and make every task that needs an entry file it
/// writes depend on it (`skeleton_dependency_verdict`; the join always does). Runs BEFORE the
/// repair passes on purpose: rule (b)'s first-claimant then guarantees module tasks never own the
/// skeleton's files (the skeleton is claimant #1 by position), and rule (d)'s `entry_owner_index`
/// resolves every advertised invocation to the skeleton, so the endpoint-append targets the one
/// task whose job is serving. The price of prepending first is that the brief bakes PRE-repair
/// ownership — `refresh_skeleton_description` regenerates it inside the repair chain (r6c), so
/// what dispatches describes the repaired plan. The dependency verdicts survive the repairs
/// without a refresh: the repairs only remove files, rewrite a shadow module INTO its package
/// (already a KEPT case), or move the join's files onto the skeleton — none turns a RELAXED
/// task into one that owns a skeleton-package file.
///
/// Returns the event to emit, or None when there is nothing to build from — no advertised
/// `python -m` invocation (the same guard `require_advertised_entry_files` applies; a spec with no
/// bootable entry has no boot for a skeleton to prove) — or when the plan already carries one
/// (idempotent: the second pass is a no-op, like every other repair).
pub(super) fn prepend_skeleton_task(
    v: &mut serde_json::Value,
    spec: &str,
) -> Option<serde_json::Value> {
    let invocations = spec_python_invocations(spec);
    if invocations.is_empty() {
        return None;
    }
    let subtasks = v.get_mut("subtasks").and_then(|s| s.as_array_mut())?;
    if subtasks.is_empty()
        || subtasks
            .iter()
            .any(|t| t.get("id").and_then(|i| i.as_str()) == Some(SKELETON_ID))
    {
        return None;
    }
    let files = skeleton_invocation_files(subtasks, &invocations);
    if files.is_empty() {
        return None;
    }
    let description = skeleton_description(subtasks, spec, &invocations, &files);
    subtasks.insert(
        0,
        serde_json::json!({
            "id": SKELETON_ID,
            "difficulty": "hard",
            "files": files,
            "depends_on": [],
            "description": description,
        }),
    );
    // The shadow list is computed WITH the skeleton in the plan, so a module beside a package the
    // skeleton alone creates (`app.py` beside its `app/__main__.py`) is named too.
    let shadowed = string_list(&decomposition_of(&v.to_string())["module_package_collisions"]);
    let packages = skeleton_packages(&files);
    let mut kept: Vec<serde_json::Value> = Vec::new();
    let mut relaxed: Vec<serde_json::Value> = Vec::new();
    if let Some(subtasks) = v.get_mut("subtasks").and_then(|s| s.as_array_mut()) {
        for t in subtasks.iter_mut().skip(1) {
            let (waits, verdict) = skeleton_dependency_verdict(t, &packages, &shadowed);
            if !waits {
                relaxed.push(verdict);
                continue;
            }
            kept.push(verdict);
            let Some(obj) = t.as_object_mut() else {
                continue;
            };
            let deps = obj
                .entry("depends_on")
                .or_insert_with(|| serde_json::json!([]));
            if let Some(a) = deps.as_array_mut() {
                if !a.iter().any(|d| d.as_str() == Some(SKELETON_ID)) {
                    a.push(serde_json::Value::String(SKELETON_ID.to_string()));
                }
            }
        }
    }
    Some(serde_json::json!({
        "event": "skeleton_prepended",
        "files": files,
        "invocations": invocations,
        "dependents": kept.len(),
        "relaxed": relaxed.len(),
        "skeleton_dep_kept": kept,
        "skeleton_dep_relaxed": relaxed,
        "description_chars": description.chars().count(),
    }))
}

/// The skeleton's brief is engine text ABOUT the plan, and `prepend_skeleton_task` must run before
/// the repair passes (rule (b)'s fence, rule (d)'s entry ownership all assume the skeleton is
/// claimant #1) — so the brief it bakes goes stale the moment a repair renames or strips a file.
/// r6c (2026-08-31, seq 1384-1386): the dispatched PLANNED MODULES block still read
/// `- ledgerd-core: app/__main__.py, app/ledgerd.py, ...` after the repairs had kept
/// `app/__main__.py` with the skeleton and rewritten `app/ledgerd.py` to `app/ledgerd/impl.py`,
/// and the live skeleton lane re-derived ownership from that contradiction ("impl.py — owned by
/// whom? Not me. \"PLANNED MODULES\" doesn't list impl.py explicitly... ledgerd-core lists
/// app/ledgerd.py (not a package!)"). Regenerating the TEXT at this seam — after the ownership
/// repairs, BEFORE rule (d) appends to this very brief — keeps the prepend-first order every
/// repair depends on while the dispatched words describe the repaired plan. It also refreshes the
/// ownership list when `repair_sink_files` moves the join's files onto the skeleton. A plan the
/// repairs did not change regenerates identical text: no-op, no action.
pub(super) fn refresh_skeleton_description(
    plan: &mut serde_json::Value,
    spec: &str,
    actions: &mut Vec<String>,
) {
    let invocations = spec_python_invocations(spec);
    if invocations.is_empty() {
        return;
    }
    let (idx, fresh) = {
        let Some(subtasks) = plan.get("subtasks").and_then(|s| s.as_array()) else {
            return;
        };
        let Some(idx) = subtasks
            .iter()
            .position(|t| t.get("id").and_then(|i| i.as_str()) == Some(SKELETON_ID))
        else {
            return;
        };
        // A skeleton without a `files` array cannot exist (`prepend_skeleton_task` always writes
        // one) — stated as a return, not an empty default, so a broken plan keeps its old brief
        // rather than gaining a fabricated empty ownership block.
        let Some(owned) = subtasks[idx]
            .get("files")
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|f| f.as_str().map(str::to_string))
                    .collect::<Vec<String>>()
            })
        else {
            return;
        };
        let fresh = skeleton_description(subtasks, spec, &invocations, &owned);
        if subtasks[idx].get("description").and_then(|d| d.as_str()) == Some(fresh.as_str()) {
            return;
        }
        (idx, fresh)
    };
    plan["subtasks"][idx]["description"] = serde_json::Value::String(fresh);
    actions.push(format!(
        "`{SKELETON_ID}` brief regenerated from the repaired plan — its PLANNED MODULES and \
         ownership blocks baked pre-repair paths"
    ));
}

#[cfg(test)]
mod tests {
    use super::super::{
        finalize_plan_before_dag, repair_plan_flags, string_list, unassigned_endpoints, TargetLang,
    };
    use super::{prepend_skeleton_task, refresh_skeleton_description};
    use goose_swarm::{EventSink, NullSink};
    use std::sync::Arc;

    fn strings(v: &serde_json::Value) -> Vec<String> {
        string_list(v)
    }

    /// III-1: the walking skeleton, assembled from r2's REAL plan and the REAL sb-7 spec. Every
    /// clause of the description is checkable against a deterministic parser: the three advertised
    /// boot invocations, the entry files the invocations boot through, every endpoint-table row
    /// (ledgerd's AND notifierd's), and `GET /` as the static shell.
    #[test]
    fn the_skeleton_is_assembled_from_the_r2_plan_and_the_real_spec() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let mut v: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/r2-plan.json")).unwrap();
        let ev = prepend_skeleton_task(&mut v, spec).expect("the sb-7 spec advertises boots");
        let skel = &v["subtasks"][0];
        assert_eq!(skel["id"], "skeleton", "prepended FIRST, not appended");
        let files = strings(&skel["files"]);
        assert_eq!(
            files,
            vec![
                "app/__main__.py",
                "app/ledgerd/__main__.py",
                "app/notifierd/__main__.py"
            ],
            "exactly the advertised entries; no __init__.py because every package has a planned owner"
        );
        let desc = skel["description"].as_str().unwrap();
        for boot in [
            "python3 -m app`",
            "python3 -m app.ledgerd`",
            "python3 -m app.notifierd`",
        ] {
            assert!(desc.contains(boot), "boot command missing: {boot}");
        }
        for route in [
            "GET /api/health",
            "/api/payments?limit=",
            "POST /api/sync",
            "GET /api/stream",
            "POST /notify/events",
            "GET /health",
            "POST /api/drafts",
            "`GET / ",
            "501 Not Implemented",
            "static shell",
        ] {
            assert!(desc.contains(route), "advertised surface missing: {route}");
        }
        // S0 (2026-09-01): DONE is proved by THIS task's own handlers; the shell page belongs to
        // the frontend task, which runs concurrently since VA-023 and may not have landed yet.
        for clause in [
            "DONE means: every boot command above starts and binds, and every route above answers \
             with the 501/JSON envelope from THIS task's own handlers (a 501 counts, a 404 does not).",
            "`GET /` serving the shell page is the frontend task's proof, not yours — it runs \
             concurrently and its files may not exist yet.",
            "then KILL the server you started",
        ] {
            assert!(desc.contains(clause), "DONE clause missing: {clause}");
        }
        assert!(
            !desc.contains("and `GET /` returns the shell page"),
            "the old DONE clause demanded the frontend's file of the skeleton"
        );
        // the planned module list rides along so the skeleton imports what will exist
        assert!(desc.contains("vendor-sync: app/vendor_sync.py"));
        // VA-023: a task waits on the skeleton only when it owns a file inside a package the
        // skeleton creates (`app/`, `app/ledgerd/`, `app/notifierd/`) — plus the join. r2's
        // `documentation` (DECISIONS.md, README.md) and `frontend-ui` (web/*) own none: Ready at
        // BUILD start instead of behind a one-node barrier.
        for t in v["subtasks"].as_array().unwrap().iter().skip(1) {
            let id = t["id"].as_str().unwrap();
            let waits = strings(&t["depends_on"]).contains(&"skeleton".to_string());
            let expected = !matches!(id, "documentation" | "frontend-ui");
            assert_eq!(waits, expected, "{id}: depends_on = {:?}", t["depends_on"]);
        }
        assert_eq!(ev["event"], "skeleton_prepended");
        assert_eq!(ev["dependents"], 9, "11 planned tasks, 2 relaxed");
        assert_eq!(ev["relaxed"], 2);
        // THE FENCE: after the repair passes, no module task owns a skeleton file — rule (b)'s
        // first claimant is the skeleton by position, and the action rows say so.
        let repairs = repair_plan_flags(&mut v, spec, TargetLang::Python);
        for f in &files {
            let owners: Vec<String> = v["subtasks"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|t| strings(&t["files"]).contains(f))
                .map(|t| t["id"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(owners, vec!["skeleton"], "{f} must be skeleton-only");
        }
        assert!(
            repairs
                .actions
                .iter()
                .any(|a| a.contains("kept by `skeleton`")),
            "the fence must be stated as a repair action: {:?}",
            repairs.actions
        );
        // rule (d) has nothing left to append: the skeleton's verbatim rows mention every
        // advertised path, so the plan ships with zero unassigned endpoints by construction.
        assert!(
            unassigned_endpoints(&v, spec).is_empty(),
            "the skeleton IS rule (d)'s guarantee"
        );
    }

    /// III-1: prepending is idempotent through the full finalize pass — a resumed or re-finalized
    /// plan gets ONE skeleton, and the second pass is byte-identical.
    #[test]
    fn the_skeleton_is_prepended_exactly_once() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let sink: Arc<dyn EventSink> = Arc::new(NullSink);
        let plan = include_str!("../../../tests/fixtures/r2-plan.json").to_string();
        let once = finalize_plan_before_dag(plan, spec, false, TargetLang::Python, &sink, "plan");
        let twice =
            finalize_plan_before_dag(once.clone(), spec, false, TargetLang::Python, &sink, "plan");
        assert_eq!(once, twice, "the second finalize must be a no-op");
        let v: serde_json::Value = serde_json::from_str(&twice).unwrap();
        let skeletons = v["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["id"] == "skeleton")
            .count();
        assert_eq!(skeletons, 1);
        assert_eq!(v["subtasks"][0]["id"], "skeleton");
        // and the finalized plan still loads as a DAG with the join intact
        let dag = goose_swarm::Dag::from_planner_json(&twice).unwrap();
        assert!(dag.tasks.contains_key("integrate-verify"));
        assert!(dag.tasks.contains_key("skeleton"));
    }

    /// III-1: a spec with no advertised `python -m` boot builds NOTHING deterministic for a
    /// skeleton to prove, so none is prepended and the plan is untouched — the same guard
    /// `require_advertised_entry_files` applies.
    #[test]
    fn no_advertised_boot_means_no_skeleton() {
        let mut v: serde_json::Value = serde_json::from_str(
            r#"{"subtasks":[
                {"id":"a","files":["app/a.py"],"depends_on":[],"description":"a"},
                {"id":"integrate-verify","files":[],"depends_on":["a"],"description":"verify"}
            ]}"#,
        )
        .unwrap();
        let before = v.clone();
        assert!(prepend_skeleton_task(&mut v, "build a CLI tool").is_none());
        assert_eq!(v, before);
    }

    /// r6c pinned verbatim (run swarm-20260831-072930517, seq 1384-1386): synthesis gave
    /// `ledgerd-core` the entry `app/__main__.py` plus the module form `app/ledgerd.py` beside
    /// the planned `app/ledgerd/` package, and `notifierd` the same module/package shadow. The
    /// repairs fixed the PLAN (fence + rewrite to `impl.py`) but the skeleton's already-baked
    /// brief still carried the pre-repair paths — the dispatched contradiction the live lane
    /// tripped on ("impl.py — owned by whom? Not me").
    const R6C_SHAPED_PLAN: &str = r#"{"subtasks":[
        {"id":"ledgerd-core","files":["app/__main__.py","app/ledgerd.py","app/db.py","app/sync.py","app/ledger.py","app/outbox.py","README.md"],"depends_on":[],"description":"Own app/ledgerd.py (ledgerd entrypoint), the sqlite layer and the sync loop."},
        {"id":"ledgerd-api","files":["app/api.py","app/webhooks.py","app/drafts.py","app/auth.py"],"depends_on":["ledgerd-core"],"description":"HTTP API surface."},
        {"id":"notifierd","files":["app/notifierd.py","app/notify_store.py"],"depends_on":[],"description":"Own app/notifierd.py (notifierd entrypoint) and its store."},
        {"id":"web-console","files":["web/index.html","web/styles.css","web/app.js"],"depends_on":["ledgerd-api"],"description":"Console UI."},
        {"id":"web-viz","files":["web/viz.js"],"depends_on":["web-console"],"description":"Viz layer."},
        {"id":"decisions-doc","files":["DECISIONS.md"],"depends_on":[],"description":"Record settled decisions."},
        {"id":"integrate-verify","files":[],"depends_on":["ledgerd-core","ledgerd-api","notifierd","web-console","web-viz","decisions-doc"],"description":"verify"}
    ]}"#;

    /// The r6c defect and its fix, on r6c's own shape: the pre-repair brief bakes the stale
    /// paths, and after `repair_plan_flags` the regenerated PLANNED MODULES block carries the
    /// POST-repair truth — `app/ledgerd/impl.py`, no fenced entry under a module task — with a
    /// loud action row riding `plan_repaired`.
    #[test]
    fn the_skeleton_brief_carries_the_post_repair_paths() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let mut v: serde_json::Value = serde_json::from_str(R6C_SHAPED_PLAN).unwrap();
        prepend_skeleton_task(&mut v, spec).expect("the sb-7 spec advertises boots");
        let stale = v["subtasks"][0]["description"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            stale.contains("- ledgerd-core: app/__main__.py, app/ledgerd.py"),
            "the prepend-time brief bakes the pre-repair paths (the r6c defect):\n{stale}"
        );
        let r = repair_plan_flags(&mut v, spec, TargetLang::Python);
        let desc = v["subtasks"][0]["description"].as_str().unwrap();
        assert!(
            desc.contains(
                "- ledgerd-core: app/ledgerd/impl.py, app/db.py, app/sync.py, app/ledger.py, \
                 app/outbox.py, README.md"
            ),
            "PLANNED MODULES must read the repaired ownership:\n{desc}"
        );
        assert!(
            desc.contains("- notifierd: app/notifierd/impl.py, app/notify_store.py"),
            "{desc}"
        );
        assert!(
            !desc.contains("app/ledgerd.py") && !desc.contains("app/notifierd.py"),
            "no pre-repair module path survives in the dispatched brief:\n{desc}"
        );
        assert!(
            r.actions.iter().any(|a| a.contains("brief regenerated")),
            "the regeneration is loud in plan_repaired: {:?}",
            r.actions
        );
    }

    /// VA-023 on r6c's own shape. Measured: r6c's BUILD ran its first 64 minutes on one node
    /// (skeleton) with two idle, then dispatched all five module tasks at B+64 — `web-console`,
    /// `web-viz` and `decisions-doc` had waited for entry files they never import. Derived from
    /// `files` alone: the three tasks under `app/` keep the dependency (their reasons name the
    /// file and the package), the join keeps it, the two frontend tasks and the doc task relax
    /// and keep exactly their planner-given deps. The event carries one row per task, both ways.
    #[test]
    fn skeleton_dependency_is_derived_from_package_membership_on_r6cs_plan() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let mut v: serde_json::Value = serde_json::from_str(R6C_SHAPED_PLAN).unwrap();
        let ev = prepend_skeleton_task(&mut v, spec).expect("the sb-7 spec advertises boots");
        let deps_of = |v: &serde_json::Value, id: &str| -> Vec<String> {
            v["subtasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == id)
                .map(|t| strings(&t["depends_on"]))
                .unwrap_or_else(|| panic!("{id} missing"))
        };
        for kept in [
            "ledgerd-core",
            "ledgerd-api",
            "notifierd",
            "integrate-verify",
        ] {
            assert!(
                deps_of(&v, kept).contains(&"skeleton".to_string()),
                "{kept} must wait on the skeleton: {:?}",
                deps_of(&v, kept)
            );
        }
        assert_eq!(deps_of(&v, "web-console"), vec!["ledgerd-api"]);
        assert_eq!(deps_of(&v, "web-viz"), vec!["web-console"]);
        assert!(
            deps_of(&v, "decisions-doc").is_empty(),
            "Ready at BUILD start"
        );
        assert_eq!(ev["dependents"], 4);
        assert_eq!(ev["relaxed"], 3);
        let rows = |key: &str| -> Vec<(String, String)> {
            ev[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    assert_eq!(r["event"], key, "rows are self-describing events");
                    (
                        r["task"].as_str().unwrap().to_string(),
                        r["reason"].as_str().unwrap().to_string(),
                    )
                })
                .collect()
        };
        let kept = rows("skeleton_dep_kept");
        let relaxed = rows("skeleton_dep_relaxed");
        assert_eq!(
            kept.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            vec![
                "ledgerd-core",
                "ledgerd-api",
                "notifierd",
                "integrate-verify"
            ]
        );
        assert_eq!(
            relaxed.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            vec!["web-console", "web-viz", "decisions-doc"]
        );
        // the reasons name the evidence, not a rule
        let reason = |rows: &[(String, String)], t: &str| {
            rows.iter().find(|(id, _)| id == t).unwrap().1.clone()
        };
        assert!(
            reason(&kept, "ledgerd-core").contains(
                "owns `app/__main__.py` inside `app/` — the skeleton writes its `app/__main__.py`"
            ),
            "{}",
            reason(&kept, "ledgerd-core")
        );
        assert!(
            reason(&kept, "notifierd").contains("owns `app/notifierd.py` inside `app/`"),
            "{}",
            reason(&kept, "notifierd")
        );
        assert_eq!(
            reason(&kept, "integrate-verify"),
            "the join waits on every task"
        );
        let web = reason(&relaxed, "web-console");
        assert!(
            web.contains("`web/index.html`, `web/styles.css`, `web/app.js`")
                && web.contains("`app/`, `app/ledgerd/`, `app/notifierd/`")
                && web.contains("planned deps kept: `ledgerd-api`"),
            "{web}"
        );
        assert!(
            reason(&relaxed, "decisions-doc").contains("no planned deps — Ready at BUILD start"),
            "{}",
            reason(&relaxed, "decisions-doc")
        );
        // the repairs neither undo a verdict nor need to revisit one, and the DAG loads
        repair_plan_flags(&mut v, spec, TargetLang::Python);
        assert_eq!(deps_of(&v, "web-console"), vec!["ledgerd-api"]);
        assert!(deps_of(&v, "ledgerd-core").contains(&"skeleton".to_string()));
        let dag = goose_swarm::Dag::from_planner_json(&v.to_string()).unwrap();
        assert!(dag.tasks["web-console"].spec.deps == vec!["ledgerd-api".to_string()]);
    }

    /// The shadow clause: a top-level `app.py` beside the skeleton's `app/__main__.py` is not
    /// under `app/` as a string, but it IS the module the collision repair rewrites into
    /// `app/impl.py` — so the pre-repair verdict must already keep it, from the same
    /// `module_package_collisions` derivation the repair uses, or the repaired plan would carry a
    /// package member with no skeleton dependency.
    #[test]
    fn a_module_shadowing_a_skeleton_package_keeps_the_dependency() {
        let spec = "Build `app`. Runs as `python -m app --port N`.";
        let mut v: serde_json::Value = serde_json::from_str(
            r#"{"subtasks":[
                {"id":"core","files":["app.py"],"depends_on":[],"description":"core"},
                {"id":"docs","files":["README.md"],"depends_on":[],"description":"docs"},
                {"id":"integrate-verify","files":[],"depends_on":["core","docs"],"description":"verify"}
            ]}"#,
        )
        .unwrap();
        let ev = prepend_skeleton_task(&mut v, spec).unwrap();
        assert_eq!(
            strings(&v["subtasks"][0]["files"]),
            vec!["app/__main__.py", "app/__init__.py"],
            "no planned task puts a file in app/, so the skeleton creates the package"
        );
        let core = &v["subtasks"][1];
        assert!(strings(&core["depends_on"]).contains(&"skeleton".to_string()));
        let reason = ev["skeleton_dep_kept"][0]["reason"].as_str().unwrap();
        assert!(
            reason.contains("owns `app.py`, the module shadow of `app/`"),
            "{reason}"
        );
        assert_eq!(ev["skeleton_dep_relaxed"][0]["task"], "docs");
        let r = repair_plan_flags(&mut v, spec, TargetLang::Python);
        assert!(
            r.actions
                .iter()
                .any(|a| a.contains("rewritten to `app/impl.py`")),
            "{:?}",
            r.actions
        );
        assert_eq!(strings(&v["subtasks"][1]["files"]), vec!["app/impl.py"]);
        assert!(strings(&v["subtasks"][1]["depends_on"]).contains(&"skeleton".to_string()));
    }

    /// The one-door check for this guarantee: the dag_fallback arm walks the SAME
    /// `finalize_plan_before_dag`, so a fallback plan's skeleton brief carries the post-repair
    /// paths too.
    #[test]
    fn the_dag_fallback_arm_regenerates_the_skeleton_brief_too() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let sink: Arc<dyn EventSink> = Arc::new(NullSink);
        let out = finalize_plan_before_dag(
            R6C_SHAPED_PLAN.to_string(),
            spec,
            false,
            TargetLang::Python,
            &sink,
            "dag_fallback",
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["subtasks"][0]["id"], "skeleton");
        let desc = v["subtasks"][0]["description"].as_str().unwrap();
        assert!(
            desc.contains("- ledgerd-core: app/ledgerd/impl.py"),
            "{desc}"
        );
        assert!(!desc.contains("app/ledgerd.py"), "{desc}");
    }

    /// A plan the repairs do not change regenerates identical text: the refresh is a no-op with
    /// no action, and the dispatched brief is byte-identical to what the prepend built.
    #[test]
    fn a_clean_plan_keeps_its_skeleton_brief_byte_identical() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let clean = r#"{"subtasks":[
            {"id":"ledgerd","files":["app/ledgerd/impl.py","app/db.py"],"depends_on":[],"description":"ledgerd service"},
            {"id":"notifier","files":["app/notifierd/impl.py"],"depends_on":[],"description":"notifier service"},
            {"id":"web","files":["web/index.html","web/app.js","web/styles.css","web/viz.js"],"depends_on":[],"description":"console"},
            {"id":"integrate-verify","files":[],"depends_on":["ledgerd","notifier","web"],"description":"verify"}
        ]}"#;
        let mut v: serde_json::Value = serde_json::from_str(clean).unwrap();
        prepend_skeleton_task(&mut v, spec).expect("the sb-7 spec advertises boots");
        let before = v["subtasks"][0]["description"]
            .as_str()
            .unwrap()
            .to_string();
        let r = repair_plan_flags(&mut v, spec, TargetLang::Python);
        assert!(r.is_noop(), "{:?}", r.actions);
        assert_eq!(v["subtasks"][0]["description"].as_str().unwrap(), before);
        let mut none = Vec::new();
        refresh_skeleton_description(&mut v, spec, &mut none);
        assert!(none.is_empty(), "{none:?}");
    }
}
