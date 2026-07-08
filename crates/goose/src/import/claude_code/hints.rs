//! Plan hint imports: Claude Code `CLAUDE.md` → goose `.goosehints` (DIRECT).
//!
//! The global `~/.claude/CLAUDE.md` maps to `<config_dir>/.goosehints`, which goose's PromptManager
//! always injects under "### Global Hints". Nested/project `CLAUDE.md` files map 1:1 to a matching-dir
//! `.goosehints` — added in a later phase; Phase 1 plans the global file.

use super::manifest::Manifest;
use super::{
    content_hash, Action, ActionClass, ApplyContext, ApplyOutcome, ConflictPolicy, ImportOptions,
    ImportPlan, ImportReport, ImportType,
};
use crate::config::paths::Paths;
use anyhow::Result;
use std::fs;

const BLOCK_START_PREFIX: &str = "<!-- goose:import claude-code";
const BLOCK_END: &str = "<!-- /goose:import -->";

/// Enumerate the global `CLAUDE.md` as a DIRECT hint action. Read-only.
pub fn plan_hints(opts: &ImportOptions, plan: &mut ImportPlan) -> Result<()> {
    let src = opts.from.join("CLAUDE.md");
    if !src.is_file() {
        return Ok(());
    }
    let target = Paths::config_dir().join(".goosehints");
    plan.push(Action {
        import_type: ImportType::Hints,
        class: ActionClass::Direct,
        name: "CLAUDE.md (global)".to_string(),
        source: Some(src),
        target: target.display().to_string(),
        note: Some("injected as Global Hints".to_string()),
    });
    Ok(())
}

/// Apply hint imports: write each CLAUDE.md into `ctx.goosehints()` inside a provenance-fenced block, so
/// a re-import replaces (never stacks) the block and the user's own hints outside the block are preserved.
pub fn apply_hints(
    plan: &ImportPlan,
    ctx: &ApplyContext,
    manifest: &mut Manifest,
    report: &mut ImportReport,
) -> Result<()> {
    for action in plan.by_type(ImportType::Hints) {
        let Some(src) = action.source.as_ref() else {
            continue;
        };
        let content = fs::read_to_string(src).unwrap_or_default();
        let hash = content_hash(content.as_bytes());
        let target = ctx.goosehints();
        let existing = fs::read_to_string(&target).unwrap_or_default();
        let existing_hash = extract_block_hash(&existing);

        let outcome = if existing_hash.is_some() {
            match ctx.conflict {
                ConflictPolicy::Skip => {
                    report.record(
                        ImportType::Hints,
                        &action.name,
                        ApplyOutcome::SkippedExists,
                        None,
                    );
                    continue;
                }
                ConflictPolicy::Merge if existing_hash.as_deref() == Some(hash.as_str()) => {
                    report.record(
                        ImportType::Hints,
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

        let block = format!(
            "{BLOCK_START_PREFIX} hash={hash} -->\n{}\n{BLOCK_END}",
            content.trim_end()
        );
        let new_content = upsert_block(&existing, &block);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, new_content)?;
        manifest.upsert(
            ImportType::Hints,
            &action.name,
            &target.display().to_string(),
            &hash,
        );
        report.record(ImportType::Hints, &action.name, outcome, None);
    }
    Ok(())
}

/// The `hash=...` value from an existing provenance block, if present.
fn extract_block_hash(existing: &str) -> Option<String> {
    let line = existing
        .lines()
        .find(|l| l.trim_start().starts_with(BLOCK_START_PREFIX))?;
    let after = line.split("hash=").nth(1)?;
    Some(
        after
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches("-->")
            .trim()
            .to_string(),
    )
}

/// Replace the existing provenance block (start marker line … end marker line, inclusive) with `block`;
/// if there is no block, append it (preserving the user's own hint content).
fn upsert_block(existing: &str, block: &str) -> String {
    let start = existing
        .lines()
        .position(|l| l.trim_start().starts_with(BLOCK_START_PREFIX));
    let end = existing.lines().position(|l| l.trim() == BLOCK_END);

    if let (Some(s), Some(e)) = (start, end) {
        if e >= s {
            let lines: Vec<&str> = existing.lines().collect();
            let before = lines[..s].join("\n");
            let after = lines[e + 1..].join("\n");
            let mut out = String::new();
            if !before.trim().is_empty() {
                out.push_str(before.trim_end());
                out.push_str("\n\n");
            }
            out.push_str(block);
            if !after.trim().is_empty() {
                out.push_str("\n\n");
                out.push_str(after.trim_start());
            }
            out.push('\n');
            return out;
        }
    }
    // No block: append, keeping any existing user content.
    if existing.trim().is_empty() {
        format!("{block}\n")
    } else {
        format!("{}\n\n{block}\n", existing.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::claude_code::{plan, ImportOptions, TypeSet};

    fn hints_fixture(body: &str) -> (tempfile::TempDir, ImportOptions) {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join("CLAUDE.md"), body).unwrap();
        let opts = ImportOptions {
            from: claude,
            claude_json: tmp.path().join("nope.json"),
            types: TypeSet::only([ImportType::Hints]),
            dry_run: false,
        };
        (tmp, opts)
    }

    fn ctx_for(tmp: &tempfile::TempDir) -> ApplyContext {
        ApplyContext {
            skills_dir: tmp.path().join("skills"),
            config_dir: tmp.path().join("cfg"),
            conflict: ConflictPolicy::Merge,
        }
    }

    #[test]
    fn apply_writes_fenced_block_and_is_idempotent() {
        let (tmp, opts) = hints_fixture("be terse\nno bullets");
        let ctx = ctx_for(&tmp);
        let p = plan(&opts).unwrap();
        let mut m = Manifest::default();
        let mut r = ImportReport::default();
        apply_hints(&p, &ctx, &mut m, &mut r).unwrap();

        let written = fs::read_to_string(ctx.goosehints()).unwrap();
        assert!(written.contains("be terse"));
        assert!(written.contains(BLOCK_START_PREFIX));
        assert!(written.contains(BLOCK_END));
        assert_eq!(r.count_outcome(ApplyOutcome::Created), 1);

        // idempotent: 2nd apply unchanged, exactly one block.
        let mut r2 = ImportReport::default();
        apply_hints(&p, &ctx, &mut m, &mut r2).unwrap();
        assert_eq!(r2.count_outcome(ApplyOutcome::Unchanged), 1);
        let written2 = fs::read_to_string(ctx.goosehints()).unwrap();
        assert_eq!(written2.matches(BLOCK_END).count(), 1, "no stacked blocks");
    }

    #[test]
    fn apply_preserves_existing_user_hints() {
        let (tmp, opts) = hints_fixture("imported rule");
        let ctx = ctx_for(&tmp);
        fs::create_dir_all(&ctx.config_dir).unwrap();
        fs::write(ctx.goosehints(), "MY OWN HINT\nkeep me").unwrap();

        let p = plan(&opts).unwrap();
        let mut m = Manifest::default();
        let mut r = ImportReport::default();
        apply_hints(&p, &ctx, &mut m, &mut r).unwrap();

        let written = fs::read_to_string(ctx.goosehints()).unwrap();
        assert!(
            written.contains("MY OWN HINT"),
            "user hint preserved: {written}"
        );
        assert!(written.contains("imported rule"));
    }
}
