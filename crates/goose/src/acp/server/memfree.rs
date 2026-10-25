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
use goose_sidecar::{hf, holders, machine};

/// Off switch for the whole feature (default on).
pub(super) const MEMFREE_ENABLED_KEY: &str = "GOOSE_MEMFREE_ENABLED";
/// User apps below this footprint are not listed (and so never offered for quitting).
pub(super) const MEMFREE_SHOW_APPS_MIN_MB_KEY: &str = "GOOSE_MEMFREE_SHOW_APPS_MIN_MB";
const DEFAULT_SHOW_APPS_MIN_MB: u64 = 200;
const MIB: u64 = 1024 * 1024;

fn enabled() -> bool {
    Config::global()
        .get_param::<bool>(MEMFREE_ENABLED_KEY)
        .unwrap_or(true)
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
            bundles: Default::default(),
        },
        port,
    ))
}

/// The census grouped, with user apps under the size floor dropped.
async fn groups(ctx: &CategorizeCtx) -> Result<Vec<Group>, agent_client_protocol::Error> {
    let ctx = ctx.clone();
    let min = show_apps_min_bytes();
    let groups = tokio::task::spawn_blocking(move || memfree::groups_now(ctx))
        .await
        .internal_err()?;
    Ok(groups
        .into_iter()
        .filter(|g| g.category != Category::UserApp || g.bytes >= min)
        .collect())
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
    let groups = groups(&ctx).await?;
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
    let other_mlx: u64 = groups
        .iter()
        .filter(|g| is_other_mlx(g))
        .map(|g| g.bytes)
        .sum();
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
        quit_apps_allowed: !relayed,
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
        let groups = groups(&ctx).await?;
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
    }

    for bundle_id in &req.quit_apps {
        if relayed {
            items.push(MlxMemoryFreeItemDto {
                id: format!("app:{bundle_id}"),
                label: bundle_id.clone(),
                ok: false,
                freed_bytes: None,
                message: "quitting apps from another Mac is not allowed".into(),
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
