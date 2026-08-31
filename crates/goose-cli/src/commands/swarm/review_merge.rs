//! The one merge of per-lane REVIEW patches. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases): `review_plan_fanned`'s inline union
//! moved here, gaining the loud drop events and per-lane provenance.
//!
//! WHY (r6c, 2026-08-31, run swarm-3node-r0 seq 1382-1384): review-camera's replace of
//! `web-console` (deps ["ledgerd-api","web-viz"], files + DECISIONS.md) vanished between its
//! lane's final output and `plan_patched` (replace=2 add=1) with NO event — review-5 had already
//! claimed `web-console` (deps ["decisions-doc"]) and the union's first-lane-wins HashSet dropped
//! the second claim silently. The surgeon reading the run could not distinguish conflict-resolved
//! from silently-dropped. Fallback gate: absence must be loud.

use goose_swarm::{EventSink, PlanPatch};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

/// Union per-lane patches under FIRST-LANE-WINS: per kind, the first lane (in section order) to
/// claim a task id keeps its element, so two lanes proposing fixes to the same task cannot
/// double-apply; ids compare lowercased and `remove` dedupes the same way. Every dropped element
/// emits `review_patch_dropped {lane, kind, target, reason}` with the rule and the winning lane
/// NAMED — never a silent drop. The returned provenance maps each lane to the ids it actually
/// contributed; it rides `plan_patched.lanes` (an additive key — tick.py and useSwarmRun.ts read
/// named keys only, so the shape stays back-compatible).
pub(super) fn union_lane_patches(
    lanes: Vec<(String, PlanPatch, Vec<String>)>,
    events: &dyn EventSink,
) -> (PlanPatch, Vec<String>, serde_json::Value) {
    let mut patch = PlanPatch::default();
    let mut findings: Vec<String> = Vec::new();
    let mut won: HashMap<(&'static str, String), String> = HashMap::new();
    let mut provenance = serde_json::Map::new();
    for (lane, p, f) in lanes {
        let mut kept_replace: Vec<String> = Vec::new();
        let mut kept_add: Vec<String> = Vec::new();
        let mut kept_remove: Vec<String> = Vec::new();
        for e in p.replace {
            match won.entry(("replace", e.id.to_lowercase())) {
                Entry::Vacant(v) => {
                    v.insert(lane.clone());
                    kept_replace.push(e.id.clone());
                    patch.replace.push(e);
                }
                Entry::Occupied(o) => dropped(events, &lane, "replace", &e.id, o.get()),
            }
        }
        for a in p.add {
            match won.entry(("add", a.id.to_lowercase())) {
                Entry::Vacant(v) => {
                    v.insert(lane.clone());
                    kept_add.push(a.id.clone());
                    patch.add.push(a);
                }
                Entry::Occupied(o) => dropped(events, &lane, "add", &a.id, o.get()),
            }
        }
        for r in p.remove {
            match won.entry(("remove", r.to_lowercase())) {
                Entry::Vacant(v) => {
                    v.insert(lane.clone());
                    kept_remove.push(r.clone());
                    patch.remove.push(r);
                }
                Entry::Occupied(o) => dropped(events, &lane, "remove", &r, o.get()),
            }
        }
        findings.extend(f);
        provenance.insert(
            lane,
            serde_json::json!({
                "replace": kept_replace,
                "add": kept_add,
                "remove": kept_remove,
            }),
        );
    }
    (patch, findings, serde_json::Value::Object(provenance))
}

fn dropped(events: &dyn EventSink, lane: &str, kind: &str, target: &str, winner: &str) {
    events.write_value(serde_json::json!({
        "event": "review_patch_dropped",
        "lane": lane,
        "kind": kind,
        "target": target,
        "reason": format!(
            "first-lane-wins: lane `{winner}` already claimed a {kind} of `{target}` \
             (section order decides; the later lane's element is discarded)"
        ),
    }));
}

/// The STRUCTURAL CLAIM a review finding makes, used to tell a genuinely distinct finding from a
/// rephrasing.
///
/// The old key was `trim().to_lowercase().take(120)` — a PREFIX comparison, which any rewording defeats
/// by construction. Measured on a live run, back when REVIEW looped: rounds 1 and 2 reported one defect
/// as "viz-interaction and viz-rendering-engine share the same file (web/viz.js)", "Two tasks write to
/// the same file (web/viz.js)" and "viz.js written by two tasks", and all three counted as NEW; a later
/// round simply prefixed each with `STILL: ` and got 9 findings with `repeated: 0` on a plan nobody had
/// touched.
///
/// REVIEW is one round now (see `review_once`), and the key collapses the same defect raised by two
/// LANES of it: every lane reads the whole plan, so a collision is reported by whichever lanes noticed.
///
/// The key is (kind, identifiers) because neither alone is safe:
///   - identifiers alone would merge "viz.js is owned twice" with "viz.js is never imported" — two real,
///     different findings about one file. Merging them drops one from the round's findings and from
///     the patch demand that names them, so the kind keeps them apart.
///   - kind alone would merge every ownership finding in the plan into one.
///
/// Identifiers are taken only from high-precision positions — backticked tokens and path-like tokens —
/// and files reduce to their BASENAME, because the same file is named `web/viz.js` in one sentence and
/// `viz.js` in the next. A finding that yields NO identifier falls back to the old text key, so prose
/// findings behave exactly as they did before.
pub(super) fn review_dedupe_key(finding: &str) -> String {
    let lower = finding.trim().to_lowercase();

    let kind = if lower.contains("share")
        || lower.contains("same file")
        || lower.contains("both own")
        || lower.contains("two tasks")
        || lower.contains("duplicate")
        || lower.contains("written by")
        || lower.contains("owned by")
    {
        "ownership"
    } else if lower.contains("unowned")
        || lower.contains("no task")
        || lower.contains("nobody")
        || lower.contains("not owned")
        || lower.contains("missing")
    {
        "unowned"
    } else if lower.contains("depend")
        || lower.contains("wire")
        || lower.contains("wired")
        || lower.contains("import")
    {
        "wiring"
    } else if lower.contains("larger")
        || lower.contains("too large")
        || lower.contains("unbalanced")
        || lower.contains("split")
    {
        "size"
    } else {
        "other"
    };

    let mut ids: Vec<String> = Vec::new();
    let mut push = |tok: &str| {
        let t = tok.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-'
        });
        if t.is_empty() {
            return;
        }
        let base = t.rsplit('/').next().unwrap_or(t);
        if base.len() >= 3 && !ids.iter().any(|x| x == base) {
            ids.push(base.to_string());
        }
    };

    for seg in lower.split('`').skip(1).step_by(2) {
        push(seg);
    }
    for tok in lower.split_whitespace() {
        let cleaned = tok.trim_matches(|c: char| "(),.;:\"'".contains(c));
        let is_pathish = cleaned.contains('/')
            || [
                ".py", ".js", ".ts", ".tsx", ".html", ".css", ".json", ".rs", ".md", ".toml",
            ]
            .iter()
            .any(|e| cleaned.ends_with(e));
        if is_pathish {
            push(cleaned);
        }
    }

    if ids.is_empty() {
        return lower.chars().take(120).collect();
    }
    ids.sort();
    format!("{kind}|{}", ids.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_swarm::{SwarmEvent, TaskEdit};
    use std::sync::Mutex;

    #[derive(Default)]
    struct ValueSink(Mutex<Vec<serde_json::Value>>);
    impl EventSink for ValueSink {
        fn emit(&self, _event: &SwarmEvent) {}
        fn write_value(&self, value: serde_json::Value) {
            self.0.lock().unwrap().push(value);
        }
    }

    fn replace(id: &str, deps: &[&str]) -> TaskEdit {
        TaskEdit {
            id: id.to_string(),
            files: None,
            depends_on: Some(deps.iter().map(|d| d.to_string()).collect()),
        }
    }

    /// The r6c shape verbatim: review-5 (earlier section) claims `web-console` first; the union
    /// keeps its element, and review-camera's competing replace is dropped WITH the rule named —
    /// the run log can now distinguish conflict-resolved from silently-dropped.
    #[test]
    fn a_conflicting_lanes_element_is_dropped_loudly_with_the_rule_named() {
        let sink = ValueSink::default();
        let (patch, _findings, lanes) = union_lane_patches(
            vec![
                (
                    "review-5-the-approval-workflow".to_string(),
                    PlanPatch {
                        replace: vec![
                            replace("web-console", &["decisions-doc"]),
                            replace("ledgerd-api", &["ledgerd-core", "decisions-doc"]),
                        ],
                        add: vec![goose_swarm::TaskAdd {
                            id: "decisions-doc".to_string(),
                            files: vec!["DECISIONS.md".to_string()],
                            ..Default::default()
                        }],
                        remove: vec![],
                    },
                    vec![],
                ),
                (
                    "review-camera-orbit".to_string(),
                    PlanPatch {
                        replace: vec![replace("web-console", &["ledgerd-api", "web-viz"])],
                        add: vec![],
                        remove: vec![],
                    },
                    vec![],
                ),
            ],
            &sink,
        );
        assert_eq!(patch.replace.len(), 2);
        assert_eq!(
            patch.replace[0].depends_on.as_deref(),
            Some(&["decisions-doc".to_string()][..]),
            "the FIRST lane's web-console replace is the one applied"
        );
        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 1, "{events:?}");
        let e = &events[0];
        assert_eq!(e["event"], "review_patch_dropped");
        assert_eq!(e["lane"], "review-camera-orbit");
        assert_eq!(e["kind"], "replace");
        assert_eq!(e["target"], "web-console");
        let reason = e["reason"].as_str().unwrap();
        assert!(reason.contains("first-lane-wins"), "{reason}");
        assert!(
            reason.contains("review-5-the-approval-workflow"),
            "{reason}"
        );
        assert_eq!(
            lanes["review-camera-orbit"]["replace"]
                .as_array()
                .unwrap()
                .len(),
            0,
            "provenance shows the losing lane contributed nothing"
        );
        assert_eq!(
            lanes["review-5-the-approval-workflow"]["add"][0],
            "decisions-doc"
        );
    }

    #[test]
    fn a_clean_merge_fires_no_drop_event() {
        let sink = ValueSink::default();
        let (patch, _findings, lanes) = union_lane_patches(
            vec![
                (
                    "review-a".to_string(),
                    PlanPatch {
                        replace: vec![replace("api", &["core"])],
                        ..Default::default()
                    },
                    vec!["finding one".to_string()],
                ),
                (
                    "review-b".to_string(),
                    PlanPatch {
                        remove: vec!["dead-task".to_string()],
                        ..Default::default()
                    },
                    vec![],
                ),
            ],
            &sink,
        );
        assert!(sink.0.lock().unwrap().is_empty());
        assert_eq!(patch.replace.len(), 1);
        assert_eq!(patch.remove, vec!["dead-task".to_string()]);
        assert_eq!(lanes["review-a"]["replace"][0], "api");
        assert_eq!(lanes["review-b"]["remove"][0], "dead-task");
    }

    /// THE THREE SENTENCES THAT DEFEATED THE OLD KEY, verbatim from the live run that measured this.
    /// They must be ONE finding, and the `STILL: ` prefix that produced 9 findings with `repeated: 0`
    /// on an untouched plan must not create a fourth.
    #[test]
    fn one_defect_worded_four_ways_is_one_finding() {
        let a = "viz-interaction and viz-rendering-engine share the same file (web/viz.js)";
        let b = "Two tasks write to the same file (web/viz.js)";
        let c = "viz.js written by two tasks";
        let d = "STILL: viz-interaction and viz-rendering-engine share the same file (web/viz.js)";
        let k = review_dedupe_key(a);
        assert_eq!(review_dedupe_key(b), k, "b rephrased a");
        assert_eq!(
            review_dedupe_key(c),
            k,
            "c named the file without its directory"
        );
        assert_eq!(
            review_dedupe_key(d),
            k,
            "a STILL: prefix is not a new finding"
        );
    }

    /// Merging two DIFFERENT findings about the same file drops one of them from the round's findings
    /// and from the patch demand that names them. (When REVIEW looped this was the early-stop risk; the
    /// round is single now, and what would be lost is the finding itself.)
    #[test]
    fn two_different_claims_about_one_file_stay_separate() {
        let owned_twice =
            "viz-interaction and viz-rendering-engine share the same file (web/viz.js)";
        let never_wired = "web/viz.js is never imported by any task";
        assert_ne!(
            review_dedupe_key(owned_twice),
            review_dedupe_key(never_wired)
        );
    }

    #[test]
    fn different_files_are_different_findings() {
        let a = "Two tasks write to the same file (web/viz.js)";
        let b = "Two tasks write to the same file (web/store.js)";
        assert_ne!(review_dedupe_key(a), review_dedupe_key(b));
    }

    /// A finding naming nothing extractable keeps the OLD behaviour exactly, so prose findings cannot
    /// regress into either direction.
    #[test]
    fn prose_with_no_identifier_falls_back_to_the_text_key() {
        let f = "The plan does not explain how the user reaches the running application";
        assert_eq!(f.to_lowercase(), review_dedupe_key(f));
        let g = "The plan does not explain how the user reaches the running application at all";
        assert_ne!(review_dedupe_key(f), review_dedupe_key(g));
    }

    #[test]
    fn backticked_identifiers_are_read_even_without_an_extension() {
        let a = "task `integrate-verify` owns files and will be cascaded on any build failure";
        let b = "STILL: `integrate-verify` owns files, which cascades it on a build failure";
        assert_eq!(review_dedupe_key(a), review_dedupe_key(b));
    }
}
