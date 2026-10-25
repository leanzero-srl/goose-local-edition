//! "Free up memory": who holds memory on this Mac, grouped the way the owner thinks of it (an
//! app and all its helpers are one app), which of it goose may give back itself, which apps it may
//! ASK to quit (an Apple Event, never a signal), and the honest verdict for a model that does not
//! fit — measured, never guessed.
//!
//! Layers: [`census`] reads the kernel (macOS: `proc_listallpids`, `proc_pid_rusage` V4
//! `ri_phys_footprint`, `proc_pidpath`, `proc_pidinfo(PROC_PIDTBSDINFO)`); [`categorize`],
//! [`never_kill`], [`verdict`] and [`smaller_variants`] are pure and table-tested; [`quit_app`]
//! is the one action here that touches another process, and only by asking it.
//!
//! The own engine is listed (category `gooseEngine`) but the fit rule already credits it as
//! available (`StartFrees`, engine.rs), so callers must NOT add it to the goose-owned sum the
//! verdict reads — that would count the same bytes twice.

use crate::fit::gb;
use crate::holders::{HolderEntry, HolderKind};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

/// One process as the kernel reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcRow {
    pub pid: u32,
    pub ppid: u32,
    pub uid: u32,
    /// Unix seconds: with the pid, the identity a later action must match.
    pub started_at: u64,
    /// Executable path; empty when unreadable.
    pub path: String,
    /// `ri_phys_footprint`; 0 when unreadable (another user's process without privileges).
    pub footprint: u64,
    /// Process group (`pbi_pgid`): a job launched as one group (heavy-run, a launchd job) keeps
    /// its group even when a child is reparented to launchd.
    pub pgid: u32,
    /// Has a controlling terminal (`pbi_flags & PROC_FLAG_CONTROLT`): someone is typing at it.
    pub has_tty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    GooseEngine,
    GooseOrphan,
    AiServer,
    UserApp,
    System,
    Job,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::GooseEngine => "gooseEngine",
            Category::GooseOrphan => "gooseOrphan",
            Category::AiServer => "aiServer",
            Category::UserApp => "userApp",
            Category::System => "system",
            Category::Job => "job",
        }
    }
}

/// What the census cannot know by itself: who we are, which pids are engines.
#[derive(Debug, Clone, Default)]
pub struct CategorizeCtx {
    pub self_pid: u32,
    pub self_uid: u32,
    /// The engine this goose runs (its pid, or its proven leftover's pids).
    pub own_engine_pids: Vec<u32>,
    /// The model the own engine serves, for the label.
    pub own_engine_model: Option<String>,
    /// Why the own engine may not be reclaimed now (busy reply, swarm run, request in flight).
    pub own_engine_busy: Option<String>,
    /// Other MLX engines (`machine::other_engines`): goose stops them per pid.
    pub other_engine_pids: Vec<u32>,
    /// Left-over goose servers PROVEN to be this install's desktop `goose serve` with its app gone
    /// (`memfree_servers::select_orphaned_goosed`): goose stops these; any other orphan is named only.
    pub orphan_goosed_pids: Vec<u32>,
    /// Outermost `.app` path → (bundle id, display name), read from its Info.plist.
    pub bundles: HashMap<String, (String, String)>,
    /// pid → command line (`memfree_protect::all_cmdlines`), for MCP and job-wrapper detection.
    pub cmdlines: HashMap<u32, String>,
    /// pid → launchd label (`memfree_protect::launchd_labels`).
    pub launchd_labels: HashMap<u32, String>,
    /// The never-touch list in force (`memfree_protect::never_touch_list`).
    pub never_touch: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub label: String,
    pub category: Category,
    pub bytes: u64,
    /// (pid, started_at)
    pub pids: Vec<(u32, u64)>,
    pub bundle_id: Option<String>,
    pub quittable: bool,
    pub reclaimable_by_goose: bool,
    pub why: Option<String>,
    /// Why nothing may free this group (`memfree_protect::protected`): never quittable or
    /// reclaimable while set.
    pub protected_why: Option<String>,
    /// Who uses it (TCP clients of a server's port, its parent, its launchd job); filled by the
    /// caller that measures it, empty here.
    pub used_by: Vec<String>,
    /// The launchd job that owns any of its processes.
    pub launchd_label: Option<String>,
    /// Ticked by default in "free everything except protected" (`memfree_protect::eligible`
    /// in Nuclear mode). A caller that changes `reclaimable_by_goose` recomputes it.
    pub nuclear_default: bool,
}

/// The kinds of reclaim goose performs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReclaimItem {
    /// This goose's own idle engine (unmount).
    OwnEngine,
    /// Another MLX engine, by exact identity.
    OtherMlxEngine { pid: u32, started_at: u64 },
    /// A user app asked to quit.
    App { bundle_id: String },
}

/// What one reclaim did, measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReclaimOutcome {
    pub id: String,
    pub label: String,
    pub ok: bool,
    pub freed_bytes: Option<u64>,
    pub message: String,
}

// ---------------------------------------------------------------------------------------------
// Pure rules
// ---------------------------------------------------------------------------------------------

/// The outermost `.app` bundle a path lives in (`/Applications/X.app/Contents/Frameworks/Y
/// Helper.app/...` → `/Applications/X.app`), so every helper folds into the app the owner sees.
pub fn outer_app(path: &str) -> Option<&str> {
    let i = path
        .find(".app/")
        .or_else(|| path.ends_with(".app").then(|| path.len() - 4))?;
    Some(&path[..i + 4])
}

pub(crate) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub(crate) const SYSTEM_PREFIXES: &[&str] = &[
    "/System/",
    "/usr/",
    "/bin/",
    "/sbin/",
    "/Library/Apple/",
    "/private/var/",
];

pub(crate) const SYSTEM_NAMES: &[&str] = &[
    "WindowServer",
    "loginwindow",
    "Finder",
    "Dock",
    "SystemUIServer",
    "launchd",
    "kernel_task",
    "ControlCenter",
    "NotificationCenter",
];

/// Apps that host terminals or coding agents: quitting one would take sessions with it.
pub(crate) const HOSTS_SESSIONS: &[&str] = &[
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "dev.warp.Warp-Stable",
    "dev.warp.Warp",
    "net.kovidgoyal.kitty",
    "org.alacritty",
    "io.alacritty",
    "com.github.wez.wezterm",
    "com.microsoft.VSCode",
    "com.todesktop.230313mzl4w4u92",
    "dev.zed.Zed",
    "com.anthropic.claudefordesktop",
];

pub(crate) fn is_goose_bundle(bundle_id: &str) -> bool {
    bundle_id.starts_with("net.leanzero.goose") || bundle_id.starts_with("com.block.goose")
}

fn is_ai_server(path: &str, bundle_id: Option<&str>) -> bool {
    let name = basename(path).to_ascii_lowercase();
    matches!(
        name.as_str(),
        "ollama" | "lms" | "llama-server" | "llamafile"
    ) || bundle_id.is_some_and(|b| b == "ai.elementlabs.lmstudio" || b == "com.electron.ollama")
        || path.contains("/LM Studio.app/")
        || path.contains("/Ollama.app/")
        || is_lmstudio_runtime(path)
}

/// LM Studio runs its models OUTSIDE its .app: a node runtime and engine helpers under
/// `~/.lmstudio/` (measured 2026-10-25 on the laptop: `~/.lmstudio/.internal/utils/node`, child of
/// the app, held the loaded model while the .app's own processes held 0.4 GB of it).
fn is_lmstudio_runtime(path: &str) -> bool {
    path.contains("/.lmstudio/")
}

/// Which model-server group each process belongs to, as (group key, label, bundle id). A server
/// is its own processes plus every descendant (the worker that holds the weights is a child), and
/// a loose `~/.lmstudio/` process with no LM Studio parent is the headless `lms` server.
fn ai_server_membership(
    rows: &[ProcRow],
    ctx: &CategorizeCtx,
) -> HashMap<u32, (String, String, Option<String>)> {
    let mut member: HashMap<u32, (String, String, Option<String>)> = HashMap::new();
    for row in rows.iter().filter(|r| r.uid == ctx.self_uid) {
        let app = outer_app(&row.path);
        let bundle = app.and_then(|a| ctx.bundles.get(a));
        let bundle_id = bundle.map(|(b, _)| b.as_str());
        if !is_ai_server(&row.path, bundle_id) {
            continue;
        }
        let entry = match (app, bundle) {
            (Some(app), _) if !is_lmstudio_runtime(&row.path) => (
                app.to_string(),
                bundle
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| basename(app).trim_end_matches(".app").to_string()),
                bundle_id.map(str::to_string),
            ),
            _ if is_lmstudio_runtime(&row.path) => {
                ("lms".to_string(), "LM Studio".to_string(), None)
            }
            _ => {
                let name = process_name(&row.path);
                (name.clone(), name, None)
            }
        };
        member.insert(row.pid, entry);
    }
    // Descendants join their server; a runtime under an app joins the app (fixed point).
    loop {
        let mut changed = false;
        for row in rows
            .iter()
            .filter(|r| r.uid == ctx.self_uid && r.pid != r.ppid)
        {
            let Some(parent) = member.get(&row.ppid).cloned() else {
                continue;
            };
            let adopt = match member.get(&row.pid) {
                None => true,
                Some((key, _, _)) => key == "lms" && parent.0 != "lms",
            };
            if adopt {
                member.insert(row.pid, parent);
                changed = true;
            }
        }
        if !changed {
            return member;
        }
    }
}

const JOB_NAMES: &[&str] = &[
    "cargo",
    "rustc",
    "clang",
    "clang++",
    "cc",
    "ld",
    "ld64",
    "swift-frontend",
    "swiftc",
    "xcodebuild",
    "ffmpeg",
    "make",
    "ninja",
    "cmake",
];

/// Every ancestor of `pid` (not `pid` itself), by the census's ppid links.
pub fn ancestors(pid: u32, rows: &[ProcRow]) -> HashSet<u32> {
    let parent: HashMap<u32, u32> = rows.iter().map(|r| (r.pid, r.ppid)).collect();
    let mut out = HashSet::new();
    let mut at = pid;
    while let Some(&p) = parent.get(&at) {
        if p == 0 || p == at || !out.insert(p) {
            break;
        }
        at = p;
    }
    out
}

/// Pids nothing may ever touch: ourselves and every ancestor, and every ancestor of a coding
/// agent (`claude`) or goose CLI, so the terminal/editor running one is never quit.
pub fn protected_pids(self_pid: u32, rows: &[ProcRow]) -> HashSet<u32> {
    let mut out = ancestors(self_pid, rows);
    out.insert(self_pid);
    for r in rows {
        if basename(&r.path) == "claude" {
            out.insert(r.pid);
            out.extend(ancestors(r.pid, rows));
        }
    }
    out
}

/// Why `row` must never be quit or signalled, or `None` when nothing forbids it. Conservative:
/// unknown (no path) is protected.
pub fn never_kill(
    row: &ProcRow,
    self_uid: u32,
    protected: &HashSet<u32>,
    bundle_id: Option<&str>,
) -> Option<&'static str> {
    if row.uid != self_uid {
        return Some("another user's process");
    }
    if row.path.is_empty() {
        return Some("its executable could not be read");
    }
    if SYSTEM_PREFIXES.iter().any(|p| row.path.starts_with(p)) {
        return Some("part of macOS");
    }
    if SYSTEM_NAMES.contains(&basename(&row.path)) {
        return Some("part of macOS");
    }
    if protected.contains(&row.pid) {
        return Some("runs goose or a coding session");
    }
    if let Some(b) = bundle_id {
        if is_goose_bundle(b) {
            return Some("this is goose");
        }
        if b.starts_with("com.apple.") {
            return Some("part of macOS");
        }
        if HOSTS_SESSIONS.contains(&b) {
            return Some("hosts terminal or coding sessions");
        }
    }
    None
}

/// `roots` plus every process whose parent chain reaches one of them.
pub(crate) fn with_descendants(roots: &[u32], rows: &[ProcRow]) -> HashSet<u32> {
    let mut set: HashSet<u32> = roots.iter().copied().collect();
    if set.is_empty() {
        return set;
    }
    loop {
        let before = set.len();
        for r in rows {
            if r.pid != r.ppid && set.contains(&r.ppid) {
                set.insert(r.pid);
            }
        }
        if set.len() == before {
            return set;
        }
    }
}

/// The name a loose process is known by. Self-updating CLIs run a binary named after its version
/// (`~/.local/share/claude/versions/2.1.299`): those take the nearest folder that is a real name, so
/// five sessions on three versions read as one "claude", not five version numbers.
pub(crate) fn process_name(path: &str) -> String {
    let is_version = |c: &str| {
        c.starts_with(|ch: char| ch.is_ascii_digit())
            && c.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ".-_+".contains(ch))
    };
    let base = basename(path);
    if !is_version(base) {
        return base.to_string();
    }
    path.rsplit('/')
        .skip(1)
        .find(|c| !c.is_empty() && !is_version(c) && !matches!(*c, "versions" | "bin" | "current"))
        .unwrap_or(base)
        .to_string()
}

/// Fold the census into the groups the owner sees, largest first.
pub fn categorize(rows: &[ProcRow], ctx: &CategorizeCtx) -> Vec<Group> {
    let protected = protected_pids(ctx.self_pid, rows);
    // goose's engine is a launcher (`uv run`) whose child python holds the weights: the whole
    // tree under the recorded pid is the engine (measured live 2026-10-25: uv 66 MB, python 4.9 GB).
    let own = with_descendants(&ctx.own_engine_pids, rows);
    let others: HashSet<u32> = ctx.other_engine_pids.iter().copied().collect();
    let ai_member = ai_server_membership(rows, ctx);
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    // An app is protected when ANY of its processes is (the main binary may be an ancestor).
    let mut app_blocked: HashMap<String, &'static str> = HashMap::new();

    let mut add = |id: String, make: &dyn Fn() -> Group, row: &ProcRow| {
        let g = groups.entry(id).or_insert_with(make);
        g.bytes = g.bytes.saturating_add(row.footprint);
        g.pids.push((row.pid, row.started_at));
    };

    for row in rows {
        let app = outer_app(&row.path).map(str::to_string);
        let bundle = app.as_ref().and_then(|a| ctx.bundles.get(a));
        let bundle_id = bundle.map(|(b, _)| b.as_str());
        let name = process_name(&row.path);

        if own.contains(&row.pid) {
            let label = match &ctx.own_engine_model {
                Some(m) => format!("goose's engine ({m})"),
                None => "goose's engine".to_string(),
            };
            let why = ctx.own_engine_busy.clone();
            add(
                "gooseEngine".into(),
                &|| Group {
                    id: "gooseEngine".into(),
                    label: label.clone(),
                    category: Category::GooseEngine,
                    bytes: 0,
                    pids: vec![],
                    bundle_id: None,
                    quittable: false,
                    reclaimable_by_goose: why.is_none(),
                    protected_why: None,
                    used_by: vec![],
                    launchd_label: None,
                    nuclear_default: false,
                    why: why.clone(),
                },
                row,
            );
            continue;
        }
        if others.contains(&row.pid) {
            let id = format!("mlx:{}", row.pid);
            add(
                id.clone(),
                &|| Group {
                    id: id.clone(),
                    label: format!("another MLX engine (pid {})", row.pid),
                    category: Category::GooseEngine,
                    bytes: 0,
                    pids: vec![],
                    bundle_id: None,
                    quittable: false,
                    reclaimable_by_goose: true,
                    protected_why: None,
                    used_by: vec![],
                    launchd_label: None,
                    nuclear_default: false,
                    why: None,
                },
                row,
            );
            continue;
        }
        let blocked = never_kill(row, ctx.self_uid, &protected, bundle_id);
        let system_like = row.uid != ctx.self_uid
            || row.path.is_empty()
            || SYSTEM_PREFIXES.iter().any(|p| row.path.starts_with(p))
            || SYSTEM_NAMES.contains(&name.as_str())
            || bundle_id.is_some_and(|b| b.starts_with("com.apple."));
        if (name == "goosed" || name == "goose")
            && row.ppid == 1
            && row.uid == ctx.self_uid
            && !protected.contains(&row.pid)
        {
            let id = format!("orphan:{}", row.pid);
            let mine = ctx.orphan_goosed_pids.contains(&row.pid);
            add(
                id.clone(),
                &|| Group {
                    id: id.clone(),
                    label: format!("goose server left running (pid {})", row.pid),
                    category: Category::GooseOrphan,
                    bytes: 0,
                    pids: vec![],
                    bundle_id: None,
                    quittable: false,
                    reclaimable_by_goose: mine,
                    protected_why: None,
                    used_by: vec![],
                    launchd_label: None,
                    nuclear_default: false,
                    why: (!mine).then(|| {
                        "it is not this install's desktop server, so goose leaves it alone".into()
                    }),
                },
                row,
            );
            continue;
        }
        if system_like {
            add(
                "system".into(),
                &|| Group {
                    id: "system".into(),
                    label: "macOS and system services".into(),
                    category: Category::System,
                    bytes: 0,
                    pids: vec![],
                    bundle_id: None,
                    quittable: false,
                    reclaimable_by_goose: false,
                    protected_why: None,
                    used_by: vec![],
                    launchd_label: None,
                    nuclear_default: false,
                    why: Some("part of macOS".into()),
                },
                row,
            );
            continue;
        }
        if let Some((key, label, bid)) = ai_member.get(&row.pid).cloned() {
            let id = format!("ai:{key}");
            add(
                id.clone(),
                &|| Group {
                    id: id.clone(),
                    label: label.clone(),
                    category: Category::AiServer,
                    bytes: 0,
                    pids: vec![],
                    bundle_id: bid.clone(),
                    quittable: false,
                    reclaimable_by_goose: false,
                    protected_why: None,
                    used_by: vec![],
                    launchd_label: None,
                    nuclear_default: false,
                    why: Some("goose cannot unload this server's models yet".into()),
                },
                row,
            );
            continue;
        }
        if let Some(app) = &app {
            let id = match bundle_id {
                Some(b) => format!("app:{b}"),
                None => format!("app:{app}"),
            };
            if let Some(reason) = blocked {
                app_blocked.entry(id.clone()).or_insert(reason);
            }
            let label = bundle
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| basename(app).trim_end_matches(".app").to_string());
            let bid = bundle_id.map(str::to_string);
            add(
                id.clone(),
                &|| Group {
                    id: id.clone(),
                    label: label.clone(),
                    category: Category::UserApp,
                    bytes: 0,
                    pids: vec![],
                    bundle_id: bid.clone(),
                    // Settled after the fold: an app needs a bundle id to be asked to quit.
                    quittable: bid.is_some(),
                    reclaimable_by_goose: false,
                    protected_why: None,
                    used_by: vec![],
                    launchd_label: None,
                    nuclear_default: false,
                    why: None,
                },
                row,
            );
            continue;
        }
        // Loose user processes: builds and background work, by name. Never quit by goose.
        let id = format!("job:{name}");
        let label = if JOB_NAMES.contains(&name.as_str()) {
            format!("{name} (build)")
        } else {
            name.clone()
        };
        let why = blocked.map(str::to_string);
        add(
            id.clone(),
            &|| Group {
                id: id.clone(),
                label: label.clone(),
                category: Category::Job,
                bytes: 0,
                pids: vec![],
                bundle_id: None,
                quittable: false,
                reclaimable_by_goose: false,
                protected_why: None,
                used_by: vec![],
                launchd_label: None,
                nuclear_default: false,
                why: why.clone(),
            },
            row,
        );
    }

    let exempt: Vec<u32> = ctx
        .own_engine_pids
        .iter()
        .chain(&ctx.orphan_goosed_pids)
        .copied()
        .collect();
    let shield = crate::memfree_protect::protected(&crate::memfree_protect::ProtectInput {
        rows,
        cmds: &ctx.cmdlines,
        labels: &ctx.launchd_labels,
        bundles: &ctx.bundles,
        self_pid: ctx.self_pid,
        self_uid: ctx.self_uid,
        exempt_roots: &exempt,
    });
    let owner = crate::memfree_protect::launchd_owner(rows, &ctx.launchd_labels);
    let mut out: Vec<Group> = groups
        .into_values()
        .map(|mut g| {
            g.launchd_label = g.pids.iter().find_map(|(p, _)| owner.get(p).cloned());
            // The reason shown is its BIGGEST protected pid's (hash order once named the whole
            // macOS group "started over ssh" after one small sshd child).
            let biggest = g
                .pids
                .iter()
                .filter_map(|(p, bytes)| shield.get(p).map(|why| (*bytes, *p, why)))
                .max_by_key(|(bytes, pid, _)| (*bytes, std::cmp::Reverse(*pid)));
            if let Some((_, _, reason)) = biggest {
                // macOS is protected for being macOS, whichever rule saw its pids first.
                let reason = if g.category == Category::System {
                    "part of macOS".to_string()
                } else {
                    reason.clone()
                };
                g.protected_why = Some(reason.clone());
                g.quittable = false;
                g.reclaimable_by_goose = false;
                if g.why.is_none() {
                    g.why = Some(reason.clone());
                }
            }
            if g.category == Category::UserApp {
                if let Some(reason) = app_blocked.get(&g.id) {
                    g.quittable = false;
                    g.why = Some((*reason).to_string());
                } else if g.bundle_id.is_none() {
                    g.why = Some("it has no bundle id to ask it to quit".into());
                }
            }
            g.nuclear_default = crate::memfree_protect::eligible(
                &g,
                crate::memfree_protect::Mode::Nuclear,
                &ctx.never_touch,
            );
            g
        })
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.id.cmp(&b.id)));
    out
}

/// Does the busy-holder record forbid unmounting the own engine? `Some(why)` when it does.
/// Unreadable is busy: an unknown holder is never "idle".
pub fn holders_busy(entries: &[HolderEntry]) -> Option<String> {
    for entry in entries {
        match entry {
            HolderEntry::Live(record) => match &record.kind {
                HolderKind::SwarmRun { what, .. } => return Some(format!("in use by {what}")),
                HolderKind::Goosed { replies } => {
                    if replies.iter().any(|r| !r.waiting) {
                        return Some("a chat reply is using it".into());
                    }
                }
            },
            HolderEntry::Stale { .. } => {}
            HolderEntry::Unreadable { path, .. } => {
                return Some(format!(
                    "a holder record is unreadable ({})",
                    path.display()
                ));
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerdictKind {
    FitsByGooseOwned,
    FitsWithApps,
    CannotFit,
}

impl VerdictKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            VerdictKind::FitsByGooseOwned => "fitsByGooseOwned",
            VerdictKind::FitsWithApps => "fitsWithApps",
            VerdictKind::CannotFit => "cannotFit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub kind: VerdictKind,
    /// cannotFit only: what is still missing after freeing everything; 0 otherwise.
    pub remaining_bytes: u64,
    pub apps_to_quit: Vec<String>,
}

/// The honest answer to a short fit.
///
/// `short` is the RAM-side gap (need − (available − margin)); `ceiling_short` is what the need
/// exceeds the GPU ceiling by once goose's own engines are stopped — no app quit can move that.
/// `goose_owned` must NOT include the own engine (the fit already credits it). `apps` are the
/// quittable apps (bundle id, footprint); `fitsWithApps` names the fewest, largest first.
pub fn verdict(
    short: u64,
    ceiling_short: u64,
    goose_owned: u64,
    apps: &[(String, u64)],
) -> Verdict {
    let mut sorted: Vec<&(String, u64)> = apps.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let apps_total: u64 = sorted.iter().map(|a| a.1).sum();
    let ram_remaining = short.saturating_sub(goose_owned).saturating_sub(apps_total);
    if ceiling_short > 0 || ram_remaining > 0 {
        return Verdict {
            kind: VerdictKind::CannotFit,
            remaining_bytes: ram_remaining.max(ceiling_short),
            apps_to_quit: sorted.iter().map(|a| a.0.clone()).collect(),
        };
    }
    if goose_owned >= short {
        return Verdict {
            kind: VerdictKind::FitsByGooseOwned,
            remaining_bytes: 0,
            apps_to_quit: vec![],
        };
    }
    let mut gap = short - goose_owned;
    let mut pick = vec![];
    for (bid, bytes) in sorted {
        if gap == 0 {
            break;
        }
        pick.push(bid.clone());
        gap = gap.saturating_sub(*bytes);
    }
    Verdict {
        kind: VerdictKind::FitsWithApps,
        remaining_bytes: 0,
        apps_to_quit: pick,
    }
}

fn is_variant_token(t: &str) -> bool {
    let t = t.to_ascii_lowercase();
    let digits_then = |s: &str, suffix: &str| {
        s.strip_suffix(suffix)
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '.'))
    };
    // Quantisation: 4bit, 8bit, bf16, fp16, f16, mxfp4, nvfp4, q4, dwq, mlx.
    if digits_then(&t, "bit")
        || [
            "bf16", "fp16", "f16", "fp8", "mxfp4", "mxfp8", "nvfp4", "dwq", "mlx",
        ]
        .contains(&t.as_str())
        || (t.starts_with('q') && t.len() > 1 && t[1..].chars().all(|c| c.is_ascii_digit()))
    {
        return true;
    }
    // Size: 27b, 0.6b, 350m, and MoE actives a3b.
    if digits_then(&t, "b") || digits_then(&t, "m") {
        return true;
    }
    t.strip_prefix('a').is_some_and(|r| digits_then(r, "b"))
}

/// A model's family: its name without org, quantisation and size tokens
/// (`mlx-community/Qwen3-30B-A3B-4bit` → `qwen3`).
pub fn family_key(model_id: &str) -> String {
    let name = model_id.rsplit('/').next().unwrap_or(model_id);
    name.split(['-', '_'])
        .filter(|t| !t.is_empty() && !is_variant_token(t))
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

/// Smaller local models of the same family whose need (weights + the target's KV charge) fits
/// `room` (the budget once everything reclaimable is freed), largest first.
pub fn smaller_variants(
    target: &str,
    target_weights: u64,
    kv_bytes: u64,
    room: u64,
    models: &[(String, u64)],
) -> Vec<(String, u64)> {
    let family = family_key(target);
    if family.is_empty() {
        return vec![];
    }
    let mut out: Vec<(String, u64)> = models
        .iter()
        .filter(|(id, size)| {
            id != target
                && *size > 0
                && *size < target_weights
                && size.saturating_add(kv_bytes) <= room
                && family_key(id) == family
        })
        .map(|(id, size)| (id.clone(), size.saturating_add(kv_bytes)))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// A bundle id safe to put inside an AppleScript string literal.
pub fn valid_bundle_id(bid: &str) -> bool {
    !bid.is_empty()
        && bid.len() <= 255
        && bid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

// ---------------------------------------------------------------------------------------------
// Kernel reads
// ---------------------------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod mac {
    use super::ProcRow;
    use std::ffi::c_void;

    extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buf: *mut c_void) -> i32;
        fn proc_listallpids(buf: *mut c_void, size: i32) -> i32;
        fn proc_pidpath(pid: i32, buf: *mut c_void, size: u32) -> i32;
    }

    const RUSAGE_INFO_V4: i32 = 4;

    pub fn footprint(pid: u32) -> Option<u64> {
        let pid = i32::try_from(pid).ok()?;
        let mut buf = [0u8; 512];
        // SAFETY: a 512-byte buffer exceeds sizeof(rusage_info_v4).
        let rc = unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V4, buf.as_mut_ptr() as *mut c_void) };
        if rc != 0 {
            return None;
        }
        Some(u64::from_le_bytes(buf[72..80].try_into().ok()?))
    }

    /// `sys/proc_info.h`: the process has a controlling terminal.
    const PROC_FLAG_CONTROLT: u32 = 0x2;

    /// (ppid, uid, started_at, pgid, has_tty)
    pub fn bsd(pid: u32) -> Option<(u32, u32, u64, u32, bool)> {
        let ipid = libc::c_int::try_from(pid).ok().filter(|p| *p > 0)?;
        // SAFETY: zeroed plain-old-data struct, size passed to the kernel.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let written = unsafe {
            libc::proc_pidinfo(
                ipid,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut libc::proc_bsdinfo as *mut c_void,
                size,
            )
        };
        (written == size).then_some((
            info.pbi_ppid,
            info.pbi_uid,
            info.pbi_start_tvsec,
            info.pbi_pgid,
            info.pbi_flags & PROC_FLAG_CONTROLT != 0,
        ))
    }

    pub fn path(pid: u32) -> String {
        let Ok(ipid) = i32::try_from(pid) else {
            return String::new();
        };
        let mut buf = vec![0u8; 4096];
        // SAFETY: buffer of the size passed.
        let n = unsafe { proc_pidpath(ipid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
        if n <= 0 {
            return String::new();
        }
        buf.truncate(n as usize);
        String::from_utf8_lossy(&buf).into_owned()
    }

    pub fn census() -> Vec<ProcRow> {
        let mut pids = vec![0i32; 16384];
        let bytes = (pids.len() * std::mem::size_of::<i32>()) as i32;
        // SAFETY: buffer of `bytes` bytes.
        let n = unsafe { proc_listallpids(pids.as_mut_ptr() as *mut c_void, bytes) };
        if n <= 0 {
            return vec![];
        }
        pids.truncate(n as usize);
        pids.into_iter()
            .filter(|p| *p > 0)
            .filter_map(|p| {
                let pid = p as u32;
                let (ppid, uid, started_at, pgid, has_tty) = bsd(pid)?;
                Some(ProcRow {
                    pid,
                    ppid,
                    uid,
                    started_at,
                    path: path(pid),
                    footprint: footprint(pid).unwrap_or(0),
                    pgid,
                    has_tty,
                })
            })
            .collect()
    }

    pub fn bundle_of(app: &str) -> Option<(String, String)> {
        let value = plist::Value::from_file(format!("{app}/Contents/Info.plist")).ok()?;
        let dict = value.as_dictionary()?;
        let bid = dict.get("CFBundleIdentifier")?.as_string()?.to_string();
        let name = ["CFBundleDisplayName", "CFBundleName"]
            .iter()
            .find_map(|k| dict.get(k).and_then(|v| v.as_string()))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| super::basename(app).trim_end_matches(".app").to_string());
        Some((bid, name))
    }
}

/// Every process the kernel lists, with its footprint. Empty off macOS.
pub fn census() -> Vec<ProcRow> {
    #[cfg(target_os = "macos")]
    return mac::census();
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

/// One process's physical footprint, or `None` when it is gone/unreadable.
pub fn footprint(pid: u32) -> Option<u64> {
    #[cfg(target_os = "macos")]
    return mac::footprint(pid);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// Is the process that started at `started_at` still alive under `pid`?
pub fn alive(pid: u32, started_at: u64) -> bool {
    #[cfg(target_os = "macos")]
    return mac::bsd(pid).is_some_and(|(_, _, s, _, _)| s == started_at);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, started_at);
        false
    }
}

/// Read every outermost `.app`'s bundle id and name for the rows given.
pub fn bundles_for(rows: &[ProcRow]) -> HashMap<String, (String, String)> {
    let mut out = HashMap::new();
    #[cfg(target_os = "macos")]
    for row in rows {
        if let Some(app) = outer_app(&row.path) {
            if !out.contains_key(app) {
                if let Some(b) = mac::bundle_of(app) {
                    out.insert(app.to_string(), b);
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = rows;
    out
}

/// This process's uid.
pub fn self_uid() -> u32 {
    #[cfg(unix)]
    // SAFETY: getuid has no failure mode.
    return unsafe { libc::getuid() };
    #[cfg(not(unix))]
    0
}

/// Census + bundles + command lines + launchd labels + categorize in one go (blocking; call from
/// `spawn_blocking`). `ctx.never_touch` is unioned with the default list.
pub fn groups_now(mut ctx: CategorizeCtx) -> Vec<Group> {
    let rows = census();
    ctx.bundles = bundles_for(&rows);
    ctx.cmdlines = crate::memfree_protect::all_cmdlines();
    ctx.launchd_labels = crate::memfree_protect::launchd_labels();
    ctx.never_touch = crate::memfree_protect::never_touch_list(&ctx.never_touch);
    ctx.orphan_goosed_pids =
        crate::memfree_servers::orphaned_goosed_now(&rows, ctx.self_pid, ctx.self_uid)
            .into_iter()
            .map(|(pid, _)| pid)
            .collect();
    categorize(&rows, &ctx)
}

/// Ask an app to quit — an Apple Event (`tell application id … to quit`), never a signal. The
/// app is looked up in a FRESH census and must be a quittable user app; it may refuse or show a
/// save dialog, which is reported, not forced. Freed bytes are measured: the footprint the app's
/// processes held before minus what is still alive after a short wait.
pub async fn quit_app(bundle_id: &str, ctx: CategorizeCtx) -> ReclaimOutcome {
    let id = format!("app:{bundle_id}");
    let refuse = |label: String, message: String| ReclaimOutcome {
        id: id.clone(),
        label,
        ok: false,
        freed_bytes: None,
        message,
    };
    if !valid_bundle_id(bundle_id) {
        return refuse(
            bundle_id.to_string(),
            format!("`{bundle_id}` is not a bundle id"),
        );
    }
    let groups = match tokio::task::spawn_blocking(move || groups_now(ctx)).await {
        Ok(g) => g,
        Err(e) => return refuse(bundle_id.to_string(), format!("the census failed: {e}")),
    };
    let Some(group) = groups.into_iter().find(|g| g.id == id) else {
        return refuse(bundle_id.to_string(), format!("{bundle_id} is not running"));
    };
    if group.category != Category::UserApp || !group.quittable {
        let why = group.why.clone().unwrap_or_else(|| "not a user app".into());
        return refuse(
            group.label.clone(),
            format!("goose will not quit {}: {why}", group.label),
        );
    }
    let before = group.bytes;
    let script = format!("tell application id \"{bundle_id}\" to quit");
    let run = tokio::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&script)
        .output();
    match tokio::time::timeout(Duration::from_secs(15), run).await {
        Ok(Ok(out)) if out.status.success() => {}
        Ok(Ok(out)) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return refuse(
                group.label.clone(),
                format!("{} did not accept quit: {err}", group.label),
            );
        }
        Ok(Err(e)) => return refuse(group.label.clone(), format!("osascript failed: {e}")),
        Err(_) => {
            return refuse(
                group.label.clone(),
                format!(
                    "{} did not answer the quit request within 15 s",
                    group.label
                ),
            )
        }
    }
    let pids = group.pids.clone();
    let mut still: Vec<(u32, u64)> = pids.clone();
    for _ in 0..32 {
        still.retain(|(p, s)| alive(*p, *s));
        if still.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let left: u64 = still.iter().filter_map(|(p, _)| footprint(*p)).sum();
    let freed = before.saturating_sub(left);
    if still.is_empty() {
        ReclaimOutcome {
            id,
            label: group.label.clone(),
            ok: true,
            freed_bytes: Some(freed),
            message: format!("{} quit, {} freed", group.label, gb(freed)),
        }
    } else {
        ReclaimOutcome {
            id,
            label: group.label.clone(),
            ok: false,
            freed_bytes: Some(freed),
            message: format!(
                "{} was asked to quit but is still running (it may be asking to save)",
                group.label
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    fn row(pid: u32, ppid: u32, uid: u32, path: &str, footprint: u64) -> ProcRow {
        ProcRow {
            pid,
            ppid,
            uid,
            started_at: 1000 + pid as u64,
            path: path.into(),
            footprint,
            pgid: pid,
            has_tty: false,
        }
    }

    fn ctx(rows: &[(&str, &str, &str)]) -> CategorizeCtx {
        CategorizeCtx {
            self_pid: 500,
            self_uid: 501,
            bundles: rows
                .iter()
                .map(|(p, b, n)| (p.to_string(), (b.to_string(), n.to_string())))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn lm_studio_counts_its_runtime_outside_the_app() {
        // Laptop 2026-10-25: the model lived in ~/.lmstudio/.internal/utils/node (child of the
        // app) and a worker under it; the .app's own processes held a fraction of it.
        let rows = vec![
            row(1, 0, 0, "/sbin/launchd", 0),
            row(1628, 1, 501, "/Applications/LM Studio.app/Contents/MacOS/LM Studio", GB / 2),
            row(
                3540,
                1628,
                501,
                "/Applications/LM Studio.app/Contents/Frameworks/LM Studio Helper (GPU).app/Contents/MacOS/LM Studio Helper (GPU)",
                GB / 10,
            ),
            row(3542, 1628, 501, "/Users/me/.lmstudio/.internal/utils/node", 2 * GB),
            row(3600, 3542, 501, "/Users/me/.lmstudio/extensions/backends/x/worker", GB),
            // headless `lms` server with no app parent: its own group, still LM Studio
            row(4000, 1, 501, "/Users/me/.lmstudio/llmster/node", 3 * GB),
            // a plain node of another tool stays a job
            row(4100, 1, 501, "/opt/homebrew/bin/node", GB),
        ];
        let c = ctx(&[(
            "/Applications/LM Studio.app",
            "ai.elementlabs.lmstudio",
            "LM Studio",
        )]);
        let groups = categorize(&rows, &c);
        let by = |id: &str| {
            groups
                .iter()
                .find(|g| g.id == id)
                .unwrap_or_else(|| panic!("{id}: {groups:?}"))
        };
        let app = by("ai:/Applications/LM Studio.app");
        assert_eq!(app.category, Category::AiServer);
        assert_eq!(app.bytes, GB / 2 + GB / 10 + 2 * GB + GB);
        assert_eq!(app.pids.len(), 4);
        assert_eq!(app.bundle_id.as_deref(), Some("ai.elementlabs.lmstudio"));
        let headless = by("ai:lms");
        assert_eq!(
            (headless.bytes, headless.label.as_str()),
            (3 * GB, "LM Studio")
        );
        assert_eq!(
            crate::memfree_servers::ai_server_kind(&headless.id, None),
            Some(crate::memfree_servers::AiServerKind::LmStudio)
        );
        assert_eq!(by("job:node").category, Category::Job);
    }

    #[test]
    fn outer_app_folds_helpers() {
        let cases = [
            (
                "/Applications/Google Chrome.app/Contents/Frameworks/G.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/x",
                Some("/Applications/Google Chrome.app"),
            ),
            ("/Applications/Slack.app/Contents/MacOS/Slack", Some("/Applications/Slack.app")),
            ("/Users/me/.cargo/bin/cargo", None),
            ("", None),
        ];
        for (path, want) in cases {
            assert_eq!(outer_app(path), want, "{path}");
        }
    }

    #[test]
    fn categorize_table() {
        let rows = vec![
            row(1, 0, 0, "/sbin/launchd", 0),
            row(90, 1, 88, "/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer", 2 * GB),
            row(100, 1, 501, "/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder", GB / 5),
            row(200, 1, 501, "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", GB),
            row(201, 200, 501, "/Applications/Google Chrome.app/Contents/Frameworks/X.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)", 3 * GB),
            row(300, 1, 501, "/Applications/Goose.app/Contents/MacOS/Goose", GB / 2),
            row(500, 300, 501, "/Applications/Goose.app/Contents/Resources/bin/goosed", GB / 4),
            row(510, 500, 501, "/Users/g/.goose/venv/bin/python3.12", 20 * GB),
            row(520, 1, 501, "/Users/g/other/venv/bin/python3", 6 * GB),
            row(600, 1, 501, "/Applications/iTerm.app/Contents/MacOS/iTerm2", GB / 3),
            row(601, 600, 501, "/bin/zsh", GB / 100),
            row(602, 601, 501, "/Users/g/.local/bin/claude", GB / 2),
            row(700, 1, 501, "/Users/g/.cargo/bin/cargo", GB),
            row(701, 700, 501, "/Users/g/.rustup/toolchains/x/bin/rustc", 2 * GB),
            row(800, 1, 501, "/Applications/Ollama.app/Contents/Resources/ollama", 5 * GB),
            row(900, 1, 501, "/Applications/Goose.app/Contents/Resources/bin/goosed", GB),
            row(950, 1, 502, "/Applications/Slack.app/Contents/MacOS/Slack", GB),
            row(960, 1, 501, "/Applications/NoPlist.app/Contents/MacOS/NoPlist", GB),
        ];
        let mut c = ctx(&[
            (
                "/Applications/Google Chrome.app",
                "com.google.Chrome",
                "Google Chrome",
            ),
            (
                "/Applications/Goose.app",
                "net.leanzero.goose-swarm",
                "Goose",
            ),
            ("/Applications/iTerm.app", "com.googlecode.iterm2", "iTerm"),
            ("/Applications/Ollama.app", "com.electron.ollama", "Ollama"),
            (
                "/Applications/Slack.app",
                "com.tinyspeck.slackmacgap",
                "Slack",
            ),
            (
                "/System/Library/CoreServices/Finder.app",
                "com.apple.finder",
                "Finder",
            ),
        ]);
        c.own_engine_pids = vec![510];
        c.own_engine_model = Some("Qwen3-30B".into());
        c.other_engine_pids = vec![520];
        let groups = categorize(&rows, &c);
        let by = |id: &str| {
            groups
                .iter()
                .find(|g| g.id == id)
                .unwrap_or_else(|| panic!("{id}: {groups:?}"))
        };

        let chrome = by("app:com.google.Chrome");
        assert_eq!(
            (chrome.category, chrome.bytes, chrome.quittable),
            (Category::UserApp, 4 * GB, true)
        );
        assert_eq!(chrome.pids.len(), 2);
        let goose = by("app:net.leanzero.goose-swarm");
        assert!(!goose.quittable);
        assert_eq!(goose.why.as_deref(), Some("runs goose or a coding session"));
        let iterm = by("app:com.googlecode.iterm2");
        assert!(!iterm.quittable, "a terminal hosting claude is never quit");
        let eng = by("gooseEngine");
        assert_eq!(
            (eng.category, eng.bytes, eng.reclaimable_by_goose),
            (Category::GooseEngine, 20 * GB, true)
        );
        let other = by("mlx:520");
        assert!(other.reclaimable_by_goose && !other.quittable);
        assert_eq!(by("job:cargo").category, Category::Job);
        assert_eq!(by("job:rustc").category, Category::Job);
        let ollama = by("ai:/Applications/Ollama.app");
        assert_eq!(
            (ollama.category, ollama.quittable),
            (Category::AiServer, false)
        );
        let orphan = by("orphan:900");
        assert_eq!(orphan.category, Category::GooseOrphan);
        // Not proven this install's desktop server: named, never stopped.
        assert!(!orphan.reclaimable_by_goose && orphan.why.is_some());
        let system = by("system");
        // launchd, WindowServer (other uid), Finder, zsh (/bin), Slack (other uid)
        assert_eq!(system.pids.len(), 5);
        assert!(groups
            .iter()
            .all(|g| !g.id.contains("Slack") && !g.id.contains("finder")));
        let noplist = by("app:/Applications/NoPlist.app");
        assert!(!noplist.quittable);
        // Largest first.
        assert!(groups.windows(2).all(|w| w[0].bytes >= w[1].bytes));
    }

    #[test]
    fn engine_child_processes_count_as_the_engine() {
        // Live shape: status pid = the uv launcher, the weights sit in its python child.
        let rows = vec![
            row(510, 1, 501, "/opt/homebrew/bin/uv", 66 * 1024 * 1024),
            row(
                511,
                510,
                501,
                "/u/.cache/uv/archive-v0/x/bin/python",
                5 * GB,
            ),
            row(512, 511, 501, "/u/.cache/uv/archive-v0/x/bin/python", GB),
            row(600, 1, 501, "/opt/homebrew/bin/python3.12", 2 * GB),
        ];
        let mut c = ctx(&[]);
        c.own_engine_pids = vec![510];
        let g = categorize(&rows, &c);
        let engine = g.iter().find(|g| g.id == "gooseEngine").unwrap();
        assert_eq!(engine.bytes, 6 * GB + 66 * 1024 * 1024);
        assert_eq!(engine.pids.len(), 3);
        assert!(g
            .iter()
            .any(|g| g.id == "job:python3.12" && g.bytes == 2 * GB));
    }

    #[test]
    fn version_named_binaries_take_their_tool_name() {
        assert_eq!(
            process_name("/Users/m/.local/share/claude/versions/2.1.299"),
            "claude"
        );
        assert_eq!(process_name("/usr/local/bin/node"), "node");
        assert_eq!(process_name("/opt/x/bin/python3.12"), "python3.12");
        assert_eq!(process_name("/opt/tool/1.2/bin/9.9"), "tool");
        assert_eq!(process_name("2.1.0"), "2.1.0");
        let rows = vec![
            row(
                700,
                1,
                501,
                "/Users/m/.local/share/claude/versions/2.1.299",
                GB,
            ),
            row(
                701,
                1,
                501,
                "/Users/m/.local/share/claude/versions/2.1.308",
                GB,
            ),
        ];
        let g = categorize(&rows, &ctx(&[]));
        assert_eq!(g.len(), 1);
        assert_eq!((g[0].id.as_str(), g[0].bytes), ("job:claude", 2 * GB));
    }

    #[test]
    fn busy_engine_is_not_reclaimable() {
        let rows = vec![row(510, 1, 501, "/v/python", 5 * GB)];
        let mut c = ctx(&[]);
        c.own_engine_pids = vec![510];
        c.own_engine_busy = Some("a chat reply is using it".into());
        let g = categorize(&rows, &c);
        assert!(!g[0].reclaimable_by_goose);
        assert_eq!(g[0].why.as_deref(), Some("a chat reply is using it"));
    }

    #[test]
    fn never_kill_table() {
        let protected: HashSet<u32> = [500, 300].into_iter().collect();
        let cases: Vec<(ProcRow, Option<&str>, bool)> = vec![
            (row(1, 0, 0, "/sbin/launchd", 0), None, true),
            (
                row(2, 1, 502, "/Applications/Slack.app/Contents/MacOS/Slack", 0),
                Some("com.tinyspeck.slackmacgap"),
                true,
            ),
            (row(3, 1, 501, "/usr/libexec/x", 0), None, true),
            (row(4, 1, 501, "/Library/Apple/System/x", 0), None, true),
            (
                row(5, 1, 501, "/Users/g/Library/WindowServer", 0),
                None,
                true,
            ),
            (row(6, 1, 501, "/Users/g/bin/Dock", 0), None, true),
            (
                row(300, 1, 501, "/Applications/X.app/Contents/MacOS/X", 0),
                Some("com.x"),
                true,
            ),
            (
                row(7, 1, 501, "/Applications/Goose.app/Contents/MacOS/Goose", 0),
                Some("net.leanzero.goose-swarm"),
                true,
            ),
            (
                row(
                    8,
                    1,
                    501,
                    "/Applications/Ghostty.app/Contents/MacOS/ghostty",
                    0,
                ),
                Some("com.mitchellh.ghostty"),
                true,
            ),
            (
                row(
                    9,
                    1,
                    501,
                    "/Applications/Safari.app/Contents/MacOS/Safari",
                    0,
                ),
                Some("com.apple.Safari"),
                true,
            ),
            (row(10, 1, 501, "", 0), None, true),
            (
                row(
                    11,
                    1,
                    501,
                    "/Applications/Slack.app/Contents/MacOS/Slack",
                    0,
                ),
                Some("com.tinyspeck.slackmacgap"),
                false,
            ),
        ];
        for (r, bid, want) in cases {
            assert_eq!(
                never_kill(&r, 501, &protected, bid).is_some(),
                want,
                "{r:?}"
            );
        }
    }

    #[test]
    fn ancestors_walk_and_stop_on_cycles() {
        let rows = vec![
            row(1, 0, 0, "a", 0),
            row(2, 1, 0, "b", 0),
            row(3, 2, 0, "c", 0),
            row(9, 9, 0, "self-loop", 0),
        ];
        assert_eq!(ancestors(3, &rows), [1, 2].into_iter().collect());
        assert!(ancestors(9, &rows).is_empty());
        assert!(ancestors(42, &rows).is_empty());
    }

    #[test]
    fn verdict_table() {
        let mb = 1_000_000u64;
        let apps = vec![
            ("com.a".to_string(), 10 * GB),
            ("com.b".to_string(), 5500 * mb),
        ];
        // Gabee fixture: need 30.6, budget 9.4 → short 21.2; goose-owned 0; apps 15.5 → cannot, 5.7.
        let v = verdict(21_200 * mb, 0, 0, &apps);
        assert_eq!(v.kind, VerdictKind::CannotFit);
        assert_eq!(v.remaining_bytes, 5_700 * mb);
        assert_eq!(v.apps_to_quit, vec!["com.a", "com.b"]);
        // Goose-owned alone closes it.
        let v = verdict(4 * GB, 0, 6 * GB, &apps);
        assert_eq!(
            (v.kind, v.remaining_bytes, v.apps_to_quit.len()),
            (VerdictKind::FitsByGooseOwned, 0, 0)
        );
        // Exactly equal counts as a fit.
        assert_eq!(
            verdict(6 * GB, 0, 6 * GB, &[]).kind,
            VerdictKind::FitsByGooseOwned
        );
        // Apps needed: the fewest, largest first.
        let v = verdict(12 * GB, 0, 3 * GB, &apps);
        assert_eq!(
            (v.kind, v.apps_to_quit.clone()),
            (VerdictKind::FitsWithApps, vec!["com.a".to_string()])
        );
        let v = verdict(14 * GB, 0, 0, &apps);
        assert_eq!(v.apps_to_quit, vec!["com.a", "com.b"]);
        // Exactly everything.
        assert_eq!(
            verdict(15_500 * mb, 0, 0, &apps).kind,
            VerdictKind::FitsWithApps
        );
        // No apps, nothing goose-owned.
        let v = verdict(GB, 0, 0, &[]);
        assert_eq!((v.kind, v.remaining_bytes), (VerdictKind::CannotFit, GB));
        // The GPU ceiling binds: no amount of quitting helps.
        let v = verdict(GB, 2 * GB, 20 * GB, &apps);
        assert_eq!(
            (v.kind, v.remaining_bytes),
            (VerdictKind::CannotFit, 2 * GB)
        );
        // Zero short (only the ceiling was the issue, and stopping engines cleared it).
        assert_eq!(verdict(0, 0, 0, &[]).kind, VerdictKind::FitsByGooseOwned);
    }

    #[test]
    fn family_and_smaller_variants() {
        assert_eq!(family_key("mlx-community/Qwen3-30B-A3B-4bit"), "qwen3");
        assert_eq!(family_key("mlx-community/Qwen3-8B-8bit"), "qwen3");
        assert_eq!(
            family_key("mlx-community/gemma-3-27b-it-4bit"),
            "gemma-3-it"
        );
        assert_eq!(
            family_key("mlx-community/gemma-3-12b-it-qat-bf16"),
            "gemma-3-it-qat"
        );
        assert_eq!(
            family_key("Llama-3.2-3B-Instruct-mxfp4"),
            "llama-3.2-instruct"
        );
        let models = vec![
            ("mlx-community/Qwen3-30B-A3B-8bit".to_string(), 32 * GB),
            ("mlx-community/Qwen3-30B-A3B-4bit".to_string(), 17 * GB),
            ("mlx-community/Qwen3-14B-4bit".to_string(), 8 * GB),
            ("mlx-community/Qwen3-0.6B-4bit".to_string(), GB / 2),
            ("mlx-community/gemma-3-4b-it-4bit".to_string(), 3 * GB),
            ("broken/empty".to_string(), 0),
        ];
        let got = smaller_variants(
            "mlx-community/Qwen3-30B-A3B-8bit",
            32 * GB,
            GB,
            12 * GB,
            &models,
        );
        assert_eq!(
            got,
            vec![
                ("mlx-community/Qwen3-14B-4bit".to_string(), 9 * GB),
                ("mlx-community/Qwen3-0.6B-4bit".to_string(), GB / 2 + GB),
            ]
        );
        assert!(smaller_variants("x/Unknown-9B", 9 * GB, 0, 100 * GB, &models).is_empty());
        assert!(smaller_variants(
            "mlx-community/Qwen3-30B-A3B-8bit",
            32 * GB,
            0,
            GB / 4,
            &models
        )
        .is_empty());
    }

    #[test]
    fn bundle_id_validation_blocks_script_injection() {
        assert!(valid_bundle_id("com.google.Chrome"));
        assert!(valid_bundle_id("com.todesktop.230313mzl4w4u92"));
        assert!(!valid_bundle_id(""));
        assert!(!valid_bundle_id("com.x\" to quit\ndo shell script \"rm"));
        assert!(!valid_bundle_id("a b"));
    }

    #[test]
    fn holders_busy_rules() {
        use crate::holders::{HolderRecord, ReplyHold};
        let goosed = |waiting: bool| {
            HolderEntry::Live(HolderRecord {
                pid: 1,
                started_at: 1,
                since: 1,
                kind: HolderKind::Goosed {
                    replies: vec![ReplyHold {
                        reply: 1,
                        session: "s".into(),
                        root_session: "s".into(),
                        way: None,
                        waiting,
                        kind: Default::default(),
                    }],
                },
            })
        };
        assert!(holders_busy(&[]).is_none());
        assert!(holders_busy(&[goosed(true)]).is_none());
        assert!(holders_busy(&[goosed(false)]).is_some());
        let unreadable = HolderEntry::Unreadable {
            path: "/x".into(),
            error: "bad".into(),
        };
        assert!(holders_busy(&[unreadable]).is_some());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn census_sees_this_process() {
        let me = std::process::id();
        let rows = census();
        let mine = rows
            .iter()
            .find(|r| r.pid == me)
            .expect("own pid in census");
        assert!(mine.footprint > 0);
        assert!(!mine.path.is_empty());
        assert_eq!(mine.uid, self_uid());
        assert!(alive(me, mine.started_at));
        assert!(!alive(me, mine.started_at + 1));
    }
}
