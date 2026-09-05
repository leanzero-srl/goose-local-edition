//! The plan's measured decomposition numbers — ONE rule in ONE place, read at SYNTHESIS,
//! after a plan repair, and by the skeleton's dependency verdicts. Sibling module under the
//! incremental-split law (development_gates::swarm_rs_line_count_only_decreases). Moved
//! verbatim from swarm.rs — behavior unchanged — paying for the VA-023 wiring in
//! `finalize_plan_before_dag` (the skeleton's per-task verdict rows flattened onto the sink as
//! first-class `skeleton_dep_kept` / `skeleton_dep_relaxed` events) landing in the same commit.

/// The three decomposition numbers, computed from a plan's own JSON.
///
/// ONE rule in ONE place. It is emitted at SYNTHESIS and again after every REVIEW patch, and those two
/// readings are only comparable if the same code produces them — a second copy would drift and the drift
/// would be invisible, because each side would look internally consistent.
///
/// `tasks_sharing_a_file` counts over-collision, `tasks_owning_nothing` counts over-decomposition, and
/// `shared_files` names the offenders so a reader is not left re-deriving the ownership map by hand.
pub(super) fn decomposition_of(plan_json: &str) -> serde_json::Value {
    let tasks: Vec<serde_json::Value> = serde_json::from_str::<serde_json::Value>(plan_json)
        .ok()
        .and_then(|v| v.get("subtasks").and_then(|t| t.as_array()).cloned())
        .unwrap_or_default();
    let mut owner_count: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for t in &tasks {
        for f in t
            .get("files")
            .and_then(|f| f.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(f) = f.as_str() {
                *owner_count.entry(f.to_string()).or_default() += 1;
            }
        }
    }
    let mut shared: Vec<serde_json::Value> = owner_count
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(f, _)| {
            let owners: Vec<&str> = tasks
                .iter()
                .filter(|t| {
                    t.get("files")
                        .and_then(|v| v.as_array())
                        .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(f.as_str())))
                })
                .filter_map(|t| t.get("id").and_then(|i| i.as_str()))
                .collect();
            serde_json::json!({ "file": f, "tasks": owners })
        })
        .collect();
    shared.sort_by_key(|v| v["file"].as_str().unwrap_or("").to_string());
    let owning_nothing: Vec<&str> = tasks
        .iter()
        .filter(|t| {
            t.get("files")
                .and_then(|f| f.as_array())
                .is_none_or(|a| a.is_empty())
        })
        .filter_map(|t| t.get("id").and_then(|i| i.as_str()))
        .filter(|id| *id != "integrate-verify")
        .collect();
    // A MODULE SHADOWED BY A PACKAGE IS DEAD CODE, and path equality cannot see it. `app/viz.py` and
    // `app/viz/layout.py` are different strings and the same import path; Python loads the package and
    // the file is unreachable. MEASURED on the first run to reach BUILD: viz-records-endpoint wrote
    // 1,796 bytes into `app/viz.py` that nothing could import, because viz-layout-transforms had created
    // `app/viz/`.
    //
    // It lived ONLY in the inline second copy of these counters, which is read at SYNTHESIS and never
    // again — so a shadow INTRODUCED BY A REVIEW PATCH was invisible in the only reading taken after the
    // patch, which is exactly the defect that killed the first run to reach BUILD.
    let dirs: std::collections::HashSet<String> = owner_count
        .keys()
        .filter_map(|f| f.rsplit_once('/').map(|(d, _)| d.to_string()))
        .collect();
    let mut shadowed_modules: Vec<String> = owner_count
        .keys()
        .filter(|f| f.ends_with(".py"))
        .filter(|f| dirs.contains(f.trim_end_matches(".py")))
        .cloned()
        .collect();
    shadowed_modules.sort();
    serde_json::json!({
        "tasks": tasks.len(),
        "distinct_files": owner_count.len(),
        "tasks_sharing_a_file": owner_count.values().filter(|n| **n > 1).count(),
        "shared_files": shared,
        "tasks_owning_nothing": owning_nothing,
        "module_package_collisions": shadowed_modules,
    })
}

#[cfg(test)]
mod tests {
    use super::decomposition_of;

    /// These three numbers decide whether a plan is over-decomposed, under-decomposed, or fine, and they
    /// are read at SYNTHESIS and again after every REVIEW patch. A wrong answer here misleads every future
    /// run silently, because each side would look internally consistent.
    #[test]
    fn decomposition_names_the_sharers_and_excludes_the_sink() {
        let plan = r#"{"subtasks":[
            {"id":"viz-rendering","files":["web/viz.js"],"depends_on":[]},
            {"id":"viz-interaction","files":["web/viz.js"],"depends_on":[]},
            {"id":"ledgerd","files":["app/ledgerd.py"],"depends_on":[]},
            {"id":"background-color","files":[],"depends_on":[]},
            {"id":"integrate-verify","files":[],"depends_on":["ledgerd"]}
        ]}"#;
        let d = decomposition_of(plan);
        assert_eq!(d["tasks"], 5);
        assert_eq!(d["distinct_files"], 2, "web/viz.js and app/ledgerd.py");
        assert_eq!(
            d["tasks_sharing_a_file"], 1,
            "only web/viz.js has two owners"
        );
        let shared = d["shared_files"].as_array().unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0]["file"], "web/viz.js");
        let owners: Vec<&str> = shared[0]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(
            owners.contains(&"viz-rendering") && owners.contains(&"viz-interaction"),
            "the collision must name BOTH owners, not just count them: {owners:?}"
        );
        let nothing: Vec<&str> = d["tasks_owning_nothing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            nothing,
            vec!["background-color"],
            "integrate-verify owns nothing BY DESIGN and must never be flagged: {nothing:?}"
        );
    }

    /// A plan that cannot be parsed must read as empty rather than panicking mid-run.
    #[test]
    fn decomposition_of_a_broken_plan_is_empty_not_a_panic() {
        let d = decomposition_of("not json at all");
        assert_eq!(d["tasks"], 0);
        assert_eq!(d["tasks_sharing_a_file"], 0);
        assert!(d["shared_files"].as_array().unwrap().is_empty());
    }
}
