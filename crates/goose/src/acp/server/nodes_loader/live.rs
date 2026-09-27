//! The loader's hands in goosed: every read and every start or stop goes through the SAME ACP
//! handlers Run it calls (`mlxEngine/mount`, `unmount`, `remoteSingleStart` / `Stop`,
//! `distributedStart` / `Stop` / `Discover`, `placementPlan`), so the serving intent, the owner
//! records, the load lock and every refusal behave exactly as for a click.

use async_trait::async_trait;
use goose_sdk_types::custom_requests::{
    MlxEngineDistributedDiscoverRequest, MlxEngineDistributedStartRequest,
    MlxEngineDistributedStopRequest, MlxEngineMountRequest, MlxEnginePlacementPlanRequest,
    MlxEngineRemoteSingleStartRequest, MlxEngineRemoteSingleStopRequest, MlxEngineUnmountRequest,
    MlxFitStatusDto, MlxPlacementActionDto, NodeLoadRefusalCode, NodeResidency, NodesServingKind,
    NodesServingWayDto,
};
use goose_sidecar::distributed::supervisor::RunState;

use super::split_config::{self, SplitPlan};
use super::switch::{
    DistributedFacts, RemoteFacts, Serving, SingleFacts, Stop, SwitchPlan, WayKind, WayRef,
};
use super::{agent, Prepared, Refusal, Residency, Start, Ways, LOOK_AGAIN};
use crate::config::Config;
use crate::nodes::residency;
use crate::nodes::{NodeDef, NodeDefKind, ResolvedNodeDef};
use crate::providers::{mlx_distributed_owner as owner, mlx_remote};

pub(super) struct AgentWays;

fn unknown(reason: impl Into<String>) -> Refusal {
    Refusal::new(NodeLoadRefusalCode::Unknown, reason)
}

fn step(reason: impl Into<String>) -> Refusal {
    Refusal::new(NodeLoadRefusalCode::NeedsStep, reason)
}

async fn mac_name() -> Result<String, String> {
    crate::nodes::acp::this_mac_name().await
}

async fn nodes() -> Result<Vec<ResolvedNodeDef>, Refusal> {
    let read = crate::nodes::read(Config::global(), mac_name().await)
        .map_err(|e| unknown(format!("the nodes config could not be read: {e:#}")))?;
    Ok(read.nodes)
}

/// A stop as the residency rule's serving way, for `names_way`.
fn as_serving_way(stop: &Stop) -> NodesServingWayDto {
    let (kind, macs) = match stop.way.kind {
        WayKind::Local => (
            NodesServingKind::Single,
            vec![crate::nodes::THIS_MAC.to_string()],
        ),
        WayKind::Peer => (
            NodesServingKind::RemoteSingle,
            vec![residency::peer_key(stop.way.peer.as_deref().unwrap_or(""))],
        ),
        WayKind::Split => (NodesServingKind::Split, Vec::new()),
    };
    NodesServingWayDto {
        kind,
        macs,
        link: None,
        model_id: stop.model_id.clone(),
        served_model_id: stop.model_id.clone(),
        mac_names: Vec::new(),
        load_phase: None,
    }
}

#[async_trait]
impl Ways for AgentWays {
    async fn resolve(&self, node: &NodeDef) -> Result<ResolvedNodeDef, Refusal> {
        nodes()
            .await?
            .into_iter()
            .find(|n| n.def.id == node.id)
            .ok_or_else(|| {
                Refusal::new(
                    NodeLoadRefusalCode::UnknownNode,
                    format!("there is no node '{}'", node.id),
                )
            })
    }

    async fn residency(&self, node: &ResolvedNodeDef) -> Result<Residency, Refusal> {
        let name = mac_name().await.unwrap_or_else(|_| "This Mac".to_string());
        let serving = residency::serving_now(&name).await;
        Ok(match residency::residency_of(node, &serving, &[]) {
            NodeResidency::Serving | NodeResidency::AlwaysReady => Residency::Serving,
            NodeResidency::Loading { phase } => Residency::Loading(phase),
            NodeResidency::Unknown { reason } => return Err(unknown(reason)),
            _ => Residency::NotServing,
        })
    }

    async fn serving(&self) -> Result<Serving, Refusal> {
        let single = goose_sidecar::engine::global_manager().status().await;
        let settled = matches!(single.state.as_str(), "mounting" | "running");
        if let (Some(port), false) = (single.stray_listener_port, settled) {
            return Err(step(format!(
                "this Mac's engine on port {port} is not this goose window's; switch it from the window that started it"
            )));
        }
        let remote = match mlx_remote::read() {
            mlx_remote::RouteRecord::Other(route) => {
                return Err(step(format!(
                    "another goose window on this Mac (goosed pid {}) routes chat to {}; switch from that window",
                    route.pid,
                    route.peer_name()
                )))
            }
            mlx_remote::RouteRecord::Unreadable { path, error } => {
                return Err(unknown(format!(
                    "the remote-single route record {} is unreadable ({error}); which way serves is unknown",
                    path.display()
                )))
            }
            mlx_remote::RouteRecord::Mine(_) => {
                let status = super::super::mlx_remote_single::current_status().await;
                Some(RemoteFacts {
                    state: status.state,
                    peer: status.peer,
                    model_id: status.model_id,
                })
            }
            mlx_remote::RouteRecord::Absent | mlx_remote::RouteRecord::Stale(_) => None,
        };
        match owner::read() {
            owner::OwnerRecord::Other(engine) => {
                return Err(step(format!(
                    "the split of another goose window (goosed pid {}) owns this Mac; switch from that window",
                    engine.pid
                )))
            }
            owner::OwnerRecord::Unreadable { path, error } => {
                return Err(unknown(format!(
                    "the distributed-engine owner record {} is unreadable ({error}); which way serves is unknown",
                    path.display()
                )))
            }
            _ => {}
        }
        let distributed = super::super::mlx_distributed::status_response()
            .await
            .map_err(|e| unknown(format!("the split's status could not be read: {e}")))?
            .status;
        Ok(Serving {
            single: Some(SingleFacts {
                state: single.state,
                model_id: single.model_id,
            }),
            remote,
            distributed: Some(DistributedFacts {
                state: distributed.state,
                mode: distributed.mode,
                model_id: distributed.model_id,
            }),
        })
    }

    async fn kept_loaded(&self, stop: &Stop) -> Result<Option<String>, Refusal> {
        let way = as_serving_way(stop);
        Ok(nodes()
            .await?
            .into_iter()
            .find(|n| {
                n.def.kind == NodeDefKind::Mlx && n.def.keep_loaded && residency::names_way(n, &way)
            })
            .map(|n| format!("{} is kept loaded on {}", n.def.name, stop.way.words())))
    }

    async fn prepare(
        &self,
        node: &ResolvedNodeDef,
        target: &WayRef,
        _plan: &SwitchPlan,
    ) -> Result<Prepared, Refusal> {
        let name = &node.def.name;
        let model = node
            .model
            .clone()
            .ok_or_else(|| step(format!("{name} names no model")))?;
        let (_, key) = super::target_of(node)?;
        let wanted = crate::nodes::acp::node_key(node).ok_or_else(|| {
            step(format!(
                "{name} follows this Mac's engine; start it in Run it"
            ))
        })?;
        let agent = agent()?;
        let plans = agent
            .on_mlx_engine_placement_plan(MlxEnginePlacementPlanRequest {
                model_id: Some(model.clone()),
                // A node saved without a goal was chosen for chat: New node's and Run it's goal.
                goal: node
                    .def
                    .goal
                    .unwrap_or(goose_sdk_types::custom_requests::MlxPlacementGoalDto::Chat),
                context: None,
            })
            .await
            .map_err(|e| {
                unknown(format!(
                    "the placement plan for {model} could not be read: {e}"
                ))
            })?;
        let plan = plans
            .plans
            .into_iter()
            .next()
            .ok_or_else(|| unknown(format!("the placement plan named no plan for {model}")))?;
        if let Some(error) = plan.error {
            return Err(unknown(format!("{model} could not be planned: {error}")));
        }
        let Some(candidate) = plan.candidates.iter().find(|c| c.key == wanted) else {
            let notes = if plan.notes.is_empty() {
                String::new()
            } else {
                format!(" ({})", plan.notes.join("; "))
            };
            return Err(step(format!(
                "the plan for {model} has no {} today — a Mac it names may not be connected{notes}",
                target.words()
            )));
        };
        if !candidate.supported {
            return Err(step(format!(
                "goose cannot run {model} as {} yet",
                target.words()
            )));
        }
        if let MlxPlacementActionDto::Unavailable { reason } = &candidate.action {
            return Err(step(format!("{name} needs a step first: {reason}")));
        }
        match candidate.fit.status {
            MlxFitStatusDto::Fits | MlxFitStatusDto::SmallerContext => {}
            MlxFitStatusDto::Short => {
                return Err(Refusal::new(
                    NodeLoadRefusalCode::Fit,
                    format!("Can't load {name}: {}", candidate.fit.detail),
                ))
            }
            MlxFitStatusDto::Unknown => {
                return Err(Refusal::new(
                    NodeLoadRefusalCode::Fit,
                    format!(
                        "Can't load {name}: whether it fits could not be judged — {}",
                        candidate.fit.detail
                    ),
                ))
            }
        }
        let start = match &candidate.action {
            MlxPlacementActionDto::MountHere => Start::MountHere,
            MlxPlacementActionDto::RemoteSingle => Start::RemoteSingle {
                peer: super::switch::peer_of(&wanted.nodes[0]).to_string(),
            },
            MlxPlacementActionDto::StartSplit {
                setup_matches: true,
            } => Start::SplitSaved,
            MlxPlacementActionDto::StartSplit {
                setup_matches: false,
            } => split_for(&agent, name, &model, &wanted.nodes).await?,
            MlxPlacementActionDto::Unavailable { reason } => {
                return Err(step(format!("{name} needs a step first: {reason}")))
            }
        };
        Ok(Prepared { model, key, start })
    }

    async fn stop(&self, stop: &Stop, plan: &SwitchPlan) -> Result<(), String> {
        let agent = agent().map_err(|r| r.reason)?;
        match stop.way.kind {
            WayKind::Local => agent
                .on_mlx_engine_unmount(MlxEngineUnmountRequest { node_id: None })
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
            WayKind::Peer => {
                let stopped = agent
                    .on_mlx_engine_remote_single_stop(MlxEngineRemoteSingleStopRequest {
                        keep_mounted: false,
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                match stopped.unmount_error {
                    // The split runs on that Mac too: it starts only once the peer let go.
                    Some(error) if plan.settle_peer_before_start => Err(error),
                    Some(error) => {
                        tracing::warn!(%error, "nodes loader: the route is withdrawn; the peer kept its engine");
                        Ok(())
                    }
                    None => Ok(()),
                }
            }
            WayKind::Split => {
                agent
                    .on_mlx_engine_distributed_stop(MlxEngineDistributedStopRequest {})
                    .await
                    .map_err(|e| e.to_string())?;
                loop {
                    let status = goose_sidecar::distributed::global_manager().status();
                    if !status.state.owns_the_mac() || status.state == RunState::Failed {
                        return Ok(());
                    }
                    tokio::time::sleep(LOOK_AGAIN).await;
                }
            }
        }
    }

    async fn start(&self, node: &ResolvedNodeDef, prepared: &Prepared) -> Result<(), String> {
        let agent = agent().map_err(|r| r.reason)?;
        let model = &prepared.model;
        match &prepared.start {
            Start::MountHere => {
                let mounted = agent
                    .on_mlx_engine_mount(MlxEngineMountRequest {
                        model_id: model.clone(),
                        node_id: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                if let Some(refused) = mounted.refusal {
                    return Err(refused.fit.message);
                }
                follow_single(model).await
            }
            Start::RemoteSingle { peer } => {
                let started = agent
                    .on_mlx_engine_remote_single_start(MlxEngineRemoteSingleStartRequest {
                        peer: peer.clone(),
                        model_id: model.clone(),
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                if !started.started {
                    return Err(started.refusal.map_or_else(
                        || "the route was not started, and said no reason".to_string(),
                        |r| r.message,
                    ));
                }
                match super::super::mlx_remote_single::follow_route_load().await {
                    Some(result) => result,
                    None => {
                        let state = super::super::mlx_remote_single::current_status()
                            .await
                            .state;
                        if state == "ready" {
                            Ok(())
                        } else {
                            Err(format!(
                                "the route to {peer} reads '{state}' instead of serving {}",
                                node.def.name
                            ))
                        }
                    }
                }
            }
            Start::SplitSaved | Start::SplitFor(_) => {
                let config = match &prepared.start {
                    Start::SplitFor(config) => Some((**config).clone()),
                    _ => None,
                };
                let started = agent
                    .on_mlx_engine_distributed_start(MlxEngineDistributedStartRequest { config })
                    .await
                    .map_err(|e| e.to_string())?;
                if !started.started {
                    return Err(started.refusal.map_or_else(
                        || "the split was not started, and said no reason".to_string(),
                        |r| match r.detail {
                            Some(detail) => format!("{} ({detail})", r.message),
                            None => r.message,
                        },
                    ));
                }
                follow_split().await
            }
        }
    }

    async fn unexplained_requests(&self) -> Option<u32> {
        let status = goose_sidecar::engine::global_manager().status().await;
        status.active_requests.filter(|n| *n > 0)
    }
}

/// Run it's `startSplitFor`: discover the candidate's Macs for this model, build the config from
/// the owner's saved one, and let `splitPlan` name what is missing. Provisioning stays a step.
async fn split_for(
    agent: &crate::acp::server::GooseAcpAgent,
    name: &str,
    model: &str,
    macs: &[String],
) -> Result<Start, Refusal> {
    let peers: Vec<String> = macs
        .iter()
        .filter(|m| m.as_str() != crate::nodes::THIS_MAC)
        .cloned()
        .collect();
    if peers.is_empty() {
        return Err(step(format!(
            "{name}'s split names no other Mac — open Run it › Details › Set up"
        )));
    }
    let discovery = agent
        .on_mlx_engine_distributed_discover(MlxEngineDistributedDiscoverRequest {
            peers,
            model_id: Some(model.to_string()),
        })
        .await
        .map_err(|e| {
            unknown(format!(
                "the split's Macs could not be probed for {model}: {e}"
            ))
        })?
        .discovery;
    let saved = super::super::mlx_distributed::persisted_config()
        .map_err(|e| unknown(format!("the saved split setup could not be read: {e}")))?
        .map(super::super::mlx_distributed::config_to_dto);
    let config = split_config::clean_config(&split_config::config_for(&discovery, saved.as_ref()));
    match split_config::plan(&discovery, model, &config) {
        SplitPlan::Blocker { blocker } => Err(step(split_config::blocker_words(&blocker, model))),
        SplitPlan::Provision { provision } if !provision.is_empty() => Err(step(format!(
            "goose must build its Python on {} before {name} can start — Run it builds it",
            provision.join(" and ")
        ))),
        SplitPlan::Provision { .. } => Ok(Start::SplitFor(Box::new(config))),
    }
}

/// This Mac's engine from `mounting` to its end: running this model, failed, or taken over.
async fn follow_single(model: &str) -> Result<(), String> {
    loop {
        let status = goose_sidecar::engine::global_manager().status().await;
        if status.model_id.as_deref() != Some(model) {
            return Err(format!(
                "this Mac's engine now holds {} — another start superseded this one",
                status.model_id.as_deref().unwrap_or("no model")
            ));
        }
        match status.state.as_str() {
            "running" => return Ok(()),
            "mounting" => tokio::time::sleep(LOOK_AGAIN).await,
            "failed" => {
                return Err(status
                    .last_error
                    .unwrap_or_else(|| "the engine failed and gave no words".to_string()))
            }
            other => {
                return Err(format!(
                    "this Mac's engine reads '{other}' before it answered"
                ))
            }
        }
    }
}

/// The split from its start to its end: ready, failed, or stopped by someone.
async fn follow_split() -> Result<(), String> {
    loop {
        let status = goose_sidecar::distributed::global_manager().status();
        match status.state {
            RunState::Ready | RunState::Serving => return Ok(()),
            RunState::Failed => {
                return Err(status
                    .last_error
                    .unwrap_or_else(|| "the split failed and gave no words".to_string()))
            }
            RunState::Stopped | RunState::Stopping => {
                return Err("the split was stopped before it answered".to_string())
            }
            RunState::Preflight | RunState::Starting | RunState::Recovering => {
                tokio::time::sleep(LOOK_AGAIN).await
            }
        }
    }
}
