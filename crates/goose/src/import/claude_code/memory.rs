//! Plan memory imports: Claude Code auto-memory notes → goose global memory (CONVERT).
//!
//! Each `~/.claude/projects/<slug>/memory/<name>.md` note becomes ONE entry in a goose memory category
//! file `<config_dir>/memory/<name>.txt` (global, because only global memory auto-injects). The `MEMORY.md`
//! index is dropped. The actual one-entry-per-note conversion + `memory` builtin enablement happen in
//! apply (Phase 2); here we only enumerate.

use super::{Action, ActionClass, ImportOptions, ImportPlan, ImportType};
use crate::config::paths::Paths;
use anyhow::Result;
use std::fs;

/// Enumerate auto-memory notes as CONVERT actions. Read-only.
pub fn plan_memory(opts: &ImportOptions, plan: &mut ImportPlan) -> Result<()> {
    let projects = opts.from.join("projects");
    if !projects.is_dir() {
        return Ok(());
    }
    let mem_root = Paths::config_dir().join("memory");

    let mut projs: Vec<_> = fs::read_dir(&projects)?.flatten().collect();
    projs.sort_by_key(|e| e.file_name());

    for proj in projs {
        let mem_dir = proj.path().join("memory");
        if !mem_dir.is_dir() {
            continue;
        }
        let mut notes: Vec<_> = fs::read_dir(&mem_dir)?.flatten().collect();
        notes.sort_by_key(|e| e.file_name());

        for note in notes {
            let path = note.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // The index file is not a memory note.
            if file_name.eq_ignore_ascii_case("MEMORY.md") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if stem.is_empty() {
                continue;
            }
            plan.push(Action {
                import_type: ImportType::Memory,
                class: ActionClass::Convert,
                name: stem.clone(),
                source: Some(path),
                target: mem_root.join(format!("{stem}.txt")).display().to_string(),
                note: Some("1 entry/note".to_string()),
            });
        }
    }
    Ok(())
}
