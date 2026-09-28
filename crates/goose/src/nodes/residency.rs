//! Per-node residency (design §4.2 `nodes/residency`): serving / loading / waiting / not running /
//! refused last time — from engine truth plus the installed loader's state. The coarse, goosed-side
//! view the nav chip and the chip menu read; the Nodes page's cards derive their richer state
//! (`nodeGlance`) from the stores they already mount.
//!
//! Engine truth is the three records the router reads, in the router's precedence (one MLX way
//! serves this Mac's goose at a time): a live remote-single route, else the split's owner record,
//! else this Mac's single engine (this process's manager, or whatever listens on its port). An
//! unreadable record makes which way serves UNKNOWN — named, never guessed.

use goose_sdk_types::custom_requests::{
    NodeResidency, NodeResidencyDto, NodesServingKind, NodesServingWayDto,
};

use super::seam::LoaderActivity;
use super::{placement_macs, NodeDefKind, NodePlacement, ResolvedNodeDef, THIS_MAC};

/// What serves this Mac's goose now.
#[derive(Debug, Clone, PartialEq)]
pub enum ServingFacts {
    Nothing,
    Way(NodesServingWayDto),
    Unknown(String),
}

/// A peer's placement key, as the placement planner writes it.
pub fn peer_key(peer: &str) -> String {
    format!("link:{peer}")
}

/// Whether `node` names the way serving now (model identity through `node_names_model`, Q-128).
pub fn names_way(node: &ResolvedNodeDef, way: &NodesServingWayDto) -> bool {
    let def = &node.def;
    if def.kind != NodeDefKind::Mlx {
        return false;
    }
    let identity = || {
        node.model.as_deref().is_some_and(|model| {
            goose_sidecar::model_identity::node_names_model(
                &def.id,
                model,
                &way.served_model_id,
                &way.model_id,
            )
        })
    };
    match &def.placement {
        // A node that follows this Mac's engine is served by whatever serves it: this Mac's
        // single or the split probed in its place — never a route to a peer, which refuses this
        // Mac's engine.
        None | Some(NodePlacement::Follows) => way.kind != NodesServingKind::RemoteSingle,
        Some(placement @ NodePlacement::Single { .. }) => {
            let macs = placement_macs(placement).map_or(&[][..], |(m, _)| m);
            let kind_matches = match macs {
                [mac] if mac == THIS_MAC => way.kind == NodesServingKind::Single,
                [_] => way.kind == NodesServingKind::RemoteSingle,
                _ => false,
            };
            kind_matches && way.macs.as_slice() == macs && identity()
        }
        // The split: its Macs as its owner published them (rank order, as the planner keys a
        // split), its link and its model. Tensor or pipeline is not compared — the runner, and so
        // the kind, follows the model's type (`Runner::for_model_type`), so model + Macs + link
        // name one way. A split whose record predates the published way names no Macs: it is
        // matched on model and link, as before the way was published.
        Some(placement) => {
            let (macs, link) = placement_macs(placement).unwrap_or((&[], None));
            way.kind == NodesServingKind::Split
                && (way.macs.is_empty() || way.macs.as_slice() == macs)
                && match (link, way.link.as_deref()) {
                    (Some(a), Some(b)) => a == b,
                    _ => true,
                }
                && identity()
        }
    }
}

/// Words for the way that serves instead ("Qwen3.8-Flash on Work's Mac Studio").
pub fn describe_way(way: &NodesServingWayDto) -> String {
    let model = way.model_id.rsplit('/').next().unwrap_or(&way.model_id);
    match way.kind {
        NodesServingKind::Split => format!("{model} split across {}", way.mac_names.join(" and ")),
        _ => format!("{model} on {}", way.mac_names.join(", ")),
    }
}

pub fn residency_of(
    node: &ResolvedNodeDef,
    serving: &ServingFacts,
    loader: &[LoaderActivity],
) -> NodeResidency {
    if node.def.kind != NodeDefKind::Mlx {
        return NodeResidency::AlwaysReady;
    }
    let activity = loader.iter().find(|a| a.node() == node.def.id);
    // The sessions the loader loads this node for, whichever record reports the phase (Q-382).
    let demanded_by = match activity {
        Some(LoaderActivity::Loading { demanded_by, .. }) => demanded_by.clone(),
        _ => Vec::new(),
    };
    let from_loader = |a: &LoaderActivity| match a {
        LoaderActivity::Loading {
            phase, demanded_by, ..
        } => NodeResidency::Loading {
            phase: phase.clone(),
            demanded_by: demanded_by.clone(),
        },
        LoaderActivity::Waiting {
            reason, replies, ..
        } => NodeResidency::Waiting {
            reason: reason.clone(),
            replies: replies.clone(),
        },
        LoaderActivity::RefusedLastTime { reason, facts, .. } => NodeResidency::RefusedLastTime {
            reason: reason.clone(),
            facts: facts.clone(),
        },
    };
    match serving {
        ServingFacts::Unknown(reason) => {
            activity
                .map(from_loader)
                .unwrap_or_else(|| NodeResidency::Unknown {
                    reason: reason.clone(),
                })
        }
        ServingFacts::Way(way) if names_way(node, way) => match &way.load_phase {
            Some(phase) => NodeResidency::Loading {
                phase: Some(phase.clone()),
                demanded_by,
            },
            None => NodeResidency::Serving,
        },
        ServingFacts::Way(way) => {
            activity
                .map(from_loader)
                .unwrap_or_else(|| NodeResidency::NotRunning {
                    other_way: Some(describe_way(way)),
                })
        }
        ServingFacts::Nothing => activity
            .map(from_loader)
            .unwrap_or(NodeResidency::NotRunning { other_way: None }),
    }
}

pub fn residencies(
    nodes: &[ResolvedNodeDef],
    serving: &ServingFacts,
    loader: &[LoaderActivity],
) -> Vec<NodeResidencyDto> {
    nodes
        .iter()
        .map(|node| NodeResidencyDto {
            node: node.def.id.clone(),
            residency: residency_of(node, serving, loader),
            load: None,
        })
        .collect()
}

/// The live read of what serves this Mac's goose, in the router's precedence.
pub async fn serving_now(this_mac_name: &str) -> ServingFacts {
    use crate::providers::{mlx_distributed_owner as owner, mlx_remote};

    match mlx_remote::read() {
        mlx_remote::RouteRecord::Mine(route) | mlx_remote::RouteRecord::Other(route) => {
            return ServingFacts::Way(NodesServingWayDto {
                kind: NodesServingKind::RemoteSingle,
                macs: vec![peer_key(&route.peer)],
                link: None,
                model_id: route.model_id.clone(),
                served_model_id: route.served_model_id.clone(),
                mac_names: vec![route.peer_name().to_string()],
                load_phase: None,
            })
        }
        mlx_remote::RouteRecord::Unreadable { path, error } => {
            return ServingFacts::Unknown(format!(
                "the remote-single route record {} is unreadable ({error}); which way serves this Mac's chat is unknown",
                path.display()
            ))
        }
        mlx_remote::RouteRecord::Absent | mlx_remote::RouteRecord::Stale(_) => {}
    }
    match owner::read() {
        owner::OwnerRecord::Mine(engine) => match own_split_readiness() {
            SplitReadiness::Serving => return split_serving(&engine, owner::read_way(), None),
            SplitReadiness::Loading(phase) => {
                return split_serving(&engine, owner::read_way(), Some(phase))
            }
            // This goosed's run ended (failed, stopping, stopped): its record is withdrawn at the
            // next status read, and it serves nothing meanwhile — this Mac's single is read.
            SplitReadiness::Over => {}
        },
        owner::OwnerRecord::Other(engine) => {
            // Another window supervises it: its engine answering is the only readiness this
            // process can measure (the record is published at the start, before the ranks load).
            let phase = owner::answering(&engine)
                .await
                .err()
                .map(|_| STARTING.to_string());
            return split_serving(&engine, owner::read_way(), phase);
        }
        owner::OwnerRecord::Unreadable { path, error } => {
            return ServingFacts::Unknown(format!(
                "the distributed-engine owner record {} is unreadable ({error}); which way serves this Mac's chat is unknown",
                path.display()
            ))
        }
        owner::OwnerRecord::Absent | owner::OwnerRecord::Stale(_) => {}
    }
    local_single(this_mac_name).await
}

/// The split as its owner record names it: its Macs are the way its owner published
/// (`read_way` — each Mac's placement key in rank order, `local` for the owner's Mac), so a split
/// node is matched on its Macs, link and model. A record from a goose that predates the way names
/// no Macs (empty — unknown, never guessed); an unreadable way makes what serves unknown.
/// `load_phase`: the split's phase while it has not answered yet (Q-271: a split publishes its
/// record the moment its start is accepted, long before its ranks load — it is never serving
/// then).
pub fn split_serving(
    engine: &crate::providers::mlx_distributed_owner::PublishedEngine,
    way: Result<Option<goose_sdk_types::custom_requests::MlxPlacementKeyDto>, String>,
    load_phase: Option<String>,
) -> ServingFacts {
    let macs = match way {
        Ok(Some(way)) => way.nodes,
        // A record from a goose that predates the way: its Macs are unknown, said by the empty list.
        Ok(None) => Vec::new(),
        Err(e) => {
            return ServingFacts::Unknown(format!(
                "the split's way in its owner record could not be read ({e}); which way serves this Mac's chat is unknown"
            ))
        }
    };
    ServingFacts::Way(NodesServingWayDto {
        kind: NodesServingKind::Split,
        macs,
        link: Some(engine.backend.clone()),
        model_id: engine.model_id.clone(),
        served_model_id: engine.served_model_id.clone(),
        mac_names: engine.node_names.clone(),
        load_phase,
    })
}

/// A way that runs but has not said it is loading yet (goose-sidecar `EngineLoad`'s own word).
const STARTING: &str = "starting";

/// How far THIS goosed's split has come, from its supervisor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitReadiness {
    Serving,
    Loading(String),
    Over,
}

/// The supervisor's run state and its ranks' own phases, as a residency phase: `ready`/`serving`
/// serve; before that the furthest-behind rank's phase (`loading` before `warming`), else
/// `starting` (launched, no rank has reported a phase); a run recovering from a rank's memory
/// death is `recovering`.
#[cfg(unix)]
pub fn split_readiness_of(
    state: goose_sidecar::distributed::supervisor::RunState,
    rank_phases: &[Option<&str>],
) -> SplitReadiness {
    use goose_sidecar::distributed::supervisor::RunState;
    match state {
        RunState::Ready | RunState::Serving => SplitReadiness::Serving,
        RunState::Preflight | RunState::Starting => {
            let has = |p: &str| rank_phases.contains(&Some(p));
            let phase = if has("loading") {
                "loading"
            } else if has("warming") {
                "warming"
            } else {
                STARTING
            };
            SplitReadiness::Loading(phase.to_string())
        }
        RunState::Recovering => SplitReadiness::Loading("recovering".to_string()),
        RunState::Failed | RunState::Stopping | RunState::Stopped => SplitReadiness::Over,
    }
}

#[cfg(unix)]
fn own_split_readiness() -> SplitReadiness {
    let status = goose_sidecar::distributed::global_manager().status();
    let phases: Vec<Option<&str>> = status
        .nodes
        .iter()
        .map(|n| n.load_phase.map(|p| p.as_str()))
        .collect();
    split_readiness_of(status.state, &phases)
}

/// No goosed on this platform supervises a split, so a record naming this process cannot be
/// current: its run is over.
#[cfg(not(unix))]
fn own_split_readiness() -> SplitReadiness {
    SplitReadiness::Over
}

async fn local_single(this_mac_name: &str) -> ServingFacts {
    use crate::config::{Config, ConfigError};
    use goose_sidecar::engine::{parse_model_info, served_model_id, EngineSettings};

    let settings = match Config::global().get_param::<EngineSettings>("mlx_engine") {
        Ok(settings) => settings,
        Err(ConfigError::NotFound(_)) => EngineSettings::default(),
        Err(e) => {
            return ServingFacts::Unknown(format!(
                "the `mlx_engine` config block is unreadable ({e}); which model this Mac's engine serves is unknown"
            ))
        }
    };
    let way = |model_id: String, served: String, load_phase: Option<String>| {
        ServingFacts::Way(NodesServingWayDto {
            kind: NodesServingKind::Single,
            macs: vec![THIS_MAC.to_string()],
            link: None,
            model_id,
            served_model_id: served,
            mac_names: vec![this_mac_name.to_string()],
            load_phase,
        })
    };
    let status = goose_sidecar::engine::global_manager().status().await;
    match (status.state.as_str(), status.model_id.clone()) {
        ("mounting", Some(model)) => {
            // Mounting is never serving (Q-271): the manager reports a load for every mounting
            // state; a mount it has not measured yet is the process starting.
            let served = served_model_id(&settings, &model);
            let phase = status
                .load
                .map_or_else(|| STARTING.to_string(), |l| l.phase);
            return way(model, served, Some(phase));
        }
        ("running", Some(model)) => {
            let served = status
                .served_model_id
                .clone()
                .unwrap_or_else(|| served_model_id(&settings, &model));
            return way(model, served, None);
        }
        _ => {}
    }
    let Some(port) = status.stray_listener_port else {
        return ServingFacts::Nothing;
    };
    // Another goosed's engine (each window runs its own) listens on this Mac's port: ask it.
    let url = format!("http://127.0.0.1:{port}/v1/models");
    let body = match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => resp.text().await.map_err(|e| e.to_string()),
        Ok(resp) => Err(format!("GET {url} answered {}", resp.status())),
        Err(e) => Err(format!("{url} unreachable ({e})")),
    };
    let served = body.and_then(|b| {
        parse_model_info(&b)
            .map_err(|e| format!("{e:#}"))
            .and_then(|(id, _, _)| id.ok_or_else(|| format!("{url} named no model")))
    });
    match served {
        Ok(served) => {
            let repo = goose_sidecar::model_identity::served_repo(&settings, &served);
            way(repo, served, None)
        }
        Err(e) => ServingFacts::Unknown(format!(
            "an engine listens on port {port}, but what it serves could not be read: {e}"
        )),
    }
}
