//! Targeted plan patches — the one way anything is ever corrected in this engine.
//!
//! THE RULE: never throw away valid work to fix a small part of it. A correction names the tasks it
//! changes and nothing else. The alternative — asking the model to re-emit the whole plan — is what
//! killed the previous attempt at this engine: six full plans, three hours forty minutes, build never
//! started, and the planner compacting at 53,902 structured bytes because every repair round shipped the
//! entire decomposition back through it.
//!
//! A patch is also DELIBERATELY STRUCTURAL. `replace` may change a task's files and dependencies; it may
//! not touch its `description`. That description is the slice owner's brief, spliced in verbatim, and it
//! is the only place the researched detail lives — a reviewer that could rewrite it would quietly
//! reintroduce the very re-emission this exists to prevent. A finding that a SPEC is wrong routes back to
//! the slice owner who wrote it, never to whoever is reviewing the plan.

use crate::dag::{specs_from_plan_json, Dag};
use serde::Deserialize;
use serde_json::{Map, Value};

/// One structural change to an existing task. Absent fields are left alone.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct TaskEdit {
    pub id: String,
    #[serde(default)]
    pub files: Option<Vec<String>>,
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,
}

/// A whole new task. It carries no description: a task added by review is a task nobody has researched,
/// so it inherits the objective it is given and the run's own detail step fills the rest.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct TaskAdd {
    pub id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub difficulty: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PlanPatch {
    #[serde(default)]
    pub replace: Vec<TaskEdit>,
    #[serde(default)]
    pub add: Vec<TaskAdd>,
    #[serde(default)]
    pub remove: Vec<String>,
}

impl PlanPatch {
    pub fn is_empty(&self) -> bool {
        self.replace.is_empty() && self.add.is_empty() && self.remove.is_empty()
    }

    /// How many tasks this patch touches — for the log, so a run can be read for whether corrections
    /// stayed small.
    pub fn touched(&self) -> usize {
        self.replace.len() + self.add.len() + self.remove.len()
    }
}

/// Parse a patch out of a model reply. Tolerates the reply being wrapped in prose or a fenced block,
/// because that is what a 27B actually emits.
pub fn parse_patch(reply: &str) -> Result<PlanPatch, String> {
    let raw =
        extract_json_object(reply).ok_or_else(|| "no JSON object in the reply".to_string())?;
    serde_json::from_str::<PlanPatch>(&raw).map_err(|e| format!("patch is not valid JSON: {e}"))
}

fn extract_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Apply a patch to a plan and revalidate. On success returns the new plan JSON.
///
/// On failure the diagnostic names ONLY what is wrong with the PATCH — never the plan — because that
/// diagnostic is what gets sent back, and sending back anything larger is how a correction turns into a
/// regeneration.
pub fn apply_patch(plan_json: &str, patch: &PlanPatch) -> Result<String, String> {
    let mut plan: Value =
        serde_json::from_str(plan_json).map_err(|e| format!("plan is not valid JSON: {e}"))?;
    let subtasks = plan
        .get_mut("subtasks")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| "plan has no `subtasks` array".to_string())?;

    let has = |arr: &Vec<Value>, id: &str| {
        arr.iter()
            .any(|t| t.get("id").and_then(|v| v.as_str()) == Some(id))
    };

    for e in &patch.replace {
        if !has(subtasks, &e.id) {
            return Err(format!(
                "replace names task `{}`, which is not in the plan",
                e.id
            ));
        }
    }
    for a in &patch.add {
        if has(subtasks, &a.id) {
            return Err(format!("add names task `{}`, which already exists", a.id));
        }
    }
    for r in &patch.remove {
        if !has(subtasks, r) {
            return Err(format!("remove names task `{r}`, which is not in the plan"));
        }
    }

    for e in &patch.replace {
        let t = subtasks
            .iter_mut()
            .find(|t| t.get("id").and_then(|v| v.as_str()) == Some(e.id.as_str()))
            .expect("existence checked above");
        if let Some(files) = &e.files {
            t["files"] = Value::from(files.clone());
        }
        if let Some(deps) = &e.depends_on {
            t["depends_on"] = Value::from(deps.clone());
        }
        // `description` is untouched, on purpose. See the module note.
    }

    subtasks.retain(|t| {
        !patch
            .remove
            .iter()
            .any(|r| t.get("id").and_then(|v| v.as_str()) == Some(r.as_str()))
    });

    for a in &patch.add {
        let mut m = Map::new();
        m.insert("id".into(), Value::from(a.id.clone()));
        m.insert("description".into(), Value::from(a.description.clone()));
        if let Some(d) = &a.difficulty {
            m.insert("difficulty".into(), Value::from(d.clone()));
        }
        if let Some(model) = &a.model {
            m.insert("model".into(), Value::from(model.clone()));
        }
        m.insert("files".into(), Value::from(a.files.clone()));
        m.insert("depends_on".into(), Value::from(a.depends_on.clone()));
        subtasks.push(Value::Object(m));
    }

    let out = serde_json::to_string(&plan).map_err(|e| e.to_string())?;

    // The ONLY validation: the same three things Dag::from_specs has always checked — duplicate ids,
    // unknown deps, cycles. Overlapping files are NOT an error; the scheduler serializes them.
    let specs =
        specs_from_plan_json(&out).map_err(|e| format!("patched plan does not parse: {e}"))?;
    Dag::from_specs(specs).map_err(|e| format!("patched plan is not a valid DAG: {e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = r#"{"subtasks":[
        {"id":"store","description":"OWNER BRIEF: sqlite schema, exports open_db(path) -> Conn","files":["app/store.py"],"depends_on":[]},
        {"id":"api","description":"OWNER BRIEF: routes, exports create_app() -> Flask","files":["app/api.py"],"depends_on":["store"]},
        {"id":"integrate-verify","description":"wire and boot","files":[],"depends_on":["api"]}
    ]}"#;

    /// THE LOAD-BEARING GUARANTEE. A structural patch must not be able to touch a task's description —
    /// that is the slice owner's researched brief, and a reviewer able to rewrite it would reintroduce
    /// the whole-plan re-emission this protocol exists to prevent.
    #[test]
    fn replace_changes_structure_and_never_the_brief() {
        let patch = PlanPatch {
            replace: vec![TaskEdit {
                id: "api".into(),
                files: Some(vec!["app/api.py".into(), "app/routes.py".into()]),
                depends_on: Some(vec![]),
            }],
            ..Default::default()
        };
        let out = apply_patch(PLAN, &patch).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let api = v["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "api")
            .unwrap();
        assert_eq!(api["files"][1], "app/routes.py", "files were patched");
        assert_eq!(
            api["depends_on"].as_array().unwrap().len(),
            0,
            "deps relaxed"
        );
        assert!(
            api["description"]
                .as_str()
                .unwrap()
                .contains("create_app() -> Flask"),
            "the owner's brief survives verbatim: {:?}",
            api["description"]
        );
    }

    /// The other half of "never throw away valid work": everything the patch did not name is untouched.
    #[test]
    fn untouched_tasks_are_byte_identical() {
        let patch = PlanPatch {
            remove: vec!["api".into()],
            replace: vec![TaskEdit {
                id: "integrate-verify".into(),
                files: None,
                depends_on: Some(vec!["store".into()]),
            }],
            ..Default::default()
        };
        let out = apply_patch(PLAN, &patch).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let store = v["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "store")
            .unwrap();
        let before: Value = serde_json::from_str(PLAN).unwrap();
        let store_before = before["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "store")
            .unwrap();
        assert_eq!(store, store_before, "an unnamed task is not rewritten");
    }

    #[test]
    fn add_appends_a_task_and_the_dag_still_validates() {
        let patch = PlanPatch {
            add: vec![TaskAdd {
                id: "web".into(),
                description: "build the web console".into(),
                files: vec!["web/index.html".into(), "web/app.js".into()],
                depends_on: vec!["api".into()],
                ..Default::default()
            }],
            replace: vec![TaskEdit {
                id: "integrate-verify".into(),
                files: None,
                depends_on: Some(vec!["api".into(), "web".into()]),
            }],
            ..Default::default()
        };
        let out = apply_patch(PLAN, &patch).unwrap();
        assert!(out.contains("web/index.html"));
    }

    /// A BAD PATCH COSTS ONE PATCH. The diagnostic names the patch, never the plan — sending back
    /// anything bigger is how a correction becomes a regeneration.
    #[test]
    fn a_bad_patch_is_diagnosed_narrowly_and_the_plan_is_untouched() {
        let dangling = PlanPatch {
            replace: vec![TaskEdit {
                id: "api".into(),
                files: None,
                depends_on: Some(vec!["nope".into()]),
            }],
            ..Default::default()
        };
        let err = apply_patch(PLAN, &dangling).unwrap_err();
        assert!(err.contains("not a valid DAG"), "got: {err}");

        let unknown = PlanPatch {
            replace: vec![TaskEdit {
                id: "ghost".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let err = apply_patch(PLAN, &unknown).unwrap_err();
        assert!(err.contains("`ghost`"), "names the offending task: {err}");
        assert!(
            !err.contains("store"),
            "says nothing about the valid plan: {err}"
        );

        let cyclic = PlanPatch {
            replace: vec![TaskEdit {
                id: "store".into(),
                files: None,
                depends_on: Some(vec!["api".into()]),
            }],
            ..Default::default()
        };
        assert!(apply_patch(PLAN, &cyclic).is_err(), "a cycle is refused");
    }

    /// Overlapping files are NOT a patch error — the scheduler serializes two tasks that own the same
    /// file. Rejecting that would be a topology preference, which is the class of rule that produced six
    /// discarded plans last time.
    #[test]
    fn overlapping_files_are_allowed() {
        let patch = PlanPatch {
            replace: vec![TaskEdit {
                id: "store".into(),
                files: Some(vec!["app/api.py".into()]),
                depends_on: None,
            }],
            ..Default::default()
        };
        assert!(apply_patch(PLAN, &patch).is_ok());
    }

    #[test]
    fn parse_tolerates_prose_and_fences() {
        let reply = "Sure — here is the patch:\n```json\n{\"remove\":[\"api\"],\"add\":[]}\n```\nHope that helps.";
        let p = parse_patch(reply).unwrap();
        assert_eq!(p.remove, vec!["api".to_string()]);
        assert_eq!(p.touched(), 1);
    }
}
