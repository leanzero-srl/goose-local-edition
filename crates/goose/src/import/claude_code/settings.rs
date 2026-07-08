//! Plan settings + subagents as SKIP+REPORT. These are listed so nothing is silently lost, but nothing is
//! written: Claude's permissions DSL, hooks, and statusLine have no goose equivalent, and a subagent is a
//! *model-delegable* unit whereas a goose recipe is *user-invoked* — a semantic downgrade the importer does
//! not perform automatically (a future `--subagents` opt-in maps them to recipes, LOSSY).

use super::{Action, ActionClass, ImportOptions, ImportPlan, ImportType};
use anyhow::Result;
use serde_json::Value;
use std::fs;

/// The only settings.json key that can port (and only if a goose provider serves the model).
const PORTABLE_KEYS: &[&str] = &["model"];

/// Enumerate settings.json + subagents as SKIP+REPORT actions. Read-only, writes nothing.
pub fn plan_settings(opts: &ImportOptions, plan: &mut ImportPlan) -> Result<()> {
    let settings = opts.from.join("settings.json");
    if opts.types.contains(ImportType::Settings) && settings.is_file() {
        if let Ok(doc) = serde_json::from_str::<Value>(&fs::read_to_string(&settings)?) {
            if let Some(obj) = doc.as_object() {
                if let Some(model) = obj.get("model").and_then(Value::as_str) {
                    plan.push(Action {
                        import_type: ImportType::Settings,
                        class: ActionClass::Skip,
                        name: format!("model = {model}"),
                        source: Some(settings.clone()),
                        target: "config.yaml (if a goose provider serves it)".to_string(),
                        note: Some("not auto-set — re-select in goose configure".to_string()),
                    });
                }
                let mut non_portable: Vec<&String> = obj
                    .keys()
                    .filter(|k| !PORTABLE_KEYS.contains(&k.as_str()))
                    .collect();
                non_portable.sort();
                if !non_portable.is_empty() {
                    let list = non_portable
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    plan.push(Action {
                        import_type: ImportType::Settings,
                        class: ActionClass::Skip,
                        name: format!("{} non-portable key(s)", non_portable.len()),
                        source: Some(settings.clone()),
                        target: "—".to_string(),
                        note: Some(format!("no goose equivalent: {list}")),
                    });
                }
            }
        }
    }

    // Subagents: report-only (a recipe is user-invoked, a subagent is model-delegable).
    let agents = opts.from.join("agents");
    if opts.types.contains(ImportType::Subagents) && agents.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&agents)?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|n| n.to_str()) else {
                continue;
            };
            plan.push(Action {
                import_type: ImportType::Subagents,
                class: ActionClass::Skip,
                name: name.to_string(),
                source: Some(path.clone()),
                target: "—".to_string(),
                note: Some(
                    "report-only — a recipe is user-invoked, a subagent model-delegable"
                        .to_string(),
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::claude_code::{plan, ImportOptions, TypeSet};

    #[test]
    fn enumerates_settings_and_subagents_as_skip_without_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        fs::create_dir_all(claude.join("agents")).unwrap();
        fs::write(
            claude.join("settings.json"),
            r#"{"model":"opus","hooks":{},"statusLine":{},"permissions":{}}"#,
        )
        .unwrap();
        fs::write(claude.join("agents").join("reviewer.md"), "you review code").unwrap();

        let opts = ImportOptions {
            from: claude,
            claude_json: tmp.path().join("nope.json"),
            types: TypeSet::only([ImportType::Settings, ImportType::Subagents]),
            dry_run: false,
            all_projects: false,
        };
        let p = plan(&opts).unwrap();

        // model note + non-portable summary
        assert_eq!(p.count(ImportType::Settings), 2);
        assert!(p
            .by_type(ImportType::Settings)
            .all(|a| a.class == ActionClass::Skip));
        // subagent reported, not written
        assert_eq!(p.count(ImportType::Subagents), 1);
        assert!(p
            .by_type(ImportType::Subagents)
            .all(|a| a.class == ActionClass::Skip));
    }
}
