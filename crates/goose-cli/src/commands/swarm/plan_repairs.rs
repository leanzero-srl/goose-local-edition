//! Two plan repairs from `repair_plan_flags`' chain, beside each other because both are about
//! WHO OWNS A FILE versus what a brief says: the join's file strip (rule on the pinned sink) and
//! the brief-mention rule (e). Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases): `repair_sink_files` moved verbatim
//! from swarm.rs with its test, paying for rule (e)'s wiring in the root.

use std::collections::HashSet;

use super::skeleton::SKELETON_ID;

/// Rule (e)'s tail header — `decisions::brief_decisions_block` cuts a brief's decisions block
/// before it, because the list is the owner's, not a repair shard's.
pub(super) const UNOWNED_FILES_HEADER: &str = "FILES NAMED ABOVE THAT ANOTHER TASK OWNS";
use super::spec_surface::path_token_named;
use super::string_list;

/// THE JOIN OWNS NOTHING — structurally, at repair time, whatever synthesis said. r4 (2026-08-30)
/// shipped `integrate-verify` owning `README.md`: scheduler.rs relaxes a dependent through an
/// upstream failure ONLY if it owns no files, so a file-owning join is cascaded-Failed by any
/// build failure and the app never binds a port (the r0 class). The files move to the first
/// file-owning non-sink task (the skeleton, when present) so a spec-mandated file keeps an owner.
pub(super) fn repair_sink_files(plan: &mut serde_json::Value, actions: &mut Vec<String>) {
    let Some(subtasks) = plan.get_mut("subtasks").and_then(|s| s.as_array_mut()) else {
        return;
    };
    let sink_idx = subtasks
        .iter()
        .position(|t| t.get("id").and_then(|i| i.as_str()) == Some(goose_swarm::SINK_ID));
    let Some(sink_idx) = sink_idx else { return };
    // An absent/non-array `files` field IS the desired state (the join owns nothing) — nothing to
    // strip, so the early return is the honest reading, not a default standing in for one.
    let Some(moved) = subtasks[sink_idx]
        .get("files")
        .and_then(|f| f.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|f| f.as_str().map(String::from))
                .collect::<Vec<String>>()
        })
        .filter(|m| !m.is_empty())
    else {
        return;
    };
    if let Some(files) = subtasks[sink_idx]
        .get_mut("files")
        .and_then(|f| f.as_array_mut())
    {
        files.clear();
    }
    // The home must have a real id (it goes in the action sentence) — an id-less task cannot load
    // as a DAG anyway, so requiring one here loses nothing.
    let home = subtasks.iter().enumerate().find_map(|(i, t)| {
        let id = t.get("id").and_then(|v| v.as_str())?;
        if id != goose_swarm::SINK_ID
            && t.get("files")
                .and_then(|f| f.as_array())
                .is_some_and(|a| !a.is_empty())
        {
            Some((i, id.to_string()))
        } else {
            None
        }
    });
    match home {
        Some((hi, home_id)) => {
            if let Some(files) = subtasks[hi].get_mut("files").and_then(|f| f.as_array_mut()) {
                for m in &moved {
                    if !files.iter().any(|f| f.as_str() == Some(m.as_str())) {
                        files.push(serde_json::Value::String(m.clone()));
                    }
                }
            }
            actions.push(format!(
                "`{}` owned {moved:?}: the join owns nothing — moved to `{home_id}`",
                goose_swarm::SINK_ID
            ));
        }
        None => actions.push(format!(
            "`{}` owned {moved:?}: the join owns nothing — dropped (no file-owning task to take them)",
            goose_swarm::SINK_ID
        )),
    }
}

/// (e) A BRIEF THAT NAMES A FILE ANOTHER TASK OWNS (VA-009). r6c web-console's brief read
/// "Ship DECISIONS.md (owned by this slice)" while its `files[]` were web/index.html,
/// web/styles.css and web/app.js and `decisions-doc` owned DECISIONS.md; the worker's think.log
/// at 0% and again at 80% — "YOU OWN 3 FILES… task text says I also own DECISIONS.md.
/// Contradiction." — burned both spans on the engine's own inconsistency and wrote the file
/// anyway. The metadata (files[]) is what the scheduler serialises on; the words are what the
/// model reads; when they disagree the model pays. So, after every ownership repair has run:
/// for each task, every OTHER task's owned path named in its description (token-bounded —
/// `spec_surface::path_token_named`, so `app.js` inside `web/app.js` is not a hit) is (1) listed
/// once at the end of the brief as not-yours-to-write with its owner named, and (2) if the brief
/// CLAIMS it — the measured phrase "<path> (owned by this slice)" / "(owned by this task)" — the
/// claim is rewritten to name the real owner. Quoted spec text and handoffs are otherwise left
/// verbatim (they are the request's and the researchers' own words). MILD and loud: a repair,
/// never a refusal; one `brief_names_unowned_file` row per (task, path). The engine-authored
/// briefs are exempt by construction: the skeleton's PLANNED MODULES block lists every task's
/// files on purpose and the join owns nothing and verifies everything.
pub(super) fn repair_brief_file_mentions(
    plan: &mut serde_json::Value,
    actions: &mut Vec<String>,
) -> Vec<serde_json::Value> {
    let Some(subtasks) = plan.get_mut("subtasks").and_then(|s| s.as_array_mut()) else {
        return Vec::new();
    };
    let owners: Vec<(String, String)> = subtasks
        .iter()
        .filter_map(|t| {
            t.get("id")
                .and_then(|i| i.as_str())
                .map(|id| (id.to_string(), t))
        })
        .flat_map(|(id, t)| {
            string_list(&t["files"])
                .into_iter()
                .map(move |f| (f, id.clone()))
        })
        .collect();
    let mut rows = Vec::new();
    for t in subtasks.iter_mut() {
        let Some(id) = t.get("id").and_then(|i| i.as_str()).map(str::to_string) else {
            continue;
        };
        if id == goose_swarm::SINK_ID || id == SKELETON_ID {
            continue;
        }
        let own: HashSet<String> = string_list(&t["files"]).into_iter().collect();
        let Some(desc) = t.get("description").and_then(|d| d.as_str()) else {
            continue;
        };
        let mut desc = desc.to_string();
        let mut named: Vec<(String, String)> = Vec::new();
        for (path, owner) in &owners {
            if *owner == id || own.contains(path) || named.iter().any(|(p, _)| p == path) {
                continue;
            }
            // Idempotent (the chain's own contract, `plan_repair_is_idempotent`): a path this
            // rule already listed for the brief is a repaired fact, not a new mention.
            if desc.contains(&format!("- `{path}` → owned by task")) {
                continue;
            }
            if !path_token_named(path, &desc) {
                continue;
            }
            let mut rewritten = false;
            for claim in ["(owned by this slice)", "(owned by this task)"] {
                for form in [format!("`{path}` {claim}"), format!("{path} {claim}")] {
                    if desc.contains(&form) {
                        desc = desc.replace(
                            &form,
                            &format!("`{path}` (owned by task `{owner}` — do not write it)"),
                        );
                        rewritten = true;
                    }
                }
            }
            actions.push(format!(
                "`{id}`: its brief names `{path}`, owned by `{owner}` — marked not-yours-to-write{}",
                if rewritten {
                    "; its ownership claim rewritten to name the owner"
                } else {
                    ""
                }
            ));
            rows.push(serde_json::json!({
                "event": "brief_names_unowned_file",
                "task": id,
                "path": path,
                "owner": owner,
                "rewritten": rewritten,
            }));
            named.push((path.clone(), owner.clone()));
        }
        if !named.is_empty() {
            desc.push_str(&format!(
                "\n\n{UNOWNED_FILES_HEADER} — read them if you need them, never write them (your \
                 own files are the YOU OWN list):\n"
            ));
            for (path, owner) in &named {
                desc.push_str(&format!("- `{path}` → owned by task `{owner}`\n"));
            }
            t["description"] = serde_json::Value::String(desc);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task<'a>(v: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
        v["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == id)
            .unwrap()
    }
    fn strings(v: &serde_json::Value) -> Vec<String> {
        string_list(v)
    }

    /// THE JOIN OWNS NOTHING, structurally (r4 kill, 2026-08-30): synthesis gave
    /// `integrate-verify` a README.md and nothing stripped it — scheduler relax works only for a
    /// file-less join, so any build failure would have cascaded the sink to Failed and the app
    /// would never bind a port (the r0 class). The file keeps an owner: the first file-owning
    /// task in plan order (the skeleton, on a real run).
    #[test]
    fn plan_repair_strips_the_sinks_files_to_a_real_owner() {
        let plan = r#"{"subtasks":[
            {"id":"skeleton","files":["app/__main__.py"],"depends_on":[],"description":"wire"},
            {"id":"svc","files":["app/svc.py"],"depends_on":["skeleton"],"description":"svc"},
            {"id":"integrate-verify","files":["README.md"],"depends_on":["skeleton","svc"],"description":"v"}
        ]}"#;
        let mut v: serde_json::Value = serde_json::from_str(plan).unwrap();
        let r = super::super::repair_plan_flags(&mut v, "");
        assert!(
            task(&v, "integrate-verify")["files"]
                .as_array()
                .unwrap()
                .is_empty(),
            "the join owns nothing after repair"
        );
        assert!(
            strings(&task(&v, "skeleton")["files"]).contains(&"README.md".to_string()),
            "the stripped file keeps a real owner"
        );
        assert!(
            r.actions
                .iter()
                .any(|a| a.contains("the join owns nothing")),
            "{:?}",
            r.actions
        );
        goose_swarm::Dag::from_planner_json(&v.to_string()).expect("loads");
    }

    /// r6c web-console, verbatim (plan_loaded seq 1387, description char 1990): the brief claims a
    /// file `decisions-doc` owns while `files[]` holds the three web files. The claim is rewritten
    /// to name the owner, the file is listed as not-yours-to-write, one row names the three facts;
    /// the OWN files named in the same brief raise nothing, and the engine-authored skeleton and
    /// join briefs — which list every file by design — raise nothing either.
    #[test]
    fn a_brief_claiming_another_tasks_file_is_told_the_owner_once() {
        let mut v = serde_json::json!({"subtasks": [
            {"id": "skeleton", "files": ["app/__main__.py"], "depends_on": [],
             "description": "PLANNED MODULES\n- web-console: web/index.html, web/styles.css, web/app.js\n- decisions-doc: DECISIONS.md"},
            {"id": "decisions-doc", "files": ["DECISIONS.md"], "depends_on": ["skeleton"],
             "description": "Publish DECISIONS.md (section 9 corners) first."},
            {"id": "web-console", "files": ["web/index.html", "web/styles.css", "web/app.js"], "depends_on": ["skeleton"],
             "description": "Q: which corners?\nA: Ship DECISIONS.md (owned by this slice) with EXACTLY the headings `## D1`, `## D2`, `## D3`, 2–3 sentences each (choice + why), matching web/app.js behaviour. Never touch app/__main__.py's route table."},
            {"id": "integrate-verify", "files": [], "depends_on": ["skeleton", "decisions-doc", "web-console"],
             "description": "boot, curl every route, read DECISIONS.md and web/app.js"}
        ]});
        let mut actions = Vec::new();
        let rows = repair_brief_file_mentions(&mut v, &mut actions);
        let desc = task(&v, "web-console")["description"].as_str().unwrap();
        assert!(
            desc.contains("`DECISIONS.md` (owned by task `decisions-doc` — do not write it)"),
            "{desc}"
        );
        assert!(!desc.contains("(owned by this slice)"), "{desc}");
        assert!(
            desc.contains("- `DECISIONS.md` → owned by task `decisions-doc`")
                && desc.contains("- `app/__main__.py` → owned by task `skeleton`"),
            "{desc}"
        );
        assert!(
            !desc.contains("`web/app.js` → owned"),
            "its own file is never listed: {desc}"
        );
        let mentions: Vec<(String, String, bool)> = rows
            .iter()
            .map(|r| {
                (
                    r["task"].as_str().unwrap().to_string(),
                    r["path"].as_str().unwrap().to_string(),
                    r["rewritten"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            mentions,
            vec![
                (
                    "web-console".to_string(),
                    "app/__main__.py".to_string(),
                    false
                ),
                ("web-console".to_string(), "DECISIONS.md".to_string(), true),
            ],
            "{rows:?}"
        );
        assert!(rows
            .iter()
            .all(|r| r["event"] == "brief_names_unowned_file"));
        assert!(
            !task(&v, "skeleton")["description"]
                .as_str()
                .unwrap()
                .contains("FILES NAMED ABOVE"),
            "the skeleton's module list is engine text and exempt"
        );
        assert!(
            !task(&v, "integrate-verify")["description"]
                .as_str()
                .unwrap()
                .contains("FILES NAMED ABOVE"),
            "the join verifies everything and is exempt"
        );
        assert!(
            !task(&v, "decisions-doc")["description"]
                .as_str()
                .unwrap()
                .contains("FILES NAMED ABOVE"),
            "a brief naming only its own file raises nothing"
        );
        assert_eq!(actions.len(), 2, "{actions:?}");
        // Idempotent: a second pass finds the owner-naming note, not a new mention.
        let mut again = Vec::new();
        let rows2 = repair_brief_file_mentions(&mut v, &mut again);
        assert!(rows2.is_empty() && again.is_empty(), "{rows2:?} {again:?}");
        assert_eq!(
            task(&v, "web-console")["description"]
                .as_str()
                .unwrap()
                .matches("FILES NAMED ABOVE")
                .count(),
            1
        );
    }
}
