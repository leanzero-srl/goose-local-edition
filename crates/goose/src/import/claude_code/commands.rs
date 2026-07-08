//! Plan/apply slash-command imports: `~/.claude/commands/<name>.md` → `~/.agents/skills/<name>/SKILL.md`.
//!
//! A Claude slash command is a prompt template with `$ARGUMENTS`/`$1` placeholders — which goose's skill
//! argument engine handles natively, so a pure-placeholder command is a clean CONVERT. A command that runs
//! `` !`cmd` `` bash, embeds `@file`, or pins a `model` uses constructs goose skills don't support, so it is
//! classified LOSSY and reported (the unsupported construct survives as inert literal text). Nested
//! `commands/foo/bar.md` (Claude `/foo:bar`) flatten to skill `foo-bar`.

use super::manifest::Manifest;
use super::{
    content_hash, Action, ActionClass, ApplyContext, ApplyOutcome, ConflictPolicy, ImportOptions,
    ImportPlan, ImportReport, ImportType,
};
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

/// Enumerate command files as CONVERT/LOSSY skill actions. Read-only.
pub fn plan_commands(opts: &ImportOptions, plan: &mut ImportPlan) -> Result<()> {
    let dir = opts.from.join("commands");
    if !dir.is_dir() {
        return Ok(());
    }
    let mut found = Vec::new();
    collect_md(&dir, &dir, &mut found);
    found.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, path) in found {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let (class, note) = if command_is_lossy(&content) {
            (
                ActionClass::Lossy,
                Some("uses bash/@file/model — unsupported bits become inert text".to_string()),
            )
        } else {
            (ActionClass::Convert, None)
        };
        plan.push(Action {
            import_type: ImportType::Commands,
            class,
            name: name.clone(),
            source: Some(path),
            target: format!("~/.agents/skills/{name}"),
            note,
        });
    }
    Ok(())
}

/// Apply command imports: write each command as a skill `SKILL.md` under `ctx.skills_dir/<name>`.
pub fn apply_commands(
    plan: &ImportPlan,
    ctx: &ApplyContext,
    manifest: &mut Manifest,
    report: &mut ImportReport,
) -> Result<()> {
    for action in plan.by_type(ImportType::Commands) {
        let Some(src) = action.source.as_ref() else {
            continue;
        };
        let content = fs::read_to_string(src).unwrap_or_default();
        let hash = content_hash(content.as_bytes());
        let dir = ctx.skills_dir.join(&action.name);
        let skill_md = dir.join("SKILL.md");

        let outcome = if skill_md.exists() {
            match ctx.conflict {
                ConflictPolicy::Skip => {
                    report.record(
                        ImportType::Commands,
                        &action.name,
                        ApplyOutcome::SkippedExists,
                        None,
                    );
                    continue;
                }
                ConflictPolicy::Merge
                    if manifest.hash_for(ImportType::Commands, &action.name)
                        == Some(hash.as_str()) =>
                {
                    report.record(
                        ImportType::Commands,
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

        let skill = build_command_skill(&action.name, &content, &hash);
        if let Err(e) = fs::create_dir_all(&dir).and_then(|_| fs::write(&skill_md, &skill)) {
            report.record(
                ImportType::Commands,
                &action.name,
                ApplyOutcome::Failed,
                Some(e.to_string()),
            );
            continue;
        }
        manifest.upsert(
            ImportType::Commands,
            &action.name,
            &dir.display().to_string(),
            &hash,
        );
        report.record(ImportType::Commands, &action.name, outcome, None);
    }
    Ok(())
}

/// Build a goose SKILL.md from a command file, preserving its body and any existing frontmatter description.
fn build_command_skill(name: &str, content: &str, hash: &str) -> String {
    let (description, body) = command_description_and_body(content);
    let desc = if description.is_empty() {
        format!("Imported Claude Code command: {name}")
    } else {
        description
    };
    format!(
        "---\nname: {name}\ndescription: {desc}\nmetadata:\n  x_imported_from: claude-code\n  x_import_hash: {hash}\n---\n{body}"
    )
}

/// A command's description (from frontmatter or first non-empty body line) and its prompt body.
fn command_description_and_body(content: &str) -> (String, String) {
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
    // No frontmatter: description = first non-empty line, body = whole content.
    let desc = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().trim_start_matches('#').trim().to_string())
        .unwrap_or_default();
    (desc, content.to_string())
}

/// True if the command uses constructs goose skills do not support (bash exec, @file, a pinned model).
fn command_is_lossy(content: &str) -> bool {
    content.contains("!`")
        || content.contains("](@")
        || content.lines().any(|l| {
            let t = l.trim_start();
            t.starts_with('@') || t.trim().starts_with("model:")
        })
}

/// Recursively collect `.md` files, keying each by its `/`-joined stem relative to `base` (`foo/bar` → `foo-bar`).
fn collect_md(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_md(&p, base, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("md") {
            if let Ok(rel) = p.strip_prefix(base) {
                let name = rel
                    .with_extension("")
                    .to_string_lossy()
                    .replace(['/', '\\'], "-");
                if !name.is_empty() {
                    out.push((name, p));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::claude_code::{plan, TypeSet};

    fn commands_fixture() -> (tempfile::TempDir, ImportOptions) {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = tmp.path().join(".claude").join("commands");
        fs::create_dir_all(cmds.join("git")).unwrap();
        fs::write(
            cmds.join("review.md"),
            "Review the diff for $ARGUMENTS carefully.",
        )
        .unwrap();
        fs::write(cmds.join("git").join("sync.md"), "!`git status`\nsync it").unwrap(); // lossy (bash)
        let opts = ImportOptions {
            from: tmp.path().join(".claude"),
            claude_json: tmp.path().join("nope.json"),
            types: TypeSet::only([ImportType::Commands]),
            dry_run: false,
            all_projects: false,
        };
        (tmp, opts)
    }

    #[test]
    fn plan_classifies_pure_convert_and_bash_lossy_with_nesting() {
        let (_tmp, opts) = commands_fixture();
        let p = plan(&opts).unwrap();
        assert_eq!(p.count(ImportType::Commands), 2);
        // nested git/sync.md → git-sync, classified LOSSY (bash)
        let sync = p
            .by_type(ImportType::Commands)
            .find(|a| a.name == "git-sync")
            .unwrap();
        assert_eq!(sync.class, ActionClass::Lossy);
        let review = p
            .by_type(ImportType::Commands)
            .find(|a| a.name == "review")
            .unwrap();
        assert_eq!(review.class, ActionClass::Convert);
    }

    #[test]
    fn apply_writes_command_skill_that_goose_discovers() {
        let (tmp, opts) = commands_fixture();
        let wd = tmp.path().join("wd");
        let ctx = ApplyContext {
            skills_dir: wd.join(".agents").join("skills"),
            config_dir: tmp.path().join("cfg"),
            conflict: ConflictPolicy::Merge,
        };
        let p = plan(&opts).unwrap();
        let mut m = Manifest::default();
        let mut r = ImportReport::default();
        apply_commands(&p, &ctx, &mut m, &mut r).unwrap();
        assert_eq!(r.count_outcome(ApplyOutcome::Created), 2);

        let review = ctx.skills_dir.join("review").join("SKILL.md");
        assert!(review.is_file());
        let content = fs::read_to_string(&review).unwrap();
        assert!(content.contains("$ARGUMENTS"), "body preserved: {content}");

        // goose discovers the imported command as a skill
        let names: Vec<_> = crate::skills::discover_skills(Some(&wd))
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert!(
            names.contains(&"review".to_string()),
            "goose discovers review: {names:?}"
        );
    }
}
