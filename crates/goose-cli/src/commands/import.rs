//! `goose import claude-code` — bring your Claude Code setup (skills, memory, hints, MCP servers) into goose.
//!
//! This is a *config/setup* importer, distinct from `goose session import` (which ingests Claude Code
//! *conversation transcripts*). Phase 1 is preview-only: it renders the action plan (`--dry-run` is
//! implied) so you can see exactly what would be imported and how each artifact maps. Applying the plan
//! arrives next.

use anyhow::Result;
use goose::import::claude_code::{
    self, ActionClass, ImportOptions, ImportPlan, ImportType, TypeSet,
};
use std::path::PathBuf;

/// Options for `goose import claude-code`.
#[derive(clap::Args, Debug)]
pub struct ImportClaudeCodeArgs {
    /// Claude root to import from (default: ~/.claude)
    #[arg(long, value_name = "PATH")]
    pub from: Option<PathBuf>,

    /// The ~/.claude.json blob holding MCP servers (default: ~/.claude.json)
    #[arg(long, value_name = "PATH")]
    pub claude_json: Option<PathBuf>,

    /// Preview only: print the action table and write nothing
    #[arg(long)]
    pub dry_run: bool,

    /// Import skills only
    #[arg(long)]
    pub skills: bool,

    /// Import memory only (CLAUDE.md hints + auto-memory notes)
    #[arg(long)]
    pub memory: bool,

    /// Import MCP servers only
    #[arg(long)]
    pub mcp: bool,

    /// Import every supported type (the default when no type flag is given)
    #[arg(long)]
    pub all: bool,
}

impl ImportClaudeCodeArgs {
    /// Resolve the requested type set from the flags. No type flag ⇒ all planned types.
    fn type_set(&self) -> TypeSet {
        if self.all || !(self.skills || self.memory || self.mcp) {
            return TypeSet::planned();
        }
        let mut types = Vec::new();
        if self.skills {
            types.push(ImportType::Skills);
        }
        if self.memory {
            // "memory" is CLAUDE.md hints + auto-memory notes.
            types.push(ImportType::Memory);
            types.push(ImportType::Hints);
        }
        if self.mcp {
            types.push(ImportType::Mcp);
        }
        TypeSet::only(types)
    }
}

/// Entry point for `goose import claude-code`.
pub async fn run_claude_code(args: ImportClaudeCodeArgs) -> Result<()> {
    let opts = ImportOptions {
        from: args
            .from
            .clone()
            .unwrap_or_else(ImportOptions::default_from),
        claude_json: args
            .claude_json
            .clone()
            .unwrap_or_else(ImportOptions::default_claude_json),
        types: args.type_set(),
        dry_run: true, // Phase 1: preview only.
    };

    let plan = claude_code::plan(&opts)?;
    render_plan(&plan, &opts);
    Ok(())
}

/// Render the grouped action table.
fn render_plan(plan: &ImportPlan, opts: &ImportOptions) {
    println!();
    println!(
        "  import · claude-code   (preview — from {})",
        opts.from.display()
    );

    if plan.is_empty() {
        println!(
            "  nothing to import — no Claude Code config found under {}",
            opts.from.display()
        );
        println!();
        return;
    }

    for ty in ImportType::display_order() {
        if !opts.types.contains(ty) {
            continue;
        }
        let count = plan.count(ty);
        if count == 0 {
            continue;
        }
        // Target + dominant class come from the first action of this type. For a multi-item type the
        // per-item target is a file/dir under a common parent, so show that parent (all N land there).
        let first = plan.by_type(ty).next();
        let raw_target = first.map(|a| a.target.as_str()).unwrap_or("");
        let target = if count > 1 {
            parent_dir(raw_target)
        } else {
            raw_target.to_string()
        };
        let class = dominant_class(plan, ty);

        // Caveats: how many SKIP / LOSSY within this type.
        let mut caveats = Vec::new();
        let skipped = plan.count_class(ty, ActionClass::Skip);
        let lossy = plan.count_class(ty, ActionClass::Lossy);
        if skipped > 0 {
            caveats.push(format!("{skipped} skipped"));
        }
        if lossy > 0 {
            caveats.push(format!("{lossy} lossy"));
        }
        let caveat = if caveats.is_empty() {
            String::new()
        } else {
            format!("  ({})", caveats.join(" · "))
        };

        println!(
            "  {:<14}{:>4}  →  {:<30} {}{}",
            ty.label(),
            count,
            truncate(&tildize(&target), 30),
            class.label(),
            caveat
        );
    }
    println!();
    println!("  preview only — applying the plan is not wired yet. Re-run with a future --apply to import.");
    println!();
}

/// The class shown for a type: CONVERT/LOSSY/SKIP if any action carries it, else the first action's class.
fn dominant_class(plan: &ImportPlan, ty: ImportType) -> ActionClass {
    for class in [
        ActionClass::Convert,
        ActionClass::Lossy,
        ActionClass::Direct,
        ActionClass::Skip,
    ] {
        if plan.count_class(ty, class) > 0 {
            return class;
        }
    }
    ActionClass::Direct
}

/// The containing directory of a path-like target (`~/.agents/skills/x` → `~/.agents/skills`).
/// A target with no path separator (e.g. "config.yaml extensions") is returned unchanged.
fn parent_dir(target: &str) -> String {
    match target.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => parent.to_string(),
        _ => target.to_string(),
    }
}

/// Replace the home-directory prefix of a path with `~` for a compact, readable display.
fn tildize(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            if let Some(rest) = path.strip_prefix(&home) {
                return format!("~{rest}");
            }
        }
    }
    path.to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
