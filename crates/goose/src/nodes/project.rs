//! Tier A (design §7.2): a strategy compiled into the `swarm` block that is handed ONLY to the
//! spawned build (the `SWARM` env var on the child). The global block is never written, so the
//! Benchmark view and `bench_dispatch.mjs` keep measuring the untouched pool (gate 3).
//!
//! What a build can reach decides what this accepts: LeanZero MLX only through this Mac's single
//! engine at `mlx_engine.port`, serving the engine's configured model (the only model the build's
//! own `SidecarEngine` mounts); a split, a single on another Mac, another model, an endpoint and a
//! cloud Planning node are each refused with their reason. LM Studio residents always join (the
//! engine rebuilds the local pool from `lms ps` with `enabled: true`), and that is stated, not
//! hidden. Every other field of the block is kept byte-for-byte.

use std::collections::{HashMap, HashSet};

use goose_sidecar::engine::{served_model_id, EngineSettings, MAX_CONCURRENT_REQUESTS};
use serde_json::{json, Map, Value};

use super::{
    effective_entry, placement_macs, BuildRefusal, BuildRefusalDto, DeviceClass, NodeDef,
    NodeDefKind, NodePlacement, NodeRole, NodeWhen, NodesConfig, PoolView, THIS_MAC,
};
use crate::config::ConfigError;

/// Everything a projection reads. The `swarm` block as a JSON value (so unknown fields survive
/// untouched): `Ok(None)` = no block.
pub struct BuildInputs<'a> {
    pub swarm: Result<Option<Value>, String>,
    pub engine: Result<EngineSettings, String>,
    pub nodes: &'a NodesConfig,
}

impl<'a> BuildInputs<'a> {
    /// From the config reads as they came: an absent `mlx_engine` block is the engine's defaults
    /// (no model configured), exactly as the router reads it; an unreadable one is named.
    pub fn from_reads(
        swarm: Result<Value, ConfigError>,
        engine: &Result<EngineSettings, String>,
        nodes: &'a NodesConfig,
    ) -> Self {
        let swarm = match swarm {
            Ok(value) => Ok(Some(value)),
            Err(ConfigError::NotFound(_)) => Ok(None),
            Err(e) => Err(format!("the `swarm` config block could not be read ({e})")),
        };
        Self {
            swarm,
            engine: engine.clone(),
            nodes,
        }
    }
}

/// A device the projection enables: one already in the pool, or one it adds.
#[derive(Debug, Clone, PartialEq)]
enum Device {
    Existing { id: String, model_id: String },
    New { id: String, value: Value },
}

impl Device {
    fn id(&self) -> &str {
        match self {
            Device::Existing { id, .. } | Device::New { id, .. } => id,
        }
    }

    fn model_id(&self) -> String {
        match self {
            Device::Existing { model_id, .. } => model_id.clone(),
            Device::New { value, .. } => value["model_id"].as_str().unwrap_or_default().to_string(),
        }
    }
}

/// What an eligible strategy projects to.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    planner: Device,
    /// Build's chain in order, with each device's `speed_weight`.
    build: Vec<(Device, u32)>,
    /// What Tier A cannot express, stated.
    pub notes: Vec<String>,
}

fn names(nodes: &NodesConfig) -> HashMap<&str, &str> {
    nodes
        .defs
        .iter()
        .map(|d| (d.id.as_str(), d.name.as_str()))
        .collect()
}

/// The English words of a refusal (the UI's `strategies.builds*` strings carry the same).
pub fn refusal_message(reason: &BuildRefusal, nodes: &NodesConfig) -> String {
    let names = names(nodes);
    let name = |id: &str| names.get(id).map_or(id.to_string(), |n| n.to_string());
    match reason {
        BuildRefusal::UnknownStrategy { strategy } => {
            format!("there is no strategy '{strategy}'")
        }
        BuildRefusal::NoRole { role } => format!(
            "the strategy sets no node for {}",
            super::role_str(*role)
        ),
        BuildRefusal::UnknownNode { node } => format!("there is no node '{node}'"),
        BuildRefusal::Split { node } => format!(
            "{} is a split; swarm builds reach LeanZero MLX only through this Mac's single engine",
            name(node)
        ),
        BuildRefusal::Remote { node, mac } => format!(
            "{} runs on {mac}; swarm builds reach LeanZero MLX only through this Mac's single engine",
            name(node)
        ),
        BuildRefusal::OtherModel {
            node,
            model: Some(model),
        } => format!(
            "swarm builds on this Mac's engine run {model}; choose {}'s model in Run it first",
            name(node)
        ),
        BuildRefusal::OtherModel { node, model: None } => format!(
            "this Mac's engine has no model configured; choose {}'s model in Run it first",
            name(node)
        ),
        BuildRefusal::CloudPlanner { node } => format!(
            "Planning on {}: the engine replaces a cloud planner with a model LM Studio has loaded",
            name(node)
        ),
        BuildRefusal::Endpoint { node } => format!(
            "{} is an endpoint; swarm builds reach cloud models only through a cloud provider",
            name(node)
        ),
        BuildRefusal::LeftPool { node } => {
            format!("{} is no longer in your swarm pool", name(node))
        }
        BuildRefusal::DeviceIdTaken { node } => format!(
            "{}'s id '{node}' is already another device in your swarm pool",
            name(node)
        ),
        BuildRefusal::Unreadable { what, error } => format!("{what}: {error}"),
    }
}

fn dto(reason: BuildRefusal, nodes: &NodesConfig) -> BuildRefusalDto {
    BuildRefusalDto {
        message: refusal_message(&reason, nodes),
        reason,
    }
}

/// `Ok` with what the strategy projects to, or every named reason it cannot drive a build.
pub fn build_eligibility(
    inputs: &BuildInputs,
    strategy_id: &str,
) -> Result<Planned, Vec<BuildRefusalDto>> {
    let nodes = inputs.nodes;
    let mut reasons: Vec<BuildRefusal> = Vec::new();
    let swarm = match &inputs.swarm {
        Ok(v) => v.clone(),
        Err(error) => {
            reasons.push(BuildRefusal::Unreadable {
                what: "the swarm pool".to_string(),
                error: error.clone(),
            });
            None
        }
    };
    let engine = match &inputs.engine {
        Ok(e) => Some(e),
        Err(error) => {
            reasons.push(BuildRefusal::Unreadable {
                what: "this Mac's engine settings".to_string(),
                error: error.clone(),
            });
            None
        }
    };
    let pool: PoolView = match &swarm {
        Some(value) => match serde_json::from_value(value.clone()) {
            Ok(pool) => pool,
            Err(e) => {
                reasons.push(BuildRefusal::Unreadable {
                    what: "the swarm pool's devices".to_string(),
                    error: e.to_string(),
                });
                PoolView::default()
            }
        },
        None => PoolView::default(),
    };
    let Some(strategy) = nodes.strategies.iter().find(|s| s.id == strategy_id) else {
        reasons.push(BuildRefusal::UnknownStrategy {
            strategy: strategy_id.to_string(),
        });
        return Err(finish(reasons, nodes));
    };
    let (Some(engine), true) = (engine, reasons.is_empty()) else {
        return Err(finish(reasons, nodes));
    };
    let defs: HashMap<&str, &NodeDef> = nodes.defs.iter().map(|d| (d.id.as_str(), d)).collect();
    let taken: HashSet<&str> = pool.devices.iter().map(|d| d.id.as_str()).collect();
    let mut notes = vec!["LM Studio models loaded on your fleet also join this build".to_string()];

    let planner = match effective_entry(strategy, NodeRole::Planning).and_then(|e| e.chain.first())
    {
        None => {
            reasons.push(BuildRefusal::NoRole {
                role: NodeRole::Planning,
            });
            None
        }
        Some(first) => match defs.get(first.node.as_str()) {
            None => {
                reasons.push(BuildRefusal::UnknownNode {
                    node: first.node.clone(),
                });
                None
            }
            Some(def) if def.kind == NodeDefKind::Cloud => {
                reasons.push(BuildRefusal::CloudPlanner {
                    node: def.id.clone(),
                });
                None
            }
            Some(def) => device_for(def, &pool, engine, &taken, 1)
                .map_err(|r| reasons.push(r))
                .ok(),
        },
    };
    if effective_entry(strategy, NodeRole::Planning).is_some_and(|e| e.chain.len() > 1) {
        notes.push(
            "Planning's 2nd node is not used: the engine's own planner fallback applies"
                .to_string(),
        );
    }

    let mut build = Vec::new();
    match effective_entry(strategy, NodeRole::Build) {
        None => reasons.push(BuildRefusal::NoRole {
            role: NodeRole::Build,
        }),
        Some(entry) => {
            let len = entry.chain.len() as u32;
            for (index, link) in entry.chain.iter().enumerate() {
                let speed_weight = match (entry.when, index) {
                    (NodeWhen::Share, _) => link.weight,
                    // Failover and overflow have no share: the 1st outweighs every other entry
                    // together (the chain's own length), the rest are equal.
                    (_, 0) => len,
                    _ => 1,
                };
                match defs.get(link.node.as_str()) {
                    None => reasons.push(BuildRefusal::UnknownNode {
                        node: link.node.clone(),
                    }),
                    Some(def) => match device_for(def, &pool, engine, &taken, speed_weight) {
                        Ok(device) => build.push((device, speed_weight)),
                        Err(r) => reasons.push(r),
                    },
                }
            }
        }
    }
    notes.push(
        "Testing, Frontend and Backend take effect when the engine learns roles; this build uses Build for every task"
            .to_string(),
    );

    match planner {
        Some(planner) if reasons.is_empty() => {
            if !build.iter().any(|(d, _)| d.id() == planner.id()) {
                notes.push(
                    "Planning's node also joins the build, because the engine keeps a planner only when a device in the pool carries it"
                        .to_string(),
                );
            }
            Ok(Planned {
                planner,
                build,
                notes,
            })
        }
        _ => Err(finish(reasons, nodes)),
    }
}

fn finish(reasons: Vec<BuildRefusal>, nodes: &NodesConfig) -> Vec<BuildRefusalDto> {
    let mut unique: Vec<BuildRefusal> = Vec::new();
    for r in reasons {
        if !unique.contains(&r) {
            unique.push(r);
        }
    }
    unique.into_iter().map(|r| dto(r, nodes)).collect()
}

/// The device a node becomes in a build, or why it cannot be one.
fn device_for(
    def: &NodeDef,
    pool: &PoolView,
    engine: &EngineSettings,
    taken: &HashSet<&str>,
    speed_weight: u32,
) -> Result<Device, BuildRefusal> {
    let node = def.id.clone();
    if def.kind == NodeDefKind::Endpoint {
        return Err(BuildRefusal::Endpoint { node });
    }
    let pool_device = match &def.pool_device {
        Some(device_id) => match pool.device(device_id) {
            Some(device) => Some(device),
            None => return Err(BuildRefusal::LeftPool { node }),
        },
        None => None,
    };
    let model = pool_device
        .map(|d| d.model_id.clone())
        .or_else(|| def.model.clone())
        .unwrap_or_default();

    if def.kind == NodeDefKind::Mlx {
        match &def.placement {
            None | Some(NodePlacement::Follows) => {}
            Some(NodePlacement::Tensor { .. }) | Some(NodePlacement::Pipeline { .. }) => {
                return Err(BuildRefusal::Split { node })
            }
            Some(placement @ NodePlacement::Single { .. }) => {
                let macs = placement_macs(placement).map(|(m, _)| m).unwrap_or(&[]);
                if macs != [THIS_MAC] {
                    return Err(BuildRefusal::Remote {
                        node,
                        mac: macs.first().cloned().unwrap_or_default(),
                    });
                }
                let Some(configured) = engine.model_id.as_deref() else {
                    return Err(BuildRefusal::OtherModel { node, model: None });
                };
                let served = served_model_id(engine, configured);
                if !goose_sidecar::model_identity::node_names_model(
                    &def.id, &model, &served, configured,
                ) {
                    return Err(BuildRefusal::OtherModel {
                        node,
                        model: Some(configured.to_string()),
                    });
                }
            }
        }
    }

    if let Some(device) = pool_device {
        debug_assert!(matches!(
            device.class(),
            DeviceClass::Mlx | DeviceClass::Cloud
        ));
        return Ok(Device::Existing {
            id: device.id.clone(),
            model_id: device.model_id.clone(),
        });
    }
    if taken.contains(def.id.as_str()) {
        return Err(BuildRefusal::DeviceIdTaken { node });
    }
    let value = match def.kind {
        NodeDefKind::Mlx => {
            // Only a pinned local single of the configured model reaches here (checked above).
            let configured = engine.model_id.as_deref().unwrap_or_default();
            json!({
                "id": def.id,
                "model_id": served_model_id(engine, configured),
                "weight": MAX_CONCURRENT_REQUESTS,
                "enabled": true,
                "instances": 1,
                "engine": "mlx-sidecar",
                "speed_weight": speed_weight,
            })
        }
        _ => {
            let provider = def.provider.clone().unwrap_or_default();
            json!({
                "id": def.id,
                "model_id": model,
                "weight": 1,
                "enabled": true,
                "instances": 1,
                "host": provider,
                "provider": provider,
                "speed_weight": speed_weight,
            })
        }
    };
    Ok(Device::New {
        id: def.id.clone(),
        value,
    })
}

/// The `swarm` block a build on `strategy_id` runs with: Build's chain enabled with its weights,
/// every other sidecar and cloud device disabled, `planner_model` = Planning's 1st, LM Studio
/// devices and every other field untouched.
pub fn project(inputs: &BuildInputs, strategy_id: &str) -> Result<Value, Vec<BuildRefusalDto>> {
    let planned = build_eligibility(inputs, strategy_id)?;
    let mut block = match &inputs.swarm {
        Ok(Some(Value::Object(map))) => map.clone(),
        _ => Map::new(),
    };
    let weights: HashMap<&str, u32> = planned
        .build
        .iter()
        .map(|(d, w)| (d.id(), *w))
        .chain(std::iter::once((planned.planner.id(), 1)))
        .fold(HashMap::new(), |mut acc, (id, w)| {
            acc.entry(id).or_insert(w);
            acc
        });

    // Edited in place, so every key keeps its position and the unchanged block stays byte-for-byte.
    let devices = block
        .entry("devices")
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(devices) = devices.as_array_mut() else {
        return Err(vec![dto(
            BuildRefusal::Unreadable {
                what: "the swarm pool's devices".to_string(),
                error: "`devices` is not a list".to_string(),
            },
            inputs.nodes,
        )]);
    };
    for device in devices.iter_mut() {
        let Ok(view) = serde_json::from_value::<super::PoolDeviceView>(device.clone()) else {
            continue;
        };
        let Some(obj) = device.as_object_mut() else {
            continue;
        };
        match weights.get(view.id.as_str()) {
            Some(w) => {
                obj.insert("enabled".to_string(), Value::Bool(true));
                obj.insert("speed_weight".to_string(), json!(w));
            }
            None if view.class() != DeviceClass::LmStudio => {
                obj.insert("enabled".to_string(), Value::Bool(false));
            }
            None => {}
        }
    }
    let mut added: HashSet<String> = HashSet::new();
    for device in planned
        .build
        .iter()
        .map(|(d, _)| d)
        .chain(std::iter::once(&planned.planner))
    {
        if let Device::New { id, value } = device {
            if added.insert(id.clone()) {
                devices.push(value.clone());
            }
        }
    }
    block.insert(
        "planner_model".to_string(),
        Value::String(planned.planner.model_id()),
    );
    Ok(Value::Object(block))
}
