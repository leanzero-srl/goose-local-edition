//! How fast a just-connected peer row turns online (2026-10-27): every row used to start
//! `Offline` and wait for a full `/nodes` + `/sessions` poll, so a peer slow to answer its first
//! poll read Offline for the whole 5 s request timeout. A peer that accepts TCP now reads online
//! from its first liveness dial; one where nothing listens stays Offline; tailscale's own flag
//! seeds the row.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use leanzero_link::controller_discovery::PeerAdmission;
use leanzero_link::controller_wire::DeviceRole;
use leanzero_link::mesh::MeshPeer;
use leanzero_link::pubsub::PubSub;
use leanzero_link::state::{PeerRegistry, PeerRegistryConfig};
use leanzero_link::wire::{NodeState, NodeStatus, SwarmNodesResponse};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// What the fake peer does with each route: answer after this long (`None` = never answer).
#[derive(Clone, Copy)]
struct Delays {
    nodes: Duration,
    sessions: Duration,
}

/// A fake peer control service: accepts every connection, answers `/v1/swarm/nodes` with a
/// node called `real-id` after `delays.nodes` and `/v1/swarm/sessions` with `[]` after
/// `delays.sessions`. Liveness dials (connect, then close) are simply accepted.
async fn fake_peer(delays: Delays) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let mut seen = Vec::new();
                loop {
                    let Ok(n) = socket.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    seen.extend_from_slice(&buf[..n]);
                    if !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        continue;
                    }
                    let head = String::from_utf8_lossy(&seen).to_string();
                    seen.clear();
                    let (delay, body) = if head.contains("/v1/swarm/nodes") {
                        let me = NodeState {
                            controller: None,
                            node_id: "real-id".to_string(),
                            hostname: "slowpeer".to_string(),
                            mesh_ip: None,
                            status: NodeStatus::Idle,
                            sessions_active: 0,
                            updated_at: Utc::now(),
                            last_poll_error: None,
                            computer_name: Some("Slow Mac".to_string()),
                            allows: None,
                            leaving: None,
                            load: None,
                        };
                        let body = serde_json::to_string(&SwarmNodesResponse {
                            self_node: me,
                            peers: Vec::new(),
                        })
                        .unwrap();
                        (delays.nodes, body)
                    } else {
                        (delays.sessions, "[]".to_string())
                    };
                    tokio::time::sleep(delay).await;
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    if socket.write_all(reply.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    port
}

/// A port where nothing listens (bound, then released).
async fn dead_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

fn registry() -> PeerRegistry {
    registry_timed(Duration::from_secs(3), Duration::from_secs(5))
}

fn registry_timed(poll_interval: Duration, request_timeout: Duration) -> PeerRegistry {
    PeerRegistry::new(
        PeerRegistryConfig {
            node_token: "t".to_string(),
            poll_interval,
            request_timeout,
            reconnect_backoff: Duration::from_secs(2),
            peer_proxy: Some(support::fake_tailnet().proxy()),
        },
        Arc::new(PubSub::new()),
    )
    .unwrap()
}

fn connect(registry: &PeerRegistry, port: u16, online: bool) {
    let ip = support::fake_tailnet().expose(port);
    registry.set_mesh_peers(
        &[MeshPeer {
            mesh_node_key: None,
            hostname: "slowpeer".to_string(),
            ip: Some(ip),
            online,
            last_seen: None,
        }],
        port,
    );
}

fn row(registry: &PeerRegistry) -> NodeState {
    registry.peer_nodes().remove(0)
}

/// Wait up to `within` for `pred` on the row; the elapsed time when it held.
async fn wait_for(
    registry: &PeerRegistry,
    within: Duration,
    pred: impl Fn(&NodeState) -> bool,
) -> Option<Duration> {
    let start = Instant::now();
    while start.elapsed() < within {
        if pred(&row(registry)) {
            return Some(start.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

/// The Gabee 09:03:26 shape: the peer is there (accepts TCP) but its first `/nodes` takes 6 s,
/// longer than the 5 s request timeout. Tailscale's flag says offline, so only the liveness
/// dial can turn the row online.
#[tokio::test]
async fn a_peer_that_accepts_but_answers_slowly_reads_online_within_1_5_s() {
    let port = fake_peer(Delays {
        nodes: Duration::from_secs(6),
        sessions: Duration::from_secs(6),
    })
    .await;
    let registry = registry();
    connect(&registry, port, false);
    assert_eq!(
        row(&registry).status,
        NodeStatus::Offline,
        "seeded from online=false"
    );
    let took = wait_for(&registry, Duration::from_millis(1500), |r| {
        r.status != NodeStatus::Offline
    })
    .await;
    assert!(took.is_some(), "still {:?} after 1.5 s", row(&registry));
    registry.shutdown();
}

/// NEGATIVE CONTROL: nothing listens on the peer's control port. Tailscale's flag says offline
/// and every liveness dial is refused: the row never reads online.
#[tokio::test]
async fn a_peer_where_nothing_listens_stays_offline() {
    let port = dead_port().await;
    let registry = registry();
    connect(&registry, port, false);
    let flipped = wait_for(&registry, Duration::from_millis(2500), |r| {
        r.status != NodeStatus::Offline
    })
    .await;
    assert_eq!(flipped, None, "{:?}", row(&registry));
    assert!(
        row(&registry).last_poll_error.is_some(),
        "the refusal is on record: {:?}",
        row(&registry)
    );
    registry.shutdown();
}

/// Tailscale's own flag seeds the row: online=true starts online, and a seeded row whose port
/// refuses is turned Offline by the first liveness dial (no fabricated Online that sticks).
#[tokio::test]
async fn tailscale_online_seeds_the_row_and_a_refusing_port_corrects_it() {
    let registry = registry();
    connect(&registry, dead_port().await, true);
    assert_ne!(
        row(&registry).status,
        NodeStatus::Offline,
        "seeded from online=true"
    );
    let corrected = wait_for(&registry, Duration::from_millis(1500), |r| {
        r.status == NodeStatus::Offline
    })
    .await;
    assert!(
        corrected.is_some(),
        "still {:?} after 1.5 s",
        row(&registry)
    );
    registry.shutdown();
}

/// A peer that answers `/nodes` at once but whose `/sessions` takes 6 s: the row carries the
/// peer's own identity from `/nodes` well before `/sessions` answers (or times out).
#[tokio::test]
async fn the_nodes_answer_is_published_without_waiting_for_sessions() {
    let port = fake_peer(Delays {
        nodes: Duration::ZERO,
        sessions: Duration::from_secs(6),
    })
    .await;
    let registry = registry();
    connect(&registry, port, false);
    let took = wait_for(&registry, Duration::from_millis(1500), |r| {
        r.node_id == "real-id" && r.status != NodeStatus::Offline
    })
    .await;
    assert!(took.is_some(), "still {:?} after 1.5 s", row(&registry));
    registry.shutdown();
}

/// A poll's own Offline verdict stands: a hung Link (accepts TCP, never answers) is marked
/// Offline after PEER_SLOW_LOOKS slow polls, and the liveness dial, which still connects every
/// 500 ms, must not flip it back online.
#[tokio::test]
async fn a_poll_verdict_of_offline_is_not_undone_by_the_liveness_dial() {
    let port = fake_peer(Delays {
        nodes: Duration::from_secs(60),
        sessions: Duration::from_secs(60),
    })
    .await;
    let registry = registry_timed(Duration::from_millis(100), Duration::from_millis(300));
    connect(&registry, port, true);
    let offline = wait_for(&registry, Duration::from_secs(5), |r| {
        r.status == NodeStatus::Offline
    })
    .await;
    assert!(
        offline.is_some(),
        "never marked Offline: {:?}",
        row(&registry)
    );
    let flapped = wait_for(&registry, Duration::from_millis(1500), |r| {
        r.status != NodeStatus::Offline
    })
    .await;
    assert_eq!(
        flapped,
        None,
        "liveness undid the poll's verdict: {:?}",
        row(&registry)
    );
    registry.shutdown();
}

fn registered_roles(host: DeviceRole, phone: DeviceRole) -> PeerAdmission {
    PeerAdmission::RegisteredHosts {
        revision: 1,
        roles: std::collections::HashMap::from([
            ("nodekey:host".to_string(), host),
            ("nodekey:phone".to_string(), phone),
        ]),
        deadline: tokio::time::Instant::now() + Duration::from_secs(30),
    }
}

fn registered_peers(port: u16, online: bool) -> [MeshPeer; 2] {
    ["host", "phone"].map(|name| MeshPeer {
        mesh_node_key: Some(format!("nodekey:{name}")),
        hostname: name.to_string(),
        ip: Some(support::fake_tailnet().expose(port)),
        online,
        last_seen: None,
    })
}

#[tokio::test]
async fn online_seeding_requires_host_authority_and_cannot_restore_revoked_compute_access() {
    let registry = registry();
    let port = dead_port().await;
    let peers = registered_peers(port, true);
    registry.set_peer_admission(registered_roles(DeviceRole::Host, DeviceRole::Controller));
    registry.set_mesh_peers(&peers, port);

    let initial = registry.peer_nodes();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].hostname, "host");
    assert_eq!(initial[0].status, NodeStatus::Idle);
    assert_eq!(registry.compute_peer_nodes().len(), 1);
    assert!(registry.require_compute_peer("host").is_ok());
    assert!(registry.peer_base_url("host").is_some());
    assert!(registry.require_compute_peer("phone").is_err());
    assert!(registry.peer_base_url("phone").is_none());

    // Expiry keeps the cached row but never turns an online flag into authority.
    registry.peer_authority_unavailable();
    registry.set_mesh_peers(&peers, port);
    assert_eq!(registry.peer_nodes().len(), 1);
    assert_eq!(registry.peer_nodes()[0].status, NodeStatus::Idle);
    assert!(registry.peer_nodes()[0]
        .last_poll_error
        .as_deref()
        .is_some_and(|error| error.contains("authorization unavailable or expired")));
    assert!(registry.compute_peer_nodes().is_empty());
    assert!(registry.require_compute_peer("host").is_err());
    assert!(registry.peer_base_url("host").is_none());

    registry.set_peer_admission(registered_roles(
        DeviceRole::Controller,
        DeviceRole::Controller,
    ));
    registry.set_mesh_peers(&peers, port);
    assert_eq!(registry.peer_nodes().len(), 1);
    assert!(registry.compute_peer_nodes().is_empty());
    assert!(registry.require_compute_peer("host").is_err());
    assert!(registry.peer_base_url("host").is_none());
    registry.shutdown();
}

#[tokio::test]
async fn early_liveness_dials_admitted_hosts_but_never_controller_only_peers() {
    let port = fake_peer(Delays {
        nodes: Duration::from_secs(6),
        sessions: Duration::from_secs(6),
    })
    .await;
    let registry = registry();
    let peers = registered_peers(port, false);
    let host_ip = peers[0].ip.as_deref().unwrap();
    let phone_ip = peers[1].ip.as_deref().unwrap();
    registry.set_peer_admission(registered_roles(DeviceRole::Host, DeviceRole::Controller));
    registry.set_mesh_peers(&peers, port);
    assert_eq!(registry.peer_nodes().len(), 1);
    assert_eq!(row(&registry).status, NodeStatus::Offline);

    let online = wait_for(&registry, Duration::from_millis(1500), |r| {
        r.status == NodeStatus::Idle
    })
    .await;
    assert!(online.is_some(), "{:?}", row(&registry));
    assert_eq!(
        row(&registry).node_id,
        "host",
        "first poll has not answered"
    );
    assert!(support::fake_tailnet().connects_to(host_ip) > 0);
    assert_eq!(support::fake_tailnet().connects_to(phone_ip), 0);
    assert!(registry.peer_node("phone").is_none());

    // Positive control on that same phone address: only a fresh host role admits its dials.
    registry.set_peer_admission(registered_roles(DeviceRole::Host, DeviceRole::Host));
    registry.set_mesh_peers(&peers, port);
    tokio::time::timeout(Duration::from_millis(1500), async {
        while support::fake_tailnet().connects_to(phone_ip) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("newly admitted host must be dialled through the same proxy");
    assert!(registry.require_compute_peer("phone").is_ok());
    registry.shutdown();
}
