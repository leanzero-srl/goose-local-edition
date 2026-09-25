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
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;
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
///
/// When the project IS a directory whose skill roots are also global roots — the home folder, the
/// desktop's default working dir — `<project>/.agents/skills` and `~/.agents/skills` are one folder.
/// It used to be listed twice, first as project, and the first listing wins the `seen` set, so every
/// global skill was labelled PROJECT. The folder keeps its first (project-precedence) position and
/// the global flag; the later duplicate is dropped.
pub fn all_skill_dirs(working_dir: Option<&Path>) -> Vec<(PathBuf, bool)> {
    let mut global_dirs: Vec<PathBuf> = Vec::new();
    let home = dirs::home_dir();
    if let Some(h) = home.as_ref() {
        global_dirs.push(h.join(".agents").join("skills"));
    }
    global_dirs.push(Paths::config_dir().join("skills"));
    if let Some(h) = home.as_ref() {
        global_dirs.push(h.join(".claude").join("skills"));
        global_dirs.push(h.join(".config").join("agents").join("skills"));
    }

    let mut dirs: Vec<(PathBuf, bool)> = Vec::new();
    if let Some(wd) = working_dir {
        for project_dir in [
            wd.join(".agents").join("skills"),
            wd.join(".goose").join("skills"),
            wd.join(".claude").join("skills"),
        ] {
            let is_global = global_dirs.iter().any(|g| same_dir(g, &project_dir));
            dirs.push((project_dir, is_global));
        }
    }
    for global_dir in global_dirs {
        if !dirs.iter().any(|(d, _)| same_dir(d, &global_dir)) {
            dirs.push((global_dir, true));
        }
    }

    dirs.extend(
        installed_plugin_skill_dirs()
            .into_iter()
            .map(|dir| (dir, true)),
    );

    dirs
}

fn same_dir(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Claude Code's cut for a description taken from the body (2.1.280 `hhe`: 97 chars + "...").
const BODY_DESCRIPTION_MAX_CHARS: usize = 100;

/// Claude Code's `hhe`: the body's first non-empty line, a heading's `#`s stripped.
fn description_from_body(body: &str) -> Option<String> {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty())?;
    let heading = line.trim_start_matches('#');
    let text = if heading.len() < line.len() && heading.starts_with(char::is_whitespace) {
        heading.trim()
    } else {
        line
    };
    if text.chars().count() > BODY_DESCRIPTION_MAX_CHARS {
        let cut: String = text.chars().take(BODY_DESCRIPTION_MAX_CHARS - 3).collect();
        return Some(format!("{cut}..."));
    }
    Some(text.to_string())
}

/// Read one SKILL.md the way Claude Code does: the frontmatter is optional, a missing `name` is the
/// skill's directory name, a missing `description` is the body's first line. `Err` is the reason the file
/// cannot be a skill, worded for "couldn't read <file>: <why>".
fn parse_skill_content(
    content: &str,
    skill_dir: &Path,
    global: bool,
) -> Result<SourceEntry, String> {
    let (metadata, body): (SkillFrontmatter, String) = match parse_frontmatter(content) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => (
            SkillFrontmatter {
                name: None,
                description: String::new(),
                metadata: HashMap::new(),
            },
            content.trim().to_string(),
        ),
        Err(e) => return Err(format!("its frontmatter is not valid YAML ({e})")),
    };

    let name = match metadata.name.filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => skill_dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .ok_or("its frontmatter has no name")?,
    };
    if name.contains('/') {
        return Err(format!("its name \"{name}\" contains '/'"));
    }

    let description = if metadata.description.trim().is_empty() {
        description_from_body(&body).ok_or("it has no description and no text")?
    } else {
        metadata.description
    };

    Ok(SourceEntry {
        source_type: SourceType::Skill,
        name,
        description,
        content: body,
        path: skill_dir.to_string_lossy().into_owned(),
        global,
        writable: true,
        supporting_files: Vec::new(),
        properties: metadata.metadata,
    })
}

/// A SKILL.md that exists and cannot be a skill: shown on the Skills page, never to the model.
#[derive(Debug, Clone, PartialEq)]
pub struct UnreadableSkill {
    pub skill_md: PathBuf,
    pub reason: String,
    pub global: bool,
}

impl UnreadableSkill {
    pub fn message(&self) -> String {
        format!("couldn't read {}: {}", self.skill_md.display(), self.reason)
    }
}

#[derive(Debug, Default)]
pub struct SkillScan {
    pub skills: Vec<SourceEntry>,
    pub unreadable: Vec<UnreadableSkill>,
}

type FileStamp = Option<(Option<SystemTime>, u64)>;

/// Every SKILL.md read by this process, by path, with the stamp it was read at.
///
/// Discovery runs several times per turn — the skills extension's instructions, the request-turn recall,
/// the slash-command list, the Skills page — and it used to re-parse every file and re-log every failure
/// each time: measured 2026-09-25, session log 20260925_222121, 40 "Failed to parse skill frontmatter"
/// lines in 22 minutes from 4 files × 10 scans (2 per turn). A file is now parsed, and its failure
/// logged, once per change of its modification time or length.
static SKILL_READS: LazyLock<Mutex<HashMap<PathBuf, (FileStamp, Result<SourceEntry, String>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn read_skill_file(skill_md: &Path, skill_dir: &Path, global: bool) -> Result<SourceEntry, String> {
    let stamp: FileStamp = std::fs::metadata(skill_md)
        .ok()
        .map(|m| (m.modified().ok(), m.len()));
    let mut reads = SKILL_READS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((seen, read)) = reads.get(skill_md) {
        if *seen == stamp {
            return read.clone().map(|mut skill| {
                skill.global = global;
                skill
            });
        }
    }
    let read = std::fs::read_to_string(skill_md)
        .map_err(|e| e.to_string())
        .and_then(|content| parse_skill_content(&content, skill_dir, global));
    if let Err(reason) = &read {
        warn!("couldn't read skill {}: {}", skill_md.display(), reason);
    }
    reads.insert(skill_md.to_path_buf(), (stamp, read.clone()));
    read
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

fn scan_skills_from_dir(
    dir: &Path,
    global: bool,
    seen: &mut HashSet<String>,
    scan: &mut SkillScan,
) {
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

    for skill_file in skill_files {
        let Some(skill_dir) = skill_file.parent() else {
            continue;
        };
        match read_skill_file(&skill_file, skill_dir, global) {
            Err(reason) => scan.unreadable.push(UnreadableSkill {
                skill_md: skill_file.clone(),
                reason,
                global,
            }),
            Ok(mut source) if !seen.contains(&source.name) => {
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
                scan.skills.push(source);
            }
            Ok(_) => {}
        }
    }
}

/// Discover skills from all configured filesystem locations and built-ins.
/// Each returned entry has `global` set according to the directory it was
/// found in (or `true` for built-ins).
pub fn discover_skills(working_dir: Option<&Path>) -> Vec<SourceEntry> {
    scan_skills(working_dir).skills
}

/// [`discover_skills`] plus every SKILL.md that could not be read, with the reason.
pub fn scan_skills(working_dir: Option<&Path>) -> SkillScan {
    let mut scan = SkillScan::default();
    let mut seen = HashSet::new();

    for (dir, is_global) in all_skill_dirs(working_dir) {
        scan_skills_from_dir(&dir, is_global, &mut seen, &mut scan);
    }

    for content in builtin::get_all() {
        match parse_skill_content(content, Path::new(""), true) {
            Ok(source) if !seen.contains(&source.name) => {
                seen.insert(source.name.clone());
                let path = format!("builtin://skills/{}", source.name);
                scan.skills.push(SourceEntry {
                    source_type: SourceType::BuiltinSkill,
                    path,
                    ..source
                });
            }
            Ok(_) => {}
            Err(reason) => warn!("built-in skill cannot be read: {reason}"),
        }
    }

    scan
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

    // UX audit 2026-09-23: the desktop's working dir was the home folder, so `~/.agents/skills` was
    // scanned first as `<project>/.agents/skills` and every global skill was labelled PROJECT.
    #[test]
    fn a_project_that_is_the_home_folder_does_not_relabel_the_global_roots() {
        let home = dirs::home_dir().expect("home dir");
        let dirs = all_skill_dirs(Some(&home));
        let agents = home.join(".agents").join("skills");
        let listed: Vec<_> = dirs.iter().filter(|(d, _)| *d == agents).collect();
        assert_eq!(listed, vec![&(agents.clone(), true)], "{dirs:?}");
        assert_eq!(
            dirs[0],
            (agents, true),
            "the folder keeps its first position"
        );
        let claude = home.join(".claude").join("skills");
        assert_eq!(
            dirs.iter().filter(|(d, _)| *d == claude).count(),
            1,
            "{dirs:?}"
        );
        assert!(dirs.contains(&(home.join(".goose").join("skills"), false)));
    }

    /// Claude Code reads a SKILL.md with no frontmatter as a skill named by its directory, described by
    /// its first line (2.1.280 `hhe`). ~/.agents/skills/talent-vault-skill is exactly that file.
    #[test]
    fn a_skill_without_frontmatter_is_named_by_its_directory() {
        let dir = Path::new("/s/talent-vault-skill");
        let raw = "# TalentVault Technical Skill — Component Map\n\n> Purpose: the map.\n\n---\n\n**Two parallel \"employee\" stores**";
        let skill = parse_skill_content(raw, dir, true).expect("a skill");
        assert_eq!(skill.name, "talent-vault-skill");
        assert_eq!(
            skill.description,
            "TalentVault Technical Skill — Component Map"
        );
        assert_eq!(skill.content, raw);

        let skill = parse_skill_content("---\ndescription: d\n---\nbody", dir, true).unwrap();
        assert_eq!(
            skill.name, "talent-vault-skill",
            "a missing name is the directory's"
        );
        let skill =
            parse_skill_content("---\nname: n\n---\n\n## Heading line\nbody", dir, true).unwrap();
        assert_eq!(
            skill.description, "Heading line",
            "a missing description is the first line"
        );

        let long = format!("# {}", "x".repeat(150));
        let skill = parse_skill_content(&long, dir, true).unwrap();
        assert_eq!(skill.description, format!("{}...", "x".repeat(97)));
    }

    #[test]
    fn a_file_that_cannot_be_a_skill_says_why() {
        let dir = Path::new("/s/empty");
        assert_eq!(
            parse_skill_content("", dir, true).unwrap_err(),
            "it has no description and no text"
        );
        assert!(parse_skill_content(
            "---\nname: x\nmetadata:\n  - [unclosed\n---\nbody",
            dir,
            true
        )
        .unwrap_err()
        .starts_with("its frontmatter is not valid YAML"));
        assert_eq!(
            parse_skill_content("---\nname: a/b\ndescription: d\n---\n", dir, true).unwrap_err(),
            "its name \"a/b\" contains '/'"
        );
        assert!(
            parse_skill_content("no frontmatter", Path::new(""), true).is_err(),
            "a built-in has no directory to be named by"
        );
    }

    /// The per-turn rescan re-parsed and re-logged every file (40 warnings in one session). A read is kept
    /// until the file's modification time or length changes.
    #[test]
    fn a_skill_file_is_read_again_only_when_it_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("cached");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let md = skill_dir.join("SKILL.md");
        std::fs::write(&md, "---\nname: cached\ndescription: first\n---\nbody").unwrap();
        assert_eq!(
            read_skill_file(&md, &skill_dir, true).unwrap().description,
            "first"
        );
        assert!(
            !read_skill_file(&md, &skill_dir, false).unwrap().global,
            "scope is per call"
        );

        std::fs::write(
            &md,
            "---\nname: cached\ndescription: second: edit\n---\nbody",
        )
        .unwrap();
        assert_eq!(
            read_skill_file(&md, &skill_dir, true).unwrap().description,
            "second: edit"
        );

        std::fs::write(&md, "---\nname: cached\nmetadata:\n  - [unclosed\n---\nx").unwrap();
        assert!(read_skill_file(&md, &skill_dir, true).is_err());
        assert!(
            read_skill_file(&md, &skill_dir, true).is_err(),
            "cached failure"
        );
    }

    #[test]
    fn a_real_project_keeps_its_skill_roots_project_scoped() {
        let project = tempfile::tempdir().expect("tempdir");
        let dirs = all_skill_dirs(Some(project.path()));
        assert_eq!(
            dirs[0],
            (project.path().join(".agents").join("skills"), false)
        );
        assert!(dirs.iter().any(|(_, global)| *global));
    }
}
