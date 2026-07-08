//! Plan memory imports: Claude Code auto-memory notes → goose global memory (CONVERT).
//!
//! Each `~/.claude/projects/<slug>/memory/<name>.md` note becomes ONE entry in a goose memory category
//! file `<config_dir>/memory/<name>.txt` (global, because only global memory auto-injects). The `MEMORY.md`
//! index is dropped. The actual one-entry-per-note conversion + `memory` builtin enablement happen in
//! apply (Phase 2); here we only enumerate.

use super::manifest::Manifest;
use super::{
    content_hash, Action, ActionClass, ApplyContext, ApplyOutcome, ConflictPolicy, ImportOptions,
    ImportPlan, ImportReport, ImportType,
};
use crate::config::paths::Paths;
use anyhow::Result;
use std::collections::{BTreeMap, HashSet};
use std::fs;

const IMPORT_TAG: &str = "imported:claude-code";

/// Enumerate auto-memory notes as CONVERT actions. Read-only.
pub fn plan_memory(opts: &ImportOptions, plan: &mut ImportPlan) -> Result<()> {
    let projects = opts.from.join("projects");
    if !projects.is_dir() {
        return Ok(());
    }
    let mem_root = Paths::config_dir().join("memory");

    let mut projs: Vec<_> = fs::read_dir(&projects)?.flatten().collect();
    projs.sort_by_key(|e| e.file_name());

    // Collect each project's memory dir + its note count.
    let mut mem_dirs: Vec<(std::path::PathBuf, usize)> = Vec::new();
    for proj in projs {
        let mem_dir = proj.path().join("memory");
        if !mem_dir.is_dir() {
            continue;
        }
        let count = fs::read_dir(&mem_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
            .filter(|e| {
                !e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("MEMORY.md")
            })
            .count();
        if count > 0 {
            mem_dirs.push((mem_dir, count));
        }
    }

    // Default (no --all-projects): import only the PRIMARY project's memory (the one with the most notes).
    // goose's memory extension injects EVERY memory into the system prompt, so importing all projects would
    // bloat it to 100K+ tokens; the primary project holds the user's general/global memory.
    if !opts.all_projects {
        if let Some((primary, _)) = mem_dirs.iter().max_by_key(|(_, c)| *c) {
            let primary = primary.clone();
            mem_dirs.retain(|(d, _)| *d == primary);
        }
    }

    for (mem_dir, _) in mem_dirs {
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

/// Apply memory imports. Each note becomes ONE entry (`# <type> imported:claude-code` + a single content
/// block) in a goose memory category file `<memory_dir>/<category>.txt` keyed by the note's frontmatter
/// `name`. Notes that collide on category are appended as separate entries (identical content deduped).
/// The importer OWNS only its `imported:claude-code`-tagged entries: user-authored entries in the same
/// file are preserved, and a re-import replaces the imported entries in place (idempotent).
pub fn apply_memory(
    plan: &ImportPlan,
    ctx: &ApplyContext,
    manifest: &mut Manifest,
    report: &mut ImportReport,
) -> Result<()> {
    // Convert every note into (category, entry), grouping collisions.
    let mut by_category: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for action in plan.by_type(ImportType::Memory) {
        let Some(src) = action.source.as_ref() else {
            continue;
        };
        let raw = fs::read_to_string(src).unwrap_or_default();
        let (category, entry) = note_to_entry(&raw, &action.name);
        if category.is_empty() || entry.trim().is_empty() {
            continue;
        }
        by_category.entry(category).or_default().push(entry);
    }

    let memory_dir = ctx.memory_dir();
    for (category, entries) in by_category {
        // Dedup identical imported entries (the same fact remembered in multiple projects).
        let mut seen = HashSet::new();
        let imported: Vec<String> = entries
            .into_iter()
            .filter(|e| seen.insert(e.clone()))
            .collect();

        let file = memory_dir.join(format!("{category}.txt"));
        let existing = fs::read_to_string(&file).unwrap_or_default();
        let had_imported = existing.contains(IMPORT_TAG);

        if had_imported && ctx.conflict == ConflictPolicy::Skip {
            report.record(
                ImportType::Memory,
                &category,
                ApplyOutcome::SkippedExists,
                None,
            );
            continue;
        }

        // Preserve the user's own (non-imported) entries; replace the imported ones.
        let user_entries: Vec<String> = existing
            .split("\n\n")
            .map(str::trim)
            .filter(|e| !e.is_empty() && !e.contains(IMPORT_TAG))
            .map(String::from)
            .collect();

        let mut all = user_entries;
        all.extend(imported);
        let new_content = format!("{}\n", all.join("\n\n"));

        if new_content == existing {
            report.record(ImportType::Memory, &category, ApplyOutcome::Unchanged, None);
            continue;
        }
        let outcome = if file.exists() {
            ApplyOutcome::Updated
        } else {
            ApplyOutcome::Created
        };

        fs::create_dir_all(&memory_dir)?;
        fs::write(&file, &new_content)?;
        manifest.upsert(
            ImportType::Memory,
            &category,
            &file.display().to_string(),
            &content_hash(new_content.as_bytes()),
        );
        report.record(ImportType::Memory, &category, outcome, None);
    }
    Ok(())
}

/// Convert one note's raw markdown into `(category, entry_text)`.
fn note_to_entry(raw: &str, fallback_name: &str) -> (String, String) {
    let (fm, body) = split_frontmatter(raw);
    let meta: serde_yaml::Mapping = serde_yaml::from_str(fm).unwrap_or_default();

    let get_str = |key: &str| -> Option<String> {
        meta.get(serde_yaml::Value::String(key.to_string()))
            .and_then(|v| v.as_str().map(String::from))
    };
    let category = kebab(&get_str("name").unwrap_or_else(|| fallback_name.to_string()));
    let mtype = meta
        .get(serde_yaml::Value::String("metadata".to_string()))
        .and_then(|m| m.as_mapping())
        .and_then(|m| m.get(serde_yaml::Value::String("type".to_string())))
        .and_then(|v| v.as_str())
        .unwrap_or("feedback")
        .to_string();
    let description = get_str("description").unwrap_or_default();

    let content = clean_content(&format!("{description}\n{body}"));
    let entry = format!("# {mtype} {IMPORT_TAG}\n{content}");
    (category, entry)
}

/// Clean a note body into the memory-entry content: flatten `[[links]]`, drop blank lines (a blank line
/// is the reader's entry separator), and de-head markdown headings (a leading `#` would be read as tags).
fn clean_content(s: &str) -> String {
    let flattened = s.replace("[[", "").replace("]]", "");
    flattened
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.trim_start_matches('#').trim_start())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Split `---\n<yaml>\n---\n<body>` into `(yaml, body)`; `("", raw)` if there is no frontmatter.
fn split_frontmatter(raw: &str) -> (&str, &str) {
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some((fm, after)) = rest.split_once("\n---") {
            let body = after.trim_start_matches(['\n', '\r']);
            return (fm, body);
        }
    }
    ("", raw)
}

/// Lowercase kebab-case a name (memory category files use kebab slugs).
fn kebab(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::claude_code::{plan, ImportOptions, TypeSet};

    /// Build a fake ~/.claude with two projects; `dup` appears in both with identical content.
    fn memory_fixture() -> (tempfile::TempDir, ImportOptions) {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        let write_note = |proj: &str, file: &str, name: &str, ty: &str, desc: &str, body: &str| {
            let d = claude.join("projects").join(proj).join("memory");
            fs::create_dir_all(&d).unwrap();
            fs::write(
                d.join(file),
                format!(
                    "---\nname: {name}\ndescription: {desc}\nmetadata:\n  type: {ty}\n---\n{body}"
                ),
            )
            .unwrap();
        };
        write_note(
            "p1",
            "add-depth.md",
            "add-depth",
            "feedback",
            "add depth",
            "## Why\nresearch first\nlink [[other]]",
        );
        write_note("p1", "MEMORY.md", "index", "", "", ""); // must be skipped
        write_note(
            "p2",
            "add-depth.md",
            "add-depth",
            "feedback",
            "add depth",
            "## Why\nresearch first\nlink [[other]]",
        ); // dup
        write_note(
            "p2",
            "commit.md",
            "commit-every-change",
            "feedback",
            "commit always",
            "push each change",
        );
        let opts = ImportOptions {
            from: claude,
            claude_json: tmp.path().join("nope.json"),
            types: TypeSet::only([ImportType::Memory]),
            dry_run: false,
            all_projects: false,
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
    fn apply_writes_one_entry_per_category_and_dedups_collisions() {
        let (tmp, opts) = memory_fixture();
        let ctx = ctx_for(&tmp);
        let p = plan(&opts).unwrap();
        let mut m = Manifest::default();
        let mut r = ImportReport::default();
        apply_memory(&p, &ctx, &mut m, &mut r).unwrap();

        // two categories: add-depth (deduped from 2 projects) + commit-every-change
        let add = fs::read_to_string(ctx.memory_dir().join("add-depth.txt")).unwrap();
        assert!(
            add.starts_with("# feedback imported:claude-code"),
            "tag line: {add}"
        );
        assert!(add.contains("research first"));
        assert!(add.contains("link other"), "wikilink flattened: {add}");
        assert!(!add.contains("## Why"), "heading de-headed: {add}");
        assert_eq!(
            add.matches("imported:claude-code").count(),
            1,
            "collision deduped to one entry"
        );
        assert!(ctx.memory_dir().join("commit-every-change.txt").is_file());

        // goose's REAL reader parses the entry (tag -> key, content -> value).
        let entries = split_entries(&add);
        assert_eq!(entries.len(), 1);

        // idempotent second apply.
        let mut r2 = ImportReport::default();
        apply_memory(&p, &ctx, &mut m, &mut r2).unwrap();
        assert!(r2.count_outcome(ApplyOutcome::Unchanged) >= 1);
    }

    #[test]
    fn apply_preserves_user_memory_entries() {
        let (tmp, opts) = memory_fixture();
        let ctx = ctx_for(&tmp);
        fs::create_dir_all(ctx.memory_dir()).unwrap();
        fs::write(
            ctx.memory_dir().join("add-depth.txt"),
            "# mine\nmy own memory line",
        )
        .unwrap();

        let p = plan(&opts).unwrap();
        let mut m = Manifest::default();
        let mut r = ImportReport::default();
        apply_memory(&p, &ctx, &mut m, &mut r).unwrap();

        let add = fs::read_to_string(ctx.memory_dir().join("add-depth.txt")).unwrap();
        assert!(
            add.contains("my own memory line"),
            "user entry preserved: {add}"
        );
        assert!(add.contains("imported:claude-code"), "imported entry added");
    }

    /// Mirror goose's memory reader entry-split for the assertion.
    fn split_entries(content: &str) -> Vec<&str> {
        content
            .split("\n\n")
            .filter(|e| !e.trim().is_empty())
            .collect()
    }
}
