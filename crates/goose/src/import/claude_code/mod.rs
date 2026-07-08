//! Import Claude Code's setup surface into goose.
//!
//! Phase 1 (this file + the planner submodules) is **read-only**: `plan()` scans the Claude Code config
//! on disk and returns an [`ImportPlan`] describing exactly what an `apply()` would do — with zero writes —
//! so `goose import claude-code --dry-run` can show the full action table before anything happens.
//! Apply/validate arrive in Phase 2.

pub mod hints;
pub mod manifest;
pub mod mcp;
pub mod memory;
pub mod skills;

use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;

/// How faithfully a Claude Code artifact maps onto a goose target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionClass {
    /// Format-preserving copy.
    Direct,
    /// Deterministic reshape, fidelity-complete.
    Convert,
    /// Some semantics do not survive — the loss is spelled out in the action note.
    Lossy,
    /// No honest goose target; listed so nothing is silently dropped.
    Skip,
}

impl ActionClass {
    pub fn label(self) -> &'static str {
        match self {
            ActionClass::Direct => "DIRECT",
            ActionClass::Convert => "CONVERT",
            ActionClass::Lossy => "LOSSY",
            ActionClass::Skip => "SKIP",
        }
    }
}

/// The importable artifact families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportType {
    Skills,
    Memory,
    Hints,
    Mcp,
    Commands,
    OutputStyles,
    Subagents,
    Settings,
    Plugins,
}

impl ImportType {
    pub fn label(self) -> &'static str {
        match self {
            ImportType::Skills => "skills",
            ImportType::Memory => "memory",
            ImportType::Hints => "hints",
            ImportType::Mcp => "mcp servers",
            ImportType::Commands => "commands",
            ImportType::OutputStyles => "output-styles",
            ImportType::Subagents => "subagents",
            ImportType::Settings => "settings",
            ImportType::Plugins => "plugins",
        }
    }

    /// The order the plan table renders types in.
    pub fn display_order() -> [ImportType; 9] {
        [
            ImportType::Skills,
            ImportType::Memory,
            ImportType::Hints,
            ImportType::Mcp,
            ImportType::Commands,
            ImportType::OutputStyles,
            ImportType::Subagents,
            ImportType::Settings,
            ImportType::Plugins,
        ]
    }
}

/// A single planned import action. In Phase 1 it is purely descriptive (nothing is executed).
#[derive(Debug, Clone)]
pub struct Action {
    pub import_type: ImportType,
    pub class: ActionClass,
    /// Human label of the source artifact (skill name, server name, file stem).
    pub name: String,
    /// Source path, when the artifact is file-backed.
    pub source: Option<PathBuf>,
    /// Target path or config location (for display).
    pub target: String,
    /// A caveat, loss description, or skip reason.
    pub note: Option<String>,
}

/// The full read-only plan.
#[derive(Debug, Clone, Default)]
pub struct ImportPlan {
    pub actions: Vec<Action>,
}

impl ImportPlan {
    pub fn push(&mut self, action: Action) {
        self.actions.push(action);
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// All actions of a given type.
    pub fn by_type(&self, ty: ImportType) -> impl Iterator<Item = &Action> {
        self.actions.iter().filter(move |a| a.import_type == ty)
    }

    /// Count of actions of a given type.
    pub fn count(&self, ty: ImportType) -> usize {
        self.by_type(ty).count()
    }

    /// Count of actions of a given type in a given class (e.g. how many mcp servers are SKIP).
    pub fn count_class(&self, ty: ImportType, class: ActionClass) -> usize {
        self.by_type(ty).filter(|a| a.class == class).count()
    }
}

/// Which importable families to include in a plan.
#[derive(Debug, Clone)]
pub struct TypeSet(HashSet<ImportType>);

impl TypeSet {
    /// The families supported by the current phase's planners (skills/memory/hints/mcp).
    /// Later phases widen this as their planners land.
    pub fn planned() -> Self {
        TypeSet(HashSet::from([
            ImportType::Skills,
            ImportType::Memory,
            ImportType::Hints,
            ImportType::Mcp,
        ]))
    }

    pub fn only(types: impl IntoIterator<Item = ImportType>) -> Self {
        TypeSet(types.into_iter().collect())
    }

    pub fn contains(&self, ty: ImportType) -> bool {
        self.0.contains(&ty)
    }
}

/// Where to read Claude Code config from, and how to run the import.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Claude root — default `~/.claude`.
    pub from: PathBuf,
    /// The `~/.claude.json` blob that holds MCP servers + project scopes.
    pub claude_json: PathBuf,
    /// Which families to import.
    pub types: TypeSet,
    /// Plan only; write nothing. (Phase 1 is always effectively dry-run.)
    pub dry_run: bool,
}

impl ImportOptions {
    /// The default Claude Code locations under the user's home directory.
    pub fn default_from() -> PathBuf {
        dirs::home_dir().unwrap_or_default().join(".claude")
    }

    pub fn default_claude_json() -> PathBuf {
        dirs::home_dir().unwrap_or_default().join(".claude.json")
    }
}

/// How an existing target artifact is treated when an import would touch it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// Reconcile by provenance hash: unchanged → no-op, changed → replace. (default)
    Merge,
    /// Replace the target wholesale.
    Overwrite,
    /// Leave any existing target untouched.
    Skip,
}

/// The result of applying a single action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    Created,
    Updated,
    Unchanged,
    SkippedExists,
    Skipped,
    Failed,
}

impl ApplyOutcome {
    pub fn label(self) -> &'static str {
        match self {
            ApplyOutcome::Created => "created",
            ApplyOutcome::Updated => "updated",
            ApplyOutcome::Unchanged => "unchanged",
            ApplyOutcome::SkippedExists => "exists (skipped)",
            ApplyOutcome::Skipped => "skipped",
            ApplyOutcome::Failed => "failed",
        }
    }
}

/// One executed action, for the post-apply report.
#[derive(Debug, Clone)]
pub struct AppliedAction {
    pub import_type: ImportType,
    pub name: String,
    pub outcome: ApplyOutcome,
    pub detail: Option<String>,
}

/// The result of an apply run.
#[derive(Debug, Clone, Default)]
pub struct ImportReport {
    pub applied: Vec<AppliedAction>,
}

impl ImportReport {
    pub fn record(
        &mut self,
        import_type: ImportType,
        name: impl Into<String>,
        outcome: ApplyOutcome,
        detail: Option<String>,
    ) {
        self.applied.push(AppliedAction {
            import_type,
            name: name.into(),
            outcome,
            detail,
        });
    }

    pub fn count_outcome(&self, outcome: ApplyOutcome) -> usize {
        self.applied.iter().filter(|a| a.outcome == outcome).count()
    }

    pub fn count_type_outcome(&self, ty: ImportType, outcome: ApplyOutcome) -> usize {
        self.applied
            .iter()
            .filter(|a| a.import_type == ty && a.outcome == outcome)
            .count()
    }
}

/// Where imported artifacts are written. In production these are the real goose homes
/// (`~/.agents/skills`, the config dir); in tests they are temp dirs, so apply is fully sandboxable
/// without touching the user's real config.
#[derive(Debug, Clone)]
pub struct ApplyContext {
    /// Where skills are written (`~/.agents/skills`, or `<wd>/.agents/skills` in tests).
    pub skills_dir: PathBuf,
    /// The goose config dir (holds `.goosehints`, `memory/`, `.import/`).
    pub config_dir: PathBuf,
    pub conflict: ConflictPolicy,
}

impl ApplyContext {
    /// The context pointing at the real goose homes.
    pub fn production(conflict: ConflictPolicy) -> Self {
        let skills_dir = crate::skills::global_skills_dir()
            .unwrap_or_else(|| crate::config::paths::Paths::config_dir().join("skills"));
        ApplyContext {
            skills_dir,
            config_dir: crate::config::paths::Paths::config_dir(),
            conflict,
        }
    }

    pub fn goosehints(&self) -> PathBuf {
        self.config_dir.join(".goosehints")
    }

    pub fn memory_dir(&self) -> PathBuf {
        self.config_dir.join("memory")
    }
}

/// A short, stable content hash used as an import provenance marker (idempotency + reconcile key).
pub fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// READ-ONLY scan of the Claude Code config → an [`ImportPlan`]. Writes nothing.
pub fn plan(opts: &ImportOptions) -> Result<ImportPlan> {
    let mut plan = ImportPlan::default();
    if opts.types.contains(ImportType::Skills) {
        skills::plan_skills(opts, &mut plan)?;
    }
    if opts.types.contains(ImportType::Hints) {
        hints::plan_hints(opts, &mut plan)?;
    }
    if opts.types.contains(ImportType::Memory) {
        memory::plan_memory(opts, &mut plan)?;
    }
    if opts.types.contains(ImportType::Mcp) {
        mcp::plan_mcp(opts, &mut plan)?;
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a fake ~/.claude tree + ~/.claude.json for read-only plan tests.
    fn fixture() -> (tempfile::TempDir, ImportOptions) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let claude = root.join(".claude");

        // two skills
        for (name, desc) in [("alpha", "does alpha"), ("beta", "does beta")] {
            let d = claude.join("skills").join(name);
            fs::create_dir_all(&d).unwrap();
            fs::write(
                d.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {desc}\n---\nbody"),
            )
            .unwrap();
        }
        // global CLAUDE.md
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join("CLAUDE.md"), "# hints\nbe terse").unwrap();
        // one memory note + a MEMORY.md index (which must be skipped)
        let mem = claude.join("projects").join("proj-a").join("memory");
        fs::create_dir_all(&mem).unwrap();
        fs::write(
            mem.join("my-fact.md"),
            "---\nname: my-fact\ndescription: a fact\nmetadata:\n  type: user\n---\nbody",
        )
        .unwrap();
        fs::write(mem.join("MEMORY.md"), "- index line").unwrap();
        // ~/.claude.json with stdio, http, and sse servers
        let claude_json = root.join(".claude.json");
        fs::write(
            &claude_json,
            r#"{"mcpServers":{
                "pw":{"type":"stdio","command":"npx","args":["-y","x"],"env":{}},
                "api":{"type":"http","url":"https://ex.com/mcp"},
                "legacy":{"type":"sse","url":"https://ex.com/sse"}
            }}"#,
        )
        .unwrap();

        let opts = ImportOptions {
            from: claude,
            claude_json,
            types: TypeSet::planned(),
            dry_run: true,
        };
        (tmp, opts)
    }

    #[test]
    fn plan_classifies_every_type_and_writes_nothing() {
        let (tmp, opts) = fixture();
        let before = fs::read_dir(tmp.path()).unwrap().count();
        let plan = plan(&opts).unwrap();

        assert_eq!(plan.count(ImportType::Skills), 2, "two skills");
        assert!(plan
            .by_type(ImportType::Skills)
            .all(|a| a.class == ActionClass::Direct));
        assert_eq!(plan.count(ImportType::Hints), 1, "one global CLAUDE.md");
        assert_eq!(
            plan.count(ImportType::Memory),
            1,
            "one note, MEMORY.md skipped"
        );
        assert!(plan
            .by_type(ImportType::Memory)
            .all(|a| a.class == ActionClass::Convert));
        assert_eq!(plan.count(ImportType::Mcp), 3, "stdio + http + sse");
        assert_eq!(
            plan.count_class(ImportType::Mcp, ActionClass::Skip),
            1,
            "sse is SKIP"
        );
        assert_eq!(
            plan.count_class(ImportType::Mcp, ActionClass::Convert),
            2,
            "stdio + http CONVERT"
        );

        // READ-ONLY: nothing was created under the temp root.
        let after = fs::read_dir(tmp.path()).unwrap().count();
        assert_eq!(before, after, "plan() must not write");
    }

    #[test]
    fn plan_is_empty_on_a_missing_claude_root() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = ImportOptions {
            from: tmp.path().join("nope"),
            claude_json: tmp.path().join("nope.json"),
            types: TypeSet::planned(),
            dry_run: true,
        };
        assert!(plan(&opts).unwrap().is_empty());
    }
}
