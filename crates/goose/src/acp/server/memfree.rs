//! ACP surface for "Free up memory": who holds memory on this Mac (goose-sidecar `memfree`
//! census), what freeing it would buy against the one fit rule, and the reclaim itself.
//!
//! Accounting rule (no double count): `budgetBytes` and `gooseOwnedBytes` always add up to what
//! goose itself could reach. With a fit, the fit's budget already credits this goose's own engine
//! (`freed_by_switch_bytes`), so `gooseOwnedBytes` leaves it out; without one, the budget is the
//! live measurement and the own engine's bytes are in `gooseOwnedBytes` when it is idle.

use super::mlx_engine::{
    core_unmount_graceful, fit_to_dto, forget_serving, synced_manager, EngineStopper,
};
use super::*;
use crate::providers::mlx_serving_intent::IntentKind;
use goose_sidecar::engine::{expand_tilde, MlxEngineManager, StartFrees};
use goose_sidecar::fit::{FitVerdict, NodeMemoryFacts};
use goose_sidecar::memfree::{self, CategorizeCtx, Category, Group, VerdictKind};
use goose_sidecar::memfree_protect::{self as protect, Mode};
use goose_sidecar::memfree_servers::{self as servers, AiServerKind, LoadedModel};
use goose_sidecar::{hf, holders, machine};
use std::collections::HashMap;

/// Off switch for the whole feature (default on).
pub(super) const MEMFREE_ENABLED_KEY: &str = "GOOSE_MEMFREE_ENABLED";
/// User apps below this footprint are not listed (and so never offered for quitting).
pub(super) const MEMFREE_SHOW_APPS_MIN_MB_KEY: &str = "GOOSE_MEMFREE_SHOW_APPS_MIN_MB";
const DEFAULT_SHOW_APPS_MIN_MB: u64 = 200;
/// Read on THIS Mac when another Mac's Free up memory reaches it over the Link: may that
/// peer quit apps here? Default OFF. Goose-owned reclaim over the Link needs only the
/// `manage_models` switch, which the control route already enforces (403 when off).
pub(super) const ALLOW_REMOTE_APP_QUIT_KEY: &str = "GOOSE_LINK_ALLOW_REMOTE_APP_QUIT";
const MIB: u64 = 1024 * 1024;
/// Apps, servers and processes no clean ever touches, on top of the built-in defaults
/// (`memfree_protect::DEFAULT_NEVER_TOUCH`): bundle id, app name or process name,
/// case-insensitive. The defaults always apply; this setting can only add.
pub(super) const MEMFREE_NEVER_TOUCH_KEY: &str = "GOOSE_MEMFREE_NEVER_TOUCH";

fn config_never_touch() -> Vec<String> {
    Config::global()
        .get_param::<Vec<String>>(MEMFREE_NEVER_TOUCH_KEY)
        .unwrap_or_else(|_| {
            protect::DEFAULT_NEVER_TOUCH
                .iter()
                .map(|s| s.to_string())
                .collect()
        })
}

/// The list in force on THIS Mac: the defaults + its own setting + the caller's (a relayed
/// request carries the other Mac's list, so never-touch holds both ways).
fn effective_never_touch(config: &[String], caller: &[String]) -> Vec<String> {
    let extra: Vec<String> = config.iter().chain(caller).cloned().collect();
    protect::never_touch_list(&extra)
}

fn enabled() -> bool {
    Config::global()
        .get_param::<bool>(MEMFREE_ENABLED_KEY)
        .unwrap_or(true)
}

pub(super) fn remote_app_quit_allowed() -> bool {
    super::link::switch_from_config(
        ALLOW_REMOTE_APP_QUIT_KEY,
        Config::global().get_param::<bool>(ALLOW_REMOTE_APP_QUIT_KEY),
    )
}

/// A local caller may always quit apps it picked; a relayed one only with the switch on.
fn quit_apps_allowed(relayed: bool, remote_switch: bool) -> bool {
    !relayed || remote_switch
}

fn show_apps_min_bytes() -> u64 {
    Config::global()
        .get_param::<u64>(MEMFREE_SHOW_APPS_MIN_MB_KEY)
        .unwrap_or(DEFAULT_SHOW_APPS_MIN_MB)
        .saturating_mul(MIB)
}

/// Who we are and which pids are engines, read live for one census.
async fn categorize_ctx(
    manager: &MlxEngineManager,
    caller_never_touch: &[String],
) -> Result<(CategorizeCtx, u16), agent_client_protocol::Error> {
    let status = manager.status().await;
    let port = manager.settings().port;
    let mut own_engine_pids: Vec<u32> = status.pid.into_iter().collect();
    let running = !own_engine_pids.is_empty();
    if !running {
        if let StartFrees::Leftover { pids, .. } = manager.start_frees().await {
            own_engine_pids = pids;
        }
    }
    let busy = match holders::holders_dir().and_then(|dir| holders::read_all(&dir)) {
        Ok(entries) => memfree::holders_busy(&entries),
        Err(e) => Some(format!("the busy record cannot be read: {e:#}")),
    };
    let busy = busy.or(match (running, status.active_requests) {
        (false, _) | (true, Some(0)) => None,
        (true, Some(n)) => Some(format!("{n} request(s) in flight")),
        (true, None) => Some("its request count cannot be read".to_string()),
    });
    let other_engine_pids = tokio::task::spawn_blocking(move || machine::other_engines(Some(port)))
        .await
        .internal_err()?
        .into_iter()
        .map(|e| e.pid)
        .collect();
    Ok((
        CategorizeCtx {
            self_pid: std::process::id(),
            self_uid: memfree::self_uid(),
            own_engine_pids,
            own_engine_model: status.model_id,
            own_engine_busy: busy,
            other_engine_pids,
            orphan_goosed_pids: vec![],
            never_touch: effective_never_touch(&config_never_touch(), caller_never_touch),
            ..Default::default()
        },
        port,
    ))
}

/// What one model server holds, as goose will unload it.
struct AiHold {
    kind: AiServerKind,
    base: String,
    models: Vec<LoadedModel>,
    /// LM Studio's REST did not answer; `lms unload --all` is the way in.
    via_cli: bool,
}

impl AiHold {
    /// The bytes unloading frees, never more than the server's own footprint.
    fn reclaim_bytes(&self, g: &Group) -> u64 {
        self.models
            .iter()
            .map(|m| m.bytes)
            .sum::<u64>()
            .min(g.bytes)
    }
}

/// Ask each listed model server what it holds; a server with models becomes reclaimable
/// (by the person's explicit pick, never by the soft clean). A protected server is not asked.
async fn enrich_ai_servers(groups: &mut [Group]) -> HashMap<String, AiHold> {
    let mut out = HashMap::new();
    for g in groups
        .iter_mut()
        .filter(|g| g.category == Category::AiServer && g.protected_why.is_none())
    {
        let Some(kind) = servers::ai_server_kind(&g.id, g.bundle_id.as_deref()) else {
            continue;
        };
        let base = match kind {
            AiServerKind::Ollama => servers::ollama_base(),
            AiServerKind::LmStudio => servers::lmstudio_base(),
        };
        match servers::loaded_models(kind, &base).await {
            Ok(models) if models.is_empty() => g.why = Some("no models loaded".into()),
            Ok(models) => {
                g.reclaimable_by_goose = true;
                g.why = None;
                g.label = format!("{} ({} model(s) loaded)", g.label, models.len());
                out.insert(
                    g.id.clone(),
                    AiHold {
                        kind,
                        base,
                        models,
                        via_cli: false,
                    },
                );
            }
            Err(e) if kind == AiServerKind::LmStudio && servers::lms_cli().is_some() => {
                g.reclaimable_by_goose = true;
                g.why = None;
                g.label = format!("{} (goose unloads its models through lms)", g.label);
                tracing::debug!("LM Studio REST: {e:#}");
                out.insert(
                    g.id.clone(),
                    AiHold {
                        kind,
                        base,
                        models: vec![],
                        via_cli: true,
                    },
                );
            }
            Err(e) => g.why = Some(format!("its API did not answer ({e:#})")),
        }
    }
    out
}

/// A model server or another MLX engine: freed only when the person ticks it.
fn is_server(g: &Group) -> bool {
    g.category == Category::AiServer || (g.category == Category::GooseEngine && is_other_mlx_id(g))
}

fn is_other_mlx_id(g: &Group) -> bool {
    g.id.starts_with("mlx:")
}

/// The TCP port a server group listens on: the model server's API port, or the other engine's
/// `--port`.
fn server_port(g: &Group, engine_ports: &HashMap<u32, u16>) -> Option<u16> {
    if g.category == Category::AiServer {
        let base = match servers::ai_server_kind(&g.id, g.bundle_id.as_deref())? {
            AiServerKind::Ollama => servers::ollama_base(),
            AiServerKind::LmStudio => servers::lmstudio_base(),
        };
        return servers::base_port(&base);
    }
    g.pids
        .iter()
        .find_map(|(pid, _)| engine_ports.get(pid).copied())
}

/// Fill `used_by` for every server group: its port's TCP clients (lsof), who started it, its
/// launchd job. Read-only.
async fn enrich_used_by(
    groups: &mut [Group],
    own_port: u16,
) -> Result<(), agent_client_protocol::Error> {
    let wanted: Vec<(usize, Group)> = groups
        .iter()
        .enumerate()
        .filter(|(_, g)| is_server(g))
        .map(|(i, g)| (i, g.clone()))
        .collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let lines = tokio::task::spawn_blocking(move || {
        let rows = memfree::census();
        let ports: HashMap<u32, u16> = machine::other_engines(Some(own_port))
            .into_iter()
            .filter_map(|e| e.port.map(|p| (e.pid, p)))
            .collect();
        wanted
            .into_iter()
            .map(|(i, g)| {
                let own: Vec<u32> = g.pids.iter().map(|p| p.0).collect();
                let clients = server_port(&g, &ports)
                    .map(|p| servers::port_clients(p, &own))
                    .unwrap_or_default();
                let mut lines = servers::used_by_lines(&g, &rows, &clients);
                lines.extend(servers::lmlink_lines_for(&g, &rows));
                (i, lines)
            })
            .collect::<Vec<_>>()
    })
    .await
    .internal_err()?;
    for (i, used_by) in lines {
        groups[i].used_by = used_by;
    }
    Ok(())
}

/// The census grouped, with user apps under the size floor dropped, model servers asked, users
/// of each server named, and the nuclear default recomputed after the server answers.
async fn groups(
    ctx: &CategorizeCtx,
    own_port: u16,
) -> Result<(Vec<Group>, HashMap<String, AiHold>), agent_client_protocol::Error> {
    let ctx = ctx.clone();
    let never_touch = ctx.never_touch.clone();
    let min = show_apps_min_bytes();
    let groups = tokio::task::spawn_blocking(move || memfree::groups_now(ctx))
        .await
        .internal_err()?;
    let mut groups: Vec<Group> = groups
        .into_iter()
        .filter(|g| g.category != Category::UserApp || g.bytes >= min)
        .collect();
    let ai = enrich_ai_servers(&mut groups).await;
    enrich_used_by(&mut groups, own_port).await?;
    let never_touch = protect::never_touch_list(&never_touch);
    for g in &mut groups {
        g.nuclear_default = protect::nuclear_default(g, &never_touch);
    }
    Ok((groups, ai))
}

/// What each clean reaches, from one census (pure; tested).
#[derive(Debug, Default, PartialEq)]
struct Reach {
    /// Group ids the soft clean frees: own idle engine + proven orphans.
    soft: Vec<String>,
    /// Soft bytes the fit does not already credit.
    goose_owned: u64,
    /// (group id, bytes) of unprotected, not-never-touch servers goose can unload/stop.
    servers: Vec<(String, u64)>,
    /// (bundle id, bytes) of unprotected, not-never-touch quittable apps.
    apps: Vec<(String, u64)>,
}

fn reach(
    groups: &[Group],
    ai_reclaim: &HashMap<String, u64>,
    never_touch: &[String],
    fit_credits_own_engine: bool,
) -> Reach {
    let soft = protect::plan(groups, Mode::Soft, never_touch);
    let goose_owned = groups
        .iter()
        .filter(|g| soft.contains(&g.id))
        .filter(|g| !(fit_credits_own_engine && g.id == "gooseEngine"))
        .map(|g| g.bytes)
        .sum();
    let nuclear = |g: &&Group| protect::eligible(g, Mode::Nuclear, never_touch);
    let servers = groups
        .iter()
        .filter(|g| is_server(g))
        .filter(nuclear)
        .map(|g| {
            let bytes = if g.category == Category::AiServer {
                ai_reclaim.get(&g.id).copied().unwrap_or(0)
            } else {
                g.bytes
            };
            (g.id.clone(), bytes)
        })
        .collect();
    let apps = groups
        .iter()
        .filter(|g| g.category == Category::UserApp)
        .filter(nuclear)
        .filter_map(|g| g.bundle_id.clone().map(|b| (b, g.bytes)))
        .collect();
    Reach {
        soft,
        goose_owned,
        servers,
        apps,
    }
}

fn ai_reclaim_bytes(groups: &[Group], ai: &HashMap<String, AiHold>) -> HashMap<String, u64> {
    groups
        .iter()
        .filter_map(|g| ai.get(&g.id).map(|h| (g.id.clone(), h.reclaim_bytes(g))))
        .collect()
}

/// Poll interval and cap while a reclaim's memory leaves (the cap is the ~10 s a person waits).
const SETTLE_POLL: std::time::Duration = std::time::Duration::from_millis(250);
const SETTLE_POLLS: usize = 40;
/// Readings in a row that must agree before the freed figure counts as settled (1 s).
const SETTLE_STEADY: usize = 4;

/// Each process's footprint right now, 0 once it is gone (pid + start time, so a reused pid
/// never counts).
fn footprints(pids: &[(u32, u64)]) -> Vec<u64> {
    pids.iter()
        .map(|&(pid, started)| {
            if memfree::alive(pid, started) {
                memfree::footprint(pid).unwrap_or(0)
            } else {
                0
            }
        })
        .collect()
}

/// What left, process by process: a process that shrank or exited counts its drop, one that grew
/// meanwhile (a UI repainting its model list) counts 0 instead of cancelling the others out.
fn freed_between(before: &[u64], now: &[u64]) -> u64 {
    before
        .iter()
        .zip(now)
        .map(|(b, n)| b.saturating_sub(*n))
        .sum()
}

/// Wait (bounded, ~10 s) for a reclaim's memory to leave and then stop moving; the bytes freed,
/// measured against `before` (taken just before the reclaim). Measured live 2026-10-25 on the
/// laptop: LM Studio's model sat in a `~/.lmstudio` node worker outside the .app, and the old
/// "group total minus what is alive, read once" figure said 0.4 GiB against a 1.6 GiB rise.
async fn settle_freed(pids: &[(u32, u64)], before: &[u64], expected: u64) -> u64 {
    let mut freed = freed_between(before, &footprints(pids));
    let mut steady = 0;
    for _ in 0..SETTLE_POLLS {
        tokio::time::sleep(SETTLE_POLL).await;
        let now = freed_between(before, &footprints(pids));
        steady = if settled(freed, now, expected) {
            steady + 1
        } else {
            0
        };
        freed = now;
        if freed >= expected / 2 && steady + 1 >= SETTLE_STEADY {
            break;
        }
    }
    freed
}

/// Two readings within 2% of the expected reclaim (at least 16 MiB) agree.
fn settled(prev: u64, now: u64, expected: u64) -> bool {
    let slack = (expected / 50).max(16 << 20);
    prev.abs_diff(now) <= slack
}

async fn unload_ai(g: &Group, hold: &AiHold) -> MlxMemoryFreeItemDto {
    let before = footprints(&g.pids);
    let mut messages = vec![];
    let mut failed = false;
    if !hold.via_cli {
        for m in &hold.models {
            match servers::unload_model(hold.kind, &hold.base, &m.name).await {
                Ok(()) => messages.push(format!("unloaded {}", m.name)),
                Err(e) => {
                    failed = true;
                    messages.push(format!("{}: {e:#}", m.name));
                }
            }
        }
    }
    if hold.kind == AiServerKind::LmStudio && (hold.via_cli || failed) {
        match servers::lms_cli() {
            Some(lms) => match servers::lms_unload_all(&lms).await {
                Ok(text) => {
                    failed = false;
                    messages.push(format!("lms unload --all: {text}"));
                }
                Err(e) => messages.push(format!("{e:#}")),
            },
            None if hold.via_cli => messages.push("the lms CLI is gone".into()),
            None => {}
        }
    }
    let expected = hold.reclaim_bytes(g).max(1);
    let freed = settle_freed(&g.pids, &before, expected).await;
    MlxMemoryFreeItemDto {
        id: g.id.clone(),
        label: g.label.clone(),
        ok: !failed,
        freed_bytes: Some(freed),
        message: messages.join("; "),
        force_stoppable: false,
    }
}

async fn reap_orphan(g: &Group) -> MlxMemoryFreeItemDto {
    let before = footprints(&g.pids);
    let mut messages = vec![];
    let mut ok = true;
    for &(pid, started_at) in &g.pids {
        match servers::reap_orphaned_goosed(pid, started_at).await {
            Ok(m) => messages.push(m),
            Err(e) => {
                ok = false;
                messages.push(format!("{e:#}"));
            }
        }
    }
    MlxMemoryFreeItemDto {
        id: g.id.clone(),
        label: g.label.clone(),
        ok,
        freed_bytes: Some(settle_freed(&g.pids, &before, before.iter().sum::<u64>().max(1)).await),
        message: messages.join("; "),
        force_stoppable: false,
    }
}

/// This Mac's memory measured live, crediting nothing a switch would free.
async fn measured_facts(port: u16) -> Result<NodeMemoryFacts, agent_client_protocol::Error> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<NodeMemoryFacts> {
        let reading = goose_sidecar::measure()?;
        let ceiling = goose_sidecar::engine::local_gpu_ceiling()?;
        let others = machine::other_engines(Some(port));
        Ok(NodeMemoryFacts {
            available_bytes: reading.available_bytes,
            total_bytes: reading.total_bytes,
            ceiling_bytes: ceiling,
            other_engines_bytes: machine::other_engines_bytes(&others),
            freed_by_switch_bytes: 0,
        })
    })
    .await
    .internal_err()?
    .internal_err()
}

/// One thing a free will do, decided from a fresh census before anything runs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// This goose's own idle engine (unmount).
    OwnEngine,
    /// A proven left-over goose server (SIGTERM only).
    Orphan(String),
    /// A model server (REST unload per model) or another MLX engine (SIGTERM only), by group id.
    Server(String),
    /// A user app asked to quit, by bundle id. Never signalled.
    App(String),
    /// A user app the person chose to force stop, by bundle id: one SIGTERM per pid, never
    /// SIGKILL (review C4).
    ForceStop(String),
    /// Not done, and why.
    Refuse {
        id: String,
        label: String,
        message: String,
    },
}

/// What a free request may touch, item by item, against a FRESH census (pure; tested). Only what
/// the request names is ever selected: the soft clean's own set, and each ticked server/app after
/// it passes protected, never-touch and "still reclaimable now". Over the Link (`relayed`)
/// everything but the soft clean needs the answering Mac's remote switch.
fn select_free(
    groups: &[Group],
    req: &MlxEngineMemoryFreeRequest,
    never_touch: &[String],
    relayed: bool,
    remote_switch: bool,
) -> Vec<Step> {
    let mut steps = vec![];
    let refuse = |id: &str, label: &str, message: String| Step::Refuse {
        id: id.to_string(),
        label: label.to_string(),
        message,
    };
    if req.goose_owned {
        let soft = protect::plan(groups, Mode::Soft, never_touch);
        for g in groups {
            if soft.contains(&g.id) {
                steps.push(if g.id == "gooseEngine" {
                    Step::OwnEngine
                } else {
                    Step::Orphan(g.id.clone())
                });
            } else if g.id == "gooseEngine" {
                let why = g
                    .protected_why
                    .clone()
                    .or_else(|| g.why.clone())
                    .unwrap_or_else(|| "it is in use".into());
                steps.push(refuse(
                    &g.id,
                    &g.label,
                    format!("goose left it running: {why}"),
                ));
            }
        }
    }
    let explicit = !relayed || remote_switch;
    let off = || {
        format!("freeing memory from another Mac beyond goose's own is off on this Mac ({ALLOW_REMOTE_APP_QUIT_KEY})")
    };
    if !explicit
        && req.nuclear
        && req.unload_servers.is_empty()
        && req.quit_apps.is_empty()
        && req.force_stop_apps.is_empty()
    {
        steps.push(refuse("nuclear", "Free everything", off()));
    }
    let mut seen: Vec<String> = vec![];
    for id in &req.unload_servers {
        if seen.contains(id) {
            continue;
        }
        seen.push(id.clone());
        if !explicit {
            steps.push(refuse(id, id, off()));
            continue;
        }
        let Some(g) = groups.iter().find(|g| &g.id == id) else {
            steps.push(refuse(id, id, format!("{id} is not running now")));
            continue;
        };
        match server_refusal(g, req, never_touch) {
            Some(message) => steps.push(refuse(id, &g.label, message)),
            None => steps.push(Step::Server(id.clone())),
        }
    }
    for (bundles, force) in [(&req.quit_apps, false), (&req.force_stop_apps, true)] {
        for bundle_id in bundles {
            let id = format!("app:{bundle_id}");
            let key = if force {
                format!("force:{id}")
            } else {
                id.clone()
            };
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            if !explicit {
                steps.push(refuse(&id, bundle_id, off()));
                continue;
            }
            let Some(g) = groups.iter().find(|g| g.id == id) else {
                steps.push(refuse(
                    &id,
                    bundle_id,
                    format!("{bundle_id} is not running"),
                ));
                continue;
            };
            match app_refusal(g, never_touch) {
                Some(message) => steps.push(refuse(&id, &g.label, message)),
                None if force => steps.push(Step::ForceStop(bundle_id.clone())),
                None => steps.push(Step::App(bundle_id.clone())),
            }
        }
    }
    steps
}

/// Why an item may not be freed now: protected, on the never-touch list, or nothing goose can
/// reclaim. `None` = it may be freed.
fn item_refusal(g: &Group, never_touch: &[String]) -> Option<String> {
    if let Some(why) = &g.protected_why {
        return Some(format!(
            "goose will not touch {}: protected ({why})",
            g.label
        ));
    }
    if let Some(hit) = protect::group_never_touch(g, never_touch) {
        return Some(format!("{} is on the never-touch list ({hit})", g.label));
    }
    (!protect::eligible(g, Mode::Nuclear, never_touch)).then(|| {
        format!(
            "goose cannot free {} now: {}",
            g.label,
            g.why.clone().unwrap_or_else(|| "nothing to unload".into())
        )
    })
}

/// The `usedBy` lines of `g` the person did not see when they ticked it.
fn new_users(g: &Group, seen_used_by: &HashMap<String, Vec<String>>) -> Vec<String> {
    let seen = seen_used_by.get(&g.id);
    g.used_by
        .iter()
        .filter(|line| !seen.is_some_and(|s| s.contains(line)))
        .cloned()
        .collect()
}

fn server_refusal(
    g: &Group,
    req: &MlxEngineMemoryFreeRequest,
    never_touch: &[String],
) -> Option<String> {
    if !is_server(g) {
        return Some(format!("{} is not a model server or MLX engine", g.label));
    }
    if let Some(message) = item_refusal(g, never_touch) {
        return Some(message);
    }
    let new = new_users(g, &req.seen_used_by);
    (!new.is_empty()).then(|| {
        format!(
            "{} became used since you looked: {}",
            g.label,
            new.join("; ")
        )
    })
}

fn app_refusal(g: &Group, never_touch: &[String]) -> Option<String> {
    if g.category != Category::UserApp {
        return Some(format!("{} is not a user app", g.label));
    }
    item_refusal(g, never_touch)
}

/// Review C3: re-validate ONE step against a census taken right before it runs (pure; tested).
/// `Some(Step::Refuse)` = do not run it; the reason is what changed.
fn recheck(
    fresh: &[Group],
    step: &Step,
    req: &MlxEngineMemoryFreeRequest,
    never_touch: &[String],
) -> Option<Step> {
    let refuse = |id: &str, label: &str, message: String| {
        Some(Step::Refuse {
            id: id.to_string(),
            label: label.to_string(),
            message,
        })
    };
    let find = |id: &str| fresh.iter().find(|g| g.id == id);
    match step {
        Step::Refuse { .. } => None,
        Step::OwnEngine | Step::Orphan(_) => {
            let id = match step {
                Step::Orphan(id) => id.as_str(),
                _ => "gooseEngine",
            };
            if protect::plan(fresh, Mode::Soft, never_touch)
                .iter()
                .any(|p| p == id)
            {
                return None;
            }
            match find(id) {
                None => refuse(id, id, format!("{id} is not running now")),
                Some(g) => {
                    let why = g
                        .protected_why
                        .clone()
                        .or_else(|| g.why.clone())
                        .unwrap_or_else(|| "it is in use".into());
                    refuse(id, &g.label, format!("goose left it running: {why}"))
                }
            }
        }
        Step::Server(id) => match find(id) {
            None => refuse(id, id, format!("{id} is not running now")),
            Some(g) => server_refusal(g, req, never_touch).and_then(|m| refuse(id, &g.label, m)),
        },
        Step::App(bundle_id) | Step::ForceStop(bundle_id) => {
            let id = format!("app:{bundle_id}");
            match find(&id) {
                None => refuse(&id, bundle_id, format!("{bundle_id} is not running now")),
                Some(g) => app_refusal(g, never_touch).and_then(|m| refuse(&id, &g.label, m)),
            }
        }
    }
}

fn group_to_dto(g: &Group) -> MlxMemoryGroupDto {
    MlxMemoryGroupDto {
        id: g.id.clone(),
        label: g.label.clone(),
        category: g.category.as_str().to_string(),
        bytes: g.bytes,
        pids: g
            .pids
            .iter()
            .map(|&(pid, started_at)| MlxMemoryPidDto { pid, started_at })
            .collect(),
        bundle_id: g.bundle_id.clone(),
        quittable: g.quittable,
        reclaimable_by_goose: g.reclaimable_by_goose,
        why: g.why.clone(),
        protected_why: g.protected_why.clone(),
        used_by: g.used_by.clone(),
        launchd_label: g.launchd_label.clone(),
        nuclear_default: g.nuclear_default,
    }
}

/// The two sides of a short fit: RAM short (what reclaiming memory can cover) and what the need
/// exceeds the whole GPU ceiling by (nothing reclaimable covers that).
fn shorts(fit: &FitVerdict) -> (u64, u64) {
    let need = fit.need.total_bytes();
    let ram_room = fit
        .facts
        .available_after_switch_bytes()
        .saturating_sub(fit.facts.margin_bytes());
    (
        need.saturating_sub(ram_room),
        need.saturating_sub(fit.facts.ceiling_bytes),
    )
}

/// Free memory on a peer from its own Link load report (available % x RAM). `None` when the
/// Link is down, the peer is unknown, or it sent no load (an older goose): unknown, never 0.
async fn peer_free_bytes(node_id: &str) -> Option<u64> {
    let manager = super::link::existing_link_manager()?;
    let node = manager.active_registry().await?.peer_node(node_id)?;
    free_bytes_from_load(&node.load?.own)
}

fn free_bytes_from_load(own: &leanzero_link::load::OwnLoad) -> Option<u64> {
    let pct = own.mem_free_pct?;
    let total_gib = own.mem_total_gb?;
    if !(pct.is_finite() && total_gib.is_finite()) || pct < 0.0 || total_gib <= 0.0 {
        return None;
    }
    Some((pct.min(100.0) / 100.0 * total_gib * (1u64 << 30) as f64) as u64)
}

async fn alternatives(
    model_id: &str,
    fit: &FitVerdict,
    room: u64,
    want_peer: bool,
) -> Vec<MlxMemoryAlternativeDto> {
    let mut out = vec![];
    let models: Vec<(String, u64)> = super::mlx_engine::load_engine_settings()
        .ok()
        .and_then(|s| hf::list_local_models(&expand_tilde(&s.models_dir)).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m.complete)
        .map(|m| (m.id, m.size_bytes))
        .collect();
    for (id, need) in memfree::smaller_variants(
        model_id,
        fit.need.weights_bytes,
        fit.need.kv_bytes,
        room,
        &models,
    ) {
        out.push(MlxMemoryAlternativeDto {
            kind: "smallerVariant".into(),
            model_id: Some(id),
            need_bytes: Some(need),
            ..Default::default()
        });
    }
    if want_peer {
        if let Ok((Some(candidate), _)) =
            super::mlx_placement::mount_alternative(model_id, fit.need.weights_bytes).await
        {
            let node_id = candidate.key.nodes.first().cloned();
            let budget_bytes = match &node_id {
                Some(id) => peer_free_bytes(id).await,
                None => None,
            };
            out.push(MlxMemoryAlternativeDto {
                kind: "peerMac".into(),
                node_id,
                label: Some(candidate.node_names.join(" + ")),
                budget_bytes,
                ..Default::default()
            });
        }
    }
    out
}

pub(super) async fn core_memory_holders(
    req: MlxEngineMemoryHoldersRequest,
    relayed: bool,
) -> Result<MlxEngineMemoryHoldersResponse, agent_client_protocol::Error> {
    if !enabled() {
        return Ok(MlxEngineMemoryHoldersResponse {
            enabled: false,
            ..Default::default()
        });
    }
    let manager = synced_manager()?;
    let (ctx, port) = categorize_ctx(manager, &req.never_touch).await?;
    let never_touch = protect::never_touch_list(&ctx.never_touch);
    let (groups, ai) = groups(&ctx, port).await?;
    let fit = match &req.fit_model_id {
        Some(id) => Some((
            id.clone(),
            manager.mount_fit(id).await.invalid_params_err()?,
        )),
        None => None,
    };
    let facts = match &fit {
        Some((_, f)) => f.facts,
        None => measured_facts(port).await?,
    };
    // The fit already credits the own engine; the bare measurement does not.
    let r = reach(
        &groups,
        &ai_reclaim_bytes(&groups, &ai),
        &never_touch,
        fit.is_some(),
    );
    let goose_owned = r.goose_owned;
    let servers_bytes: u64 = r.servers.iter().map(|s| s.1).sum();
    let apps_bytes: u64 = r.apps.iter().map(|a| a.1).sum();
    // Servers and apps are what the person may confirm; soft never needs them.
    let confirmable: Vec<(String, u64)> = r.servers.iter().chain(&r.apps).cloned().collect();
    let budget_bytes = match &fit {
        Some((_, f)) => f.budget_bytes,
        None => facts.budget_bytes(),
    };

    let mut verdict = None;
    let mut alts = vec![];
    let short_bytes = fit.as_ref().and_then(|(_, f)| f.short_bytes());
    if let (Some((id, f)), Some(_)) = (&fit, short_bytes) {
        let (ram_short, ceiling_short) = shorts(f);
        let v = memfree::verdict(ram_short, ceiling_short, goose_owned, &confirmable);
        let room = f
            .facts
            .available_after_switch_bytes()
            .saturating_sub(f.facts.margin_bytes())
            .saturating_add(goose_owned)
            .saturating_add(servers_bytes)
            .saturating_add(apps_bytes)
            .min(f.facts.ceiling_bytes);
        alts = alternatives(id, f, room, v.kind == VerdictKind::CannotFit).await;
        verdict = Some(MlxMemoryVerdictDto {
            kind: v.kind.as_str().to_string(),
            remaining_bytes: v.remaining_bytes,
            apps_to_quit: v.apps_to_quit,
        });
    }

    Ok(MlxEngineMemoryHoldersResponse {
        available_bytes: facts.available_bytes,
        budget_bytes,
        total_bytes: facts.total_bytes,
        fit: fit.as_ref().map(|(id, f)| fit_to_dto(id, f)),
        groups: groups.iter().map(group_to_dto).collect(),
        goose_owned_bytes: goose_owned,
        servers_bytes,
        apps_bytes,
        short_bytes,
        verdict,
        alternatives: alts,
        quit_apps_allowed: quit_apps_allowed(relayed, relayed && remote_app_quit_allowed()),
        enabled: true,
    })
}

fn refused(id: String, label: String, message: String) -> MlxMemoryFreeItemDto {
    MlxMemoryFreeItemDto {
        id,
        label,
        ok: false,
        freed_bytes: None,
        message,
        force_stoppable: false,
    }
}

async fn stop_own_engine(g: &Group) -> MlxMemoryFreeItemDto {
    let before = footprints(&g.pids);
    // Review C5: SIGTERM only; an engine that does not exit is left running and reported.
    let (ok, message) = match core_unmount_graceful(EngineStopper::Owner).await {
        Ok(()) => {
            forget_serving(IntentKind::Single);
            (true, format!("stopped {}", g.label))
        }
        Err(why) => (false, why),
    };
    let freed = if ok {
        Some(settle_freed(&g.pids, &before, before.iter().sum::<u64>().max(1)).await)
    } else {
        None
    };
    MlxMemoryFreeItemDto {
        id: g.id.clone(),
        label: g.label.clone(),
        ok,
        freed_bytes: freed,
        message,
        force_stoppable: false,
    }
}

/// Another MLX engine the person ticked: SIGTERM per pid after a fresh re-proof, never SIGKILL.
async fn stop_other_engine(g: &Group, own_port: u16) -> MlxMemoryFreeItemDto {
    let before = footprints(&g.pids);
    let mut messages = vec![];
    let mut ok = true;
    for &(pid, started_at) in &g.pids {
        match servers::terminate_engine(pid, started_at, own_port).await {
            Ok(m) => messages.push(m),
            Err(e) => {
                ok = false;
                messages.push(format!("{e:#}"));
            }
        }
    }
    let freed = settle_freed(&g.pids, &before, before.iter().sum::<u64>().max(1)).await;
    MlxMemoryFreeItemDto {
        id: g.id.clone(),
        label: g.label.clone(),
        ok,
        freed_bytes: Some(freed),
        message: messages.join("; "),
        force_stoppable: false,
    }
}

/// What the person is told about an app it asked to quit that did not (review C4; pure). It is
/// left running, never signalled; `force_stoppable` offers an explicit Force stop when goose
/// could signal it. An app gone by the re-check has quit after all.
fn not_quit_report(
    mut item: MlxMemoryFreeItemDto,
    still_there: bool,
    force_stoppable: bool,
) -> MlxMemoryFreeItemDto {
    if !still_there {
        item.ok = true;
        item.message.push_str("; it has quit since");
        return item;
    }
    let detail = std::mem::take(&mut item.message);
    item.message = format!(
        "{} didn't quit (maybe unsaved work); it was left running",
        item.label
    );
    if !detail.is_empty() {
        item.message.push_str(&format!(" ({detail})"));
    }
    item.force_stoppable = force_stoppable;
    item
}

/// The pids goose may SIGTERM for app `bundle_id`, from a fresh census: this user's,
/// unprotected, not never-touch. `Ok(None)` = the app is not running.
async fn app_targets(
    bundle_id: &str,
    ctx: &CategorizeCtx,
    never_touch: &[String],
) -> Result<Option<(Group, Vec<(u32, u64)>)>, String> {
    let id = format!("app:{bundle_id}");
    let ctx = ctx.clone();
    let (rows, group) = tokio::task::spawn_blocking(move || {
        let rows = memfree::census();
        let group = memfree::groups_now(ctx).into_iter().find(|g| g.id == id);
        (rows, group)
    })
    .await
    .map_err(|_| "the census failed, nothing was signalled".to_string())?;
    let Some(group) = group else { return Ok(None) };
    let targets = servers::app_term_targets(
        Some(&group),
        &rows,
        memfree::self_uid(),
        std::process::id(),
        never_touch,
    )?;
    Ok(Some((group, targets)))
}

/// Ask an app to quit (osascript). Never a signal: an app that does not quit is reported and
/// left running, with `forceStoppable` when the person could still choose Force stop.
async fn quit_one_app(
    bundle_id: &str,
    ctx: &CategorizeCtx,
    never_touch: &[String],
) -> MlxMemoryFreeItemDto {
    let outcome = memfree::quit_app(bundle_id, ctx.clone()).await;
    let item = MlxMemoryFreeItemDto {
        id: outcome.id,
        label: outcome.label,
        ok: outcome.ok,
        freed_bytes: outcome.freed_bytes,
        message: outcome.message,
        force_stoppable: false,
    };
    if item.ok {
        return item;
    }
    match app_targets(bundle_id, ctx, never_touch).await {
        Ok(None) => not_quit_report(item, false, false),
        Ok(Some((_, targets))) => not_quit_report(item, true, !targets.is_empty()),
        Err(_) => not_quit_report(item, true, false),
    }
}

/// Force stop, only for a bundle the person explicitly chose (review C4): one SIGTERM per pid a
/// FRESH census still shows as this user's unprotected, not-never-touch user app. Never SIGKILL.
async fn force_stop_app(
    bundle_id: &str,
    ctx: &CategorizeCtx,
    never_touch: &[String],
) -> MlxMemoryFreeItemDto {
    let id = format!("app:{bundle_id}");
    let mut item = MlxMemoryFreeItemDto {
        id: id.clone(),
        label: bundle_id.to_string(),
        ok: false,
        freed_bytes: None,
        message: String::new(),
        force_stoppable: false,
    };
    match app_targets(bundle_id, ctx, never_touch).await {
        Ok(None) => {
            item.ok = true;
            item.message = format!("{bundle_id} is not running");
        }
        Ok(Some((group, targets))) => {
            item.label = group.label.clone();
            if targets.is_empty() {
                item.message = format!(
                    "none of {}'s processes run as you, nothing was signalled",
                    group.label
                );
                return item;
            }
            let left = servers::terminate_pids(&targets).await;
            let after: u64 = targets
                .iter()
                .filter(|(p, s)| memfree::alive(*p, *s))
                .filter_map(|(p, _)| memfree::footprint(*p))
                .sum();
            item.freed_bytes = Some(group.bytes.saturating_sub(after));
            if left == 0 {
                item.ok = true;
                item.message = format!("force stopped {} (SIGTERM)", group.label);
            } else {
                item.message = format!(
                    "sent SIGTERM, {left} process(es) of {} still running (goose does not force it further)",
                    group.label
                );
            }
        }
        Err(why) => item.message = format!("not signalled: {why}"),
    }
    item
}

pub(super) async fn core_memory_free(
    req: MlxEngineMemoryFreeRequest,
    relayed: bool,
) -> Result<MlxEngineMemoryFreeResponse, agent_client_protocol::Error> {
    if !enabled() {
        return Err(agent_client_protocol::Error::invalid_params().data(format!(
            "Free up memory is turned off ({MEMFREE_ENABLED_KEY})"
        )));
    }
    let manager = synced_manager()?;
    let (ctx, port) = categorize_ctx(manager, &req.never_touch).await?;
    let never_touch = protect::never_touch_list(&ctx.never_touch);
    let before = measured_facts(port).await?.budget_bytes();
    let mut items = vec![];

    // A fresh census: only what is reclaimable NOW, and only what was asked for, is touched.
    let (first, _) = groups(&ctx, port).await?;
    let steps = select_free(
        &first,
        &req,
        &never_touch,
        relayed,
        relayed && remote_app_quit_allowed(),
    );
    drop(first);
    for step in steps {
        if let Step::Refuse { id, label, message } = step {
            items.push(refused(id, label, message));
            continue;
        }
        // Review C3: steps run one after another (an app may take 15 s), so each is re-checked
        // against a census taken right before it, and run with what that census found.
        let (fresh, ai) = groups(&ctx, port).await?;
        if let Some(Step::Refuse { id, label, message }) =
            recheck(&fresh, &step, &req, &never_touch)
        {
            items.push(refused(id, label, message));
            continue;
        }
        let find = |id: &str| fresh.iter().find(|g| g.id == id);
        match step {
            Step::Refuse { .. } => {}
            Step::OwnEngine => {
                if let Some(g) = find("gooseEngine") {
                    items.push(stop_own_engine(g).await);
                }
            }
            Step::Orphan(id) => {
                if let Some(g) = find(&id) {
                    items.push(reap_orphan(g).await);
                }
            }
            Step::Server(id) => {
                let Some(g) = find(&id) else { continue };
                if g.category == Category::AiServer {
                    match ai.get(&g.id) {
                        Some(hold) => items.push(unload_ai(g, hold).await),
                        None => items.push(refused(
                            g.id.clone(),
                            g.label.clone(),
                            format!("{} has no models loaded", g.label),
                        )),
                    }
                } else {
                    items.push(stop_other_engine(g, port).await);
                }
            }
            Step::App(bundle_id) => {
                items.push(quit_one_app(&bundle_id, &ctx, &never_touch).await);
            }
            Step::ForceStop(bundle_id) => {
                items.push(force_stop_app(&bundle_id, &ctx, &never_touch).await);
            }
        }
    }

    let after = measured_facts(port).await?.budget_bytes();
    let fit = match &req.fit_model_id {
        Some(id) => manager.mount_fit(id).await.ok().map(|f| fit_to_dto(id, &f)),
        None => None,
    };
    Ok(MlxEngineMemoryFreeResponse {
        items,
        before: MlxMemoryBudgetDto {
            budget_bytes: before,
        },
        after: MlxMemoryBudgetDto {
            budget_bytes: after,
        },
        fit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_sidecar::fit::{Need, Verdict};

    fn fit(need: u64, available: u64, total: u64, ceiling: u64, freed: u64) -> FitVerdict {
        FitVerdict {
            verdict: Verdict::Block,
            need: Need {
                weights_bytes: need,
                kv_bytes: 0,
                context_tokens: 0,
                kv_gap: None,
            },
            facts: NodeMemoryFacts {
                available_bytes: available,
                total_bytes: total,
                ceiling_bytes: ceiling,
                other_engines_bytes: 0,
                freed_by_switch_bytes: freed,
            },
            budget_bytes: 0,
            message: String::new(),
        }
    }

    #[test]
    fn peer_free_bytes_come_from_its_load_report() {
        let mut own = leanzero_link::load::OwnLoad {
            mem_free_pct: Some(50.0),
            mem_total_gb: Some(128.0),
            ..Default::default()
        };
        assert_eq!(free_bytes_from_load(&own), Some(64 * (1u64 << 30)));
        own.mem_free_pct = None;
        assert_eq!(
            free_bytes_from_load(&own),
            None,
            "no reading is unknown, not 0"
        );
        own.mem_free_pct = Some(f64::NAN);
        assert_eq!(free_bytes_from_load(&own), None);
        own.mem_free_pct = Some(10.0);
        own.mem_total_gb = None;
        assert_eq!(free_bytes_from_load(&own), None);
    }

    #[test]
    fn a_reclaim_settles_only_when_readings_agree() {
        let gib = 1u64 << 30;
        // still falling by 0.5 GiB between polls: not settled
        assert!(!settled(gib / 2, gib, 2 * gib));
        // within 2% of the expected reclaim: settled
        assert!(settled(gib - gib / 100, gib, 2 * gib));
        // tiny reclaims get a 16 MiB floor of slack
        assert!(settled(0, 10 << 20, 1));
    }

    #[test]
    fn freed_counts_each_process_drop_and_ignores_growth() {
        let mib = 1u64 << 20;
        // LM Studio 2026-10-25: the worker (2519 MiB) exits, the GPU helper shrinks 200 MiB,
        // the renderer grows 300 MiB while the UI repaints: freed = 2519 + 200, not 2419.
        let before = [298 * mib, 657 * mib, 282 * mib, 2519 * mib];
        let now = [298 * mib, 457 * mib, 582 * mib, 0];
        assert_eq!(freed_between(&before, &now), 2719 * mib);
        assert_eq!(freed_between(&before, &before), 0);
    }

    #[test]
    fn remote_app_quit_needs_the_peers_switch() {
        assert!(
            quit_apps_allowed(false, false),
            "a local caller picks its own apps"
        );
        assert!(
            !quit_apps_allowed(true, false),
            "relayed with the switch off: refused"
        );
        assert!(
            quit_apps_allowed(true, true),
            "relayed with the switch on: allowed"
        );
    }

    #[test]
    fn shorts_credit_the_switch_once_and_split_ram_from_ceiling() {
        const GB: u64 = 1_000_000_000;
        // Own engine frees 10 GB: it is counted once, through available_after_switch.
        let f = fit(30 * GB, 6 * GB, 64 * GB, 48 * GB, 10 * GB);
        let (ram, ceiling) = shorts(&f);
        assert_eq!(ram, 30 * GB - (16 * GB - f.facts.margin_bytes()));
        assert_eq!(ceiling, 0);
        let f = fit(60 * GB, 64 * GB, 64 * GB, 48 * GB, 0);
        assert_eq!(shorts(&f).1, 12 * GB);
    }

    const GB: u64 = 1_000_000_000;

    fn grp(id: &str, category: Category, bytes: u64) -> Group {
        let bundle_id = id.strip_prefix("app:").map(str::to_string);
        Group {
            id: id.into(),
            label: id.into(),
            category,
            bytes,
            pids: vec![(bytes as u32 % 9000 + 100, 1)],
            bundle_id,
            quittable: category == Category::UserApp,
            reclaimable_by_goose: matches!(
                category,
                Category::GooseEngine | Category::GooseOrphan | Category::AiServer
            ),
            why: None,
            protected_why: None,
            used_by: vec![],
            launchd_label: None,
            nuclear_default: false,
        }
    }

    /// The workhorse shape (dry run 2026-10-25): own idle engine, a proven orphan, rapid-mlx on
    /// 8093, LM Studio with a model, Teams, TextEdit, a launchd-owned MCP server, a heavy-run TTS.
    fn workhorse() -> Vec<Group> {
        let mut mcp = grp("job:mcp-web-search-upd", Category::Job, 300 * MIB);
        mcp.protected_why = Some("launchd job net.leanzero.mcp-web-search-upd".into());
        mcp.reclaimable_by_goose = false;
        let mut tts = grp("job:python3", Category::Job, 4 * GB);
        tts.protected_why = Some("heavy-run job (pgid 36740)".into());
        tts.reclaimable_by_goose = false;
        let mut protected_lms = grp("ai:lms", Category::AiServer, 2 * GB);
        protected_lms.protected_why = Some("started from a terminal".into());
        protected_lms.reclaimable_by_goose = false;
        vec![
            grp("gooseEngine", Category::GooseEngine, 20 * GB),
            grp("goosed:4242", Category::GooseOrphan, GB),
            grp("mlx:97100", Category::GooseEngine, 17 * GB),
            grp("ai:/Applications/LM Studio.app", Category::AiServer, 6 * GB),
            protected_lms,
            grp("app:com.microsoft.teams2", Category::UserApp, 2 * GB),
            grp("app:com.apple.TextEdit", Category::UserApp, GB),
            mcp,
            tts,
        ]
    }

    fn nt(caller: &[&str]) -> Vec<String> {
        let caller: Vec<String> = caller.iter().map(|s| s.to_string()).collect();
        effective_never_touch(&config_list(), &caller)
    }

    fn config_list() -> Vec<String> {
        vec!["Safari".into()]
    }

    fn ids(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .map(|s| match s {
                Step::OwnEngine => "gooseEngine".to_string(),
                Step::Orphan(id) | Step::Server(id) => id.clone(),
                Step::App(b) => format!("app:{b}"),
                Step::ForceStop(b) => format!("force:app:{b}"),
                Step::Refuse { id, .. } => format!("REFUSED {id}"),
            })
            .collect()
    }

    #[test]
    fn memfree_soft_never_includes_a_non_goose_model_or_engine() {
        let groups = workhorse();
        let req = MlxEngineMemoryFreeRequest {
            goose_owned: true,
            ..Default::default()
        };
        for relayed in [false, true] {
            let steps = select_free(&groups, &req, &nt(&[]), relayed, false);
            assert_eq!(
                ids(&steps),
                vec!["gooseEngine", "goosed:4242"],
                "relayed={relayed}"
            );
        }
        // The holders' soft figure counts the orphan (the fit credits the own engine), never the
        // other engine or the model server.
        let ai = HashMap::from([("ai:/Applications/LM Studio.app".to_string(), 5 * GB)]);
        let r = reach(&groups, &ai, &nt(&[]), true);
        assert_eq!(r.soft, vec!["gooseEngine", "goosed:4242"]);
        assert_eq!(r.goose_owned, GB);
        assert_eq!(reach(&groups, &ai, &nt(&[]), false).goose_owned, 21 * GB);
        assert_eq!(
            r.servers,
            vec![
                ("mlx:97100".to_string(), 17 * GB),
                ("ai:/Applications/LM Studio.app".to_string(), 5 * GB)
            ]
        );
        assert_eq!(r.apps, vec![("com.apple.TextEdit".to_string(), GB)]);
    }

    #[test]
    fn memfree_unticked_items_are_untouched() {
        let groups = workhorse();
        let req = MlxEngineMemoryFreeRequest {
            unload_servers: vec!["ai:/Applications/LM Studio.app".into()],
            quit_apps: vec!["com.apple.TextEdit".into()],
            nuclear: true,
            ..Default::default()
        };
        let steps = select_free(&groups, &req, &nt(&[]), false, false);
        assert_eq!(
            steps,
            vec![
                Step::Server("ai:/Applications/LM Studio.app".into()),
                Step::App("com.apple.TextEdit".into())
            ],
            "only the ticked server and app: not the own engine, the orphan or rapid-mlx"
        );
    }

    #[test]
    fn memfree_protected_never_touch_and_unknown_items_are_refused() {
        let groups = workhorse();
        let req = MlxEngineMemoryFreeRequest {
            unload_servers: vec![
                "ai:lms".into(),
                "job:mcp-web-search-upd".into(),
                "mlx:1".into(),
            ],
            quit_apps: vec!["com.microsoft.teams2".into(), "com.apple.Safari".into()],
            nuclear: true,
            ..Default::default()
        };
        let steps = select_free(&groups, &req, &nt(&[]), false, false);
        assert_eq!(
            ids(&steps),
            vec![
                "REFUSED ai:lms",
                "REFUSED job:mcp-web-search-upd",
                "REFUSED mlx:1",
                "REFUSED app:com.microsoft.teams2",
                "REFUSED app:com.apple.Safari"
            ]
        );
    }

    #[test]
    fn memfree_relayed_never_touch_is_honored() {
        let groups = workhorse();
        // The caller's list names TextEdit; the answering Mac adds its own (Safari) + defaults.
        let list = nt(&["com.apple.TextEdit"]);
        assert!(list.iter().any(|e| e == "Safari"));
        assert!(list.iter().any(|e| e == "com.microsoft.teams2"));
        let req = MlxEngineMemoryFreeRequest {
            quit_apps: vec!["com.apple.TextEdit".into()],
            nuclear: true,
            never_touch: vec!["com.apple.TextEdit".into()],
            ..Default::default()
        };
        let steps = select_free(&groups, &req, &list, true, true);
        assert!(
            matches!(&steps[..], [Step::Refuse { message, .. }] if message.contains("never-touch")),
            "{steps:?}"
        );
    }

    #[test]
    fn memfree_relayed_nuclear_is_refused_with_the_switch_off() {
        let groups = workhorse();
        let req = MlxEngineMemoryFreeRequest {
            goose_owned: true,
            unload_servers: vec!["mlx:97100".into()],
            quit_apps: vec!["com.apple.TextEdit".into()],
            nuclear: true,
            ..Default::default()
        };
        let steps = select_free(&groups, &req, &nt(&[]), true, false);
        assert_eq!(
            ids(&steps),
            vec![
                "gooseEngine",
                "goosed:4242",
                "REFUSED mlx:97100",
                "REFUSED app:com.apple.TextEdit"
            ],
            "soft still runs over the Link; everything else needs the peer's switch"
        );
        let bare = MlxEngineMemoryFreeRequest {
            nuclear: true,
            ..Default::default()
        };
        assert_eq!(
            ids(&select_free(&groups, &bare, &nt(&[]), true, false)),
            vec!["REFUSED nuclear"]
        );
        // Local, or relayed with the switch on: the ticked items go through.
        let on = select_free(&groups, &req, &nt(&[]), true, true);
        assert!(on.contains(&Step::Server("mlx:97100".into())));
        assert!(on.contains(&Step::App("com.apple.TextEdit".into())));
    }

    /// Review C3: every item is re-checked against a FRESH census right before it runs. A server
    /// that gained a user the person did not see is refused and named; one whose users are all
    /// ones they saw goes through; an app that became protected or quit is refused.
    #[test]
    fn memfree_c3_each_item_is_rechecked_and_a_new_user_refuses_a_server() {
        let mut req = MlxEngineMemoryFreeRequest {
            unload_servers: vec!["mlx:97100".into()],
            quit_apps: vec!["com.apple.TextEdit".into()],
            nuclear: true,
            ..Default::default()
        };
        req.seen_used_by.insert(
            "mlx:97100".into(),
            vec!["sshd-session (pid 97112) is connected to it".into()],
        );
        let mut fresh = workhorse();
        let rapid = fresh.iter_mut().find(|g| g.id == "mlx:97100").unwrap();
        rapid.used_by = vec!["sshd-session (pid 97112) is connected to it".into()];
        let server = Step::Server("mlx:97100".into());
        assert_eq!(
            recheck(&fresh, &server, &req, &nt(&[])),
            None,
            "only what they saw"
        );

        let rapid = fresh.iter_mut().find(|g| g.id == "mlx:97100").unwrap();
        rapid
            .used_by
            .push("python3 (pid 555) is connected to it".into());
        let Some(Step::Refuse { message, .. }) = recheck(&fresh, &server, &req, &nt(&[])) else {
            panic!("a server with a new user must be refused");
        };
        assert!(
            message.contains("became used since you looked"),
            "{message}"
        );
        assert!(message.contains("python3 (pid 555)"), "{message}");
        assert!(
            !message.contains("97112"),
            "only the NEW user is named: {message}"
        );
        // The first selection applies the same rule.
        assert_eq!(
            ids(&select_free(&fresh, &req, &nt(&[]), false, false)),
            vec!["REFUSED mlx:97100", "app:com.apple.TextEdit"]
        );

        let app = Step::App("com.apple.TextEdit".into());
        assert_eq!(recheck(&fresh, &app, &req, &nt(&[])), None);
        let edit = fresh
            .iter_mut()
            .find(|g| g.id == "app:com.apple.TextEdit")
            .unwrap();
        edit.protected_why = Some("started from a terminal".into());
        assert!(matches!(
            recheck(&fresh, &app, &req, &nt(&[])),
            Some(Step::Refuse { .. })
        ));
        fresh.retain(|g| g.id != "app:com.apple.TextEdit" && g.id != "gooseEngine");
        assert!(matches!(
            recheck(&fresh, &app, &req, &nt(&[])),
            Some(Step::Refuse { .. })
        ));
        assert!(matches!(
            recheck(&fresh, &Step::OwnEngine, &req, &nt(&[])),
            Some(Step::Refuse { .. })
        ));
    }

    /// Review C4: nuclear only ASKS apps to quit. A SIGTERM happens only for a bundle the
    /// person chose to force stop (`forceStopApps`), after the same checks, and over the Link
    /// only with the peer's switch.
    #[test]
    fn memfree_c4_nuclear_never_force_stops_unless_the_person_asks() {
        let groups = workhorse();
        let nuclear = MlxEngineMemoryFreeRequest {
            quit_apps: vec!["com.apple.TextEdit".into()],
            nuclear: true,
            ..Default::default()
        };
        assert_eq!(
            select_free(&groups, &nuclear, &nt(&[]), false, false),
            vec![Step::App("com.apple.TextEdit".into())]
        );
        let force = MlxEngineMemoryFreeRequest {
            force_stop_apps: vec!["com.apple.TextEdit".into(), "com.microsoft.teams2".into()],
            nuclear: true,
            ..Default::default()
        };
        assert_eq!(
            ids(&select_free(&groups, &force, &nt(&["Teams"]), false, false)),
            vec![
                "force:app:com.apple.TextEdit",
                "REFUSED app:com.microsoft.teams2"
            ]
        );
        assert_eq!(
            ids(&select_free(&groups, &force, &nt(&[]), true, false)),
            vec![
                "REFUSED app:com.apple.TextEdit",
                "REFUSED app:com.microsoft.teams2"
            ],
            "over the Link a force stop needs the peer's switch"
        );
    }

    /// Review C4: an app still running after it was asked to quit is reported, left running,
    /// and offered for an explicit force stop only when goose could signal it.
    #[test]
    fn memfree_c4_an_app_that_did_not_quit_is_reported_and_left_running() {
        let asked = MlxMemoryFreeItemDto {
            id: "app:com.apple.TextEdit".into(),
            label: "TextEdit".into(),
            ok: false,
            freed_bytes: Some(0),
            message: "TextEdit is still running (it may be asking to save)".into(),
            force_stoppable: false,
        };
        let left = not_quit_report(asked.clone(), true, true);
        assert!(!left.ok);
        assert!(left.force_stoppable);
        assert!(
            left.message.contains("didn't quit (maybe unsaved work)"),
            "{}",
            left.message
        );
        assert!(left.message.contains("left running"), "{}", left.message);
        assert!(!left.message.contains("SIGTERM"), "{}", left.message);
        assert!(!not_quit_report(asked.clone(), true, false).force_stoppable);
        let gone = not_quit_report(asked, false, false);
        assert!(gone.ok && !gone.force_stoppable, "{}", gone.message);
    }

    /// LIVE, opt-in: drives the real holders + free handlers (nuclear) against ONE app the
    /// tester launched. `MEMFREE_LIVE_QUIT=<bundle id>` names it, `MEMFREE_LIVE_NEVER_TOUCH`
    /// (comma list) is the caller's never-touch. Only the named app is in the request, so
    /// nothing else can be selected. Run: `cargo test -p goose --lib live_nuclear_free --
    /// --ignored --nocapture` with `GOOSE_MEMFREE_SHOW_APPS_MIN_MB=0` for a small app.
    #[tokio::test]
    #[ignore = "live: quits the app named in MEMFREE_LIVE_QUIT"]
    async fn live_nuclear_free_quits_only_the_named_app() {
        let Ok(bundle) = std::env::var("MEMFREE_LIVE_QUIT") else {
            return;
        };
        let never_touch: Vec<String> = std::env::var("MEMFREE_LIVE_NEVER_TOUCH")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let holders = core_memory_holders(
            MlxEngineMemoryHoldersRequest {
                never_touch: never_touch.clone(),
                ..Default::default()
            },
            false,
        )
        .await
        .expect("holders");
        for g in &holders.groups {
            let id = g.id.to_lowercase();
            if g.id == format!("app:{bundle}") || id.contains("teams") {
                println!(
                    "HOLDER {} label={:?} bytes={} pids={:?} nuclearDefault={} protected={:?} why={:?}",
                    g.id,
                    g.label,
                    g.bytes,
                    g.pids.iter().map(|p| p.pid).collect::<Vec<_>>(),
                    g.nuclear_default,
                    g.protected_why,
                    g.why
                );
            }
        }
        let resp = core_memory_free(
            MlxEngineMemoryFreeRequest {
                quit_apps: vec![bundle.clone()],
                nuclear: true,
                never_touch,
                ..Default::default()
            },
            false,
        )
        .await
        .expect("free");
        for item in &resp.items {
            println!(
                "FREED {} ok={} freed={:?} message={:?}",
                item.id, item.ok, item.freed_bytes, item.message
            );
        }
        assert_eq!(resp.items.len(), 1, "only the named app is acted on");
        assert_eq!(resp.items[0].id, format!("app:{bundle}"));
    }
}
