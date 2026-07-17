//! Everything specific to skills: filesystem discovery (`SKILL.md` walking +
//! built-ins) and the runtime MCP client (`client` submodule). User-facing
//! CRUD lives in `crate::sources`, which generalizes across source types.

mod arguments;
mod builtin;
pub mod client;

pub use client::{SkillsClient, EXTENSION_NAME};

use crate::config::paths::Paths;
use crate::plugins::installed_plugin_skill_dirs;
use crate::sources::parse_frontmatter;
use agent_client_protocol::Error;
use anyhow::Result;
use arguments::apply_skill_arguments;
use goose_sdk_types::custom_requests::{SourceEntry, SourceType};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tracing::warn;

#[derive(Debug, Deserialize)]
pub struct SkillFrontmatter {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: String,
    /// Free-form bag for caller-defined fields. Per the agentskills.io spec
    /// (<https://agentskills.io/specification#frontmatter>), arbitrary
    /// metadata lives in this nested mapping so it doesn't collide with
    /// reserved frontmatter fields.
    #[serde(default)]
    pub metadata: HashMap<String, Value>,
}

/// Canonical writable location for global user skills: `~/.agents/skills`.
pub fn global_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".agents").join("skills"))
}

/// Canonical writable location for project-scoped skills:
/// `<project>/.agents/skills`.
pub fn project_skills_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".agents").join("skills")
}

pub(crate) fn skills_dir_global_or_err() -> Result<PathBuf, Error> {
    global_skills_dir()
        .ok_or_else(|| Error::internal_error().data("Could not determine home directory"))
}

pub(crate) fn skills_dir_project_or_err(project_dir: &str) -> Result<PathBuf, Error> {
    if project_dir.trim().is_empty() {
        return Err(
            Error::invalid_params().data("projectDir must not be empty when global is false")
        );
    }
    Ok(project_skills_dir(Path::new(project_dir)))
}

pub(crate) fn skill_base_dir(global: bool, project_dir: Option<&str>) -> Result<PathBuf, Error> {
    if global {
        skills_dir_global_or_err()
    } else {
        let pd = project_dir.ok_or_else(|| {
            Error::invalid_params().data("projectDir is required when global is false")
        })?;
        skills_dir_project_or_err(pd)
    }
}

pub(crate) fn validate_skill_name(name: &str) -> Result<(), Error> {
    if name.is_empty() {
        return Err(Error::invalid_params().data("Skill name must not be empty"));
    }
    if name.len() > 64 {
        return Err(Error::invalid_params().data(format!(
            "Invalid skill name \"{}\". Names must be at most 64 characters.",
            name
        )));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(Error::invalid_params().data(format!(
            "Invalid skill name \"{}\". Names may only contain lowercase letters, digits, and hyphens.",
            name
        )));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(Error::invalid_params().data(format!(
            "Invalid skill name \"{}\". Names must not start or end with a hyphen.",
            name
        )));
    }
    Ok(())
}

/// How many supporting files a loaded skill may enumerate before the manifest is summarised.
///
/// This list is one line per file and there was no cap: MEASURED, `load_skill` on a real user skill
/// returned ~948,000 characters (~263K tokens) from 3,069 supporting files — about TWICE the whole
/// context window of the local fleet, in a single tool result, from a call the system prompt invites.
/// The dependency-tree exclusions in `should_skip_dir` remove the bulk of that; this cap is the backstop
/// for a skill that legitimately holds many files.
const MAX_LISTED_SUPPORTING_FILES: usize = 60;

fn loaded_skill_context(skill: &SourceEntry, content: &str) -> String {
    let title = format!("{} ({})", skill.name, skill.source_type);
    let mut output = format!(
        "# Loaded Skill: {title}\n\n{}\n\n## Content\n\n{}\n",
        skill.description, content
    );

    if !skill.supporting_files.is_empty() {
        let skill_dir = Path::new(&skill.path);
        output.push_str(&format!(
            "\n## Supporting Files\n\nSkill directory: {}\n\n\
             Relative paths in this skill resolve from the skill directory. \
             The shell tool runs in the session working directory, so use the \
             resolved path below or `cd` into the skill directory before running \
             supporting scripts.\n\n",
            skill.path
        ));
        let mut listed = 0usize;
        for file in &skill.supporting_files {
            if listed >= MAX_LISTED_SUPPORTING_FILES {
                break;
            }
            if let Ok(relative) = Path::new(file).strip_prefix(skill_dir) {
                let rel_str = relative.to_string_lossy().replace('\\', "/");
                let resolved_path = Path::new(file).to_string_lossy().replace('\\', "/");
                output.push_str(&format!(
                    "- {} → {} (load_skill(name: \"{}/{}\"))\n",
                    rel_str, resolved_path, skill.name, rel_str
                ));
                listed += 1;
            }
        }
        // SAY WHAT WAS DROPPED. A silent truncation reads as "that is the whole skill" — and the model
        // would then never ask for a file it was not shown. Any path omitted here is still loadable by
        // name; the model just has to be told the list is partial.
        let total = skill.supporting_files.len();
        if total > listed {
            output.push_str(&format!(
                "- …and {} more file(s) not listed. Load any of them by relative path with \
                 load_skill(name: \"{}/<path>\"), or use the shell tool to list the skill directory.\n",
                total - listed,
                skill.name
            ));
        }
    }

    output
}

pub fn loaded_skill_context_with_args(skill: &SourceEntry, args: Option<&str>) -> Result<String> {
    let content = if let Some(args) = args {
        apply_skill_arguments(&skill.content, args, &skill_argument_names(skill))?
    } else {
        skill.content.clone()
    };

    Ok(loaded_skill_context(skill, &content))
}

pub fn skill_argument_hint(skill: &SourceEntry) -> Option<String> {
    skill
        .properties
        .get("argument-hint")
        .and_then(|value| value.as_str())
        .filter(|hint| !hint.is_empty())
        .map(str::to_string)
}

pub fn skill_argument_names(skill: &SourceEntry) -> Vec<String> {
    skill
        .properties
        .get("arguments")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn canonicalize_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn inferred_discoverable_skill_root(path: &Path) -> Option<PathBuf> {
    let canonical_path = canonicalize_or_original(path);

    let mut global_roots = Vec::new();
    if let Some(global_root) = global_skills_dir() {
        global_roots.push(global_root);
    }
    global_roots.push(Paths::config_dir().join("skills"));
    if let Some(home) = dirs::home_dir() {
        global_roots.push(home.join(".claude").join("skills"));
        global_roots.push(home.join(".config").join("agents").join("skills"));
    }
    global_roots.extend(installed_plugin_skill_dirs());

    for root in global_roots {
        let canonical_root = canonicalize_or_original(&root);
        if canonical_path.starts_with(&canonical_root) {
            return Some(canonical_root);
        }
    }

    canonical_path.ancestors().find_map(|ancestor| {
        let parent = ancestor.parent()?;
        let is_project_skills_root = ancestor.file_name().and_then(|name| name.to_str())
            == Some("skills")
            && matches!(
                parent.file_name().and_then(|name| name.to_str()),
                Some(".goose") | Some(".claude") | Some(".agents")
            );
        is_project_skills_root.then(|| ancestor.to_path_buf())
    })
}

pub(crate) fn resolve_discoverable_skill_dir(path: &str) -> Result<PathBuf, Error> {
    if path.is_empty() {
        return Err(Error::invalid_params().data("Source path must not be empty"));
    }

    let canonical_dir = Path::new(path)
        .canonicalize()
        .map_err(|_| Error::invalid_params().data(format!("Source \"{}\" not found", path)))?;

    if inferred_discoverable_skill_root(&canonical_dir).is_none()
        || !canonical_dir.is_dir()
        || !canonical_dir.join("SKILL.md").is_file()
    {
        return Err(Error::invalid_params().data(format!("Source \"{}\" not found", path)));
    }

    Ok(canonical_dir)
}

pub(crate) fn resolve_skill_dir(path: &str) -> Result<PathBuf, Error> {
    resolve_discoverable_skill_dir(path)
}

pub(crate) fn is_global_skill_dir(path: &Path) -> bool {
    global_skills_dir().as_deref().is_some_and(|root| {
        canonicalize_or_original(path).starts_with(canonicalize_or_original(root))
    })
}

pub(crate) fn infer_skill_name(dir: &Path) -> String {
    let md = dir.join("SKILL.md");
    if let Ok(raw) = std::fs::read_to_string(&md) {
        if let Ok(Some((meta, _))) = parse_frontmatter::<SkillFrontmatter>(&raw) {
            if let Some(n) = meta.name.filter(|n| !n.is_empty()) {
                return n;
            }
        }
    }
    dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unnamed")
        .to_string()
}

pub(crate) fn build_skill_md(
    name: &str,
    description: &str,
    content: &str,
    metadata: &HashMap<String, Value>,
) -> String {
    let safe_desc = description.replace('\'', "''");
    let mut md = String::from("---\n");
    md.push_str(&format!("name: {}\n", name));
    md.push_str(&format!("description: '{}'\n", safe_desc));
    if !metadata.is_empty() {
        md.push_str("metadata:\n");
        // Use YAML for the nested metadata block. We render it with serde_yaml
        // and indent every line by two spaces so it nests under `metadata:`.
        let yaml = serde_yaml::to_string(metadata).unwrap_or_default();
        for line in yaml.lines() {
            if line.is_empty() {
                continue;
            }
            md.push_str("  ");
            md.push_str(line);
            md.push('\n');
        }
    }
    md.push_str("---\n");
    if !content.is_empty() {
        md.push('\n');
        md.push_str(content);
        md.push('\n');
    }
    md
}

pub(crate) fn parse_skill_frontmatter(raw: &str) -> (String, String) {
    if !raw.trim_start().starts_with("---") {
        return (String::new(), raw.to_string());
    }
    match parse_frontmatter::<SkillFrontmatter>(raw) {
        Ok(Some((meta, body))) => (meta.description, body),
        _ => (String::new(), raw.to_string()),
    }
}

/// Every directory the agent reads skills from, paired with whether each is a
/// global (home-rooted) location. Order matches discovery precedence: project
/// dirs first, then global dirs.
pub fn all_skill_dirs(working_dir: Option<&Path>) -> Vec<(PathBuf, bool)> {
    let mut dirs: Vec<(PathBuf, bool)> = Vec::new();

    if let Some(wd) = working_dir {
        dirs.push((wd.join(".agents").join("skills"), false));
        dirs.push((wd.join(".goose").join("skills"), false));
        dirs.push((wd.join(".claude").join("skills"), false));
    }

    let home = dirs::home_dir();
    if let Some(h) = home.as_ref() {
        dirs.push((h.join(".agents").join("skills"), true));
    }
    dirs.push((Paths::config_dir().join("skills"), true));
    if let Some(h) = home.as_ref() {
        dirs.push((h.join(".claude").join("skills"), true));
        dirs.push((h.join(".config").join("agents").join("skills"), true));
    }

    dirs.extend(
        installed_plugin_skill_dirs()
            .into_iter()
            .map(|dir| (dir, true)),
    );

    dirs
}

fn parse_skill_content(content: &str, path: &Path, global: bool) -> Option<SourceEntry> {
    let (metadata, body): (SkillFrontmatter, String) = match parse_frontmatter(content) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return None,
        Err(e) => {
            warn!("Failed to parse skill frontmatter: {}", e);
            return None;
        }
    };

    let name = match metadata.name.filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => {
            warn!(
                "Skill at '{}' is missing a required 'name' in frontmatter, skipping",
                path.display()
            );
            return None;
        }
    };

    if name.contains('/') {
        warn!("Skill name '{}' contains '/', skipping", name);
        return None;
    }

    Some(SourceEntry {
        source_type: SourceType::Skill,
        name,
        description: metadata.description,
        content: body,
        path: path.to_string_lossy().into_owned(),
        global,
        writable: true,
        supporting_files: Vec::new(),
        properties: metadata.metadata,
    })
}

/// Directories a skill's walk must never descend into: VCS metadata, and DEPENDENCY TREES.
///
/// A skill that ships scripts also ships their dependencies, and nothing here belongs to the author.
/// MEASURED on a real user skill (~/.agents/skills/atlassian-community-leanzero): 3,082 files, of which
/// **2,130 live under node_modules**. Two consequences, both live before this list existed:
///   * DISCOVERY (the walk below) matches any SKILL.md at any depth, so two SKILL.md files vendored inside
///     playwright-core were promoted to first-class skills — `playwright-cli` and `playwright-trace` were
///     injected into every system prompt as if the user had written them.
///   * THE MANIFEST (loaded_skill_context) prints one line per supporting file with no cap, so
///     load_skill on that skill emits ~948,000 characters — roughly TWICE the 128K-token window of the
///     local fleet — from a single tool call the system prompt invites the model to make.
fn should_skip_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".git")
            | Some(".hg")
            | Some(".svn")
            | Some("node_modules")
            | Some("__pycache__")
            | Some(".venv")
            | Some("venv")
            | Some(".tox")
            | Some(".mypy_cache")
            | Some(".pytest_cache")
            | Some("target")
            | Some("dist")
            | Some(".next")
            | Some(".cache")
    )
}

fn walk_files_recursively<F, G>(
    dir: &Path,
    visited_dirs: &mut HashSet<PathBuf>,
    should_descend: &mut G,
    visit_file: &mut F,
) where
    F: FnMut(&Path),
    G: FnMut(&Path) -> bool,
{
    let canonical_dir = match std::fs::canonicalize(dir) {
        Ok(path) => path,
        Err(_) => return,
    };

    if !visited_dirs.insert(canonical_dir) {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if should_descend(&path) {
                walk_files_recursively(&path, visited_dirs, should_descend, visit_file);
            }
        } else if path.is_file() {
            visit_file(&path);
        }
    }
}

fn scan_skills_from_dir(dir: &Path, global: bool, seen: &mut HashSet<String>) -> Vec<SourceEntry> {
    let mut skill_files = Vec::new();
    let mut visited_dirs = HashSet::new();

    walk_files_recursively(
        dir,
        &mut visited_dirs,
        &mut |path| !should_skip_dir(path),
        &mut |path| {
            if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
                skill_files.push(path.to_path_buf());
            }
        },
    );

    let mut sources = Vec::new();
    for skill_file in skill_files {
        let Some(skill_dir) = skill_file.parent() else {
            continue;
        };
        let content = match std::fs::read_to_string(&skill_file) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read skill file {}: {}", skill_file.display(), e);
                continue;
            }
        };

        if let Some(mut source) = parse_skill_content(&content, skill_dir, global) {
            if !seen.contains(&source.name) {
                let mut files = Vec::new();
                let mut visited_support_dirs = HashSet::new();
                walk_files_recursively(
                    skill_dir,
                    &mut visited_support_dirs,
                    &mut |path| !should_skip_dir(path) && !path.join("SKILL.md").is_file(),
                    &mut |path| {
                        if path.file_name().and_then(|n| n.to_str()) != Some("SKILL.md") {
                            files.push(path.to_string_lossy().into_owned());
                        }
                    },
                );
                source.supporting_files = files;

                seen.insert(source.name.clone());
                sources.push(source);
            }
        }
    }
    sources
}

/// Discover skills from all configured filesystem locations and built-ins.
/// Each returned entry has `global` set according to the directory it was
/// found in (or `true` for built-ins).
pub fn discover_skills(working_dir: Option<&Path>) -> Vec<SourceEntry> {
    let mut sources: Vec<SourceEntry> = Vec::new();
    let mut seen = HashSet::new();

    for (dir, is_global) in all_skill_dirs(working_dir) {
        for source in scan_skills_from_dir(&dir, is_global, &mut seen) {
            sources.push(source);
        }
    }

    for content in builtin::get_all() {
        if let Some(source) = parse_skill_content(content, &PathBuf::new(), true) {
            if !seen.contains(&source.name) {
                seen.insert(source.name.clone());
                let path = format!("builtin://skills/{}", source.name);
                sources.push(SourceEntry {
                    source_type: SourceType::BuiltinSkill,
                    path,
                    ..source
                });
            }
        }
    }

    sources
}

pub fn list_installed_skills(working_dir: Option<&Path>) -> Vec<SourceEntry> {
    let fallback;
    let wd = match working_dir {
        Some(p) => Some(p),
        None => {
            fallback = std::env::current_dir().ok();
            fallback.as_deref()
        }
    };
    discover_skills(wd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn skill_with_content(content: &str) -> SourceEntry {
        SourceEntry {
            source_type: SourceType::Skill,
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            content: content.to_string(),
            path: String::new(),
            global: false,
            writable: true,
            supporting_files: Vec::new(),
            properties: HashMap::from([(
                "arguments".to_string(),
                json!(["component", "from", "to"]),
            )]),
        }
    }

    #[test]
    fn loaded_skill_context_with_args_replaces_arguments_placeholder_with_raw_args() {
        let skill = skill_with_content("Review $ARGUMENTS carefully.");

        let rendered = loaded_skill_context_with_args(&skill, Some("src/foo.rs --strict")).unwrap();

        assert!(rendered.contains("Review src/foo.rs --strict carefully."));
    }

    #[test]
    fn loaded_skill_context_with_args_uses_context_without_args() {
        let skill = skill_with_content("Review the code carefully.");

        let rendered = loaded_skill_context_with_args(&skill, None).unwrap();

        assert!(rendered.contains("# Loaded Skill: test-skill (skill)"));
        assert!(rendered.contains("## Content\n\nReview the code carefully."));
    }

    #[test]
    fn loaded_skill_context_shows_resolved_paths_for_supporting_files() {
        let skill_dir = std::env::temp_dir().join("goose-test-skill");
        let script_path = skill_dir.join("scripts").join("my-tool.exe");
        let mut skill = skill_with_content("Run scripts/my-tool.exe.");
        skill.path = skill_dir.to_string_lossy().into_owned();
        skill.supporting_files = vec![script_path.to_string_lossy().into_owned()];

        let rendered = loaded_skill_context_with_args(&skill, None).unwrap();
        let resolved_path = script_path.to_string_lossy().replace('\\', "/");

        assert!(rendered.contains("Relative paths in this skill resolve from the skill directory"));
        assert!(rendered.contains("scripts/my-tool.exe"));
        assert!(rendered.contains(&resolved_path));
        assert!(rendered.contains("load_skill(name: \"test-skill/scripts/my-tool.exe\")"));
    }

    /// A skill that ships scripts also ships their dependencies. Nothing under a dependency tree was
    /// written by the skill's author, and none of it belongs in the model's context.
    #[test]
    fn dependency_trees_are_never_walked() {
        for vendored in [
            "node_modules",
            "__pycache__",
            ".venv",
            "venv",
            "target",
            "dist",
            ".next",
            ".cache",
            ".pytest_cache",
        ] {
            assert!(
                should_skip_dir(Path::new("/s/scripts").join(vendored).as_path()),
                "{vendored} must never be descended into"
            );
        }
        // ...and the VCS dirs it always skipped
        for vcs in [".git", ".hg", ".svn"] {
            assert!(should_skip_dir(Path::new("/s").join(vcs).as_path()));
        }
        // A real skill directory must still be walked.
        assert!(!should_skip_dir(Path::new("/s/scripts")));
        assert!(!should_skip_dir(Path::new("/s/references")));
        // Guard against matching a SUBSTRING: only the exact directory name is vendored.
        assert!(!should_skip_dir(Path::new("/s/my-node_modules-notes")));
        assert!(!should_skip_dir(Path::new("/s/target-audience")));
    }

    /// REGRESSION, from the real thing: load_skill on ~/.agents/skills/atlassian-community-leanzero
    /// returned ~948,000 characters — one line per supporting file, 3,069 of them, no cap — which is about
    /// TWICE the whole context window of the local 27b fleet, in one tool result.
    #[test]
    fn supporting_file_manifest_is_capped_and_says_what_it_dropped() {
        let skill_dir = std::env::temp_dir().join("goose-test-skill");
        let mut skill = skill_with_content("Big skill.");
        skill.path = skill_dir.to_string_lossy().into_owned();
        skill.supporting_files = (0..3069)
            .map(|i| {
                skill_dir
                    .join(format!("f{i}.txt"))
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();

        let rendered = loaded_skill_context_with_args(&skill, None).unwrap();
        let listed = rendered.matches("load_skill(name: \"test-skill/f").count();
        assert_eq!(
            listed, MAX_LISTED_SUPPORTING_FILES,
            "manifest must be capped"
        );
        // The drop is NAMED — a silent truncation reads as "that is the whole skill", and the model would
        // never ask for a file it was not shown.
        assert!(
            rendered.contains(&format!(
                "…and {} more file(s)",
                3069 - MAX_LISTED_SUPPORTING_FILES
            )),
            "the manifest must say how many it dropped: {}",
            // Take CHARACTERS, not bytes. Byte-slicing a String panics when the cut lands inside a UTF-8
            // sequence — and this is the failure MESSAGE, so it would panic exactly when the assert was
            // already failing and the text was the only thing worth reading.
            rendered
                .chars()
                .rev()
                .take(300)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        );
        // Whatever the cap, the result must stay a sane fraction of a local model's window.
        assert!(
            rendered.len() < 40_000,
            "manifest still too big for a local context: {} chars",
            rendered.len()
        );
    }
}
