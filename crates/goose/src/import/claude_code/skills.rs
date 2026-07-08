//! Plan skill imports: `~/.claude/skills/<name>/SKILL.md` → `~/.agents/skills/<name>/` (DIRECT).
//!
//! goose already discovers `~/.claude/skills` in place (`all_skill_dirs`), so import's value is making
//! the skill canonical in goose's writable home `~/.agents/skills` (which precedes `~/.claude/skills` in
//! dedup, so the imported copy wins) and normalizing the frontmatter. The copy + normalize happens in
//! apply (Phase 2); here we only enumerate.

use super::manifest::Manifest;
use super::{
    content_hash, Action, ActionClass, ApplyContext, ApplyOutcome, ConflictPolicy, ImportOptions,
    ImportPlan, ImportReport, ImportType,
};
use crate::skills::{global_skills_dir, parse_skill_frontmatter};
use anyhow::Result;
use std::fs;
use std::path::Path;

/// Enumerate the skills under `<from>/skills` as DIRECT actions. Read-only.
pub fn plan_skills(opts: &ImportOptions, plan: &mut ImportPlan) -> Result<()> {
    let src_root = opts.from.join("skills");
    if !src_root.is_dir() {
        return Ok(());
    }
    let target_root = global_skills_dir();

    let mut entries: Vec<_> = fs::read_dir(&src_root)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let dir = entry.path();
        let skill_md = dir.join("SKILL.md");
        if !dir.is_dir() || !skill_md.is_file() {
            continue;
        }
        let name = match dir.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let (description, _body) = parse_skill_frontmatter(&fs::read_to_string(&skill_md)?);
        let target = target_root
            .as_ref()
            .map(|t| t.join(&name).display().to_string())
            .unwrap_or_else(|| format!("~/.agents/skills/{name}"));

        plan.push(Action {
            import_type: ImportType::Skills,
            class: ActionClass::Direct,
            name,
            source: Some(skill_md),
            target,
            note: (!description.trim().is_empty()).then(|| description.trim().to_string()),
        });
    }
    Ok(())
}

/// Apply the planned skill imports: copy each skill directory to `ctx.skills_dir/<name>`, reconciling by
/// the source SKILL.md content-hash recorded in the manifest (idempotent), honoring the conflict policy.
pub fn apply_skills(
    plan: &ImportPlan,
    ctx: &ApplyContext,
    manifest: &mut Manifest,
    report: &mut ImportReport,
) -> Result<()> {
    for action in plan.by_type(ImportType::Skills) {
        let Some(skill_md) = action.source.as_ref() else {
            continue;
        };
        let Some(src_dir) = skill_md.parent() else {
            continue;
        };
        let dst_dir = ctx.skills_dir.join(&action.name);
        let hash = content_hash(&fs::read(skill_md).unwrap_or_default());
        let dst_target = dst_dir.display().to_string();

        let exists = dst_dir.exists();
        let outcome = if exists {
            match ctx.conflict {
                ConflictPolicy::Skip => {
                    report.record(
                        ImportType::Skills,
                        &action.name,
                        ApplyOutcome::SkippedExists,
                        None,
                    );
                    continue;
                }
                ConflictPolicy::Merge
                    if manifest.hash_for(ImportType::Skills, &action.name)
                        == Some(hash.as_str()) =>
                {
                    // We wrote this exact source before and it hasn't changed — nothing to do.
                    report.record(
                        ImportType::Skills,
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

        // Replace the destination with a fresh verbatim copy.
        if let Err(e) = replace_dir(src_dir, &dst_dir) {
            report.record(
                ImportType::Skills,
                &action.name,
                ApplyOutcome::Failed,
                Some(e.to_string()),
            );
            continue;
        }
        manifest.upsert(ImportType::Skills, &action.name, &dst_target, &hash);
        report.record(ImportType::Skills, &action.name, outcome, None);
    }
    Ok(())
}

/// Remove `dst` (if present) and recursively copy `src` into it.
fn replace_dir(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        fs::remove_dir_all(dst)?;
    }
    copy_dir_all(src, dst)
}

/// Heavy/regenerable directories a skill may embed — skipped on copy (they are reinstalled, not carried).
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".venv",
    "target",
    "__pycache__",
    "dist",
    "build",
];

/// Recursively copy a directory tree, skipping heavy build dirs and symlinks. A skill can embed a whole
/// `node_modules` (with broken symlinks) that would otherwise fail the entire copy — those are skipped so
/// the skill's own files (SKILL.md, scripts, docs) always land.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if SKIP_DIRS.iter().any(|d| name.as_os_str() == *d) {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            // Skip symlinks: a broken link would abort the whole copy, and a skill needs its real files.
            continue;
        } else if file_type.is_dir() {
            copy_dir_all(&from, &to)?;
        } else if file_type.is_file() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::claude_code::{plan, ImportOptions, TypeSet};

    fn skill_fixture() -> (tempfile::TempDir, ImportOptions) {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        for (name, desc) in [("alpha", "does alpha"), ("beta", "does beta")] {
            let d = claude.join("skills").join(name);
            fs::create_dir_all(&d).unwrap();
            fs::write(
                d.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {desc}\n---\nbody for {name}"),
            )
            .unwrap();
            // an extra asset that must be carried verbatim
            fs::write(d.join("helper.py"), "print('x')").unwrap();
        }
        let opts = ImportOptions {
            from: claude,
            claude_json: tmp.path().join("nope.json"),
            types: TypeSet::only([ImportType::Skills]),
            dry_run: false,
            all_projects: false,
        };
        (tmp, opts)
    }

    #[test]
    fn apply_copies_skills_and_goose_discovers_them() {
        let (tmp, opts) = skill_fixture();
        // Use a project-scoped skills dir so goose's real discover_skills() finds them.
        let wd = tmp.path().join("workdir");
        let ctx = ApplyContext {
            skills_dir: wd.join(".agents").join("skills"),
            config_dir: tmp.path().join("cfg"),
            conflict: ConflictPolicy::Merge,
        };
        let p = plan(&opts).unwrap();
        let mut manifest = Manifest::default();
        let mut report = ImportReport::default();
        apply_skills(&p, &ctx, &mut manifest, &mut report).unwrap();

        assert_eq!(report.count_outcome(ApplyOutcome::Created), 2);
        // verbatim asset carried
        assert!(ctx.skills_dir.join("alpha").join("helper.py").is_file());
        // goose's REAL discovery sees the imported skills
        let discovered = crate::skills::discover_skills(Some(&wd));
        let names: Vec<_> = discovered.iter().map(|s| s.name.clone()).collect();
        assert!(
            names.contains(&"alpha".to_string()),
            "goose should discover alpha: {names:?}"
        );
        assert!(
            names.contains(&"beta".to_string()),
            "goose should discover beta"
        );

        // IDEMPOTENT: a second apply is all Unchanged, no re-copy.
        let mut report2 = ImportReport::default();
        apply_skills(&p, &ctx, &mut manifest, &mut report2).unwrap();
        assert_eq!(report2.count_outcome(ApplyOutcome::Unchanged), 2);
        assert_eq!(report2.count_outcome(ApplyOutcome::Created), 0);
    }

    #[test]
    fn conflict_skip_leaves_existing_untouched() {
        let (tmp, opts) = skill_fixture();
        let ctx = ApplyContext {
            skills_dir: tmp.path().join("skills-out"),
            config_dir: tmp.path().join("cfg"),
            conflict: ConflictPolicy::Skip,
        };
        // pre-create alpha with sentinel content
        let alpha = ctx.skills_dir.join("alpha");
        fs::create_dir_all(&alpha).unwrap();
        fs::write(alpha.join("SKILL.md"), "SENTINEL").unwrap();

        let p = plan(&opts).unwrap();
        let mut manifest = Manifest::default();
        let mut report = ImportReport::default();
        apply_skills(&p, &ctx, &mut manifest, &mut report).unwrap();

        assert_eq!(
            report.count_type_outcome(ImportType::Skills, ApplyOutcome::SkippedExists),
            1
        );
        assert_eq!(
            report.count_type_outcome(ImportType::Skills, ApplyOutcome::Created),
            1
        ); // beta
        assert_eq!(
            fs::read_to_string(alpha.join("SKILL.md")).unwrap(),
            "SENTINEL"
        );
    }
}
