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
use std::sync::{Arc, LazyLock, Mutex};
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

/// Canonical writable location for global user skills: `~/.agents/skills`, or
/// `<GOOSE_PATH_ROOT>/.agents/skills` under a root — `Paths::agents_home_dir`, the rule plugins and
/// global agents already follow. Q-188: it was `dirs::home_dir()` alone, so an isolated profile
/// read the owner's skills and its imports wrote into the owner's home.
pub fn global_skills_dir() -> PathBuf {
    Paths::in_agents_home_dir("skills")
}

/// [`global_skills_dir`] as a person reads it: `~/.agents/skills` under the home folder — the text
/// every surface has always shown — and the full path for a root outside it.
pub fn global_skills_dir_display() -> String {
    display_home_relative(&global_skills_dir(), dirs::home_dir().as_deref())
}

pub(crate) fn display_home_relative(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => {
            let parts: Vec<_> = rest
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect();
            format!("~/{}", parts.join("/"))
        }
        None => path.display().to_string(),
    }
}

/// Every global (home- or root-rooted) skills folder, in discovery precedence. goose's own two hang
/// from goose's dirs (under GOOSE_PATH_ROOT when set); `~/.claude/skills` and
/// `~/.config/agents/skills` belong to other tools and stay in the owner's home.
fn global_skill_roots() -> Vec<PathBuf> {
    let mut roots = vec![global_skills_dir(), Paths::config_dir().join("skills")];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".claude").join("skills"));
        roots.push(home.join(".config").join("agents").join("skills"));
    }
    roots
}

/// Canonical writable location for project-scoped skills:
/// `<project>/.agents/skills`.
pub fn project_skills_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".agents").join("skills")
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
        Ok(global_skills_dir())
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

/// A skill folder an update, delete or export may touch: one the skills listing offers for the SAME
/// request — a folder holding SKILL.md inside one of `all_skill_dirs(projectDir)` (goose's global
/// roots, `~/.claude/skills` and `~/.config/agents/skills`, the project's `.agents|.goose|.claude/skills`,
/// installed plugins' skill dirs), which the listing walks recursively. Q-221: the old rule accepted
/// any folder under an ancestor named `.agents|.goose|.claude/skills`, so a client naming the owner's
/// `~/.agents/skills/<x>` explicitly could rewrite or delete it from an isolated GOOSE_PATH_ROOT
/// profile that never lists it (the skills side of Q-213). A folder outside every listed root is
/// refused by name, never "not found".
pub(crate) fn resolve_listed_skill_dir(
    path: &str,
    working_dir: Option<&Path>,
) -> Result<PathBuf, Error> {
    if path.is_empty() {
        return Err(Error::invalid_params().data("Source path must not be empty"));
    }

    let canonical_dir = Path::new(path)
        .canonicalize()
        .map_err(|_| Error::invalid_params().data(format!("Source \"{}\" not found", path)))?;

    if !canonical_dir.is_dir() || !canonical_dir.join("SKILL.md").is_file() {
        return Err(Error::invalid_params().data(format!("Source \"{}\" not found", path)));
    }

    let listed = all_skill_dirs(working_dir);
    if !listed
        .iter()
        .any(|(root, _)| canonical_dir.starts_with(canonicalize_or_original(root)))
    {
        let folders: Vec<String> = listed
            .iter()
            .map(|(root, _)| root.display().to_string())
            .collect();
        return Err(Error::invalid_params().data(format!(
            "Skill \"{}\" is outside the skill folders goose lists for this request ({}); a project skill needs the request's projectDir",
            path,
            folders.join(", ")
        )));
    }

    Ok(canonical_dir)
}

pub(crate) fn is_global_skill_dir(path: &Path) -> bool {
    canonicalize_or_original(path).starts_with(canonicalize_or_original(&global_skills_dir()))
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
    let global_dirs = global_skill_roots();

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
type SkillRead = Result<SourceEntry, String>;

/// Every SKILL.md read by this process, by path, with the stamp it was read at.
///
/// Discovery runs several times per turn — the skills extension's instructions, the request-turn recall,
/// the slash-command list, the Skills page — and it used to re-parse every file and re-log every failure
/// each time: measured 2026-09-25, session log 20260925_222121, 40 "Failed to parse skill frontmatter"
/// lines in 22 minutes from 4 files × 10 scans (2 per turn). A file is now parsed, and its failure
/// logged, once per change of its modification time or length — and again while that time is not yet
/// [`settled`] against the read, where an edit can leave both unchanged.
static SKILL_READS: LazyLock<Mutex<HashMap<PathBuf, StampedRead>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A SKILL.md's stamp, when it was read, and what it read as.
type StampedRead = (FileStamp, Option<StampTime>, SkillRead);

fn read_skill_file(skill_md: &Path, skill_dir: &Path, global: bool) -> SkillRead {
    let read_at = stamp_time(SystemTime::now());
    let stamp: FileStamp = std::fs::metadata(skill_md)
        .ok()
        .map(|m| (m.modified().ok(), m.len()));
    let modified = stamp
        .and_then(|(modified, _)| modified)
        .and_then(stamp_time);
    let mut reads = SKILL_READS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((seen, seen_at, read)) = reads.get(skill_md) {
        if *seen == stamp && settled(modified, *seen_at) {
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
    reads.insert(skill_md.to_path_buf(), (stamp, read_at, read.clone()));
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

/// Whether a directory entry is a directory and whether it is a file, symlinks followed — what
/// `path.is_dir()` / `path.is_file()` answer, without their two `stat` calls per entry: the kind comes
/// with the directory listing, and only a symlink is stat'ed to see what it points at. Q-517, measured
/// on a real `~/.claude/skills` (37,002 files — skills keep state and ledgers beside SKILL.md): one
/// scan took ~330 ms in a release build, ~98 ms with this and ~77 ms with [`dir_stamp`]'s identity.
/// A followed link's answer is recorded: a link whose target appears or goes changes no directory the
/// walk read (Q-518).
fn entry_kind(entry: &std::fs::DirEntry, path: &Path, stamps: &mut WalkStamps) -> (bool, bool) {
    match entry.file_type() {
        Ok(kind) if !kind.is_symlink() => (kind.is_dir(), kind.is_file()),
        _ => {
            let kind = link_kind(path);
            stamps.keep_link(path, kind);
            kind
        }
    }
}

fn link_kind(path: &Path) -> (bool, bool) {
    std::fs::metadata(path)
        .map(|m| (m.is_dir(), m.is_file()))
        .unwrap_or((false, false))
}

/// The identity a walk remembers a directory by, so a symlink cycle or a second route to the same
/// directory is walked once. On unix it is the directory's device and inode — one `stat` — where the
/// canonical path costs `realpath`'s walk of every component for every directory (Q-517).
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DirId(u64, u64);
#[cfg(not(unix))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DirId(PathBuf);

/// Seconds and nanoseconds since the epoch.
type StampTime = (i64, i64);

/// What `stat` says about a directory a walk listed: which directory it is, when an entry was last
/// added, removed or renamed in it (mtime), and when its inode last changed at all (ctime —
/// permissions, a replacement at the same path). Equal stamps mean the listing is the one the walk read.
#[derive(Debug, Clone, PartialEq)]
struct DirStamp {
    id: DirId,
    modified: Option<StampTime>,
    changed: Option<StampTime>,
}

#[cfg(unix)]
fn dir_stamp(dir: &Path) -> Option<DirStamp> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(dir).ok()?;
    Some(DirStamp {
        id: DirId(m.dev(), m.ino()),
        modified: Some((m.mtime(), m.mtime_nsec())),
        changed: Some((m.ctime(), m.ctime_nsec())),
    })
}

#[cfg(not(unix))]
fn dir_stamp(dir: &Path) -> Option<DirStamp> {
    let modified = std::fs::metadata(dir)
        .ok()?
        .modified()
        .ok()
        .and_then(stamp_time);
    Some(DirStamp {
        id: DirId(std::fs::canonicalize(dir).ok()?),
        modified,
        changed: modified,
    })
}

fn stamp_time(t: SystemTime) -> Option<StampTime> {
    let since = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some((since.as_secs() as i64, since.subsec_nanos() as i64))
}

/// Whether a stamp taken at `observed_at` can be trusted to move if its path changes afterward. A
/// filesystem stamps a change with its own clock's tick — the kernel tick on ext4, a whole second on
/// HFS+ — so a change made in the same tick as the read can leave the stamp equal to the one read.
/// Any tick of a second or less lies inside one second of the wall clock, so a stamp from an earlier
/// second than the read is settled, and one from the read's own second is not (git's "racily clean"
/// rule): what was read with it is read again on the next call.
fn settled(stamp: Option<StampTime>, observed_at: Option<StampTime>) -> bool {
    #[cfg(test)]
    if TRUST_FRESH_STAMPS.with(|trust| trust.get()) {
        return stamp.is_some();
    }
    matches!((stamp, observed_at), (Some((stamp_secs, _)), Some((read_secs, _))) if stamp_secs < read_secs)
}

/// Everything one walk of a skill root read, as `stat` can re-check it without listing anything:
/// every directory it listed (or found missing) and what every symlink it followed pointed at. A
/// directory is kept once, under the first path that reached it: a second route to it runs through a
/// link, and re-pointing the link changes the directory that holds it.
#[derive(Debug, Default)]
struct WalkStamps {
    dirs: Vec<(PathBuf, Option<DirStamp>)>,
    links: Vec<(PathBuf, (bool, bool))>,
    kept_dirs: HashSet<DirId>,
    kept_links: HashSet<PathBuf>,
}

impl WalkStamps {
    fn keep_dir(&mut self, dir: &Path, stamp: Option<&DirStamp>) {
        if stamp.is_none_or(|stamp| self.kept_dirs.insert(stamp.id.clone())) {
            self.dirs.push((dir.to_path_buf(), stamp.cloned()));
        }
    }

    fn keep_link(&mut self, link: &Path, kind: (bool, bool)) {
        if self.kept_links.insert(link.to_path_buf()) {
            self.links.push((link.to_path_buf(), kind));
        }
    }

    fn settled(&self, walked_at: Option<StampTime>) -> bool {
        self.dirs.iter().all(|(_, stamp)| match stamp {
            Some(stamp) => settled(stamp.modified, walked_at) && settled(stamp.changed, walked_at),
            None => true,
        })
    }

    fn unchanged(&self) -> bool {
        self.dirs
            .iter()
            .all(|(dir, stamp)| dir_stamp(dir) == *stamp)
            && self
                .links
                .iter()
                .all(|(link, kind)| link_kind(link) == *kind)
    }
}

fn walk_files_recursively<F, G>(
    dir: &Path,
    visited_dirs: &mut HashSet<DirId>,
    stamps: &mut WalkStamps,
    should_descend: &mut G,
    visit_file: &mut F,
) where
    F: FnMut(&Path),
    G: FnMut(&Path) -> bool,
{
    let stamp = dir_stamp(dir);
    stamps.keep_dir(dir, stamp.as_ref());
    let Some(stamp) = stamp else {
        return;
    };

    if !visited_dirs.insert(stamp.id) {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let (is_dir, is_file) = entry_kind(&entry, &path, stamps);
        if is_dir {
            if should_descend(&path) {
                walk_files_recursively(&path, visited_dirs, stamps, should_descend, visit_file);
            }
        } else if is_file {
            visit_file(&path);
        }
    }
}

/// One skill root as a walk found it: every SKILL.md in walk order, and the stamps that tell a later
/// call whether a walk would find the same. A skill's supporting files are walked the first time a
/// scan lists that skill — never for a SKILL.md that cannot be read or whose name an earlier one took,
/// as before the walk was remembered — and kept with it. The stamps already cover that walk: every
/// directory under a skill is a directory the root's walk listed, by the same inode.
#[derive(Debug)]
struct RootWalk {
    skill_files: Vec<(PathBuf, std::sync::OnceLock<Vec<String>>)>,
    stamps: WalkStamps,
    settled: bool,
}

#[cfg(test)]
thread_local! {
    /// The roots this thread walked in full, in order — the walks the stamps are there to spare.
    static ROOTS_WALKED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Trust a stamp taken in the same second as its read, so a test need not wait out the clock to
    /// see whether the stamps themselves catch a change.
    static TRUST_FRESH_STAMPS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn walk_root(dir: &Path) -> RootWalk {
    #[cfg(test)]
    ROOTS_WALKED.with(|walked| walked.borrow_mut().push(dir.to_path_buf()));
    let walked_at = stamp_time(SystemTime::now());
    let mut stamps = WalkStamps::default();
    let mut found = Vec::new();

    walk_files_recursively(
        dir,
        &mut HashSet::new(),
        &mut stamps,
        &mut |path| !should_skip_dir(path),
        &mut |path| {
            if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
                found.push(path.to_path_buf());
            }
        },
    );

    let settled = stamps.settled(walked_at);
    RootWalk {
        skill_files: found
            .into_iter()
            .map(|skill_file| (skill_file, std::sync::OnceLock::new()))
            .collect(),
        stamps,
        settled,
    }
}

fn supporting_files(skill_dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    walk_files_recursively(
        skill_dir,
        &mut HashSet::new(),
        &mut WalkStamps::default(),
        &mut |path| !should_skip_dir(path) && !path.join("SKILL.md").is_file(),
        &mut |path| {
            if path.file_name().and_then(|n| n.to_str()) != Some("SKILL.md") {
                files.push(path.to_string_lossy().into_owned());
            }
        },
    );
    // read_dir order is the filesystem's; a loaded skill must render the same text on every load for
    // load_skill to recognise the copy already in the conversation (Q-297).
    files.sort();
    files
}

/// The last walk of every skill root, by path. Q-518: a scan runs in the skills extension's
/// instructions and in recall's turn context before a turn's first provider call, and in recall again
/// before every later one; on the owner's `~/.claude/skills` (109 SKILL.md in 1,230 directories, 36,022
/// supporting files listed) each walk cost ~80 ms release to find the same skills. A root is walked again
/// only when a directory its last walk listed, or a link it followed, no longer stats the same — every
/// SKILL.md or supporting file added, removed or renamed anywhere in the tree moves its directory's
/// mtime — so an unchanged tree costs one `stat` per directory. SKILL.md contents are not part of the
/// walk: [`read_skill_file`] re-checks each file's own stamp on every scan.
static ROOT_WALKS: LazyLock<Mutex<HashMap<PathBuf, Arc<RootWalk>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn root_walk(dir: &Path) -> Arc<RootWalk> {
    let last = ROOT_WALKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(dir)
        .cloned();
    if let Some(last) = last {
        if last.settled && last.stamps.unchanged() {
            return last;
        }
    }
    let walk = Arc::new(walk_root(dir));
    ROOT_WALKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(dir.to_path_buf(), walk.clone());
    walk
}

fn scan_skills_from_dir(
    dir: &Path,
    global: bool,
    seen: &mut HashSet<String>,
    scan: &mut SkillScan,
) {
    scan_walk(&root_walk(dir), global, seen, scan);
}

fn scan_walk(walk: &RootWalk, global: bool, seen: &mut HashSet<String>, scan: &mut SkillScan) {
    for (skill_file, supporting) in &walk.skill_files {
        let Some(skill_dir) = skill_file.parent() else {
            continue;
        };
        match read_skill_file(skill_file, skill_dir, global) {
            Err(reason) => scan.unreadable.push(UnreadableSkill {
                skill_md: skill_file.clone(),
                reason,
                global,
            }),
            Ok(mut source) if !seen.contains(&source.name) => {
                source.supporting_files = supporting
                    .get_or_init(|| supporting_files(skill_dir))
                    .clone();
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

/// The skills a folder's chat can use; `None` lists the user's own and goose's built-in skills
/// only. Q-267: `None` used to read the process cwd as the project — goosed's, since Q-257 the
/// shared $HOME — so a caller without a folder listed $HOME's project skills as if they were its own.
pub fn list_installed_skills(working_dir: Option<&Path>) -> Vec<SourceEntry> {
    discover_skills(working_dir)
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
    /// The hermetic root, held so a test that points GOOSE_PATH_ROOT elsewhere under `env_lock`
    /// (config::paths) cannot swap it mid-assertion.
    fn hold_the_hermetic_root() -> env_lock::EnvGuard<'static> {
        let root = goose_test_support::hermetic_path_root().to_str().unwrap();
        env_lock::lock_env([("GOOSE_PATH_ROOT", Some(root))])
    }

    // Since Q-188 the global `.agents/skills` hangs from `Paths::agents_home_dir()` — under the unit
    // tests' hermetic root, so "the home folder" here is the folder holding that `.agents`.
    #[test]
    fn a_project_that_is_the_home_folder_does_not_relabel_the_global_roots() {
        let _root = hold_the_hermetic_root();
        let agents_home = Paths::agents_home_dir();
        let home = agents_home
            .parent()
            .expect("the .agents folder has a parent");
        let dirs = all_skill_dirs(Some(home));
        let agents = home.join(".agents").join("skills");
        let listed: Vec<_> = dirs.iter().filter(|(d, _)| *d == agents).collect();
        assert_eq!(listed, vec![&(agents.clone(), true)], "{dirs:?}");
        assert_eq!(
            dirs[0],
            (agents, true),
            "the folder keeps its first position"
        );
        let owner_home = dirs::home_dir().expect("home dir");
        let claude = owner_home.join(".claude").join("skills");
        let dirs = all_skill_dirs(Some(&owner_home));
        assert_eq!(
            dirs.iter()
                .filter(|(d, _)| *d == claude)
                .collect::<Vec<_>>(),
            vec![&(claude.clone(), true)],
            "{dirs:?}"
        );
        assert!(dirs.contains(&(owner_home.join(".goose").join("skills"), false)));
    }

    /// Q-188: global skills are read from and written to `<GOOSE_PATH_ROOT>/.agents/skills` under a
    /// root — never the owner's `~/.agents/skills`, which an isolated profile's imports wrote into.
    /// The unit tests always run under a hermetic root (`Paths::root_override`), so this is the
    /// root case without touching the env.
    #[test]
    fn global_skills_hang_from_the_path_root() {
        let _root = hold_the_hermetic_root();
        let root = Paths::root_override().expect("unit tests run under a hermetic root");
        let rooted = root.join(".agents").join("skills");
        assert_eq!(global_skills_dir(), rooted);
        assert_eq!(skill_base_dir(true, None).unwrap(), rooted);
        let owner = dirs::home_dir().unwrap().join(".agents").join("skills");
        let dirs = all_skill_dirs(None);
        assert_eq!(dirs[0], (rooted.clone(), true), "{dirs:?}");
        assert!(!dirs.iter().any(|(d, _)| *d == owner), "{dirs:?}");
        assert!(is_global_skill_dir(&rooted.join("x")));
        assert!(!is_global_skill_dir(&owner.join("x")));
    }

    /// Q-188, unset root: `Paths` takes the home folder from etcetera, the old code from `dirs`. They
    /// must be the same folder, or moving to `Paths` moved every owner's global skills.
    #[test]
    fn unset_the_global_skills_dir_is_the_old_home_path() {
        let home = etcetera::home_dir().unwrap();
        assert_eq!(Some(home.clone()), dirs::home_dir());
        let unset = home.join(".agents").join("skills");
        assert_eq!(
            display_home_relative(&unset, Some(&home)),
            "~/.agents/skills"
        );
        assert_eq!(
            display_home_relative(Path::new("/elsewhere/.agents/skills"), Some(&home)),
            "/elsewhere/.agents/skills"
        );
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

    /// Q-517: the walk reads each entry's kind from the directory listing and remembers directories by
    /// inode instead of stat'ing every entry and canonicalising every directory. What it finds must not
    /// move: symlinked files and directories are followed, a broken link is skipped, a link back into
    /// the skill is walked once, dependency trees and nested skills stay out of the manifest.
    #[cfg(unix)]
    #[test]
    fn the_skill_walk_follows_links_once_and_finds_what_it_always_found() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("skills");
        let alpha = root.join("alpha");
        let shared = tmp.path().join("shared");
        std::fs::create_dir_all(alpha.join("node_modules")).unwrap();
        std::fs::create_dir_all(alpha.join("nested")).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(
            alpha.join("SKILL.md"),
            "---\nname: alpha\ndescription: a\n---\nA",
        )
        .unwrap();
        std::fs::write(alpha.join("notes.md"), "n").unwrap();
        std::fs::write(alpha.join("node_modules").join("dep.js"), "d").unwrap();
        std::fs::write(
            alpha.join("nested").join("SKILL.md"),
            "---\nname: nested\ndescription: n\n---\nN",
        )
        .unwrap();
        std::fs::write(alpha.join("nested").join("inner.md"), "i").unwrap();
        std::fs::write(shared.join("data.txt"), "s").unwrap();
        symlink(&shared, alpha.join("linked_dir")).unwrap();
        symlink(shared.join("data.txt"), alpha.join("linked_file.txt")).unwrap();
        symlink(tmp.path().join("missing"), alpha.join("broken")).unwrap();
        symlink(&alpha, alpha.join("loop")).unwrap();

        let mut scan = SkillScan::default();
        scan_skills_from_dir(&root, true, &mut HashSet::new(), &mut scan);

        let mut names: Vec<&str> = scan.skills.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["alpha", "nested"]);
        let files_of = |name: &str| -> Vec<String> {
            let skill = scan.skills.iter().find(|s| s.name == name).unwrap();
            skill
                .supporting_files
                .iter()
                .map(|f| {
                    Path::new(f)
                        .strip_prefix(&skill.path)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        };
        assert_eq!(
            files_of("alpha"),
            ["linked_dir/data.txt", "linked_file.txt", "notes.md"]
        );
        assert_eq!(files_of("nested"), ["inner.md"]);
        assert!(scan.unreadable.is_empty());
    }

    fn walks_of(root: &Path) -> usize {
        ROOTS_WALKED.with(|walked| walked.borrow().iter().filter(|r| *r == root).count())
    }

    /// Trusts stamps taken in the same second as their walk while held, so a test sees what the
    /// stamps themselves catch instead of waiting out the clock.
    struct TrustFreshStamps;

    impl TrustFreshStamps {
        fn hold() -> Self {
            TRUST_FRESH_STAMPS.with(|trust| trust.set(true));
            TrustFreshStamps
        }
    }

    impl Drop for TrustFreshStamps {
        fn drop(&mut self) {
            TRUST_FRESH_STAMPS.with(|trust| trust.set(false));
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn skill_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\nBody of {name}.")
    }

    fn scan_root(root: &Path) -> SkillScan {
        let mut scan = SkillScan::default();
        scan_skills_from_dir(root, false, &mut HashSet::new(), &mut scan);
        scan
    }

    fn described(scan: &SkillScan) -> Vec<(String, String)> {
        let mut skills: Vec<_> = scan
            .skills
            .iter()
            .map(|s| (s.name.clone(), s.description.clone()))
            .collect();
        skills.sort();
        skills
    }

    fn files_of(scan: &SkillScan, name: &str) -> Vec<String> {
        let skill = scan.skills.iter().find(|s| s.name == name).unwrap();
        skill
            .supporting_files
            .iter()
            .map(|f| {
                Path::new(f)
                    .strip_prefix(&skill.path)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    /// Q-518: the scan a remembered walk answers is the scan a fresh walk of the same tree gives —
    /// every skill, every field, every supporting file, every unreadable SKILL.md — and the second
    /// call walks nothing.
    #[test]
    fn a_remembered_walk_answers_what_a_fresh_walk_finds() {
        let _trust = TrustFreshStamps::hold();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("skills");
        write(&root.join("alpha/SKILL.md"), &skill_md("alpha", "first"));
        write(&root.join("alpha/scripts/run.sh"), "echo");
        write(&root.join("alpha/references/deep/notes.md"), "n");
        write(&root.join("alpha/node_modules/dep/index.js"), "d");
        write(
            &root.join("alpha/inner/SKILL.md"),
            &skill_md("inner", "nested"),
        );
        write(&root.join("alpha/inner/inner.md"), "i");
        write(
            &root.join("group/beta/SKILL.md"),
            &skill_md("beta", "grouped"),
        );
        write(
            &root.join("shadow/SKILL.md"),
            &skill_md("alpha", "a second alpha"),
        );
        write(
            &root.join("broken/SKILL.md"),
            "---\nname: x\nmetadata:\n  - [unclosed\n---\nx",
        );

        let first = scan_root(&root);
        let remembered = scan_root(&root);
        assert_eq!(walks_of(&root), 1, "the unchanged tree was walked again");

        let mut fresh = SkillScan::default();
        scan_walk(&walk_root(&root), false, &mut HashSet::new(), &mut fresh);
        for scan in [&first, &remembered] {
            assert_eq!(
                serde_json::to_value(&scan.skills).unwrap(),
                serde_json::to_value(&fresh.skills).unwrap()
            );
            assert_eq!(scan.unreadable, fresh.unreadable);
        }
        let names: Vec<String> = described(&fresh)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            names,
            ["alpha", "beta", "inner"],
            "one alpha: the first found"
        );
        assert_eq!(files_of(&fresh, "beta"), Vec::<String>::new());
        assert_eq!(files_of(&fresh, "inner"), ["inner.md"]);
        assert_eq!(fresh.unreadable.len(), 1);
    }

    /// Q-518: every change the listing exposes is seen on the very next call — a SKILL.md added,
    /// edited or removed, a supporting file added or removed at any depth, a folder becoming a nested
    /// skill, a link's target appearing, the root itself going and coming back — and a call after an
    /// unchanged tree walks nothing.
    #[test]
    fn a_change_to_the_skill_tree_is_seen_on_the_next_call() {
        let _trust = TrustFreshStamps::hold();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("skills");
        write(&root.join("alpha/SKILL.md"), &skill_md("alpha", "first"));
        write(&root.join("alpha/scripts/run.sh"), "echo");
        write(&root.join("alpha/sub/notes.md"), "n");
        write(&root.join("beta/SKILL.md"), &skill_md("beta", "second"));

        let scan = scan_root(&root);
        assert_eq!(described(&scan).len(), 2);
        assert_eq!(files_of(&scan, "alpha"), ["scripts/run.sh", "sub/notes.md"]);
        scan_root(&root);
        assert_eq!(walks_of(&root), 1, "the unchanged tree was walked again");

        write(&root.join("gamma/SKILL.md"), &skill_md("gamma", "added"));
        let scan = scan_root(&root);
        assert!(described(&scan).contains(&("gamma".into(), "added".into())));

        std::fs::write(root.join("alpha/SKILL.md"), skill_md("alpha", "edited")).unwrap();
        let walks = walks_of(&root);
        let scan = scan_root(&root);
        assert!(described(&scan).contains(&("alpha".into(), "edited".into())));
        assert_eq!(
            walks_of(&root),
            walks,
            "an edited SKILL.md is re-read, not re-walked"
        );

        std::fs::remove_file(root.join("beta/SKILL.md")).unwrap();
        let scan = scan_root(&root);
        assert!(!scan.skills.iter().any(|s| s.name == "beta"));

        write(&root.join("alpha/scripts/deep/new.sh"), "new");
        let scan = scan_root(&root);
        assert_eq!(
            files_of(&scan, "alpha"),
            ["scripts/deep/new.sh", "scripts/run.sh", "sub/notes.md"]
        );

        std::fs::remove_file(root.join("alpha/scripts/run.sh")).unwrap();
        let scan = scan_root(&root);
        assert_eq!(
            files_of(&scan, "alpha"),
            ["scripts/deep/new.sh", "sub/notes.md"]
        );

        write(&root.join("alpha/sub/SKILL.md"), &skill_md("sub", "nested"));
        let scan = scan_root(&root);
        assert_eq!(files_of(&scan, "alpha"), ["scripts/deep/new.sh"]);
        assert_eq!(files_of(&scan, "sub"), ["notes.md"]);

        #[cfg(unix)]
        {
            let outside = tmp.path().join("outside");
            std::os::unix::fs::symlink(outside.join("target.txt"), root.join("alpha/link.txt"))
                .unwrap();
            let scan = scan_root(&root);
            assert_eq!(files_of(&scan, "alpha"), ["scripts/deep/new.sh"]);
            write(&outside.join("target.txt"), "t");
            let scan = scan_root(&root);
            assert_eq!(
                files_of(&scan, "alpha"),
                ["link.txt", "scripts/deep/new.sh"],
                "a link whose target appeared"
            );
        }

        let walks = walks_of(&root);
        scan_root(&root);
        assert_eq!(
            walks_of(&root),
            walks,
            "the unchanged tree was walked again"
        );

        std::fs::remove_dir_all(&root).unwrap();
        assert!(scan_root(&root).skills.is_empty());
        write(&root.join("delta/SKILL.md"), &skill_md("delta", "back"));
        assert_eq!(
            described(&scan_root(&root)),
            [("delta".to_string(), "back".to_string())]
        );
    }

    /// Q-518: a directory stamped in the same second as the walk that read it may carry the same stamp
    /// after a later change on a coarse-clock filesystem, so that walk is not reused; the first walk
    /// in a later second is.
    #[test]
    fn a_walk_in_the_second_its_tree_changed_is_not_reused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("skills");
        write(&root.join("alpha/SKILL.md"), &skill_md("alpha", "first"));
        let created = stamp_time(SystemTime::now()).unwrap().0;

        scan_root(&root);
        scan_root(&root);
        if stamp_time(SystemTime::now()).unwrap().0 == created {
            assert_eq!(walks_of(&root), 2, "a same-second walk was reused");
        }

        let into_next_second = 1_000_000_000
            - SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos();
        std::thread::sleep(std::time::Duration::from_nanos(u64::from(into_next_second)));
        scan_root(&root);
        let walks = walks_of(&root);
        scan_root(&root);
        assert_eq!(walks_of(&root), walks, "a settled walk was not reused");
    }

    /// The instrument behind Q-517/Q-518's numbers: the per-call cost of a scan over the owner's real
    /// global skill roots (`~/.claude/skills` and the rest), from a folder with no project skills.
    /// `cargo test --release -p goose --lib -- --ignored --nocapture skills_scan_cost_on_the_real_tree`
    #[test]
    #[ignore]
    fn skills_scan_cost_on_the_real_tree() {
        let project = tempfile::tempdir().unwrap();
        let mut times = Vec::new();
        let mut last = SkillScan::default();
        for _ in 0..21 {
            let started = std::time::Instant::now();
            last = scan_skills(Some(project.path()));
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        let first = times[0];
        let mut rest = times[1..].to_vec();
        rest.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let files: usize = last.skills.iter().map(|s| s.supporting_files.len()).sum();
        eprintln!(
            "skills scan: {} skills, {} supporting files, {} unreadable; first call {:.1} ms; \
             next 20 calls median {:.2} ms (min {:.2}, max {:.2})",
            last.skills.len(),
            files,
            last.unreadable.len(),
            first,
            rest[rest.len() / 2],
            rest[0],
            rest[rest.len() - 1]
        );
        for (root, _) in all_skill_dirs(Some(project.path())) {
            let walk = root_walk(&root);
            let started = std::time::Instant::now();
            let unchanged = walk.stamps.unchanged();
            eprintln!(
                "  {}: {} SKILL.md, {} directories and {} links stamped, settled {}, \
                 unchanged {unchanged} (checked in {:.2} ms)",
                root.display(),
                walk.skill_files.len(),
                walk.stamps.dirs.len(),
                walk.stamps.links.len(),
                walk.settled,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}
