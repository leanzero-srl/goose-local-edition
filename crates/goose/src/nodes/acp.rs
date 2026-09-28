//! The body of every `nodes/*` ACP method (the dispatch functions in
//! `acp/server/custom_dispatch.rs` call these and nothing else).

use agent_client_protocol::Error as AcpError;
use goose_sdk_types::custom_requests::{
    MlxPlacementKeyDto, MlxPlacementKindDto, NodeEnsureServing, NodeLoadRefusalCode, NodeResidency,
    NodesBuildEligibilityRequest, NodesBuildEligibilityResponse, NodesEnsureServingRequest,
    NodesEnsureServingResponse, NodesLoadHistoryRequest, NodesLoadHistoryResponse,
    NodesReadRequest, NodesRemoveNodeRequest, NodesRemoveStrategyRequest, NodesResidencyRequest,
    NodesResidencyResponse, NodesServedLastRequest, NodesServedLastResponse,
    NodesSetChatNodesRequest, NodesSetChatNodesResponse, NodesTakeOverNowRequest,
    NodesTakeOverNowResponse, NodesWriteRequest,
};
use goose_sidecar::engine::EngineSettings;

use super::residency::{self, ServingFacts};
use super::{
    project, seam, served, NodeDefKind, NodesReadResponse, NodesWriteResponse, ResolvedNodeDef,
    SWARM_KEY, SWARM_PROVIDER,
};
use crate::config::{Config, ConfigError};
use crate::session::SessionManager;

fn internal(e: impl std::fmt::Display) -> AcpError {
    AcpError::internal_error().data(e.to_string())
}

/// This Mac's name (macOS ComputerName) — what adoption names this Mac's engine node.
pub async fn this_mac_name() -> Result<String, String> {
    let out = tokio::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .output()
        .await
        .map_err(|e| format!("scutil: {e}"))?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() {
        return Err(format!(
            "scutil printed no ComputerName: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(name)
}

/// The `mlx_engine` block as the router reads it: absent = the engine's defaults (no model
/// configured); unreadable = named.
pub fn engine_settings(config: &Config) -> Result<EngineSettings, String> {
    match config.get_param::<EngineSettings>("mlx_engine") {
        Ok(settings) => Ok(settings),
        Err(ConfigError::NotFound(_)) => Ok(EngineSettings::default()),
        Err(e) => Err(format!("the `mlx_engine` config block is unreadable ({e})")),
    }
}

pub async fn read(_req: NodesReadRequest) -> Result<NodesReadResponse, AcpError> {
    super::read(Config::global(), this_mac_name().await).map_err(internal)
}

pub async fn write(req: NodesWriteRequest) -> Result<NodesWriteResponse, AcpError> {
    let config = Config::global();
    let engine = engine_settings(config);
    super::write(
        config,
        req.config,
        super::WriteFacts {
            this_mac: this_mac_name().await,
            engine: &engine,
        },
    )
    .map_err(internal)
}

/// Sessions set to `node:<id>` (the removal's acknowledgement counts them).
async fn sessions_on_node(session_manager: &SessionManager, id: &str) -> Result<u32, AcpError> {
    let model = super::format_route_model(&super::RouteModel::Node { id: id.to_string() });
    let sessions = session_manager.list_sessions().await.map_err(internal)?;
    Ok(sessions
        .iter()
        .filter(|s| {
            s.provider_name.as_deref() == Some(SWARM_PROVIDER)
                && s.model_config
                    .as_ref()
                    .is_some_and(|m| m.model_name == model)
        })
        .count() as u32)
}

pub async fn remove_node(
    session_manager: &SessionManager,
    req: NodesRemoveNodeRequest,
) -> Result<NodesWriteResponse, AcpError> {
    let config = Config::global();
    let engine = engine_settings(config);
    let live_sessions = sessions_on_node(session_manager, &req.id).await?;
    super::remove_node(
        config,
        super::RemoveNode {
            id: &req.id,
            also_from_strategies: req.also_from_strategies,
            also_from_chat_node_sets: req.also_from_chat_node_sets,
            and_new_chats_auto: req.and_new_chats_auto,
            acknowledged_sessions: req.acknowledged_sessions,
            live_sessions,
        },
        super::WriteFacts {
            this_mac: this_mac_name().await,
            engine: &engine,
        },
    )
    .map_err(internal)
}

/// A chat's own nodes, stored through the one write door. The chat must exist: a set for a
/// session nobody has would be kept for nothing. The caller sets the chat's model to `model`.
pub async fn set_chat_nodes(
    session_manager: &SessionManager,
    req: NodesSetChatNodesRequest,
) -> Result<NodesSetChatNodesResponse, AcpError> {
    session_manager
        .get_session(&req.session, false)
        .await
        .map_err(|e| {
            AcpError::invalid_params().data(format!("there is no chat '{}' ({e})", req.session))
        })?;
    let config = Config::global();
    let engine = engine_settings(config);
    let write = super::set_chat_nodes(
        config,
        super::SetChatNodes {
            session: &req.session,
            nodes: req.nodes,
            answer_on_next: req.answer_on_next,
        },
        super::WriteFacts {
            this_mac: this_mac_name().await,
            engine: &engine,
        },
    )
    .map_err(internal)?;
    let model = write
        .written
        .then(|| super::chat_set_route(&write.read.config, &req.session))
        .flatten();
    Ok(NodesSetChatNodesResponse { write, model })
}

/// A deleted chat's node set goes with it. The chat is already deleted, so a set that could not be
/// removed is said in the log by its chat, never raised as the delete's failure.
pub async fn forget_chat(session: &str) {
    let config = Config::global();
    let engine = engine_settings(config);
    let facts = super::WriteFacts {
        this_mac: this_mac_name().await,
        engine: &engine,
    };
    match super::forget_chat(config, session, facts) {
        Ok(Some(write)) if !write.written => tracing::error!(
            %session,
            refusals = ?write.refusals,
            "nodes: the deleted chat's node set was refused removal; it stays in the nodes config"
        ),
        Ok(_) => {}
        Err(e) => tracing::error!(
            %session,
            error = %format!("{e:#}"),
            "nodes: the deleted chat's node set could not be removed; it stays in the nodes config"
        ),
    }
}

pub async fn remove_strategy(
    req: NodesRemoveStrategyRequest,
) -> Result<NodesWriteResponse, AcpError> {
    let config = Config::global();
    let engine = engine_settings(config);
    super::remove_strategy(
        config,
        super::RemoveStrategy {
            id: &req.id,
            and_new_chats_auto: req.and_new_chats_auto,
            and_builds_pool: req.and_builds_pool,
        },
        super::WriteFacts {
            this_mac: this_mac_name().await,
            engine: &engine,
        },
    )
    .map_err(internal)
}

pub async fn build_eligibility(
    req: NodesBuildEligibilityRequest,
) -> Result<NodesBuildEligibilityResponse, AcpError> {
    let config = Config::global();
    let current = super::read(config, this_mac_name().await).map_err(internal)?;
    let engine = engine_settings(config);
    let mut nodes = current.config;
    if let Some(draft) = req.draft {
        super::with_draft_strategy(&mut nodes, &req.strategy, draft);
    }
    let inputs = project::BuildInputs::from_reads(
        config.get_param::<serde_json::Value>(SWARM_KEY),
        &engine,
        &nodes,
    );
    Ok(match project::build_eligibility(&inputs, &req.strategy) {
        Ok(planned) => NodesBuildEligibilityResponse {
            eligible: true,
            reasons: Vec::new(),
            notes: planned.notes,
        },
        Err(reasons) => NodesBuildEligibilityResponse {
            eligible: false,
            reasons,
            notes: Vec::new(),
        },
    })
}

pub async fn residency(_req: NodesResidencyRequest) -> Result<NodesResidencyResponse, AcpError> {
    let mac = this_mac_name().await;
    let current = super::read(Config::global(), mac.clone()).map_err(internal)?;
    let mac_name = mac.unwrap_or_else(|_| "This Mac".to_string());
    let serving = residency::serving_now(&mac_name).await;
    let loader = seam::in_progress();
    let mut nodes = residency::residencies(&current.nodes, &serving, &loader);
    let loads_error = with_load_medians(&current.nodes, &mut nodes);
    // A displaced node that serves again (any hand brought it back) has no notice left.
    let back: Vec<&str> = nodes
        .iter()
        .filter(|n| matches!(n.residency, NodeResidency::Serving))
        .map(|n| n.node.as_str())
        .collect();
    for node in &back {
        seam::forget_displaced(node);
    }
    let (serving, serving_error) = match serving {
        ServingFacts::Nothing => (None, None),
        ServingFacts::Way(way) => (Some(way), None),
        ServingFacts::Unknown(reason) => (None, Some(reason)),
    };
    Ok(NodesResidencyResponse {
        nodes,
        serving,
        serving_error,
        loader_installed: seam::loader_installed(),
        displaced: seam::displaced(),
        loads_error,
        behind_switches: seam::behind_switches(),
    })
}

/// Q-443: the person takes the Mac over for this chat's waiting turn only.
pub async fn take_over_now(
    req: NodesTakeOverNowRequest,
) -> Result<NodesTakeOverNowResponse, AcpError> {
    Ok(NodesTakeOverNowResponse {
        taken: seam::take_over_now(&req.session_id, &req.node),
    })
}

/// Each pinned MLX node's measured load (the load store's Ready median for its own way and
/// model). `Some(words)` = the store could not be read, so no node carries a measurement.
#[cfg(unix)]
fn with_load_medians(
    resolved: &[ResolvedNodeDef],
    nodes: &mut [goose_sdk_types::custom_requests::NodeResidencyDto],
) -> Option<String> {
    use goose_sdk_types::custom_requests::NodeLoadMedianDto;
    use goose_sidecar::placement::loads::{median_for, LoadStore, LOADS_FILE};

    let store = LoadStore::new(crate::config::paths::Paths::in_data_dir(LOADS_FILE));
    let read = match store.read() {
        Ok(read) => read,
        Err(e) => return Some(format!("{e:#}")),
    };
    for dto in nodes.iter_mut() {
        let Some(node) = resolved.iter().find(|n| n.def.id == dto.node) else {
            continue;
        };
        if node_key(node).is_none() {
            continue;
        }
        let groups = load_groups(node, &read.records, |model, key| {
            median_for(&read.records, model, key)
        });
        dto.load = match groups.as_slice() {
            [group] => group.median_total_ms.map(|median_ms| NodeLoadMedianDto {
                median_ms,
                count: group.count,
            }),
            _ => None,
        };
    }
    None
}

#[cfg(not(unix))]
fn with_load_medians(
    _resolved: &[ResolvedNodeDef],
    _nodes: &mut [goose_sdk_types::custom_requests::NodeResidencyDto],
) -> Option<String> {
    Some("the MLX load store requires macOS".to_string())
}

/// The node's way as a placement key (`None` for a node that follows this Mac's engine).
pub fn node_key(node: &ResolvedNodeDef) -> Option<MlxPlacementKeyDto> {
    let placement = node.def.placement.as_ref()?;
    let (macs, link) = super::placement_macs(placement)?;
    let kind = match placement {
        super::NodePlacement::Single { .. } => MlxPlacementKindDto::Single,
        super::NodePlacement::Tensor { .. } => MlxPlacementKindDto::Tensor,
        super::NodePlacement::Pipeline { .. } => MlxPlacementKindDto::Pipeline,
        super::NodePlacement::Follows => return None,
    };
    Some(MlxPlacementKeyDto {
        kind,
        nodes: macs.to_vec(),
        link: link.map(str::to_string),
    })
}

#[cfg(unix)]
pub async fn load_history(
    req: NodesLoadHistoryRequest,
) -> Result<NodesLoadHistoryResponse, AcpError> {
    use goose_sidecar::placement::loads::{median_for, LoadStore, LOADS_FILE};

    let current = super::read(Config::global(), this_mac_name().await).map_err(internal)?;
    let node = current
        .nodes
        .iter()
        .find(|n| n.def.id == req.node)
        .ok_or_else(|| {
            AcpError::invalid_params().data(format!("there is no node '{}'", req.node))
        })?;
    let store = LoadStore::new(crate::config::paths::Paths::in_data_dir(LOADS_FILE));
    let read = store.read().map_err(|e| internal(format!("{e:#}")))?;
    let groups = load_groups(node, &read.records, |model, key| {
        median_for(&read.records, model, key)
    });
    Ok(NodesLoadHistoryResponse {
        groups,
        store_errors: read.unreadable,
        path: store.path().display().to_string(),
    })
}

#[cfg(not(unix))]
pub async fn load_history(
    _req: NodesLoadHistoryRequest,
) -> Result<NodesLoadHistoryResponse, AcpError> {
    Err(AcpError::internal_error().data("the MLX load store requires macOS"))
}

/// The node's loads grouped per way: one group for a pinned node (its way), one per way for a
/// node that follows this Mac's engine. A cloud node has no loads.
#[cfg(unix)]
pub fn load_groups(
    node: &ResolvedNodeDef,
    records: &[goose_sidecar::placement::loads::LoadRecord],
    median: impl Fn(
        &str,
        &goose_sidecar::placement::store::PlacementKey,
    ) -> Option<goose_sidecar::placement::loads::LoadMedian>,
) -> Vec<goose_sdk_types::custom_requests::NodeLoadGroupDto> {
    use goose_sdk_types::custom_requests::{
        NodeLoadGroupDto, NodeLoadOutcomeDto, NodeLoadPhasesMsDto, NodeLoadRecordDto,
    };
    use goose_sidecar::placement::loads::LoadOutcome;
    use goose_sidecar::placement::store::{PlacementKey, PlacementKind};

    let Some(model) = node.model.as_deref() else {
        return Vec::new();
    };
    if node.def.kind != NodeDefKind::Mlx {
        return Vec::new();
    }
    let to_dto_key = |k: &PlacementKey| MlxPlacementKeyDto {
        kind: match k.kind {
            PlacementKind::Single => MlxPlacementKindDto::Single,
            PlacementKind::Tensor => MlxPlacementKindDto::Tensor,
            PlacementKind::Pipeline => MlxPlacementKindDto::Pipeline,
        },
        nodes: k.nodes.clone(),
        link: k.link.clone(),
    };
    let wanted = node_key(node);
    let names_model = |row_model: &str| {
        goose_sidecar::model_identity::node_names_model(&node.def.id, model, row_model, row_model)
    };
    let mut keys: Vec<PlacementKey> = Vec::new();
    for row in records {
        if !names_model(&row.model) || keys.contains(&row.placement) {
            continue;
        }
        if wanted
            .as_ref()
            .is_none_or(|w| *w == to_dto_key(&row.placement))
        {
            keys.push(row.placement.clone());
        }
    }
    if let (Some(w), true) = (&wanted, keys.is_empty()) {
        // A pinned node always answers for its own way: "not measured yet" is a group with no
        // median, never a missing answer.
        return vec![NodeLoadGroupDto {
            placement: w.clone(),
            median_total_ms: None,
            count: 0,
            records: Vec::new(),
        }];
    }
    keys.iter()
        .map(|key| {
            let rows: Vec<_> = records
                .iter()
                .filter(|r| names_model(&r.model) && &r.placement == key)
                .collect();
            let m = rows.first().and_then(|r| median(&r.model, key));
            NodeLoadGroupDto {
                placement: to_dto_key(key),
                median_total_ms: m.map(|m| m.total_ms),
                count: m.map_or(0, |m| m.count as u32),
                records: rows
                    .into_iter()
                    .map(|r| NodeLoadRecordDto {
                        model: r.model.clone(),
                        placement: to_dto_key(&r.placement),
                        macs: r.macs.clone(),
                        weights_bytes: r.weights_bytes,
                        phases_ms: NodeLoadPhasesMsDto {
                            starting: r.phases_ms.starting,
                            loading: r.phases_ms.loading,
                            warming: r.phases_ms.warming,
                        },
                        total_ms: r.total_ms,
                        file_cache_warm: r.file_cache_warm,
                        outcome: match &r.outcome {
                            LoadOutcome::Ready => NodeLoadOutcomeDto::Ready,
                            LoadOutcome::Failed { words } => NodeLoadOutcomeDto::Failed {
                                words: words.clone(),
                            },
                            LoadOutcome::CancelledAfterStop => {
                                NodeLoadOutcomeDto::CancelledAfterStop
                            }
                        },
                        recorded_at_ms: r.recorded_at_ms,
                    })
                    .collect(),
            }
        })
        .collect()
}

pub async fn served_last(
    session_manager: &SessionManager,
    req: NodesServedLastRequest,
) -> Result<NodesServedLastResponse, AcpError> {
    let record = served::last(session_manager, &req.session_id)
        .await
        .map_err(|e| internal(format!("{e:#}")))?;
    Ok(NodesServedLastResponse { record })
}

/// A cloud or endpoint node is always ready, and a node that already serves needs no loader;
/// everything else goes to the installed loader through the seam — or, with none installed, is
/// refused by name.
pub async fn ensure_serving(
    req: NodesEnsureServingRequest,
) -> Result<NodesEnsureServingResponse, AcpError> {
    let mac = this_mac_name().await;
    let current = super::read(Config::global(), mac.clone()).map_err(internal)?;
    let Some(node) = current.nodes.iter().find(|n| n.def.id == req.node) else {
        return Ok(NodesEnsureServingResponse {
            answer: NodeEnsureServing::Refused {
                code: NodeLoadRefusalCode::UnknownNode,
                reason: format!("there is no node '{}'", req.node),
                facts: None,
            },
        });
    };
    if node.def.kind != NodeDefKind::Mlx {
        return Ok(NodesEnsureServingResponse {
            answer: NodeEnsureServing::Ready,
        });
    }
    let mac_name = mac.unwrap_or_else(|_| "This Mac".to_string());
    let serving = residency::serving_now(&mac_name).await;
    let answer = match residency::residency_of(node, &serving, &seam::in_progress()) {
        NodeResidency::Serving => NodeEnsureServing::Ready,
        NodeResidency::Unknown { reason } if !seam::loader_installed() => {
            NodeEnsureServing::Refused {
                code: NodeLoadRefusalCode::Unknown,
                reason,
                facts: None,
            }
        }
        _ => {
            seam::ensure_serving(seam::Demand {
                node: node.def.clone(),
                from: req
                    .session_id
                    .map_or(seam::DemandFrom::Ui, seam::DemandFrom::Turn),
                role: None,
                // A person's Start (or a caller naming a node outright) takes the Mac over as it
                // always did; the running reply is still never cut.
                if_serving_other: goose_sdk_types::custom_requests::NodeIfServingOther::TakeOver,
            })
            .await
        }
    };
    Ok(NodesEnsureServingResponse { answer })
}
