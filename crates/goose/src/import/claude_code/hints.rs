//! Plan hint imports: Claude Code `CLAUDE.md` → goose `.goosehints` (DIRECT).
//!
//! The global `~/.claude/CLAUDE.md` maps to `<config_dir>/.goosehints`, which goose's PromptManager
//! always injects under "### Global Hints". Nested/project `CLAUDE.md` files map 1:1 to a matching-dir
//! `.goosehints` — added in a later phase; Phase 1 plans the global file.

use super::{Action, ActionClass, ImportOptions, ImportPlan, ImportType};
use crate::config::paths::Paths;
use anyhow::Result;

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
