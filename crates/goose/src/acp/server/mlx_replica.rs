//! `mlxEngine/{linkFacts,replicaTargets,replicate,replicaPull,replicaProgress,replicaCancel}`:
//! copy a model this node already has to a LeanZero Link peer over the best DIRECT path the
//! two share — a Thunderbolt cable first, else a shared LAN — instead of downloading it again.
//!
//! The flow, sender A → receiver B:
//! 1. A reads its own interfaces and B's (`linkFacts`, relayed over the mesh) and picks the
//!    path with [`netpath::choose_path`]. No shared subnet → no copy, stated per peer.
//! 2. A binds a replica listener on ITS address of that path (never `0.0.0.0`), offers the
//!    model (a fresh 256-bit capability token for that one model), and sends B a
//!    `replicaPull` over the node-token-authenticated `/v1/swarm/mlx/replicaPull` proxy.
//! 3. B pulls file by file into its models dir with the `.part` → final convention the HF
//!    downloader uses, verifying each file's SHA-256 against A's bytes, then releases the
//!    offer. B's `modelsList` lists the model once the last file lands.
//!
//! Only the control messages ride the mesh; the bytes cross the cable/LAN directly.

use super::*;
use goose_sidecar::engine::expand_tilde;
use goose_sidecar::hf;
use leanzero_link::control::DEFAULT_CONTROL_PORT;
use leanzero_link::manager::{AuthState, LinkManager};
use leanzero_link::netpath::{self, InterfaceFact, InterfaceKind, LinkKind, LinkPath};
use leanzero_link::replica::{
    Preflight, PullSpec, ReplicaListener, ReplicaOffers, ReplicaPhase, ReplicaProgress,
    ReplicaState, ReplicaTracker, PART_SUFFIX,
};
use leanzero_link::state::MlxOp;
use leanzero_link::wire::NodeStatus;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::LazyLock;
use std::time::Duration;

/// The models THIS node offers to peers (sending side).
static OFFERS: LazyLock<ReplicaOffers> = LazyLock::new(ReplicaOffers::new);

/// One replica listener per local address a copy has used, kept for the process lifetime:
/// each serves only offered models and admits only their tokens.
static LISTENERS: LazyLock<tokio::sync::Mutex<HashMap<Ipv4Addr, ReplicaListener>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

/// Copies arriving on THIS node (receiving side).
pub(super) static REPLICAS: LazyLock<ReplicaTracker> = LazyLock::new(ReplicaTracker::new);

/// Transport bound on the reachability probe of a peer's address — a connect, never a
/// transfer.
const PROBE_CONNECT: Duration = Duration::from_secs(2);

fn kind_tag(kind: InterfaceKind) -> &'static str {
    match kind {
        InterfaceKind::Thunderbolt => "thunderbolt",
        InterfaceKind::Ethernet => "ethernet",
        InterfaceKind::Wifi => "wifi",
        InterfaceKind::Other => "other",
    }
}

fn kind_from_tag(tag: &str) -> Result<InterfaceKind, String> {
    Ok(match tag {
        "thunderbolt" => InterfaceKind::Thunderbolt,
        "ethernet" => InterfaceKind::Ethernet,
        "wifi" => InterfaceKind::Wifi,
        "other" => InterfaceKind::Other,
        other => return Err(format!("unknown interface kind '{other}'")),
    })
}

fn link_tag(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Thunderbolt => "thunderbolt",
        LinkKind::Network => "network",
    }
}

fn link_from_tag(tag: &str) -> Result<LinkKind, String> {
    match tag {
        "thunderbolt" => Ok(LinkKind::Thunderbolt),
        "network" => Ok(LinkKind::Network),
        other => Err(format!("unknown link kind '{other}'")),
    }
}

fn fact_to_dto(fact: &InterfaceFact) -> MlxNetInterfaceDto {
    MlxNetInterfaceDto {
        device: fact.device.clone(),
        hardware_port: fact.hardware_port.clone(),
        kind: kind_tag(fact.kind).to_string(),
        ipv4: fact.ipv4.to_string(),
        prefix_len: fact.prefix_len,
        link_speed: fact.link_speed.clone(),
    }
}

fn fact_from_dto(dto: &MlxNetInterfaceDto) -> Result<InterfaceFact, String> {
    Ok(InterfaceFact {
        device: dto.device.clone(),
        hardware_port: dto.hardware_port.clone(),
        kind: kind_from_tag(&dto.kind)?,
        ipv4: dto
            .ipv4
            .parse()
            .map_err(|e| format!("interface {} reports ipv4 '{}': {e}", dto.device, dto.ipv4))?,
        prefix_len: dto.prefix_len,
        link_speed: dto.link_speed.clone(),
    })
}

fn path_to_dto(path: &LinkPath) -> MlxReplicaLinkDto {
    MlxReplicaLinkDto {
        kind: link_tag(path.kind).to_string(),
        local: fact_to_dto(&path.local),
        peer: fact_to_dto(&path.peer),
    }
}

/// "Thunderbolt 3 en3 192.168.0.1 → 192.168.0.2 (80 Gb/s)" — what the receiver shows as
/// the path its bytes come over.
fn link_detail(link: &MlxReplicaLinkDto) -> String {
    let port = link
        .local
        .hardware_port
        .as_deref()
        .unwrap_or(&link.local.device);
    let speed = link
        .local
        .link_speed
        .as_deref()
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    format!(
        "{port} {} {} → {}{speed}",
        link.local.device, link.local.ipv4, link.peer.ipv4
    )
}

async fn local_facts() -> Result<(Vec<InterfaceFact>, Option<String>), agent_client_protocol::Error>
{
    tokio::task::spawn_blocking(netpath::local_interface_facts)
        .await
        .internal_err_ctx("reading this node's interfaces")?
        .internal_err()
}

fn models_dir() -> Result<PathBuf, agent_client_protocol::Error> {
    Ok(expand_tilde(
        &super::mlx_engine::load_engine_settings()?.models_dir,
    ))
}

async fn require_connected(manager: &LinkManager) -> Result<(), agent_client_protocol::Error> {
    if matches!(manager.status().await.auth, AuthState::Connected { .. }) {
        Ok(())
    } else {
        Err(agent_client_protocol::Error::invalid_params().data("not connected to the mesh"))
    }
}

/// A peer's own interface facts, over the mesh.
async fn peer_facts(manager: &LinkManager, node_id: &str) -> Result<Vec<InterfaceFact>, String> {
    let value = manager
        .mlx_proxy(node_id, MlxOp::LinkFacts, serde_json::json!({}))
        .await
        .map_err(|e| e.to_string())?;
    let response: MlxEngineLinkFactsResponse = serde_json::from_value(value)
        .map_err(|e| format!("the peer's interface report did not parse: {e}"))?;
    response.interfaces.iter().map(fact_from_dto).collect()
}

/// Does anything answer at the peer's address of the path? A refused connection IS an
/// answer (the host is there, nothing listens on that port); only silence or an
/// unreachable route means the path is not usable.
async fn probe_peer_address(ip: Ipv4Addr) -> Result<(), String> {
    let addr = SocketAddr::new(IpAddr::V4(ip), DEFAULT_CONTROL_PORT);
    match tokio::time::timeout(PROBE_CONNECT, tokio::net::TcpStream::connect(addr)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => Ok(()),
        Ok(Err(e)) => Err(format!("{ip} did not answer: {e}")),
        Err(_) => Err(format!(
            "{ip} did not answer within {}s",
            PROBE_CONNECT.as_secs()
        )),
    }
}

/// The direct path from this node to `node_id`, or the reason there is none.
async fn path_to(
    manager: &LinkManager,
    local: &[InterfaceFact],
    node_id: &str,
    hostname: &str,
) -> Result<LinkPath, String> {
    let peer = peer_facts(manager, node_id).await?;
    let path = netpath::choose_path(local, &peer).ok_or_else(|| {
        format!(
            "this node and {hostname} share no Thunderbolt or LAN subnet; a copy needs a direct \
             path"
        )
    })?;
    probe_peer_address(path.peer.ipv4).await?;
    Ok(path)
}

pub(super) async fn core_link_facts(
    _req: MlxEngineLinkFactsRequest,
) -> Result<MlxEngineLinkFactsResponse, agent_client_protocol::Error> {
    let (facts, warning) = local_facts().await?;
    Ok(MlxEngineLinkFactsResponse {
        interfaces: facts.iter().map(fact_to_dto).collect(),
        warning,
    })
}

pub(super) async fn core_replica_targets(
    manager: Arc<LinkManager>,
    _req: MlxEngineReplicaTargetsRequest,
) -> Result<MlxEngineReplicaTargetsResponse, agent_client_protocol::Error> {
    if !matches!(manager.status().await.auth, AuthState::Connected { .. }) {
        return Ok(MlxEngineReplicaTargetsResponse {
            mesh_connected: false,
            targets: Vec::new(),
            warning: None,
        });
    }
    let (local, warning) = local_facts().await?;
    let peers = match manager.active_registry().await {
        Some(registry) => registry.peer_nodes(),
        None => Vec::new(),
    };
    let targets = futures::future::join_all(peers.into_iter().map(|peer| {
        let manager = manager.clone();
        let local = local.clone();
        async move {
            let result = if peer.status == NodeStatus::Offline {
                Err(format!("{} is offline", peer.hostname))
            } else {
                path_to(&manager, &local, &peer.node_id, &peer.hostname).await
            };
            match result {
                Ok(path) => MlxReplicaTargetDto {
                    node_id: peer.node_id,
                    hostname: peer.hostname,
                    link: Some(path_to_dto(&path)),
                    unavailable: None,
                },
                Err(reason) => MlxReplicaTargetDto {
                    node_id: peer.node_id,
                    hostname: peer.hostname,
                    link: None,
                    unavailable: Some(reason),
                },
            }
        }
    }))
    .await;
    Ok(MlxEngineReplicaTargetsResponse {
        mesh_connected: true,
        targets,
        warning,
    })
}

async fn listener_on(ip: Ipv4Addr) -> Result<String, agent_client_protocol::Error> {
    let mut listeners = LISTENERS.lock().await;
    if let Some(listener) = listeners.get(&ip) {
        return Ok(listener.base_url());
    }
    let listener = ReplicaListener::bind(SocketAddr::new(IpAddr::V4(ip), 0), OFFERS.clone())
        .await
        .internal_err_ctx(&format!("binding the replica listener on {ip}"))?;
    let url = listener.base_url();
    listeners.insert(ip, listener);
    Ok(url)
}

pub(super) async fn core_replicate(
    manager: Arc<LinkManager>,
    req: MlxEngineReplicateRequest,
) -> Result<MlxEngineReplicateResponse, agent_client_protocol::Error> {
    require_connected(&manager).await?;
    hf::validate_model_id(&req.model_id).invalid_params_err()?;
    let models_dir = models_dir()?;
    let local_models = hf::list_local_models(&models_dir).internal_err()?;
    if !local_models
        .iter()
        .any(|m| m.id == req.model_id && m.complete)
    {
        return Err(agent_client_protocol::Error::invalid_params().data(format!(
            "'{}' is not a complete model on this node; only a complete model can be copied",
            req.model_id
        )));
    }
    let hostname = match manager.active_registry().await {
        Some(registry) => registry
            .peer_nodes()
            .into_iter()
            .find(|peer| peer.node_id == req.target_node_id)
            .map(|peer| peer.hostname),
        None => None,
    }
    .ok_or_else(|| {
        agent_client_protocol::Error::invalid_params()
            .data(format!("unknown peer node '{}'", req.target_node_id))
    })?;
    let (local, _) = local_facts().await?;
    let path = path_to(&manager, &local, &req.target_node_id, &hostname)
        .await
        .map_err(|reason| agent_client_protocol::Error::invalid_params().data(reason))?;
    let source_url = listener_on(path.local.ipv4).await?;
    let ticket = OFFERS
        .offer(&req.model_id, &models_dir.join(&req.model_id))
        .invalid_params_err()?;
    let link = path_to_dto(&path);
    let pull = MlxEngineReplicaPullRequest {
        model_id: req.model_id.clone(),
        source_url: source_url.clone(),
        offer_token: ticket.token.clone(),
        link: link.clone(),
        node_id: None,
    };
    let body = serde_json::to_value(&pull).internal_err_ctx("serializing the pull request")?;
    if let Err(error) = manager
        .mlx_proxy(&req.target_node_id, MlxOp::ReplicaPull, body)
        .await
    {
        OFFERS.release(&ticket.token);
        return Err(super::link::mlx_proxy_err(
            error,
            &super::link::peer_display_name(&manager, &req.target_node_id).await,
        ));
    }
    info!(
        model = %req.model_id,
        target = %req.target_node_id,
        link = link_tag(path.kind),
        %source_url,
        bytes = ticket.manifest.total_bytes,
        "mlx replica: offered a model to a peer"
    );
    Ok(MlxEngineReplicateResponse { link, source_url })
}

/// Bytes of the manifest already on disk here (finished files at their size, `.part`
/// prefixes), so the space check asks only for what is still to come.
fn bytes_already_here(dest_root: &Path, manifest: &leanzero_link::replica::ReplicaManifest) -> u64 {
    manifest
        .files
        .iter()
        .map(|file| {
            let dest = dest_root.join(&file.path);
            match std::fs::metadata(&dest) {
                Ok(meta) if meta.is_file() && meta.len() == file.size => file.size,
                _ => match std::fs::metadata(format!("{}{PART_SUFFIX}", dest.display())) {
                    Ok(meta) if meta.is_file() && meta.len() < file.size => meta.len(),
                    _ => 0,
                },
            }
        })
        .sum()
}

pub(super) async fn core_replica_pull(
    req: MlxEngineReplicaPullRequest,
) -> Result<EmptyResponse, agent_client_protocol::Error> {
    hf::validate_model_id(&req.model_id).invalid_params_err()?;
    let models_dir = models_dir()?;
    let present = hf::list_local_models(&models_dir).internal_err()?;
    if present.iter().any(|m| m.id == req.model_id && m.complete) {
        return Err(agent_client_protocol::Error::invalid_params().data(format!(
            "'{}' is already complete on this node",
            req.model_id
        )));
    }
    if let Some(progress) = super::mlx_engine::download_progress(&req.model_id) {
        if matches!(
            progress.state,
            hf::DownloadState::Queued | hf::DownloadState::Downloading | hf::DownloadState::Paused
        ) {
            return Err(agent_client_protocol::Error::invalid_params().data(format!(
                "a Hugging Face download of '{}' is active on this node; cancel it or let it \
                 finish first",
                req.model_id
            )));
        }
    }
    let link = link_from_tag(&req.link.kind).invalid_params_err()?;
    let dest_root = models_dir.join(&req.model_id);
    let space_root = models_dir.clone();
    let preflight: Preflight = Arc::new(move |manifest| {
        let (available, _) = goose_sidecar::disk_space(&space_root).map_err(|e| e.to_string())?;
        let needed = manifest
            .total_bytes
            .saturating_sub(bytes_already_here(&dest_root, manifest));
        if needed > available {
            return Err(format!(
                "'{}' needs {needed} more bytes but {} has {available} free",
                manifest.model_id,
                space_root.display()
            ));
        }
        Ok(())
    });
    REPLICAS
        .start(
            PullSpec {
                model_id: req.model_id.clone(),
                source_url: req.source_url.clone(),
                offer_token: req.offer_token,
                link,
                link_detail: link_detail(&req.link),
            },
            &models_dir,
            preflight,
        )
        .invalid_params_err()?;
    info!(
        model = %req.model_id,
        source = %req.source_url,
        link = %req.link.kind,
        "mlx replica: pulling a model from a peer"
    );
    Ok(EmptyResponse {})
}

fn progress_to_dto(progress: ReplicaProgress) -> MlxReplicaProgressDto {
    MlxReplicaProgressDto {
        state: match progress.state {
            ReplicaState::Queued => "queued",
            ReplicaState::Copying => "copying",
            ReplicaState::Done => "done",
            ReplicaState::Failed => "failed",
            ReplicaState::Cancelled => "cancelled",
        }
        .to_string(),
        source_url: progress.source_url,
        link: link_tag(progress.link).to_string(),
        link_detail: progress.link_detail,
        total_bytes: progress.total_bytes,
        copied_bytes: progress.copied_bytes,
        files_total: progress.files_total,
        files_done: progress.files_done,
        current_file: progress.current_file,
        phase: progress.phase.map(|phase| {
            match phase {
                ReplicaPhase::Transferring => "transferring",
                ReplicaPhase::Verifying => "verifying",
            }
            .to_string()
        }),
        resumed_files: progress.resumed_files,
        restarted_files: progress.restarted_files,
        skipped_files: progress.skipped_files,
        wire_bytes: progress.wire_bytes,
        wire_millis: progress.wire_millis,
        elapsed_millis: progress.elapsed_millis,
        error: progress.error,
        local_network_blocked: progress.local_network_blocked,
        release_error: progress.release_error,
    }
}

pub(super) async fn core_replica_progress(
    req: MlxEngineReplicaProgressRequest,
) -> Result<MlxEngineReplicaProgressResponse, agent_client_protocol::Error> {
    Ok(MlxEngineReplicaProgressResponse {
        progress: REPLICAS.progress(&req.model_id).map(progress_to_dto),
    })
}

pub(super) async fn core_replica_cancel(
    req: MlxEngineReplicaCancelRequest,
) -> Result<EmptyResponse, agent_client_protocol::Error> {
    REPLICAS.cancel(&req.model_id).invalid_params_err()?;
    Ok(EmptyResponse {})
}

impl GooseAcpAgent {
    pub(super) async fn on_mlx_engine_link_facts(
        &self,
        req: MlxEngineLinkFactsRequest,
    ) -> Result<MlxEngineLinkFactsResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::LinkFacts, &req).await;
        }
        core_link_facts(req).await
    }

    pub(super) async fn on_mlx_engine_replica_targets(
        &self,
        req: MlxEngineReplicaTargetsRequest,
    ) -> Result<MlxEngineReplicaTargetsResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self
                .mlx_engine_relay(&node, MlxOp::ReplicaTargets, &req)
                .await;
        }
        core_replica_targets(self.link_manager().await?, req).await
    }

    pub(super) async fn on_mlx_engine_replicate(
        &self,
        req: MlxEngineReplicateRequest,
    ) -> Result<MlxEngineReplicateResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::Replicate, &req).await;
        }
        core_replicate(self.link_manager().await?, req).await
    }

    pub(super) async fn on_mlx_engine_replica_pull(
        &self,
        req: MlxEngineReplicaPullRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::ReplicaPull, &req).await;
        }
        core_replica_pull(req).await
    }

    pub(super) async fn on_mlx_engine_replica_progress(
        &self,
        req: MlxEngineReplicaProgressRequest,
    ) -> Result<MlxEngineReplicaProgressResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self
                .mlx_engine_relay(&node, MlxOp::ReplicaProgress, &req)
                .await;
        }
        core_replica_progress(req).await
    }

    pub(super) async fn on_mlx_engine_replica_cancel(
        &self,
        req: MlxEngineReplicaCancelRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self
                .mlx_engine_relay(&node, MlxOp::ReplicaCancel, &req)
                .await;
        }
        core_replica_cancel(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(device: &str, port: &str, kind: InterfaceKind, ip: &str, prefix: u8) -> InterfaceFact {
        InterfaceFact {
            device: device.into(),
            hardware_port: Some(port.into()),
            kind,
            ipv4: ip.parse().unwrap(),
            prefix_len: prefix,
            link_speed: (kind == InterfaceKind::Thunderbolt).then(|| "80 Gb/s".to_string()),
        }
    }

    #[test]
    fn interface_facts_survive_the_dto_round_trip() {
        for kind in [
            InterfaceKind::Thunderbolt,
            InterfaceKind::Ethernet,
            InterfaceKind::Wifi,
            InterfaceKind::Other,
        ] {
            let original = fact("en3", "Thunderbolt 3", kind, "192.168.0.1", 30);
            let back = fact_from_dto(&fact_to_dto(&original)).unwrap();
            assert_eq!(back, original);
        }
        let mut bad = fact_to_dto(&fact(
            "en3",
            "Thunderbolt 3",
            InterfaceKind::Wifi,
            "10.0.0.1",
            8,
        ));
        bad.ipv4 = "not-an-ip".into();
        assert!(fact_from_dto(&bad).is_err());
        bad.ipv4 = "10.0.0.1".into();
        bad.kind = "carrier-pigeon".into();
        assert!(
            fact_from_dto(&bad).is_err(),
            "an unknown kind is refused, never Other"
        );
    }

    #[test]
    fn the_link_detail_names_the_port_the_addresses_and_the_speed() {
        let path = LinkPath {
            kind: LinkKind::Thunderbolt,
            local: fact(
                "en3",
                "Thunderbolt 3",
                InterfaceKind::Thunderbolt,
                "192.168.0.1",
                30,
            ),
            peer: fact(
                "en3",
                "Thunderbolt 2",
                InterfaceKind::Thunderbolt,
                "192.168.0.2",
                30,
            ),
        };
        let dto = path_to_dto(&path);
        assert_eq!(dto.kind, "thunderbolt");
        assert_eq!(
            link_detail(&dto),
            "Thunderbolt 3 en3 192.168.0.1 → 192.168.0.2 (80 Gb/s)"
        );
        assert_eq!(link_from_tag(&dto.kind).unwrap(), LinkKind::Thunderbolt);
        assert!(link_from_tag("mesh").is_err());
    }

    #[test]
    fn the_space_check_counts_only_what_is_still_to_come() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("done.bin"), vec![0u8; 10]).unwrap();
        std::fs::write(dir.path().join("half.bin.part"), vec![0u8; 4]).unwrap();
        std::fs::write(dir.path().join("wrong.bin"), vec![0u8; 3]).unwrap();
        let manifest = leanzero_link::replica::ReplicaManifest {
            model_id: "pub/m".into(),
            files: vec![
                leanzero_link::replica::ReplicaFile {
                    path: "done.bin".into(),
                    size: 10,
                },
                leanzero_link::replica::ReplicaFile {
                    path: "half.bin".into(),
                    size: 8,
                },
                leanzero_link::replica::ReplicaFile {
                    path: "wrong.bin".into(),
                    size: 5,
                },
            ],
            total_bytes: 23,
        };
        assert_eq!(bytes_already_here(dir.path(), &manifest), 14);
    }

    #[tokio::test]
    async fn a_refused_connection_proves_the_address_answers() {
        // Loopback answers on the control port whether or not a control service listens
        // there: a refusal is an answer from a live host.
        assert!(probe_peer_address(Ipv4Addr::LOCALHOST).await.is_ok());
        // TEST-NET-1 (RFC 5737) is never routed: silence, not an answer.
        let err = probe_peer_address("192.0.2.1".parse().unwrap())
            .await
            .unwrap_err();
        assert!(err.contains("192.0.2.1"), "{err}");
    }
}
