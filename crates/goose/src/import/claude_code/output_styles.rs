//! Plan/apply output-style imports: `~/.claude/output-styles/<name>.md` → `<config_dir>/recipes/<name>.yaml`.
//!
//! A Claude output style is a named instruction set — the same shape as a goose recipe's `instructions`.
//! Frontmatter `name`/`description` + body become a minimal recipe the user can run with `goose run --recipe`.

use super::manifest::Manifest;
use super::{
    content_hash, Action, ActionClass, ApplyContext, ApplyOutcome, ConflictPolicy, ImportOptions,
    ImportPlan, ImportReport, ImportType,
};
use anyhow::Result;
use std::fs;

/// Enumerate output styles as CONVERT recipe actions. Read-only.
pub fn plan_output_styles(opts: &ImportOptions, plan: &mut ImportPlan) -> Result<()> {
    let dir = opts.from.join("output-styles");
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(&dir)?.flatten().collect();
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
            import_type: ImportType::OutputStyles,
            class: ActionClass::Convert,
            name: name.to_string(),
            source: Some(path.clone()),
            target: format!("recipes/{name}.yaml"),
            note: Some("→ recipe instructions".to_string()),
        });
    }
    Ok(())
}

/// Apply output-style imports: write each as a minimal recipe yaml under `ctx.config_dir/recipes`.
pub fn apply_output_styles(
    plan: &ImportPlan,
    ctx: &ApplyContext,
    manifest: &mut Manifest,
    report: &mut ImportReport,
) -> Result<()> {
    let recipes_dir = ctx.config_dir.join("recipes");
    for action in plan.by_type(ImportType::OutputStyles) {
        let Some(src) = action.source.as_ref() else {
            continue;
        };
        let content = fs::read_to_string(src).unwrap_or_default();
        let hash = content_hash(content.as_bytes());
        let file = recipes_dir.join(format!("{}.yaml", action.name));

        let outcome = if file.exists() {
            match ctx.conflict {
                ConflictPolicy::Skip => {
                    report.record(
                        ImportType::OutputStyles,
                        &action.name,
                        ApplyOutcome::SkippedExists,
                        None,
                    );
                    continue;
                }
                ConflictPolicy::Merge
                    if manifest.hash_for(ImportType::OutputStyles, &action.name)
                        == Some(hash.as_str()) =>
                {
                    report.record(
                        ImportType::OutputStyles,
                        &action.name,
                        ApplyOutcome::Unchanged,
                        None,
                    );
                    continue;
                }
                _ => ApplyOutcome::Updated,
            }
        } else {
            ApplyOutcome::Created
        };

        let yaml = build_recipe_yaml(&action.name, &content);
        if let Err(e) = fs::create_dir_all(&recipes_dir).and_then(|_| fs::write(&file, &yaml)) {
            report.record(
                ImportType::OutputStyles,
                &action.name,
                ApplyOutcome::Failed,
                Some(e.to_string()),
            );
            continue;
        }
        manifest.upsert(
            ImportType::OutputStyles,
            &action.name,
            &file.display().to_string(),
            &hash,
        );
        report.record(ImportType::OutputStyles, &action.name, outcome, None);
    }
    Ok(())
}

/// Build a minimal, valid recipe yaml from an output-style file.
fn build_recipe_yaml(name: &str, content: &str) -> String {
    let (description, instructions) = split_desc_body(content);
    let desc = if description.is_empty() {
        format!("Imported Claude Code output style: {name}")
    } else {
        description
    };
    let recipe = serde_json::json!({
        "version": "1.0.0",
        "title": name,
        "description": desc,
        "instructions": instructions,
    });
    serde_yaml::to_string(&recipe).unwrap_or_default()
}

/// Split frontmatter `description` and the body from an output-style markdown file.
fn split_desc_body(content: &str) -> (String, String) {
    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some((fm, after)) = rest.split_once("\n---") {
            let body = after.trim_start_matches(['\n', '\r']).to_string();
            let desc = fm
                .lines()
                .find_map(|l| l.trim().strip_prefix("description:"))
                .map(|d| d.trim().trim_matches('"').to_string())
                .unwrap_or_default();
            return (desc, body);
        }
    }
    (String::new(), content.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::claude_code::{plan, TypeSet};

    #[test]
    fn apply_writes_a_valid_recipe_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude").join("output-styles");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("terse.md"),
            "---\nname: terse\ndescription: be brief\n---\nAnswer in one sentence.",
        )
        .unwrap();
        let opts = ImportOptions {
            from: tmp.path().join(".claude"),
            claude_json: tmp.path().join("nope.json"),
            types: TypeSet::only([ImportType::OutputStyles]),
            dry_run: false,
            all_projects: false,
        };
        let ctx = ApplyContext {
            skills_dir: tmp.path().join("skills"),
            config_dir: tmp.path().join("cfg"),
            conflict: ConflictPolicy::Merge,
        };
        let p = plan(&opts).unwrap();
        let mut m = Manifest::default();
        let mut r = ImportReport::default();
        apply_output_styles(&p, &ctx, &mut m, &mut r).unwrap();

        let file = ctx.config_dir.join("recipes").join("terse.yaml");
        assert!(file.is_file());
        let yaml = fs::read_to_string(&file).unwrap();
        // valid yaml with instructions
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed["title"], serde_yaml::Value::String("terse".into()));
        assert!(yaml.contains("Answer in one sentence"));
    }
}
