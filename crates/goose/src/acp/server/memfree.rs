//! ACP surface for "Free up memory": who holds memory on this Mac (goose-sidecar `memfree`
//! census), what freeing it would buy against the one fit rule, and the reclaim itself.
//!
//! Accounting rule (no double count): `budgetBytes` and `gooseOwnedBytes` always add up to what
//! goose itself could reach. With a fit, the fit's budget already credits this goose's own engine
//! (`freed_by_switch_bytes`), so `gooseOwnedBytes` leaves it out; without one, the budget is the
//! live measurement and the own engine's bytes are in `gooseOwnedBytes` when it is idle.

use super::mlx_engine::{core_unmount, fit_to_dto, forget_serving, synced_manager, EngineStopper};
use super::*;
use crate::providers::mlx_serving_intent::IntentKind;
use goose_sidecar::engine::{expand_tilde, MlxEngineManager, StartFrees};
use goose_sidecar::fit::{FitVerdict, NodeMemoryFacts};
use goose_sidecar::memfree::{self, CategorizeCtx, Category, Group, VerdictKind};
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
            bundles: Default::default(),
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

/// Ask each listed model server what it holds; a server with models becomes reclaimable.
async fn enrich_ai_servers(groups: &mut [Group]) -> HashMap<String, AiHold> {
    let mut out = HashMap::new();
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

/// The census grouped, with user apps under the size floor dropped and model servers asked.
async fn groups(
    ctx: &CategorizeCtx,
) -> Result<(Vec<Group>, HashMap<String, AiHold>), agent_client_protocol::Error> {
    let ctx = ctx.clone();
    let min = show_apps_min_bytes();
    let groups = tokio::task::spawn_blocking(move || memfree::groups_now(ctx))
        .await
        .internal_err()?;
    let mut groups: Vec<Group> = groups
        .into_iter()
        .filter(|g| g.category != Category::UserApp || g.bytes >= min)
        .collect();
    let ai = enrich_ai_servers(&mut groups).await;
    Ok((groups, ai))
}

fn is_orphan(g: &Group) -> bool {
    g.category == Category::GooseOrphan && g.reclaimable_by_goose
}

/// Other MLX engines + proven left-over goose servers + loaded models of model servers.
fn goose_owned_outside_engine(groups: &[Group], ai: &HashMap<String, AiHold>) -> u64 {
    groups
        .iter()
        .map(|g| {
            if is_other_mlx(g) || is_orphan(g) {
                g.bytes
            } else {
                ai.get(&g.id).map_or(0, |h| h.reclaim_bytes(g))
            }
        })
        .sum()
}

/// Wait (bounded, transport-style) for a reclaim's memory to leave; the measured remainder.
async fn settle_footprint(pids: &[(u32, u64)], before: u64, expected: u64) -> u64 {
    let target = before.saturating_sub(expected / 2);
    let mut alive = footprint_alive(pids);
    for _ in 0..32 {
        if alive <= target {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        alive = footprint_alive(pids);
    }
    alive
}

async fn unload_ai(g: &Group, hold: &AiHold) -> MlxMemoryFreeItemDto {
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
    let alive = settle_footprint(&g.pids, g.bytes, expected).await;
    MlxMemoryFreeItemDto {
        id: g.id.clone(),
        label: g.label.clone(),
        ok: !failed,
        freed_bytes: Some(g.bytes.saturating_sub(alive)),
        message: messages.join("; "),
    }
}

async fn reap_orphan(g: &Group) -> MlxMemoryFreeItemDto {
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
        freed_bytes: Some(g.bytes.saturating_sub(footprint_alive(&g.pids))),
        message: messages.join("; "),
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

fn is_other_mlx(g: &Group) -> bool {
    g.id.starts_with("mlx:") && g.reclaimable_by_goose
}

fn is_own_engine(g: &Group) -> bool {
    g.category == Category::GooseEngine && g.id == "gooseEngine" && g.reclaimable_by_goose
}

fn quittable_apps(groups: &[Group]) -> Vec<(String, u64)> {
    groups
        .iter()
        .filter(|g| g.category == Category::UserApp && g.quittable)
        .filter_map(|g| g.bundle_id.clone().map(|b| (b, g.bytes)))
        .collect()
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

/// What is still alive of `pids`, by footprint.
fn footprint_alive(pids: &[(u32, u64)]) -> u64 {
    pids.iter()
        .filter(|&&(pid, started)| memfree::alive(pid, started))
        .filter_map(|&(pid, _)| memfree::footprint(pid))
        .sum()
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
            out.push(MlxMemoryAlternativeDto {
                kind: "peerMac".into(),
                node_id: candidate.key.nodes.first().cloned(),
                label: Some(candidate.node_names.join(" + ")),
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
    let (ctx, port) = categorize_ctx(manager).await?;
    let (groups, ai) = groups(&ctx).await?;
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
    let other_mlx = goose_owned_outside_engine(&groups, &ai);
    // The fit already credits the own engine; the bare measurement does not.
    let own: u64 = groups
        .iter()
        .filter(|g| is_own_engine(g))
        .map(|g| g.bytes)
        .sum();
    let goose_owned = if fit.is_some() {
        other_mlx
    } else {
        other_mlx.saturating_add(own)
    };
    let apps = quittable_apps(&groups);
    let apps_bytes: u64 = apps.iter().map(|a| a.1).sum();
    let budget_bytes = match &fit {
        Some((_, f)) => f.budget_bytes,
        None => facts.budget_bytes(),
    };

    let mut verdict = None;
    let mut alts = vec![];
    let short_bytes = fit.as_ref().and_then(|(_, f)| f.short_bytes());
    if let (Some((id, f)), Some(_)) = (&fit, short_bytes) {
        let (ram_short, ceiling_short) = shorts(f);
        let v = memfree::verdict(ram_short, ceiling_short, goose_owned, &apps);
        let room = f
            .facts
            .available_after_switch_bytes()
            .saturating_sub(f.facts.margin_bytes())
            .saturating_add(goose_owned)
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
        apps_bytes,
        short_bytes,
        verdict,
        alternatives: alts,
        quit_apps_allowed: quit_apps_allowed(relayed, relayed && remote_app_quit_allowed()),
        enabled: true,
    })
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
    let (ctx, port) = categorize_ctx(manager).await?;
    let before = measured_facts(port).await?.budget_bytes();
    let mut items = vec![];

    if req.goose_owned {
        // A fresh census: only what is reclaimable NOW is touched.
        let (groups, ai) = groups(&ctx).await?;
        if let Some(g) = groups.iter().find(|g| g.id == "gooseEngine") {
            if g.reclaimable_by_goose {
                let result =
                    core_unmount(MlxEngineUnmountRequest::default(), EngineStopper::Owner).await;
                let (ok, message) = match result {
                    Ok(_) => {
                        forget_serving(IntentKind::Single);
                        (true, format!("stopped {}", g.label))
                    }
                    Err(e) => (false, e.message.clone()),
                };
                items.push(MlxMemoryFreeItemDto {
                    id: g.id.clone(),
                    label: g.label.clone(),
                    ok,
                    freed_bytes: ok.then(|| g.bytes.saturating_sub(footprint_alive(&g.pids))),
                    message,
                });
            } else {
                items.push(MlxMemoryFreeItemDto {
                    id: g.id.clone(),
                    label: g.label.clone(),
                    ok: false,
                    freed_bytes: None,
                    message: format!(
                        "goose left it running: {}",
                        g.why.clone().unwrap_or_else(|| "it is in use".into())
                    ),
                });
            }
        }
        for g in groups.iter().filter(|g| is_other_mlx(g)) {
            let mut messages = vec![];
            let mut ok = true;
            for &(pid, started_at) in &g.pids {
                match machine::stop_other_engine(pid, started_at, port).await {
                    Ok(report) => messages.push(report.message),
                    Err(e) => {
                        ok = false;
                        messages.push(format!("{e:#}"));
                    }
                }
            }
            let freed = g.bytes.saturating_sub(footprint_alive(&g.pids));
            items.push(MlxMemoryFreeItemDto {
                id: g.id.clone(),
                label: g.label.clone(),
                ok,
                freed_bytes: Some(freed),
                message: messages.join("; "),
            });
        }
        for g in &groups {
            if let Some(hold) = ai.get(&g.id) {
                items.push(unload_ai(g, hold).await);
            } else if is_orphan(g) {
                items.push(reap_orphan(g).await);
            }
        }
    }

    let may_quit = quit_apps_allowed(relayed, relayed && remote_app_quit_allowed());
    for bundle_id in &req.quit_apps {
        if !may_quit {
            items.push(MlxMemoryFreeItemDto {
                id: format!("app:{bundle_id}"),
                label: bundle_id.clone(),
                ok: false,
                freed_bytes: None,
                message: format!(
                    "quitting apps from another Mac is off on this Mac ({ALLOW_REMOTE_APP_QUIT_KEY})"
                ),
            });
            continue;
        }
        let outcome = memfree::quit_app(bundle_id, ctx.clone()).await;
        items.push(MlxMemoryFreeItemDto {
            id: outcome.id,
            label: outcome.label,
            ok: outcome.ok,
            freed_bytes: outcome.freed_bytes,
            message: outcome.message,
        });
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
}
