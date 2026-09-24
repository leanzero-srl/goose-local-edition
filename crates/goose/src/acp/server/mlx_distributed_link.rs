//! LeanZero Link as the distributed MLX engine's discovery and control plane — goose's halves of
//! `goose_sidecar::distributed::{link_control, link_host}`:
//!
//! - [`GoosedDistributedNode`]: THIS Mac as a node of another Mac's engine, served on the control
//!   route `POST /v1/swarm/distributed/<op>` behind the owner's switch
//!   [`ALLOW_DISTRIBUTED_NODE_KEY`] (off by default, read on every request). Discovery runs goose's
//!   own probe here; every other op is the sidecar's host.
//! - [`GoosedLinkTransport`]: this Mac as the REQUESTER — the sidecar's `link:` nodes reached
//!   through the `LinkManager`'s peer proxy.
//! - [`link_discovery`]: the same-account Macs on the mesh, each described by its own goosed.
//! - The hosting record: a rank this goosed serves is published under the state dir, so another
//!   window's goosed on this Mac refuses its single engine too (one engine owns a Mac).

use std::sync::{Arc, OnceLock};

use goose_sdk_types::custom_requests::{
    MlxDistributedHostedRankDto, MlxDistributedLinkDiscoveryDto, MlxDistributedLinkPeerDto,
    MlxDistributedLinkPeerModelDto, MlxDistributedLinkPortDto, MlxDistributedLinkRdmaDto,
};
use goose_sidecar::distributed::exec::{BoxFuture, NodeExec, SystemExec};
use goose_sidecar::distributed::link_control::{
    self, DiscoverRequest, ExecAnswer, LinkCallError, LinkOp, LinkRefusal, LinkTransport, Requester,
};
use goose_sidecar::distributed::link_host::{self, HostError, HostedRankStatus};
use leanzero_link::manager::{AuthState, LinkError};
use leanzero_link::netpath::InterfaceKind;
use leanzero_link::state::{DistributedNode, DistributedNodeError};
use leanzero_link::wire::NodeStatus;
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::link::{existing_link_manager, hostname_string, stable_node_id};
use super::mlx_distributed_discover as discover;
use crate::config::paths::Paths;
use crate::config::{Config, ConfigError};
use crate::providers::mlx_distributed_owner::{self as owner_record, OwnerRecord};

/// The owner's switch: a goose config key, or the same name as an env override
/// (`Config::get_param` upper-cases the key). Absent = OFF — a Mac serves no rank until its owner
/// opts in; while off the control route answers 403 `servingDisabled` to every distributed op.
pub const ALLOW_DISTRIBUTED_NODE_KEY: &str = "LEANZERO_LINK_ALLOW_DISTRIBUTED_NODE";

/// Read the switch. `NotFound` is the documented default (OFF); an unreadable value is logged
/// and read as OFF — fail closed, never silently open.
pub(super) fn distributed_node_allowed() -> bool {
    match Config::global().get_param::<bool>(ALLOW_DISTRIBUTED_NODE_KEY) {
        Ok(value) => value,
        Err(ConfigError::NotFound(_)) => false,
        Err(error) => {
            warn!(
                %error,
                key = ALLOW_DISTRIBUTED_NODE_KEY,
                "leanzeroLink: the distributed-node setting is unreadable; treating it as OFF"
            );
            false
        }
    }
}

fn host_error(error: HostError) -> DistributedNodeError {
    match error {
        HostError::UnknownOp(m) => DistributedNodeError::UnknownOp(m),
        HostError::BadRequest(m) => DistributedNodeError::BadRequest(m),
        HostError::Refused(r) => DistributedNodeError::Refused {
            code: r.code,
            message: r.message,
        },
        HostError::Failed(m) => DistributedNodeError::Failed(m),
    }
}

/// THIS Mac as a node of another Mac's distributed engine (the control route's seam).
pub struct GoosedDistributedNode;

#[async_trait::async_trait]
impl DistributedNode for GoosedDistributedNode {
    fn serving_allowed(&self) -> bool {
        distributed_node_allowed()
    }

    async fn dispatch(
        &self,
        op: &str,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, DistributedNodeError> {
        let Some(op) = LinkOp::from_path(op) else {
            return Err(DistributedNodeError::UnknownOp(format!(
                "unknown distributed op '{op}' (this goose serves: {})",
                LinkOp::ALL.map(LinkOp::path).join(", ")
            )));
        };
        ensure_link_transport();
        match op {
            LinkOp::Discover => discover_here(request).await,
            LinkOp::RankStart => {
                // Another window's goosed on this Mac runs a distributed engine (the sidecar's own
                // check sees only this process's run).
                if let OwnerRecord::Other(engine) = owner_record::read() {
                    return Err(DistributedNodeError::Refused {
                        code: "distributedEngineActive".to_string(),
                        message: format!(
                            "another goose window on this Mac (goosed pid {}) runs a distributed \
                             engine ('{}' at {}); one engine owns a Mac at a time",
                            engine.pid, engine.served_model_id, engine.base_url
                        ),
                    });
                }
                link_host::dispatch(op, request).await.map_err(host_error)
            }
            _ => link_host::dispatch(op, request).await.map_err(host_error),
        }
    }
}

/// Goose's discovery probe, run by this Mac on itself, over its own models dir as well as the
/// roots the requester names.
async fn discover_here(
    request: serde_json::Value,
) -> Result<serde_json::Value, DistributedNodeError> {
    let request: DiscoverRequest = serde_json::from_value(request)
        .map_err(|e| DistributedNodeError::BadRequest(format!("discover request: {e}")))?;
    let mut roots = request.extra_roots;
    let settings = super::mlx_engine::load_engine_settings()
        .map_err(|e| DistributedNodeError::Failed(format!("this Mac's MLX settings: {e:?}")))?;
    roots.push(
        goose_sidecar::engine::expand_tilde(&settings.models_dir)
            .display()
            .to_string(),
    );
    let out = SystemExec
        .run(None, &discover::discover_script(&roots))
        .await
        .map_err(|e| DistributedNodeError::Failed(format!("{e:#}")))?;
    serde_json::to_value(ExecAnswer::from(out))
        .map_err(|e| DistributedNodeError::Failed(format!("encoding the probe: {e}")))
}

fn link_call_error(error: LinkError) -> LinkCallError {
    match error {
        LinkError::DistributedNode(DistributedNodeError::Disabled(m)) => LinkCallError::Disabled(m),
        LinkError::DistributedNode(DistributedNodeError::UnknownOp(m)) => {
            LinkCallError::NotServed(m)
        }
        LinkError::DistributedNode(DistributedNodeError::BadRequest(m)) => {
            LinkCallError::BadRequest(m)
        }
        LinkError::DistributedNode(DistributedNodeError::Refused { code, message }) => {
            LinkCallError::Refused(LinkRefusal { code, message })
        }
        LinkError::DistributedNode(DistributedNodeError::Failed(m)) => LinkCallError::Failed(m),
        LinkError::DistributedProxy(m) if m.contains("peer returned 501") => {
            LinkCallError::NotServed(format!(
                "its goose does not serve distributed nodes ({m}); update goose there"
            ))
        }
        LinkError::NotConnected => LinkCallError::NotConnected(
            "this Mac is not connected to the LeanZero Link mesh".to_string(),
        ),
        LinkError::UnknownPeer(id) => LinkCallError::Unreachable(format!(
            "no Link peer '{id}' on the mesh (offline, or signed into another account)"
        )),
        other => LinkCallError::Unreachable(other.to_string()),
    }
}

/// This Mac as the REQUESTER: a `link:` node's ops through the `LinkManager`'s peer proxy.
pub struct GoosedLinkTransport;

impl LinkTransport for GoosedLinkTransport {
    fn call<'a>(
        &'a self,
        peer: &'a str,
        op: LinkOp,
        body: serde_json::Value,
    ) -> BoxFuture<'a, Result<serde_json::Value, LinkCallError>> {
        Box::pin(async move {
            let manager = existing_link_manager().ok_or_else(|| {
                LinkCallError::NotConnected(
                    "LeanZero Link has not started in this goose — sign in and connect first"
                        .to_string(),
                )
            })?;
            manager
                .distributed_proxy(peer, op.path(), &body)
                .await
                .map_err(link_call_error)
        })
    }

    fn requester(&self) -> BoxFuture<'_, Result<Requester, LinkCallError>> {
        Box::pin(async move {
            let hostname = hostname_string();
            Ok(Requester {
                node_id: stable_node_id(),
                name: computer_name().await.unwrap_or_else(|| hostname.clone()),
                hostname,
            })
        })
    }
}

/// `scutil --get ComputerName` — the name the owner gave this Mac.
async fn computer_name() -> Option<String> {
    let out = tokio::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .output()
        .await
        .ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !name.is_empty()).then_some(name)
}

/// Install the transport (and the hosting record's observer) once per process. Every
/// distributed entry point calls it, so a `link:` node is reachable from the first request.
pub(super) fn ensure_link_transport() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        link_control::install_transport(Arc::new(GoosedLinkTransport));
        link_host::observe(Box::new(|hosted| {
            let result = match hosted {
                Some(status) => publish_hosting(status),
                None => withdraw_hosting(),
            };
            if let Err(e) = result {
                warn!(error = %e, path = %hosting_path().display(), "the distributed hosting record could not be written; other windows will not see this Mac's rank");
            }
        }));
    });
}

/// The rank this goosed serves, for the status DTO.
pub(super) fn hosting_dto() -> Option<MlxDistributedHostedRankDto> {
    link_host::hosting().map(hosted_dto)
}

fn hosted_dto(status: HostedRankStatus) -> MlxDistributedHostedRankDto {
    MlxDistributedHostedRankDto {
        rank: status.rank as u32,
        size: status.size as u32,
        requester_name: status.requester.name,
        requester_node_id: status.requester.node_id,
        requester_hostname: status.requester.hostname,
        model_id: status.model_id,
        served_model_id: status.served_model_id,
        backend: status.backend.as_str().to_string(),
        runner: status.runner.as_str().to_string(),
        pid: status.pid,
        state: status.state,
        started_ms: status.started_ms,
        last_poll_ms: status.last_poll_ms,
    }
}

// ---------------------------------------------------------------------------
// The hosting record: another goosed on this Mac (another window) must refuse its single engine
// while this one serves a rank.
// ---------------------------------------------------------------------------

const HOSTING_FILE: &str = "mlx-distributed-hosting.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct HostingRecord {
    pid: u32,
    rank: usize,
    requester: String,
    model_id: String,
}

fn hosting_path() -> std::path::PathBuf {
    Paths::in_state_dir(HOSTING_FILE)
}

fn publish_hosting(status: &HostedRankStatus) -> anyhow::Result<()> {
    let path = hosting_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let record = HostingRecord {
        pid: std::process::id(),
        rank: status.rank,
        requester: status.requester.name.clone(),
        model_id: status.model_id.clone(),
    };
    let tmp = path.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&record)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn withdraw_hosting() -> anyhow::Result<()> {
    let path = hosting_path();
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<HostingRecord>(&bytes) {
            Ok(record) if record.pid != std::process::id() => Ok(()),
            _ => Ok(std::fs::remove_file(&path)?),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Why this Mac's single engine may not mount because A rank is served here — by this goosed or
/// by another window's (its record, with its goosed alive). An unreadable record refuses too:
/// the mount cannot prove the Mac is free.
pub(super) fn hosting_refusal() -> Option<String> {
    if let Some(status) = link_host::hosting() {
        return Some(format!(
            "hostingRank: this Mac serves rank {} of {}'s distributed engine ('{}') over LeanZero \
             Link; one engine owns a Mac at a time — stop it from {} first",
            status.rank, status.requester.name, status.model_id, status.requester.name
        ));
    }
    let bytes = match std::fs::read(hosting_path()) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            return Some(format!(
                "hostingRank: the hosting record {} is unreadable ({e}); refusing rather than \
                 guessing this Mac is free",
                hosting_path().display()
            ))
        }
    };
    match serde_json::from_slice::<HostingRecord>(&bytes) {
        Ok(record) if record.pid == std::process::id() => None,
        Ok(record) if pid_alive(record.pid) => Some(format!(
            "hostingRank: another goose window on this Mac (goosed pid {}) serves rank {} of {}'s \
             distributed engine ('{}'); one engine owns a Mac at a time",
            record.pid, record.rank, record.requester, record.model_id
        )),
        Ok(_) => None,
        Err(e) => Some(format!(
            "hostingRank: the hosting record {} does not parse ({e}); refusing rather than \
             guessing this Mac is free",
            hosting_path().display()
        )),
    }
}

fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // Signal 0 checks existence only; EPERM means it exists under another user.
    let delivered = unsafe { libc::kill(pid, 0) } == 0;
    delivered || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

// ---------------------------------------------------------------------------
// Discovery: the same-account Macs on the mesh, offered before any ssh alias.
// ---------------------------------------------------------------------------

/// The Link peers, each probed by its own goosed. Never an empty list standing in for "not
/// connected": that is its own state with the reason.
pub(super) async fn link_discovery() -> MlxDistributedLinkDiscoveryDto {
    ensure_link_transport();
    let Some(manager) = existing_link_manager() else {
        return MlxDistributedLinkDiscoveryDto {
            state: "unavailable".to_string(),
            detail: Some(
                "LeanZero Link has not started in this goose — sign in under LeanZero Link to \
                 find your other Macs"
                    .to_string(),
            ),
            peers: Vec::new(),
        };
    };
    let Some(registry) = manager.active_registry().await else {
        let why = match manager.status().await.auth {
            AuthState::LoggedOut | AuthState::CodeSent { .. } => {
                "this Mac is not signed in to LeanZero Link".to_string()
            }
            AuthState::LoggedIn { email } => format!("signed in as {email}, not connected"),
            AuthState::Connecting { email } => format!("connecting as {email}"),
            AuthState::Connected { .. } => "connected, but the peer view is gone".to_string(),
        };
        return MlxDistributedLinkDiscoveryDto {
            state: "notConnected".to_string(),
            detail: Some(why),
            peers: Vec::new(),
        };
    };
    let self_id = stable_node_id();
    let peers = registry
        .peer_nodes()
        .into_iter()
        .filter(|p| p.node_id != self_id);
    let mut out: Vec<MlxDistributedLinkPeerDto> =
        futures::future::join_all(peers.map(|peer| async move {
            let host = link_control::link_host(&peer.node_id);
            let base = MlxDistributedLinkPeerDto {
                node_id: peer.node_id.clone(),
                hostname: peer.hostname.clone(),
                host,
                ..Default::default()
            };
            if peer.status == NodeStatus::Offline {
                return MlxDistributedLinkPeerDto {
                    state: "offline".to_string(),
                    detail: peer.last_poll_error.clone(),
                    ..base
                };
            }
            let answer: Result<ExecAnswer, _> = link_control::call_typed(
                &peer.node_id,
                LinkOp::Discover,
                &DiscoverRequest {
                    extra_roots: Vec::new(),
                },
            )
            .await;
            match answer {
                Ok(out) => match discover::parse_node(&out.stdout) {
                    Ok(probe) => peer_dto(base, &probe),
                    Err(e) => MlxDistributedLinkPeerDto {
                        state: "unreadable".to_string(),
                        detail: Some(format!("its probe did not parse: {e}")),
                        ..base
                    },
                },
                Err(error) => MlxDistributedLinkPeerDto {
                    state: match &error {
                        LinkCallError::Disabled(_) => "servingDisabled",
                        LinkCallError::NotServed(_) => "notServed",
                        _ => "unreachable",
                    }
                    .to_string(),
                    detail: Some(error.to_string()),
                    ..base
                },
            }
        }))
        .await;
    out.sort_by_key(|p| (p.state != "ready", p.hostname.clone()));
    MlxDistributedLinkDiscoveryDto {
        state: "connected".to_string(),
        detail: None,
        peers: out,
    }
}

fn peer_dto(
    base: MlxDistributedLinkPeerDto,
    probe: &discover::NodeProbe,
) -> MlxDistributedLinkPeerDto {
    MlxDistributedLinkPeerDto {
        state: "ready".to_string(),
        name: Some(if probe.computer_name.is_empty() {
            probe.hostname.clone()
        } else {
            probe.computer_name.clone()
        }),
        total_bytes: Some(probe.memory.total_bytes),
        available_bytes: Some(probe.memory.available_bytes),
        pressure: Some(probe.pressure.as_str().to_string()),
        thunderbolt: probe
            .facts
            .iter()
            .filter(|f| f.kind == InterfaceKind::Thunderbolt)
            .map(|f| MlxDistributedLinkPortDto {
                device: f.device.clone(),
                hardware_port: f.hardware_port.clone(),
                ipv4: f.ipv4.to_string(),
                prefix_len: f.prefix_len,
                speed: f.link_speed.clone(),
            })
            .collect(),
        rdma: probe
            .rdma
            .iter()
            .map(|d| MlxDistributedLinkRdmaDto {
                device: d.name.clone(),
                active: d.active,
                ipv4_gid_index: d
                    .gids
                    .iter()
                    .find(|(_, gid)| gid.starts_with("::ffff:"))
                    .map(|(index, _)| *index),
            })
            .collect(),
        models: probe
            .models
            .iter()
            .map(|m| MlxDistributedLinkPeerModelDto {
                dir: m.dir.clone(),
                model_type: m.model_type.clone(),
                weights_bytes: goose_sidecar::distributed::preflight::loaded_files(&m.files)
                    .values()
                    .sum(),
            })
            .collect(),
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_class_of_a_peers_answer_keeps_its_meaning_for_the_requester() {
        let refused = link_call_error(LinkError::DistributedNode(DistributedNodeError::Refused {
            code: "singleEngineMounted".into(),
            message: "m".into(),
        }));
        assert_eq!(
            refused,
            LinkCallError::Refused(LinkRefusal {
                code: "singleEngineMounted".into(),
                message: "m".into()
            })
        );
        assert!(matches!(
            link_call_error(LinkError::DistributedNode(DistributedNodeError::Disabled(
                "x".into()
            ))),
            LinkCallError::Disabled(_)
        ));
        assert!(matches!(
            link_call_error(LinkError::DistributedProxy(
                "peer returned 501: distributed node serving is not wired on this node".into()
            )),
            LinkCallError::NotServed(_)
        ));
        assert!(matches!(
            link_call_error(LinkError::DistributedProxy("connection refused".into())),
            LinkCallError::Unreachable(_)
        ));
        assert!(matches!(
            link_call_error(LinkError::NotConnected),
            LinkCallError::NotConnected(_)
        ));
        let unknown = link_call_error(LinkError::UnknownPeer("workhorse-7f3a".into())).to_string();
        assert!(unknown.contains("workhorse-7f3a"), "{unknown}");
    }

    #[test]
    fn a_peer_card_carries_its_memory_thunderbolt_rdma_and_models() {
        let fixture =
            include_str!("../../../tests/fixtures/mlx-distributed-discover/workhorse.txt");
        let probe = discover::parse_node(fixture).expect("the recorded workhorse probe parses");
        let dto = peer_dto(
            MlxDistributedLinkPeerDto {
                node_id: "workhorse-7f3a".into(),
                hostname: "workhorse".into(),
                host: "link:workhorse-7f3a".into(),
                ..Default::default()
            },
            &probe,
        );
        assert_eq!(dto.state, "ready");
        assert!(dto.total_bytes.unwrap() > 0 && dto.available_bytes.is_some());
        assert!(
            dto.thunderbolt.iter().any(|p| p.ipv4 == "192.168.0.2"),
            "{:?}",
            dto.thunderbolt
        );
        assert!(
            dto.rdma
                .iter()
                .any(|d| d.device == "rdma_en3" && d.ipv4_gid_index.is_some()),
            "{:?}",
            dto.rdma
        );
        assert!(!dto.models.is_empty());
    }
}
