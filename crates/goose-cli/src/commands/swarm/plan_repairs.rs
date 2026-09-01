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
/// Rule (d)'s tail header — the engine-required endpoint note `repair_unassigned_endpoints`
/// appends to an entry owner's brief. `decisions::brief_decisions_block` cuts before this one
/// too: rule (d) runs BEFORE rule (e) in `repair_plan_flags`, so a block-carrying entry owner's
/// brief ends `…decisions block…\n\nADVERTISED SURFACE…\n\nFILES NAMED ABOVE…`, and a cut at
/// the unowned tail alone handed a repair shard the owner's ENTRY instruction as "decisions"
/// (2a D11's independent refuter, 2026-09-01).
pub(super) const ADVERTISED_SURFACE_HEADER: &str = "ADVERTISED SURFACE (engine-required)";
use super::spec_surface::{path_token_named, spec_surface_rows, SpecSurface};
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

/// (f) THE JOIN WAITS ON EVERY TASK — structurally, whatever put the task in the plan. r6c's
/// `integrate-verify.depends_on` was [ledgerd-api, ledgerd-core, notifierd, skeleton, web-console,
/// web-viz]: `decisions-doc`, added by a plan patch that never touched the join's deps, was absent,
/// so the join was Ready while a non-sink task could still be running and would have integrated
/// an incomplete tree. `pin_sink_id` only renames; with the replanner's claim gate deleted (D11b)
/// DAG readiness is the only predicate the join has. So, LAST in the chain (after every rule that
/// adds or removes a task): the join's deps ∪= every non-sink task id. The join owns nothing, so a
/// wider dependency set never cascades a failure onto it — the scheduler relaxes a file-less
/// dependent through an upstream failure — it only makes the join wait for what it verifies.
/// Loud: one action line and a self-describing `sink_deps_completed{added}` row when anything
/// was added; nothing at all on a plan whose join already waits on everything (idempotent).
pub(super) fn repair_sink_deps(
    plan: &mut serde_json::Value,
    actions: &mut Vec<String>,
) -> Option<serde_json::Value> {
    let subtasks = plan.get_mut("subtasks").and_then(|s| s.as_array_mut())?;
    let ids: Vec<String> = subtasks
        .iter()
        .filter_map(|t| t.get("id").and_then(|i| i.as_str()).map(String::from))
        .filter(|id| id != goose_swarm::SINK_ID)
        .collect();
    let sink = subtasks
        .iter_mut()
        .find(|t| t.get("id").and_then(|i| i.as_str()) == Some(goose_swarm::SINK_ID))?;
    let obj = sink.as_object_mut()?;
    let deps = obj
        .entry("depends_on")
        .or_insert_with(|| serde_json::json!([]));
    let deps = deps.as_array_mut()?;
    let mut added: Vec<String> = Vec::new();
    for id in ids {
        if !deps.iter().any(|d| d.as_str() == Some(id.as_str())) {
            deps.push(serde_json::Value::String(id.clone()));
            added.push(id);
        }
    }
    if added.is_empty() {
        return None;
    }
    actions.push(format!(
        "`{}` did not depend on {added:?}: the join waits on every task — added",
        goose_swarm::SINK_ID
    ));
    Some(serde_json::json!({
        "event": "sink_deps_completed",
        "task": goose_swarm::SINK_ID,
        "added": added,
    }))
}

/// (d) AN ADVERTISED ENDPOINT NO SERVICE BRIEF MENTIONS is appended to the brief of the task
/// owning that service's entry file (r0 shipped `GET /` as a 404: the frontend brief said the page
/// is served at `/`, the sink said probe it, no backend brief served it). Moved verbatim from
/// swarm.rs under the incremental-split law, paying for rule (f)'s wiring in the root.
pub(super) fn repair_unassigned_endpoints(
    plan: &mut serde_json::Value,
    spec: &str,
    actions: &mut Vec<String>,
) {
    let missing = super::unassigned_endpoints(plan, spec);
    if missing.is_empty() {
        return;
    }
    let invocations = super::spec_python_invocations(spec);
    let Some(subtasks) = plan.get_mut("subtasks").and_then(|s| s.as_array_mut()) else {
        return;
    };
    // A row's service is the invocation whose last segment is the service's name (`ledgerd` ->
    // `app.ledgerd`); a service the spec boots under another name falls to the first invocation.
    let invocation_for = |service: Option<&str>| -> Option<&String> {
        service
            .and_then(|name| {
                invocations
                    .iter()
                    .find(|inv| inv.rsplit('.').next() == Some(name))
            })
            .or(invocations.first())
    };
    let mut by_task: Vec<(usize, String, Vec<AdvertisedEndpoint>)> = Vec::new();
    for ep in missing {
        let Some(inv) = invocation_for(ep.service.as_deref()) else {
            continue;
        };
        let Some(ti) = super::entry_owner_index(subtasks, inv) else {
            continue;
        };
        match by_task.iter_mut().find(|(i, _, _)| *i == ti) {
            Some((_, _, eps)) => eps.push(ep),
            None => by_task.push((ti, inv.clone(), vec![ep])),
        }
    }
    for (ti, inv, eps) in by_task {
        let st = &mut subtasks[ti];
        let id = st
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let Some(desc) = st
            .get_mut("description")
            .and_then(|d| d.as_str().map(str::to_string))
        else {
            continue;
        };
        let rows: Vec<String> = eps
            .iter()
            .map(|e| match &e.expect {
                Some(x) => format!("- `{} {}` -> EXPECT {x}", e.method, e.path),
                None => format!("- `{} {}`", e.method, e.path),
            })
            .collect();
        let note = format!(
            "\n\n{ADVERTISED_SURFACE_HEADER}: the spec's endpoint table lists these on this \
             service and no brief of a task owning service code mentions them, so as planned they \
             would ship as 404s. This task owns the entry of `python -m {inv}`, so it serves each \
             one exactly as the table says:\n{}",
            rows.join("\n")
        );
        st["description"] = serde_json::Value::String(format!("{desc}{note}"));
        actions.push(format!(
            "`{id}` (entry of `{inv}`): appended {} advertised endpoint(s) no service brief \
             mentioned: {}",
            eps.len(),
            eps.iter()
                .map(|e| format!("{} {}", e.method, e.path))
                .collect::<Vec<_>>()
                .join(", ")
        ));
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

/// One advertised endpoint row, with the shape the spec expects of it (`spec_surface_rows`'
/// `-> EXPECT …` tail) when the table carried one.
pub(super) struct AdvertisedEndpoint {
    pub(super) service: Option<String>,
    pub(super) method: String,
    pub(super) path: String,
    pub(super) expect: Option<String>,
}

/// The spec's endpoint rows that no brief of a task owning service code mentions.
///
/// "Service code" is any file under a `python -m` invocation's top-level package (the module form
/// `X.py` included), and with no invocation in the spec, any file at all. That restriction is the whole
/// finding from r0: `index-html`'s brief said the page is served at `/` and the sink's brief said to
/// probe it, and neither task can serve anything — `GET /` was mentioned twice and implemented nowhere.
pub(super) fn unassigned_endpoints(
    plan: &serde_json::Value,
    spec: &str,
) -> Vec<AdvertisedEndpoint> {
    let SpecSurface { rows, .. } = spec_surface_rows(spec);
    if rows.is_empty() {
        return Vec::new();
    }
    let Some(subtasks) = plan.get("subtasks").and_then(|s| s.as_array()) else {
        return Vec::new();
    };
    let roots: Vec<String> = super::spec_python_invocations(spec)
        .iter()
        .map(|inv| inv.split('.').next().unwrap_or(inv).to_string())
        .collect();
    let serves = |file: &str| {
        roots.is_empty()
            || roots
                .iter()
                .any(|r| file.starts_with(&format!("{r}/")) || file == format!("{r}.py"))
    };
    let serving_briefs: Vec<&str> = subtasks
        .iter()
        .filter(|st| {
            st.get("files")
                .and_then(|f| f.as_array())
                .is_some_and(|a| a.iter().any(|f| f.as_str().is_some_and(serves)))
        })
        .filter_map(|st| st.get("description").and_then(|d| d.as_str()))
        .collect();
    // One row per METHOD + PATH (query string ignored, as `brief_mentions_path` ignores it). The
    // FIRST row used to win outright, so sb-7's overview table (line 129: `POST/GET |
    // /api/drafts... | section 5`) shadowed §5's own rows (316: `create from {...} →
    // draft.created`; 320: `GET /api/drafts?state=` → `{"data": [...], "total": <int>}`) and the
    // builder read a pointer as the response contract (S11). A shape-bearing row REPLACES a
    // `section N` / `§N` pointer row — path and all, the section's row is the authoritative one;
    // otherwise the first row stays.
    let sans_query = |path: &str| path.split('?').next().unwrap_or(path).to_string();
    let mut kept: Vec<AdvertisedEndpoint> = Vec::new();
    for (service, row) in rows {
        let mut it = row.splitn(3, ' ');
        let (Some(method), Some(path)) = (it.next(), it.next()) else {
            continue;
        };
        let expect = it
            .next()
            .and_then(|rest| rest.strip_prefix("-> EXPECT "))
            .map(str::to_string);
        let this = AdvertisedEndpoint {
            service,
            method: method.to_string(),
            path: path.to_string(),
            expect,
        };
        match kept
            .iter_mut()
            .find(|k| k.method == this.method && sans_query(&k.path) == sans_query(&this.path))
        {
            Some(k) => {
                let kept_is_pointer = k.expect.as_deref().is_some_and(section_pointer);
                let this_is_shape = this.expect.as_deref().is_some_and(|x| !section_pointer(x));
                if kept_is_pointer && this_is_shape {
                    *k = this;
                }
            }
            None => kept.push(this),
        }
    }
    kept.into_iter()
        .filter(|e| {
            !serving_briefs
                .iter()
                .any(|d| brief_mentions_path(d, &e.path))
        })
        .collect()
}

/// An EXPECT cell that points at a spec section instead of stating a shape: `section 5`,
/// `§5`, `see section 3.2`, `sections 5-6`. Anything with a word of its own is a shape.
pub(super) fn section_pointer(expect: &str) -> bool {
    let e = expect.trim().to_ascii_lowercase();
    let e = e.strip_prefix("see ").unwrap_or(&e).trim();
    let rest = e
        .strip_prefix('§')
        .or_else(|| e.strip_prefix("sections"))
        .or_else(|| e.strip_prefix("section"))
        .map(str::trim);
    rest.is_some_and(|r| {
        !r.is_empty()
            && r.chars().any(|c| c.is_ascii_digit())
            && r.chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | ' ' | '-' | '&' | '§'))
    })
}

/// Whether a brief names an advertised path: `/api/payments` inside a longer path (`/api/payments/x`,
/// `app/api/payments`) does not count, a parameter segment (`<id>`, `{id}`, `:id`) matches any one
/// segment, a query string is ignored, and a lone `/` counts only when it is spelled the way the
/// endpoint table spells it — backticked, after an HTTP method, or closing a URL — because a slash
/// between two words is punctuation.
fn brief_mentions_path(brief: &str, path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    let is_path_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-');
    let is_param = |seg: &str| seg.contains(['<', '{', ':']);
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let from = |pos: usize| brief.get(pos..).unwrap_or("");
    let last_before = |pos: usize| brief.get(..pos).and_then(|s| s.chars().next_back());
    for (start, _) in brief.match_indices('/') {
        let prev = last_before(start);
        if prev.is_some_and(|c| is_path_char(c) && !c.is_ascii_digit()) {
            continue;
        }
        let mut pos = start;
        let mut matched = true;
        for seg in &segments {
            if !from(pos).starts_with('/') {
                matched = false;
                break;
            }
            pos += 1;
            if is_param(seg) {
                let n = from(pos)
                    .chars()
                    .take_while(|c| {
                        !matches!(c, '/' | '`' | '"' | '\'' | ')' | ']' | ',' | '?' | '#')
                            && !c.is_whitespace()
                    })
                    .map(char::len_utf8)
                    .sum::<usize>();
                if n == 0 {
                    matched = false;
                    break;
                }
                pos += n;
            } else if from(pos).starts_with(seg) {
                pos += seg.len();
            } else {
                matched = false;
                break;
            }
        }
        if !matched {
            continue;
        }
        if from(pos).chars().next().is_some_and(is_path_char) {
            continue;
        }
        if path == "/" {
            let before = brief.get(..start).unwrap_or("").trim_end();
            let word = before
                .trim_end_matches(|c: char| !c.is_ascii_alphabetic())
                .rsplit(|c: char| !c.is_ascii_alphabetic())
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            let standalone_tick = prev == Some('`')
                && from(pos).starts_with('`')
                && last_before(start - 1).is_none_or(|c| c.is_whitespace() || c == '(');
            let anchored = standalone_tick
                || prev.is_some_and(|c| c.is_ascii_digit())
                || matches!(
                    word.as_str(),
                    "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
                );
            if !anchored {
                continue;
            }
        }
        return true;
    }
    false
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

    /// r6c's real plan shape (plan_loaded seq 1387: eight tasks, ids/files/deps verbatim): the
    /// review patch added `decisions-doc` without touching the join's deps, so the join could
    /// claim while decisions-doc still ran. The repair adds exactly that one id, says so, and a
    /// second pass adds nothing. A shard/merger task added by a later patch is covered the same
    /// way — the rule is "every non-sink task", not a list.
    #[test]
    fn the_join_waits_on_every_task_r6c_shape() {
        let mut v = serde_json::json!({"subtasks": [
            {"id": "skeleton", "files": ["app/__main__.py", "app/ledgerd/__main__.py", "app/ledgerd/__init__.py", "app/notifierd/__main__.py", "app/notifierd/__init__.py"], "depends_on": [], "description": "skeleton"},
            {"id": "ledgerd-core", "files": ["app/ledgerd/impl.py", "app/db.py", "app/sync.py", "app/ledger.py", "app/outbox.py", "README.md"], "depends_on": ["skeleton"], "description": "core"},
            {"id": "ledgerd-api", "files": ["app/api.py", "app/webhooks.py", "app/drafts.py", "app/auth.py"], "depends_on": ["ledgerd-core", "skeleton"], "description": "api"},
            {"id": "notifierd", "files": ["app/notifierd/impl.py", "app/notify_store.py"], "depends_on": ["skeleton"], "description": "notifierd"},
            {"id": "decisions-doc", "files": ["DECISIONS.md"], "depends_on": ["skeleton"], "description": "decisions"},
            {"id": "web-console", "files": ["web/index.html", "web/styles.css", "web/app.js"], "depends_on": ["skeleton"], "description": "console"},
            {"id": "web-viz", "files": ["web/viz.js"], "depends_on": ["skeleton"], "description": "viz"},
            {"id": "integrate-verify", "files": [], "depends_on": ["ledgerd-core", "ledgerd-api", "notifierd", "web-console", "web-viz", "skeleton"], "description": "verify"}
        ]});
        let mut actions = Vec::new();
        let row = repair_sink_deps(&mut v, &mut actions).expect("decisions-doc was missing");
        assert_eq!(row["event"], "sink_deps_completed");
        assert_eq!(row["added"], serde_json::json!(["decisions-doc"]));
        assert_eq!(actions.len(), 1, "{actions:?}");
        assert!(actions[0].contains("decisions-doc"), "{actions:?}");
        let deps = strings(&task(&v, "integrate-verify")["depends_on"]);
        for id in [
            "skeleton",
            "ledgerd-core",
            "ledgerd-api",
            "notifierd",
            "decisions-doc",
            "web-console",
            "web-viz",
        ] {
            assert!(deps.contains(&id.to_string()), "{id} missing from {deps:?}");
        }
        assert!(!deps.contains(&"integrate-verify".to_string()));
        goose_swarm::Dag::from_planner_json(&v.to_string()).expect("loads");
        let mut again = Vec::new();
        assert!(repair_sink_deps(&mut v, &mut again).is_none(), "idempotent");
        assert!(again.is_empty());
        // Through the whole chain the row rides beside the mentions and the action beside the rest.
        let mut w = v.clone();
        w["subtasks"][7]["depends_on"] = serde_json::json!(["skeleton"]);
        let r = super::super::repair_plan_flags(&mut w, "");
        assert!(
            r.actions
                .iter()
                .any(|a| a.contains("the join waits on every task")),
            "{:?}",
            r.actions
        );
        assert_eq!(
            r.mentions
                .iter()
                .filter(|m| m["event"] == "sink_deps_completed")
                .count(),
            1
        );
        assert_eq!(
            strings(&task(&w, "integrate-verify")["depends_on"]).len(),
            7
        );
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
    #[test]
    fn plan_repair_brief_mentions_path_truth_table() {
        let yes = [
            ("serve `GET /` from web/", "/"),
            ("the page at http://127.0.0.1:8080/ loads", "/"),
            ("the table says `/` is the frontend", "/"),
            (
                "fetch `/api/payments?limit=5`",
                "/api/payments?limit=<int>&offset=<int>",
            ),
            (
                "`/api/payments/{id}/note` with If-Match",
                "/api/payments/<id>/note",
            ),
            ("POST /api/payments/:id/note", "/api/payments/<id>/note"),
            ("GET /api/health returns", "/api/health"),
            ("(see /api/health)", "/api/health"),
        ];
        for (brief, path) in yes {
            assert!(
                brief_mentions_path(brief, path),
                "{brief:?} should mention {path}"
            );
        }
        let no = [
            ("`.cur-total`/`.rev-total` elements", "/"),
            ("`except:`/`except: pass`", "/"),
            ("maker / checker", "/"),
            ("GET /api/payments/<id>", "/api/payments"),
            ("app/api/payments", "/api/payments"),
            ("/api/payments/<id>", "/api/payments/<id>/note"),
            ("/api/v12", "/api/v1"),
            ("/api/healthz", "/api/health"),
        ];
        for (brief, path) in no {
            assert!(
                !brief_mentions_path(brief, path),
                "{brief:?} must not mention {path}"
            );
        }
    }

    /// S11 — sb-7's `/api/drafts` appears twice: the overview table (spec line 129) points at
    /// `section 5`, §5's own table (line 316) states the shape. One rule: a shape-bearing EXPECT
    /// replaces a pointer, whatever the row order; a task owning ledgerd's service file whose
    /// brief names nothing therefore inherits the shape, never the pointer.
    #[test]
    fn a_shape_bearing_expect_replaces_a_section_pointer_on_sb7s_drafts_pair() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let plan = serde_json::json!({"subtasks": [
            {"id": "ledgerd-core", "description": "the ledger service", "files": ["app/ledgerd/impl.py"], "depends_on": []},
        ]});
        let missing = unassigned_endpoints(&plan, spec);
        let drafts: Vec<&AdvertisedEndpoint> = missing
            .iter()
            .filter(|e| e.method == "POST" && e.path == "/api/drafts")
            .collect();
        assert_eq!(drafts.len(), 1, "one row per METHOD PATH");
        let expect = drafts[0].expect.as_deref().expect("§5 states a shape");
        assert!(
            expect.contains("draft.created"),
            "the shape row wins over the pointer: {expect:?}"
        );
        assert!(!section_pointer(expect), "{expect:?}");
        // The GET pair differs by query string (`/api/drafts` vs §5's `/api/drafts?state=`): one
        // endpoint, so one row — the section's, path and shape.
        let get: Vec<&AdvertisedEndpoint> = missing
            .iter()
            .filter(|e| e.method == "GET" && e.path.starts_with("/api/drafts"))
            .collect();
        assert_eq!(
            get.len(),
            1,
            "{:?}",
            missing
                .iter()
                .map(|e| format!("{} {}", e.method, e.path))
                .collect::<Vec<_>>()
        );
        assert_eq!(get[0].path, "/api/drafts?state=");
        assert!(
            get[0]
                .expect
                .as_deref()
                .is_some_and(|x| x.contains("\"total\"")),
            "{:?}",
            get[0].expect
        );
        for (yes, no) in [
            ("section 5", "maker or checker"),
            ("§5", "{\"accepted\": true}"),
            ("see section 3.2", "create from {...} → draft.created"),
            ("sections 5-6", "5 items"),
            ("Section 5 & 6", "section-scoped list"),
        ] {
            assert!(section_pointer(yes), "{yes:?} is a pointer");
            assert!(!section_pointer(no), "{no:?} is a shape");
        }
    }
}
