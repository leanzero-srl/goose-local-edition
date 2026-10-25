//! Read-only dry run of "Free up memory" on this Mac: what the 3.0.130 one-click soft clean would
//! have freed, what the new soft clean frees, and what "free everything except protected" ticks by
//! default, plus every protected group and why. It EXECUTES NOTHING: a process census, `launchctl
//! list`, `lsof` and GET-only reads of the model servers' APIs. No POST, no signal, no osascript.
//!
//! Usage: cargo run -p goose-sidecar --example memfree_dryrun -- [--own-port N] [--self-pid N]
//! `--own-port` is goose's engine port (default: the engine default). `--self-pid` is the goosed
//! that would run the clean (default: the desktop's `goose serve`, else this process).

use anyhow::Result;
use goose_sidecar::engine::EngineSettings;
use goose_sidecar::fit::gb;
use goose_sidecar::machine;
use goose_sidecar::memfree::{self, CategorizeCtx, Category, Group};
use goose_sidecar::memfree_protect::{self as protect, Mode};
use goose_sidecar::memfree_servers::{self as servers, AiServerKind};
use std::collections::HashMap;

fn flag(args: &[String], name: &str) -> Option<u32> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn line(g: &Group, note: &str) -> String {
    let mut s = format!("  {:>9}  {:<40} {}", gb(g.bytes), g.id, g.label);
    if !note.is_empty() {
        s.push_str(&format!("  [{note}]"));
    }
    s
}

fn print_list(title: &str, groups: &[&Group], note: impl Fn(&Group) -> String) {
    let total: u64 = groups.iter().map(|g| g.bytes).sum();
    println!("\n== {title}: {} group(s), {} ==", groups.len(), gb(total));
    for g in groups {
        println!("{}", line(g, &note(g)));
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let own_port = flag(&args, "--own-port")
        .map(|p| p as u16)
        .unwrap_or(EngineSettings::default().port);
    let rows = memfree::census();
    let cmds = protect::all_cmdlines();
    let self_uid = memfree::self_uid();
    let me = std::process::id();
    let orphans: Vec<u32> = servers::orphaned_goosed_now(&rows, me, self_uid)
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    let self_pid = flag(&args, "--self-pid").unwrap_or_else(|| {
        rows.iter()
            .filter(|r| r.uid == self_uid && !orphans.contains(&r.pid))
            .find(|r| {
                cmds.get(&r.pid)
                    .is_some_and(|c| servers::is_desktop_serve(c))
            })
            .map_or(me, |r| r.pid)
    });
    let engines = machine::other_engines(None);
    let ports: HashMap<u32, u16> = engines
        .iter()
        .filter_map(|e| e.port.map(|p| (e.pid, p)))
        .collect();
    let (own, other): (Vec<_>, Vec<_>) = engines.iter().partition(|e| e.port == Some(own_port));
    println!(
        "memfree dry run (nothing executed): {} processes, self pid {self_pid}, own engine port \
         {own_port} ({} own engine pid(s), {} other MLX engine(s)), {} launchd job pid(s)",
        rows.len(),
        own.len(),
        other.len(),
        protect::launchd_labels().len()
    );
    let ctx = CategorizeCtx {
        self_pid,
        self_uid,
        own_engine_pids: own.iter().map(|e| e.pid).collect(),
        other_engine_pids: other.iter().map(|e| e.pid).collect(),
        ..Default::default()
    };
    let never_touch = protect::never_touch_list(&[]);
    let mut groups = memfree::groups_now(ctx);

    // GET-only model-server reads, as the ACP enrichment does them.
    let mut legacy_ai: HashMap<String, String> = HashMap::new();
    for g in groups
        .iter_mut()
        .filter(|g| g.category == Category::AiServer)
    {
        let Some(kind) = servers::ai_server_kind(&g.id, g.bundle_id.as_deref()) else {
            continue;
        };
        let base = match kind {
            AiServerKind::Ollama => servers::ollama_base(),
            AiServerKind::LmStudio => servers::lmstudio_base(),
        };
        let reach = match servers::loaded_models(kind, &base).await {
            Ok(m) if m.is_empty() => None,
            Ok(m) => Some(format!("{} model(s) loaded", m.len())),
            Err(_) if kind == AiServerKind::LmStudio && servers::lms_cli().is_some() => {
                Some("REST refused, would run `lms unload --all`".into())
            }
            Err(e) => {
                g.why = Some(format!("its API did not answer ({e:#})"));
                None
            }
        };
        if let Some(r) = reach {
            legacy_ai.insert(g.id.clone(), r.clone());
            if g.protected_why.is_none() {
                g.reclaimable_by_goose = true;
                g.why = Some(r);
            }
        }
    }
    for g in groups.iter_mut() {
        let is_server = g.category == Category::AiServer || g.id.starts_with("mlx:");
        if is_server {
            let port = if g.category == Category::AiServer {
                match servers::ai_server_kind(&g.id, g.bundle_id.as_deref()) {
                    Some(AiServerKind::Ollama) => servers::base_port(&servers::ollama_base()),
                    Some(AiServerKind::LmStudio) => servers::base_port(&servers::lmstudio_base()),
                    None => None,
                }
            } else {
                g.pids.iter().find_map(|(p, _)| ports.get(p).copied())
            };
            let own: Vec<u32> = g.pids.iter().map(|p| p.0).collect();
            let clients = port
                .map(|p| servers::port_clients(p, &own))
                .unwrap_or_default();
            g.used_by = servers::used_by_lines(g, &rows, &clients);
            g.used_by.extend(servers::lmlink_lines_for(g, &rows));
        }
        g.nuclear_default = protect::nuclear_default(g, &never_touch);
    }

    // LEGACY 3.0.130 soft: own engine (assumed idle here) + EVERY other MLX engine + every model
    // server with models or the lms CLI + orphans. No protection, no confirmation.
    let legacy: Vec<&Group> = groups
        .iter()
        .filter(|g| {
            g.id == "gooseEngine"
                || g.id.starts_with("mlx:")
                || legacy_ai.contains_key(&g.id)
                || g.category == Category::GooseOrphan
        })
        .collect();
    print_list(
        "LEGACY 3.0.130 soft (one click, no confirm)",
        &legacy,
        |g| {
            let mut n = legacy_ai.get(&g.id).cloned().unwrap_or_default();
            if let Some(p) = &g.protected_why {
                n = format!(
                    "{n}{}NOW PROTECTED: {p}",
                    if n.is_empty() { "" } else { "; " }
                );
            }
            n
        },
    );

    let soft = protect::plan(&groups, Mode::Soft, &never_touch);
    let soft_g: Vec<&Group> = groups.iter().filter(|g| soft.contains(&g.id)).collect();
    print_list("NEW soft (one click)", &soft_g, |_| String::new());

    let nuclear = protect::plan(&groups, Mode::Nuclear, &never_touch);
    let used = |g: &Group| {
        if g.used_by.is_empty() {
            String::new()
        } else {
            format!("used by: {}", g.used_by.join("; "))
        }
    };
    let ticked: Vec<&Group> = groups
        .iter()
        .filter(|g| nuclear.contains(&g.id) && g.nuclear_default)
        .collect();
    print_list(
        "NEW nuclear default selection (ticked, one confirm)",
        &ticked,
        used,
    );
    let unticked: Vec<&Group> = groups
        .iter()
        .filter(|g| nuclear.contains(&g.id) && !g.nuclear_default)
        .collect();
    print_list(
        "NEW nuclear offered but NOT ticked (in use; owner must tick it)",
        &unticked,
        used,
    );

    let prot: Vec<&Group> = groups
        .iter()
        .filter(|g| g.protected_why.is_some())
        .collect();
    print_list("PROTECTED (never offered)", &prot, |g| {
        g.protected_why.clone().unwrap_or_default()
    });

    let nt: Vec<&Group> = groups
        .iter()
        .filter(|g| g.protected_why.is_none())
        .filter(|g| protect::group_never_touch(g, &never_touch).is_some())
        .collect();
    print_list("NEVER-TOUCH (default list)", &nt, |g| {
        protect::group_never_touch(g, &never_touch).unwrap_or_default()
    });

    let servers_left: Vec<&Group> = groups
        .iter()
        .filter(|g| g.category == Category::AiServer || g.id.starts_with("mlx:"))
        .collect();
    print_list(
        "ALL model servers / MLX engines (with users)",
        &servers_left,
        |g| {
            let mut parts = vec![];
            if let Some(w) = &g.why {
                parts.push(w.clone());
            }
            if !g.used_by.is_empty() {
                parts.push(format!("used by: {}", g.used_by.join("; ")));
            }
            parts.join(" | ")
        },
    );
    Ok(())
}
