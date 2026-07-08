//! Plan skill imports: `~/.claude/skills/<name>/SKILL.md` → `~/.agents/skills/<name>/` (DIRECT).
//!
//! goose already discovers `~/.claude/skills` in place (`all_skill_dirs`), so import's value is making
//! the skill canonical in goose's writable home `~/.agents/skills` (which precedes `~/.claude/skills` in
//! dedup, so the imported copy wins) and normalizing the frontmatter. The copy + normalize happens in
//! apply (Phase 2); here we only enumerate.

use super::{Action, ActionClass, ImportOptions, ImportPlan, ImportType};
use crate::skills::{global_skills_dir, parse_skill_frontmatter};
use anyhow::Result;
use std::fs;

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
