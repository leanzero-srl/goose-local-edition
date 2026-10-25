//! "Free up memory" safety: what goose must never touch, and what a clean may touch.
//!
//! [`protected`] is the pure rule that names every process another person, job or session is using
//! (pid → why); [`crate::memfree::categorize`] folds it into the groups so a protected group is
//! never quittable or reclaimable and carries `protected_why`. [`never_touch_hit`] matches the
//! owner's never-touch list. [`plan`] is the pure answer to "what would a clean free": SOFT is
//! goose's own idle engine and its proven left-over servers ONLY; NUCLEAR adds quittable apps,
//! model servers and other MLX engines, minus everything protected or never-touch.
//!
//! The runners ([`launchd_labels`], [`all_cmdlines`]) read only; nothing here signals anything.

use crate::memfree::{
    basename, is_goose_bundle, outer_app, process_name, with_descendants, Category, Group, ProcRow,
    HOSTS_SESSIONS, SYSTEM_NAMES, SYSTEM_PREFIXES,
};
use std::collections::{HashMap, HashSet};

/// Apps the owner keeps open while working: never quit by a clean, local or relayed. Matched
/// case-insensitively against a group's bundle id, app name or process name.
pub const DEFAULT_NEVER_TOUCH: &[&str] = &[
    "com.microsoft.teams2",
    "com.microsoft.teams",
    "com.microsoft.Outlook",
    "com.tinyspeck.slackmacgap",
    "us.zoom.xos",
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "dev.warp.Warp-Stable",
];

/// The default list plus `extra` (settings, the caller's list over the Link), deduplicated
/// case-insensitively, defaults first.
pub fn never_touch_list(extra: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in DEFAULT_NEVER_TOUCH
        .iter()
        .map(|s| s.to_string())
        .chain(extra.iter().map(|s| s.trim().to_string()))
    {
        if !item.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&item)) {
            out.push(item);
        }
    }
    out
}

/// The never-touch entry any of `names` matches, if one does: case-insensitive, trimmed, and a
/// name that CONTAINS the entry counts ("Teams" protects "Microsoft Teams", "com.microsoft"
/// every Microsoft bundle id). Matching more only ever protects more.
pub fn never_touch_hit<'a>(
    names: impl IntoIterator<Item = &'a str>,
    list: &[String],
) -> Option<String> {
    let names: Vec<String> = names
        .into_iter()
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty())
        .collect();
    list.iter()
        .find(|entry| {
            let entry = entry.trim().to_lowercase();
            !entry.is_empty() && names.iter().any(|n| n.contains(&entry))
        })
        .cloned()
}

/// The never-touch entry a group matches: its bundle id, its label (app name / process name), or
/// the name in its id (`app:<bundle or path>`, `job:<name>`, `ai:<key>`).
pub fn group_never_touch(group: &Group, list: &[String]) -> Option<String> {
    let id_name = group.id.split_once(':').map(|(_, n)| n).unwrap_or("");
    let app_name = id_name
        .ends_with(".app")
        .then(|| basename(id_name).trim_end_matches(".app"));
    let label_base = group.label.trim_end_matches(" (build)");
    never_touch_hit(
        [
            group.bundle_id.as_deref(),
            Some(label_base),
            Some(id_name),
            app_name,
        ]
        .into_iter()
        .flatten(),
        list,
    )
}

/// `launchctl list` output (`PID\tStatus\tLabel`) → pid → label. Not-running jobs (`-`) and GUI
/// app launches (`application.*`, every app the owner opened) are skipped: only real jobs remain.
pub fn parse_launchctl_list(out: &str) -> HashMap<u32, String> {
    out.lines()
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let pid = cols.next()?.trim().parse::<u32>().ok()?;
            let _status = cols.next()?;
            let label = cols.next()?.trim();
            (pid > 1 && !label.is_empty() && !label.starts_with("application."))
                .then(|| (pid, label.to_string()))
        })
        .collect()
}

/// This user's running launchd jobs (pid → label). Empty when `launchctl` cannot be run.
pub fn launchd_labels() -> HashMap<u32, String> {
    #[cfg(target_os = "macos")]
    {
        match std::process::Command::new("/bin/launchctl")
            .arg("list")
            .output()
        {
            Ok(out) if out.status.success() => {
                parse_launchctl_list(&String::from_utf8_lossy(&out.stdout))
            }
            _ => HashMap::new(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    HashMap::new()
}

/// Every process's command line (sysinfo; argv joined by spaces). Blocking.
#[cfg(unix)]
pub fn all_cmdlines() -> HashMap<u32, String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    sys.processes()
        .iter()
        .filter_map(|(pid, p)| {
            let cmd: Vec<String> = p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            (!cmd.is_empty()).then(|| (pid.as_u32(), cmd.join(" ")))
        })
        .collect()
}

#[cfg(not(unix))]
pub fn all_cmdlines() -> HashMap<u32, String> {
    HashMap::new()
}

/// Does this command line run an MCP server? Any argv token whose last path part names one
/// (`mcp-…`, `…-mcp`, `…_mcp`, `mcp_server`, `modelcontextprotocol`, or exactly `mcp`). A false
/// hit only protects a process, so the match is generous.
pub fn is_mcp_command(cmd: &str) -> bool {
    cmd.split_whitespace().any(|token| {
        token.split('/').any(|part| {
            let p = part.to_ascii_lowercase();
            p == "mcp"
                || p.starts_with("mcp-")
                || p.contains("-mcp")
                || p.contains("_mcp")
                || p.contains("mcp_server")
                || p.contains("modelcontextprotocol")
        })
    })
}

/// Is this browser/Electron process driven by a tool over the DevTools protocol? Started with
/// `--remote-debugging-port`, `--remote-debugging-pipe` or `--enable-automation` (Playwright,
/// Puppeteer, a CDP harness). The workhorse's mp-vendor-profile Chrome (2026-10-25) was a nuclear
/// default before this rule.
pub fn is_automation_browser(cmd: &str) -> bool {
    cmd.split_whitespace().any(|token| {
        token.starts_with("--remote-debugging-port")
            || token == "--remote-debugging-pipe"
            || token == "--enable-automation"
    })
}

/// Is this a heavy-run / node-run wrapper (the owner's job launchers)? By the basename of any argv
/// token, or of the executable.
fn is_job_wrapper(path: &str, cmd: Option<&str>) -> bool {
    let hit = |s: &str| matches!(basename(s), "heavy-run" | "node-run");
    hit(path) || cmd.is_some_and(|c| c.split_whitespace().any(hit))
}

/// pid → the launchd label that owns it: a labelled job, its descendants, every process in the
/// process group of any of those (a child reparented to launchd keeps its group), and their
/// descendants.
pub fn launchd_owner(rows: &[ProcRow], labels: &HashMap<u32, String>) -> HashMap<u32, String> {
    let mut out: HashMap<u32, String> = HashMap::new();
    let mut by_label: Vec<(&u32, &String)> = labels.iter().collect();
    by_label.sort();
    for (&root, label) in by_label {
        if !rows.iter().any(|r| r.pid == root) {
            continue;
        }
        for pid in tree_and_groups(&[root], rows) {
            out.entry(pid).or_insert_with(|| label.clone());
        }
    }
    out
}

/// `roots` + descendants + everyone sharing a process group with one of them (+ their
/// descendants). Groups 0 and 1 (launchd's) are never followed.
fn tree_and_groups(roots: &[u32], rows: &[ProcRow]) -> HashSet<u32> {
    let tree = with_descendants(roots, rows);
    let groups: HashSet<u32> = rows
        .iter()
        .filter(|r| tree.contains(&r.pid) && r.pgid > 1)
        .map(|r| r.pgid)
        .collect();
    let mut more: Vec<u32> = tree.iter().copied().collect();
    more.extend(
        rows.iter()
            .filter(|r| groups.contains(&r.pgid))
            .map(|r| r.pid),
    );
    with_descendants(&more, rows)
}

/// What [`protected`] reads.
#[derive(Debug, Clone, Copy)]
pub struct ProtectInput<'a> {
    pub rows: &'a [ProcRow],
    /// pid → command line ([`all_cmdlines`]).
    pub cmds: &'a HashMap<u32, String>,
    /// pid → launchd label ([`launchd_labels`]).
    pub labels: &'a HashMap<u32, String>,
    /// Outermost `.app` path → (bundle id, display name).
    pub bundles: &'a HashMap<String, (String, String)>,
    pub self_pid: u32,
    pub self_uid: u32,
    /// goose's own engine pids: this tree is goose's to free, so no rule but "another user's"
    /// protects it.
    pub exempt_roots: &'a [u32],
    /// goose's PROVEN left-over servers: exempt only from the session, "this is goose" and
    /// MCP-host rules (they ARE goose and host its MCP children); launchd, terminal, ssh, MCP,
    /// automation and heavy-run protection still apply to them.
    pub orphan_roots: &'a [u32],
}

/// pid → why nothing may quit, signal or unload it. First matching rule names the reason:
/// another user's; this goose and its ancestors; launchd jobs (+ descendants + their process
/// groups); MCP servers (+ descendants); browsers driven over remote debugging (+ descendants);
/// anything with a terminal (+ descendants); anything
/// started over ssh; claude/goose sessions (+ descendants + ancestors); heavy-run/node-run jobs
/// (+ descendants + their process groups); macOS; goose, Apple and terminal/editor apps.
pub fn protected(input: &ProtectInput) -> HashMap<u32, String> {
    let rows = input.rows;
    let exempt = with_descendants(input.exempt_roots, rows);
    let orphans = with_descendants(input.orphan_roots, rows);
    let mut out: HashMap<u32, String> = HashMap::new();
    // Another user's process: never ours to touch, exempt or not.
    for r in rows.iter().filter(|r| r.uid != input.self_uid) {
        out.insert(r.pid, "another user's process".into());
    }

    let mut mark = |pids: &mut dyn Iterator<Item = u32>, why: &dyn Fn(u32) -> String| {
        for pid in pids {
            if !exempt.contains(&pid) {
                out.entry(pid).or_insert_with(|| why(pid));
            }
        }
    };
    let cmd = |pid: u32| input.cmds.get(&pid).map(String::as_str);

    let mut me = crate::memfree::ancestors(input.self_pid, rows);
    me.insert(input.self_pid);
    mark(&mut me.into_iter(), &|_| "runs this goose".into());

    let owner = launchd_owner(rows, input.labels);
    mark(&mut owner.keys().copied(), &|pid| {
        format!("launchd job {}", owner[&pid])
    });

    let mcp: Vec<u32> = rows
        .iter()
        .filter(|r| cmd(r.pid).is_some_and(is_mcp_command) || is_mcp_command(&r.path))
        .map(|r| r.pid)
        .collect();
    mark(&mut with_descendants(&mcp, rows).into_iter(), &|_| {
        "an MCP server".into()
    });
    // An app that hosts MCP servers as children (an Electron coding app not in HOSTS_SESSIONS):
    // quitting it ends them. Every ancestor inside an `.app` bundle is protected; a proven
    // orphan goose hosts goose's own MCP children and stays goose's to free.
    let mut hosts: Vec<(u32, u32)> = vec![];
    for &m in &mcp {
        for a in crate::memfree::ancestors(m, rows) {
            let in_app = rows
                .iter()
                .find(|r| r.pid == a)
                .is_some_and(|r| outer_app(&r.path).is_some());
            if a > 1 && in_app && !orphans.contains(&a) {
                hosts.push((a, m));
            }
        }
    }
    let host_of: HashMap<u32, u32> = hosts.iter().copied().collect();
    mark(&mut hosts.iter().map(|h| h.0), &|pid| {
        format!("hosts an MCP server (pid {})", host_of[&pid])
    });

    let driven: Vec<u32> = rows
        .iter()
        .filter(|r| cmd(r.pid).is_some_and(is_automation_browser))
        .map(|r| r.pid)
        .collect();
    mark(&mut with_descendants(&driven, rows).into_iter(), &|_| {
        "a browser driven by automation (remote debugging)".into()
    });

    let tty: Vec<u32> = rows.iter().filter(|r| r.has_tty).map(|r| r.pid).collect();
    mark(&mut with_descendants(&tty, rows).into_iter(), &|_| {
        "runs in a terminal".into()
    });

    let ssh: Vec<u32> = rows
        .iter()
        .filter(|r| matches!(basename(&r.path), "sshd" | "sshd-session"))
        .map(|r| r.pid)
        .collect();
    mark(&mut with_descendants(&ssh, rows).into_iter(), &|_| {
        "started over ssh".into()
    });

    let sessions: Vec<u32> = rows
        .iter()
        .filter(|r| !exempt.contains(&r.pid) && !orphans.contains(&r.pid))
        .filter(|r| {
            matches!(
                process_name(&r.path).as_str(),
                "claude" | "goose" | "goosed"
            )
        })
        .map(|r| r.pid)
        .collect();
    let mut session_set = with_descendants(&sessions, rows);
    for &s in &sessions {
        session_set.extend(crate::memfree::ancestors(s, rows));
    }
    mark(
        &mut session_set.into_iter().filter(|p| !orphans.contains(p)),
        &|_| "part of a claude or goose session".into(),
    );

    let jobs: Vec<u32> = rows
        .iter()
        .filter(|r| is_job_wrapper(&r.path, cmd(r.pid)))
        .map(|r| r.pid)
        .collect();
    mark(&mut tree_and_groups(&jobs, rows).into_iter(), &|_| {
        "a heavy-run/node-run job".into()
    });

    for r in rows {
        let bundle_id = outer_app(&r.path)
            .and_then(|a| input.bundles.get(a))
            .map(|(b, _)| b.as_str());
        let why = if r.path.is_empty() {
            Some("its executable could not be read")
        } else if SYSTEM_PREFIXES.iter().any(|p| r.path.starts_with(p))
            || SYSTEM_NAMES.contains(&basename(&r.path))
            || bundle_id.is_some_and(|b| b.starts_with("com.apple."))
        {
            Some("part of macOS")
        } else if bundle_id.is_some_and(is_goose_bundle) {
            (!orphans.contains(&r.pid)).then_some("this is goose")
        } else if bundle_id.is_some_and(|b| HOSTS_SESSIONS.contains(&b)) {
            Some("hosts terminal or coding sessions")
        } else {
            None
        };
        if let Some(why) = why {
            mark(&mut std::iter::once(r.pid), &|_| why.to_string());
        }
    }
    out
}

/// Which clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// goose's own idle engine + its proven left-over servers. Nothing anyone else uses.
    Soft,
    /// Soft + quittable apps + model servers + other MLX engines, minus protected and never-touch.
    Nuclear,
}

fn soft_kind(g: &Group) -> bool {
    g.reclaimable_by_goose
        && ((g.category == Category::GooseEngine && g.id == "gooseEngine")
            || g.category == Category::GooseOrphan)
}

/// May a clean in `mode` free this group? Never when protected or on the never-touch list.
pub fn eligible(g: &Group, mode: Mode, never_touch: &[String]) -> bool {
    if g.protected_why.is_some() || group_never_touch(g, never_touch).is_some() {
        return false;
    }
    if soft_kind(g) {
        return true;
    }
    mode == Mode::Nuclear
        && match g.category {
            Category::UserApp => g.quittable,
            Category::AiServer => g.reclaimable_by_goose,
            Category::GooseEngine => g.id.starts_with("mlx:") && g.reclaimable_by_goose,
            _ => false,
        }
}

/// A model server or another MLX engine: something goose did not start (LM Studio, Ollama,
/// rapid-mlx, any other MLX server).
pub fn is_foreign_server(g: &Group) -> bool {
    g.category == Category::AiServer
        || (g.category == Category::GooseEngine && g.id.starts_with("mlx:"))
}

/// Is it ticked by default in the "free everything except protected" view? NEVER for a server
/// goose did not start ([`is_foreign_server`]): a client between requests or an LM Link peer
/// is invisible to a census (review C1/C2, 2026-10-25), so the person ticks those. Otherwise
/// eligible for nuclear AND nobody is using it: a server with a client connected (the workhorse's rapid-mlx 8093 had an
/// ssh port-forward on it, 2026-10-25), a parent or a label in `used_by` stays offered but
/// unticked. Explicit picks are re-validated with [`eligible`] alone.
pub fn nuclear_default(g: &Group, never_touch: &[String]) -> bool {
    !is_foreign_server(g) && eligible(g, Mode::Nuclear, never_touch) && g.used_by.is_empty()
}

/// The group ids a clean in `mode` frees, in the groups' order.
pub fn plan(groups: &[Group], mode: Mode, never_touch: &[String]) -> Vec<String> {
    groups
        .iter()
        .filter(|g| eligible(g, mode, never_touch))
        .map(|g| g.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memfree::{categorize, CategorizeCtx};

    const GB: u64 = 1_000_000_000;
    const UID: u32 = 501;

    struct Fx {
        rows: Vec<ProcRow>,
        cmds: HashMap<u32, String>,
        labels: HashMap<u32, String>,
        bundles: HashMap<String, (String, String)>,
    }

    impl Fx {
        fn new() -> Self {
            Fx {
                rows: vec![],
                cmds: HashMap::new(),
                labels: HashMap::new(),
                bundles: HashMap::new(),
            }
        }
        fn p(&mut self, pid: u32, ppid: u32, path: &str) -> &mut ProcRow {
            self.rows.push(ProcRow {
                pid,
                ppid,
                uid: UID,
                started_at: 1000 + pid as u64,
                path: path.into(),
                footprint: GB,
                pgid: pid,
                has_tty: false,
            });
            self.rows.last_mut().unwrap()
        }
        fn cmd(&mut self, pid: u32, cmd: &str) {
            self.cmds.insert(pid, cmd.into());
        }
        fn app(&mut self, app: &str, bid: &str, name: &str) {
            self.bundles.insert(app.into(), (bid.into(), name.into()));
        }
    }

    /// The workhorse dry run (2026-10-25), shaped into one census.
    fn workhorse() -> Fx {
        let mut f = Fx::new();
        f.p(1, 0, "/sbin/launchd").uid = 0;
        // This goose: the app, its `goose serve` (self 500), its own engine uv 600 → python 601.
        f.p(
            400,
            1,
            "/Applications/Goose Swarm.app/Contents/MacOS/Goose Swarm",
        );
        f.app(
            "/Applications/Goose Swarm.app",
            "net.leanzero.goose-swarm",
            "Goose Swarm",
        );
        f.p(
            500,
            400,
            "/Applications/Goose Swarm.app/Contents/Resources/bin/goose",
        );
        f.p(600, 500, "/Users/workhorse/.local/bin/uv");
        f.p(601, 600, "/Users/workhorse/.cache/uv/python3.12")
            .footprint = 5 * GB;
        // A PROVEN left-over desktop server and an unproven goosed.
        f.p(
            700,
            1,
            "/Applications/Goose Swarm.app/Contents/Resources/bin/goose",
        );
        f.p(701, 1, "/Users/workhorse/.local/bin/goosed");
        // launchd-labelled MCP server + its browser child.
        f.p(1200, 1, "/opt/homebrew/bin/node");
        f.cmd(
            1200,
            "node /Users/workhorse/Projects/mcp-web-search-upd/dist/index.js",
        );
        f.labels
            .insert(1200, "net.leanzero.mcp-web-search-upd".into());
        f.p(
            1201,
            1200,
            "/Users/workhorse/Library/Caches/ms-playwright/chrome",
        );
        // heavy-run job: bash 36740 owns the group; TTS python 9375 reparented to launchd, same
        // group; its child 9474.
        f.p(36740, 1, "/opt/homebrew/bin/bash");
        f.cmd(
            36740,
            "/opt/homebrew/bin/bash /Users/workhorse/Projects/machine-sync/bin/heavy-run -- python tts.py",
        );
        let tts = f.p(9375, 1, "/Users/workhorse/tts/.venv/bin/python3.12");
        tts.pgid = 36740;
        tts.footprint = 6 * GB;
        f.cmd(9375, "python tts.py");
        f.p(9474, 9375, "/Users/workhorse/tts/.venv/bin/python3.12")
            .pgid = 36740;
        // A claude session in a terminal-less shell + its MCP child.
        f.p(
            7000,
            1,
            "/Users/workhorse/.local/share/claude/versions/2.1.299",
        );
        f.p(7001, 7000, "/opt/homebrew/bin/node");
        f.cmd(
            7001,
            "node /x/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js",
        );
        // Teams (never-touch), a plain app, an app started from a terminal shell.
        f.p(
            8000,
            1,
            "/Applications/Microsoft Teams.app/Contents/MacOS/MSTeams",
        );
        f.app(
            "/Applications/Microsoft Teams.app",
            "com.microsoft.teams2",
            "Microsoft Teams",
        );
        f.p(9000, 1, "/Applications/Plain.app/Contents/MacOS/Plain");
        // Two Chromes: one a CDP harness drives, one the owner's. One bundle = one group.
        let chrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
        f.p(38975, 1, chrome);
        f.cmd(
            38975,
            &format!("{chrome} --user-data-dir=/Users/w/.local/share/mp-vendor-profile --remote-debugging-port=9333 about:blank"),
        );
        f.p(
            38986,
            38975,
            "/Applications/Google Chrome.app/Contents/Frameworks/Helper (GPU)",
        );
        f.p(63438, 1, chrome);
        f.cmd(63438, chrome);
        f.app(
            "/Applications/Google Chrome.app",
            "com.google.Chrome",
            "Google Chrome",
        );
        f.app("/Applications/Plain.app", "org.example.Plain", "Plain");
        f.p(6999, 1, "/opt/homebrew/bin/fish").has_tty = true;
        f.p(
            6100,
            6999,
            "/Applications/FromTerm.app/Contents/MacOS/FromTerm",
        );
        f.app(
            "/Applications/FromTerm.app",
            "org.example.FromTerm",
            "FromTerm",
        );
        // rapid-mlx other engine (port 8093), ppid 1, not goose's.
        f.p(97100, 1, "/Users/workhorse/rapid/.venv/bin/python3.12");
        f.cmd(97100, "python -m rapid_mlx serve --port 8093");
        // An ssh-launched worker (no tty: `ssh host cmd`).
        f.p(5000, 1, "/usr/libexec/sshd-session").uid = 0;
        f.p(5001, 5000, "/usr/libexec/sshd-session");
        f.p(5002, 5001, "/Users/workhorse/bin/worker");
        // LM Studio.
        f.p(
            13856,
            1,
            "/Applications/LM Studio.app/Contents/MacOS/LM Studio",
        );
        f.app(
            "/Applications/LM Studio.app",
            "ai.elementlabs.lmstudio",
            "LM Studio",
        );
        f
    }

    fn input<'a>(f: &'a Fx, exempt: &'a [u32], orphans: &'a [u32]) -> ProtectInput<'a> {
        ProtectInput {
            rows: &f.rows,
            cmds: &f.cmds,
            labels: &f.labels,
            bundles: &f.bundles,
            self_pid: 500,
            self_uid: UID,
            exempt_roots: exempt,
            orphan_roots: orphans,
        }
    }

    fn groups(f: &Fx) -> Vec<Group> {
        let ctx = CategorizeCtx {
            self_pid: 500,
            self_uid: UID,
            own_engine_pids: vec![600],
            other_engine_pids: vec![97100],
            orphan_goosed_pids: vec![700],
            bundles: f.bundles.clone(),
            cmdlines: f.cmds.clone(),
            launchd_labels: f.labels.clone(),
            never_touch: never_touch_list(&[]),
            ..Default::default()
        };
        let mut gs = categorize(&f.rows, &ctx);
        // The ACP marks a model server reclaimable once its API lists loaded models.
        for g in gs.iter_mut().filter(|g| g.category == Category::AiServer) {
            g.reclaimable_by_goose = true;
        }
        gs
    }

    #[test]
    fn launchctl_list_parse() {
        let out = "PID\tStatus\tLabel\n1200\t0\tnet.leanzero.mcp-web-search-upd\n-\t0\tnet.leanzero.idle\n\
                   4410\t0\tapplication.com.microsoft.teams2.123.456\n14200\t-15\tnet.leanzero.link\n\
                   garbage line\n";
        let m = parse_launchctl_list(out);
        assert_eq!(m.len(), 2);
        assert_eq!(m[&1200], "net.leanzero.mcp-web-search-upd");
        assert_eq!(m[&14200], "net.leanzero.link");
    }

    #[test]
    fn mcp_command_shapes() {
        for yes in [
            "node /Users/w/Projects/mcp-web-search-upd/dist/index.js",
            "npx -y @modelcontextprotocol/server-filesystem /tmp",
            "uvx serena_mcp",
            "python -m my_mcp_server",
            "/usr/local/bin/github-mcp-server stdio",
            "docker run mcp",
        ] {
            assert!(is_mcp_command(yes), "{yes}");
        }
        for no in ["python tts.py", "node server.js --port 3000", "/bin/zsh -l"] {
            assert!(!is_mcp_command(no), "{no}");
        }
    }

    #[test]
    fn protected_names_every_class() {
        let f = workhorse();
        let p = protected(&input(&f, &[600], &[700]));
        let why = |pid: u32| p.get(&pid).map(String::as_str);
        assert_eq!(why(500), Some("runs this goose"));
        assert_eq!(why(400), Some("runs this goose"));
        assert_eq!(
            why(1200),
            Some("launchd job net.leanzero.mcp-web-search-upd")
        );
        assert_eq!(
            why(1201),
            Some("launchd job net.leanzero.mcp-web-search-upd")
        );
        assert_eq!(why(36740), Some("a heavy-run/node-run job"));
        assert_eq!(
            why(9375),
            Some("a heavy-run/node-run job"),
            "same pgid, ppid 1"
        );
        assert_eq!(why(9474), Some("a heavy-run/node-run job"));
        assert_eq!(why(7000), Some("part of a claude or goose session"));
        assert_eq!(why(7001), Some("an MCP server"));
        assert_eq!(why(6999), Some("runs in a terminal"));
        assert_eq!(why(6100), Some("runs in a terminal"));
        assert_eq!(why(5002), Some("started over ssh"));
        let driven = Some("a browser driven by automation (remote debugging)");
        assert_eq!(why(38975), driven);
        assert_eq!(why(38986), driven, "its helper");
        assert_eq!(why(63438), None, "the owner's own Chrome is not driven");
        assert_eq!(why(5000), Some("another user's process"));
        assert_eq!(why(701), Some("part of a claude or goose session"));
        // goose's own engine and its proven orphan are goose's to free.
        for free in [600, 601, 700] {
            assert_eq!(why(free), None, "pid {free}");
        }
        // Not protected: plain apps, never-touch apps (that is a separate list), other engine.
        for free in [8000, 9000, 97100, 13856] {
            assert_eq!(why(free), None, "pid {free}");
        }
        // Without the exemption, the own engine is a goose session's child.
        let p = protected(&input(&f, &[], &[]));
        assert!(p.contains_key(&601) && p.contains_key(&700));
    }

    #[test]
    fn launchd_owner_follows_groups_not_launchd() {
        let mut f = Fx::new();
        f.p(10, 1, "/a/job");
        f.labels.insert(10, "com.x.job".into());
        f.p(11, 1, "/a/child").pgid = 10; // reparented, same group
        f.p(12, 11, "/a/grandchild").pgid = 12;
        f.p(13, 1, "/a/stranger").pgid = 1; // launchd's group: never followed
        f.labels.insert(99, "com.x.gone".into()); // labelled pid not in census
        let o = launchd_owner(&f.rows, &f.labels);
        assert_eq!(o.len(), 3);
        assert!(o.contains_key(&10) && o.contains_key(&11) && o.contains_key(&12));
    }

    #[test]
    fn never_touch_matching() {
        let list = never_touch_list(&["  Plain ".into(), "COM.MICROSOFT.TEAMS2".into()]);
        assert_eq!(
            list.len(),
            DEFAULT_NEVER_TOUCH.len() + 1,
            "dedup is case-insensitive"
        );
        assert_eq!(
            never_touch_hit(["com.Microsoft.Teams2"], &list).as_deref(),
            Some("com.microsoft.teams2")
        );
        assert_eq!(never_touch_hit(["plain"], &list).as_deref(), Some("Plain"));
        assert_eq!(never_touch_hit(["Plai", ""], &list), None);
        let gs = groups(&workhorse());
        let g = |id: &str| gs.iter().find(|g| g.id == id).unwrap().clone();
        assert!(group_never_touch(&g("app:com.microsoft.teams2"), &list).is_some());
        assert!(group_never_touch(&g("app:org.example.Plain"), &list).is_some());
        assert!(group_never_touch(&g("app:org.example.Plain"), &never_touch_list(&[])).is_none());
    }

    #[test]
    fn categorize_carries_protection() {
        let gs = groups(&workhorse());
        let g = |id: &str| {
            gs.iter().find(|g| g.id == id).unwrap_or_else(|| {
                panic!("{id} in {:?}", gs.iter().map(|g| &g.id).collect::<Vec<_>>())
            })
        };
        let tts = g("job:python3.12");
        assert_eq!(
            tts.protected_why.as_deref(),
            Some("a heavy-run/node-run job")
        );
        let from_term = g("app:org.example.FromTerm");
        assert!(!from_term.quittable);
        assert_eq!(
            from_term.protected_why.as_deref(),
            Some("runs in a terminal")
        );
        let mcp = g("job:node");
        assert!(mcp.protected_why.is_some());
        assert_eq!(
            mcp.launchd_label.as_deref(),
            Some("net.leanzero.mcp-web-search-upd")
        );
        let own = g("gooseEngine");
        assert!(own.reclaimable_by_goose && own.protected_why.is_none() && own.nuclear_default);
        assert!(g("orphan:700").reclaimable_by_goose);
        let unproven = g("orphan:701");
        assert!(!unproven.reclaimable_by_goose && unproven.protected_why.is_some());
        let plain = g("app:org.example.Plain");
        assert!(plain.quittable && plain.nuclear_default && plain.used_by.is_empty());
        assert!(
            !g("app:com.microsoft.teams2").nuclear_default,
            "never-touch"
        );
        assert!(
            !g("mlx:97100").nuclear_default,
            "an engine goose did not start is never pre-ticked"
        );
        // A bundle with one driven instance is protected whole: a quit names the bundle.
        assert_eq!(
            g("system").protected_why.as_deref(),
            Some("part of macOS"),
            "not the ssh child's reason"
        );
        let chrome = g("app:com.google.Chrome");
        assert!(!chrome.nuclear_default && !chrome.quittable);
        assert_eq!(
            chrome.protected_why.as_deref(),
            Some("a browser driven by automation (remote debugging)")
        );
    }

    #[test]
    fn a_server_in_use_is_offered_but_not_ticked() {
        let mut gs = groups(&workhorse());
        let nt = never_touch_list(&[]);
        let engine = gs.iter_mut().find(|g| g.id == "mlx:97100").unwrap();
        assert!(!nuclear_default(engine, &nt), "never pre-ticked, used or not");
        engine.used_by = vec!["sshd-session (pid 97112) is connected to it".into()];
        assert!(!nuclear_default(engine, &nt), "a client is connected");
        assert!(
            eligible(engine, Mode::Nuclear, &nt),
            "still offered and freeable when picked"
        );
    }

    #[test]
    fn automation_browser_shapes() {
        assert!(is_automation_browser("chrome --remote-debugging-port=9333"));
        assert!(is_automation_browser("chrome --remote-debugging-port 0"));
        assert!(is_automation_browser(
            "chromium --remote-debugging-pipe --headless"
        ));
        assert!(is_automation_browser("chrome --enable-automation"));
        assert!(!is_automation_browser(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
        ));
        assert!(!is_automation_browser("chrome --remote-allow-origins=*"));
    }

    #[test]
    fn soft_plan_is_goose_own_only() {
        let gs = groups(&workhorse());
        let soft = plan(&gs, Mode::Soft, &never_touch_list(&[]));
        assert_eq!(
            soft,
            vec!["gooseEngine".to_string(), "orphan:700".to_string()]
        );
        // Even with every server and engine marked reclaimable, soft never adds one.
        let mut all = gs.clone();
        for g in &mut all {
            g.reclaimable_by_goose = true;
            g.quittable = g.category == Category::UserApp;
        }
        let soft = plan(&all, Mode::Soft, &[]);
        for id in &soft {
            assert!(id == "gooseEngine" || id.starts_with("orphan:"), "{id}");
        }
        assert!(
            !soft.contains(&"orphan:701".to_string()),
            "unproven orphan is protected"
        );
        // A busy own engine is not freed.
        let mut busy = gs.clone();
        busy.iter_mut()
            .find(|g| g.id == "gooseEngine")
            .unwrap()
            .reclaimable_by_goose = false;
        assert_eq!(plan(&busy, Mode::Soft, &[]), vec!["orphan:700".to_string()]);
    }

    #[test]
    fn nuclear_plan_spares_protected_and_never_touch() {
        let gs = groups(&workhorse());
        let nt = never_touch_list(&[]);
        let nuke = plan(&gs, Mode::Nuclear, &nt);
        for want in [
            "gooseEngine",
            "orphan:700",
            "app:org.example.Plain",
            "mlx:97100",
            "ai:/Applications/LM Studio.app",
        ] {
            assert!(nuke.contains(&want.to_string()), "{want} in {nuke:?}");
        }
        for g in &gs {
            if nuke.contains(&g.id) {
                assert!(g.protected_why.is_none(), "{} is protected", g.id);
                assert!(
                    group_never_touch(g, &nt).is_none(),
                    "{} is never-touch",
                    g.id
                );
                assert!(
                    !matches!(g.category, Category::Job | Category::System),
                    "{} is a job/system group",
                    g.id
                );
            }
        }
        assert!(!nuke.contains(&"app:com.microsoft.teams2".to_string()));
        assert!(!nuke.contains(&"app:org.example.FromTerm".to_string()));
        assert!(!nuke.contains(&"orphan:701".to_string()));
        // A never-touch entry from the caller (relayed list) is honored.
        let nt2 = never_touch_list(&["org.example.Plain".into(), "LM Studio".into()]);
        let nuke2 = plan(&gs, Mode::Nuclear, &nt2);
        assert!(!nuke2.contains(&"app:org.example.Plain".to_string()));
        assert!(!nuke2.contains(&"ai:/Applications/LM Studio.app".to_string()));
        // A protected group is never selected even when its flags say it could be.
        let mut forced = gs.clone();
        for g in &mut forced {
            g.reclaimable_by_goose = true;
            g.quittable = true;
        }
        for id in plan(&forced, Mode::Nuclear, &nt) {
            let g = forced.iter().find(|g| g.id == id).unwrap();
            assert!(g.protected_why.is_none(), "{id}");
        }
    }

    #[test]
    fn memfree_c1_c2_servers_goose_did_not_start_are_never_ticked() {
        let gs = groups(&workhorse());
        let nt = never_touch_list(&[]);
        let g = |id: &str| gs.iter().find(|g| g.id == id).unwrap();
        for id in ["mlx:97100", "ai:/Applications/LM Studio.app"] {
            assert!(g(id).used_by.is_empty(), "{id}: no client connected right now");
            assert!(eligible(g(id), Mode::Nuclear, &nt), "{id} is still offered");
            assert!(!nuclear_default(g(id), &nt), "{id} must not be pre-ticked");
        }
        // goose's own and plain quittable apps still default ticked.
        for id in ["gooseEngine", "orphan:700", "app:org.example.Plain"] {
            assert!(nuclear_default(g(id), &nt), "{id}");
        }
    }

    #[test]
    fn memfree_c6_a_proven_orphan_keeps_launchd_tty_and_ssh_protection() {
        for case in ["launchd", "tty", "ssh"] {
            let mut f = workhorse();
            let row = f.rows.iter_mut().find(|r| r.pid == 700).unwrap();
            match case {
                "tty" => row.has_tty = true,
                "ssh" => row.ppid = 5001,
                _ => {
                    f.labels.insert(700, "net.example.goose-serve".into());
                }
            }
            let p = protected(&input(&f, &[600], &[700]));
            assert!(p.contains_key(&700), "{case}: {:?}", p.get(&700));
            let gs = groups(&f);
            assert!(
                !plan(&gs, Mode::Soft, &[]).contains(&"orphan:700".to_string()),
                "{case}"
            );
        }
        // A plain proven orphan (ppid 1, no label, no tty) is still goose's to free.
        let f = workhorse();
        assert_eq!(protected(&input(&f, &[600], &[700])).get(&700), None);
    }

    #[test]
    fn memfree_c7_never_touch_matches_natural_names_and_bundle_prefixes() {
        let gs = groups(&workhorse());
        let teams = gs
            .iter()
            .find(|g| g.id == "app:com.microsoft.teams2")
            .unwrap();
        assert!(group_never_touch(teams, &["Teams".into()]).is_some());
        assert!(group_never_touch(teams, &["com.microsoft".into()]).is_some());
        assert!(group_never_touch(teams, &["Slack".into()]).is_none());
        assert_eq!(
            never_touch_hit(["Microsoft Outlook"], &["outlook".into()]).as_deref(),
            Some("outlook")
        );
    }

    #[test]
    fn memfree_c8_an_app_hosting_an_mcp_child_is_protected() {
        let mut f = workhorse();
        f.p(4000, 1, "/Applications/Windsurf.app/Contents/MacOS/Electron");
        f.app(
            "/Applications/Windsurf.app",
            "com.exafunction.windsurf",
            "Windsurf",
        );
        f.p(
            4001,
            4000,
            "/Applications/Windsurf.app/Contents/Frameworks/Windsurf Helper (Plugin).app/Contents/MacOS/Windsurf Helper (Plugin)",
        );
        f.p(4002, 4001, "/opt/homebrew/bin/node");
        f.cmd(4002, "node /x/bin/github-mcp-server stdio");
        // The proven orphan's own MCP child does not make the orphan untouchable.
        f.p(702, 700, "/opt/homebrew/bin/node");
        f.cmd(702, "npx -y @modelcontextprotocol/server-filesystem /tmp");
        let p = protected(&input(&f, &[600], &[700]));
        assert_eq!(
            p.get(&4000).map(String::as_str),
            Some("hosts an MCP server (pid 4002)")
        );
        assert!(p.contains_key(&4001));
        assert_eq!(p.get(&700), None, "the orphan stays goose's");
        let gs = groups(&f);
        let app = gs
            .iter()
            .find(|g| g.id == "app:com.exafunction.windsurf")
            .unwrap();
        assert!(app.protected_why.is_some() && !app.quittable && !app.nuclear_default);
        assert!(!plan(&gs, Mode::Nuclear, &never_touch_list(&[]))
            .contains(&"app:com.exafunction.windsurf".to_string()));
    }
}
