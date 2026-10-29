use super::*;
use crate::{
    controller_tls::{start_controller_tls, ControllerTlsIdentity},
    peer_dial::MeshProxy,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[derive(Clone)]
struct Fixture {
    entry: CatalogEntry,
    device: Uuid,
    history_reads: Arc<AtomicUsize>,
    catalog_reads: Arc<AtomicUsize>,
    updates: Arc<Mutex<Option<CatalogChanges>>>,
    accepted: Arc<Mutex<HashMap<Uuid, WorkCommand>>>,
    lose_ack: Arc<AtomicBool>,
    command_requests: Arc<AtomicUsize>,
    required_outbox: Option<PathBuf>,
    refuse_next: Arc<Mutex<Option<(StatusCode, &'static str)>>>,
}
impl Fixture {
    fn new(owner: Uuid, device: Uuid) -> Self {
        Self {
            entry: CatalogEntry {
                work: WorkRef {
                    owner_node_id: owner,
                    kind: WorkKind::Chat,
                    work_id: Uuid::new_v4(),
                },
                store_generation: Uuid::new_v4(),
                revision: 1,
                title: "Original owner work".into(),
                working_dir: None,
                state: WorkState::Idle,
                current_run_id: None,
                capabilities: vec![WorkAction::Send],
                parent: None,
                related: vec![],
                updated_at: Utc::now(),
                last_event: None,
                archived: false,
            },
            device,
            history_reads: Arc::new(AtomicUsize::new(0)),
            catalog_reads: Arc::new(AtomicUsize::new(0)),
            updates: Arc::new(Mutex::new(None)),
            accepted: Arc::new(Mutex::new(HashMap::new())),
            lose_ack: Arc::new(AtomicBool::new(true)),
            command_requests: Arc::new(AtomicUsize::new(0)),
            required_outbox: None,
            refuse_next: Arc::new(Mutex::new(None)),
        }
    }
    fn receipt(&self, command: &WorkCommand) -> CommandReceipt {
        CommandReceipt {
            command_id: command.command_id,
            work: command.work.clone(),
            authenticated_device_id: self.device,
            revision: 2,
            state: CommandState::Delivered,
            run_id: None,
            message_id: Some("original-message".into()),
            input_id: None,
            checkpoint_id: None,
            reason: None,
            updated_at: self.entry.updated_at,
        }
    }
    fn cursor(&self) -> CatalogCursor {
        CatalogCursor {
            owner_node_id: self.entry.work.owner_node_id,
            store_generation: self.entry.store_generation,
            revision: 1,
        }
    }
}
async fn catalog(State(f): State<Fixture>) -> Json<CatalogPage> {
    f.catalog_reads.fetch_add(1, Ordering::SeqCst);
    Json(CatalogPage {
        resume_cursor: "fixture-signed-catalog".into(),
        entries: vec![f.entry.clone()],
        watermark: f.cursor(),
        next_page_token: None,
    })
}
async fn changes(State(f): State<Fixture>) -> Json<CatalogChanges> {
    if let Some(updates) = f.updates.lock().await.clone() {
        return Json(updates);
    }
    Json(CatalogChanges {
        resume_cursor: "fixture-signed-catalog".into(),
        changes: vec![],
        cursor: f.cursor(),
        has_more: false,
    })
}
async fn history(State(f): State<Fixture>) -> Json<HistoryPage> {
    f.history_reads.fetch_add(1, Ordering::SeqCst);
    Json(HistoryPage {
        work_cursor: "fixture-signed-work".into(),
        work: f.entry.work.clone(),
        store_generation: f.entry.store_generation,
        revision: 1,
        items: vec![],
        next_page_token: None,
    })
}
async fn history_latest(
    State(f): State<Fixture>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<HistoryPage> {
    assert_eq!(
        query.get("cursor").map(String::as_str),
        Some("snapshot-boundary")
    );
    assert_eq!(
        query.get("store_generation"),
        Some(&f.entry.store_generation.to_string())
    );
    history(State(f)).await
}
async fn command(
    State(f): State<Fixture>,
    Json(command): Json<WorkCommand>,
) -> axum::response::Response {
    f.command_requests.fetch_add(1, Ordering::SeqCst);
    if let Some(path) = &f.required_outbox {
        let persisted = private_json::<Vec<PendingCommand>>(path).unwrap().unwrap();
        assert!(persisted
            .iter()
            .any(|entry| entry.device_id == f.device && entry.command == command));
    }
    if let Some((status, code)) = f.refuse_next.lock().await.take() {
        return (status, Json(serde_json::json!({"error": {"code": code}}))).into_response();
    }
    let mut accepted = f.accepted.lock().await;
    if let Some(old) = accepted.get(&command.command_id) {
        if old != &command {
            return StatusCode::CONFLICT.into_response();
        }
    }
    accepted.insert(command.command_id, command.clone());
    if f.lose_ack.swap(false, Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    (StatusCode::ACCEPTED, Json(f.receipt(&command))).into_response()
}
async fn receipt(State(f): State<Fixture>, Path(id): Path<Uuid>) -> axum::response::Response {
    match f.accepted.lock().await.get(&id) {
        Some(command) => Json(f.receipt(command)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn socks(
    destinations: HashMap<[u8; 4], SocketAddr>,
) -> (MeshProxy, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = MeshProxy::socks5(listener.local_addr().unwrap()).unwrap();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut client, _)) = listener.accept().await else {
                return;
            };
            let destinations = destinations.clone();
            tokio::spawn(async move {
                let mut greeting = [0; 2];
                client.read_exact(&mut greeting).await.unwrap();
                assert_eq!(greeting[0], 5);
                let mut methods = vec![0; greeting[1] as usize];
                client.read_exact(&mut methods).await.unwrap();
                client.write_all(&[5, 0]).await.unwrap();
                let mut connect = [0; 4];
                client.read_exact(&mut connect).await.unwrap();
                assert_eq!(connect, [5, 1, 0, 1]);
                let mut ip = [0; 4];
                client.read_exact(&mut ip).await.unwrap();
                let _port = client.read_u16().await.unwrap();
                let mut owner = tokio::net::TcpStream::connect(destinations[&ip])
                    .await
                    .unwrap();
                client
                    .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                    .await
                    .unwrap();
                let _ = tokio::io::copy_bidirectional(&mut client, &mut owner).await;
            });
        }
    });
    (proxy, task)
}

fn credentials(device: Uuid, expiry: DateTime<Utc>) -> DeviceCredentials {
    DeviceCredentials {
        access_token: "fixture-access".into(),
        token_type: "Bearer".into(),
        expires_at: expiry,
        refresh_token: "fixture-refresh".into(),
        device: DeviceRecord {
            device_id: device,
            name: "Fixture desktop".into(),
            platform: "desktop".into(),
            role: DeviceRole::Controller,
            revision: 1,
            scopes: vec![
                ControllerScope::CatalogRead,
                ControllerScope::HistoryRead,
                ControllerScope::WorkSend,
            ],
            created_at: Utc::now(),
            revoked_at: None,
        },
    }
}

#[tokio::test]
async fn two_pinned_owners_refresh_offline_cache_and_lost_ack_outbox() {
    let directory = tempfile::tempdir().unwrap();
    let authority = MockServer::start().await;
    let device = Uuid::new_v4();
    let first = Fixture::new(Uuid::new_v4(), device);
    let second = Fixture::new(Uuid::new_v4(), device);
    let mut servers = Vec::new();
    let mut hosts = Vec::new();
    let mut destinations = HashMap::new();
    for (index, fixture) in [first.clone(), second.clone()].into_iter().enumerate() {
        let identity = ControllerTlsIdentity::load_or_create(
            directory.path().join(format!("tls-{index}")),
            fixture.entry.work.owner_node_id,
        )
        .unwrap();
        let pin = identity.spki_sha256();
        let router = Router::new()
            .route("/v1/controller/catalog", get(catalog))
            .route("/v1/controller/catalog/changes", get(changes))
            .route("/v1/controller/work/chat/{id}/history", get(history))
            .route(
                "/v1/controller/work/chat/{id}/history/latest",
                get(history_latest),
            )
            .route("/v1/controller/work/chat/{id}/commands", post(command))
            .route("/v1/controller/commands/{id}", get(receipt))
            .with_state(fixture.clone());
        let server = start_controller_tls("127.0.0.1:0".parse().unwrap(), router, identity)
            .await
            .unwrap();
        let ip = [100, 64, 0, index as u8 + 1];
        destinations.insert(ip, server.local_addr);
        hosts.push(HostDescriptor {
            owner_node_id: fixture.entry.work.owner_node_id,
            name: format!("Mac {index}"),
            device_role: DeviceRole::Host,
            protocol_version: 1,
            availability: HostAvailability::Online,
            last_seen_at: Some(Utc::now()),
            endpoints: vec![MeshEndpoint {
                url: format!(
                    "https://100.64.0.{}:{}",
                    index + 1,
                    server.local_addr.port()
                ),
                tls_spki_sha256: pin,
            }],
            capabilities: vec![
                ControllerScope::CatalogRead,
                ControllerScope::HistoryRead,
                ControllerScope::WorkSend,
            ],
        });
        servers.push(server);
    }
    let (proxy, socks_task) = socks(destinations).await;
    let mut refreshed = credentials(device, Utc::now() + chrono::Duration::minutes(10));
    refreshed.device.revision = 2;
    Mock::given(method("POST"))
        .and(path("/v1/controller/auth/refresh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&refreshed))
        .expect(1)
        .mount(&authority)
        .await;
    let hosts_mock = Mock::given(method("GET"))
        .and(path("/v1/controller/hosts"))
        .and(header("authorization", "Bearer fixture-access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(HostsResponse {
            revision: 1,
            hosts: hosts.clone(),
        }))
        .mount_as_scoped(&authority)
        .await;
    let service = DesktopController::load(
        directory.path().join("desktop"),
        authority.uri(),
        "fixture@example.test".into(),
        "Fixture".into(),
    )
    .unwrap();
    service
        .accept_enrollment(credentials(
            device,
            Utc::now() - chrono::Duration::seconds(1),
        ))
        .await
        .unwrap();
    let catalog = service.sync(Some(proxy)).await.unwrap();
    assert_eq!(catalog.hosts.len(), 2);
    assert!(catalog
        .hosts
        .iter()
        .all(|h| !h.stale && h.entries.len() == 1));
    assert_eq!(
        first.history_reads.load(Ordering::SeqCst) + second.history_reads.load(Ordering::SeqCst),
        0
    );
    let catalog_path = directory.path().join("desktop/catalog.json");
    let original_bytes = std::fs::read(&catalog_path).unwrap();
    let unchanged_mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1);
    std::fs::File::options()
        .write(true)
        .open(&catalog_path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(unchanged_mtime))
        .unwrap();
    let refreshed_catalog = service.sync(Some(proxy)).await.unwrap();
    assert!(refreshed_catalog.updated_at > catalog.updated_at);
    assert_eq!(std::fs::read(&catalog_path).unwrap(), original_bytes);
    assert_eq!(
        std::fs::metadata(&catalog_path)
            .unwrap()
            .modified()
            .unwrap(),
        unchanged_mtime
    );
    let stale = service.sync(None).await.unwrap();
    assert!(stale.hosts.iter().all(|h| h.stale && h.error.is_some()));
    let persisted = private_json::<FleetCatalog>(&catalog_path)
        .unwrap()
        .unwrap();
    assert!(persisted.hosts == stale.hosts);
    assert_ne!(std::fs::read(&catalog_path).unwrap(), original_bytes);
    service.sync(Some(proxy)).await.unwrap();
    assert_eq!(first.catalog_reads.load(Ordering::SeqCst), 1);
    assert_eq!(second.catalog_reads.load(Ordering::SeqCst), 1);
    assert!(service
        .open_work(&first.entry.work, Uuid::new_v4(), None)
        .await
        .is_err());
    service
        .history(
            &first.entry.work,
            first.entry.store_generation,
            None,
            Some(proxy),
        )
        .await
        .unwrap();
    assert_eq!(first.history_reads.load(Ordering::SeqCst), 1);
    assert_eq!(second.history_reads.load(Ordering::SeqCst), 0);
    service
        .history_latest(
            &first.entry.work,
            first.entry.store_generation,
            "snapshot-boundary",
            Some(proxy),
        )
        .await
        .unwrap();
    assert_eq!(first.history_reads.load(Ordering::SeqCst), 2);
    assert_eq!(second.history_reads.load(Ordering::SeqCst), 0);
    let command = WorkCommand {
        command_id: Uuid::new_v4(),
        work: first.entry.work.clone(),
        expected_store_generation: first.entry.store_generation,
        expected_revision: 1,
        expected_run_id: None,
        action: ActionPayload::Send {
            content: vec![ControllerContent::Text {
                text: "One prompt".into(),
            }],
        },
    };
    assert!(service.command(command.clone(), Some(proxy)).await.is_err());
    assert_eq!(
        service.pending_commands(&command.work).await.unwrap().len(),
        1
    );
    drop(service);
    let service = DesktopController::load(
        directory.path().join("desktop"),
        authority.uri(),
        "fixture@example.test".into(),
        "Fixture".into(),
    )
    .unwrap();
    assert_eq!(
        service.pending_commands(&command.work).await.unwrap()[0].command,
        command
    );
    assert!(
        service.command(command.clone(), Some(proxy)).await.is_err(),
        "disk endpoints alone cannot authorize a connection"
    );
    service.sync(Some(proxy)).await.unwrap();
    let receipt = service.command(command.clone(), Some(proxy)).await.unwrap();
    assert!(matches!(receipt.state, CommandState::Delivered));
    assert_eq!(first.accepted.lock().await.len(), 1);
    let mut conflicting = command.clone();
    conflicting.expected_revision = 2;
    assert!(service.command(conflicting, Some(proxy)).await.is_err());
    assert!(service
        .pending_commands(&command.work)
        .await
        .unwrap()
        .is_empty());
    let stale = WorkCommand {
        command_id: Uuid::new_v4(),
        ..command.clone()
    };
    *first.refuse_next.lock().await = Some((StatusCode::CONFLICT, "stale_revision"));
    let failure = service
        .command(stale.clone(), Some(proxy))
        .await
        .unwrap_err();
    assert_eq!(
        (failure.status, failure.code.as_str()),
        (409, "stale_revision")
    );
    assert!(!first.accepted.lock().await.contains_key(&stale.command_id));
    assert!(service
        .pending_commands(&stale.work)
        .await
        .unwrap()
        .is_empty());
    let restarted = DesktopController::load(
        directory.path().join("desktop"),
        authority.uri(),
        "fixture@example.test".into(),
        "Fixture".into(),
    )
    .unwrap();
    assert!(restarted
        .pending_commands(&stale.work)
        .await
        .unwrap()
        .is_empty());
    let reviewed = WorkCommand {
        command_id: Uuid::new_v4(),
        expected_revision: 2,
        ..stale.clone()
    };
    let delivered = service
        .command(reviewed.clone(), Some(proxy))
        .await
        .unwrap();
    assert_eq!(delivered.command_id, reviewed.command_id);
    assert_eq!(first.accepted.lock().await[&reviewed.command_id], reviewed);
    for (status, code) in [
        (StatusCode::CONFLICT, "stale_run"),
        (StatusCode::SERVICE_UNAVAILABLE, "stale_revision"),
        (StatusCode::NOT_FOUND, "command_not_found"),
    ] {
        let uncertain = WorkCommand {
            command_id: Uuid::new_v4(),
            ..command.clone()
        };
        *first.refuse_next.lock().await = Some((status, code));
        assert!(service
            .command(uncertain.clone(), Some(proxy))
            .await
            .is_err());
        assert!(service
            .pending_commands(&uncertain.work)
            .await
            .unwrap()
            .iter()
            .any(|p| p.command == uncertain));
        let changed = WorkCommand {
            expected_revision: 2,
            ..uncertain.clone()
        };
        assert_eq!(
            service
                .command(changed, Some(proxy))
                .await
                .unwrap_err()
                .code,
            "invalid_request"
        );
        service.command(uncertain, Some(proxy)).await.unwrap();
    }
    // Even a contradictory owner response cannot erase an already recorded receipt.
    *first.refuse_next.lock().await = Some((StatusCode::CONFLICT, "stale_revision"));
    assert_eq!(
        service
            .command(command.clone(), Some(proxy))
            .await
            .unwrap_err()
            .code,
        "invalid_request"
    );
    let saved = private_json::<Vec<PendingCommand>>(&service.outbox_path)
        .unwrap()
        .unwrap();
    assert!(saved
        .iter()
        .any(|p| p.command == command && p.receipt.is_some()));
    let before_entry_change = std::fs::read(&catalog_path).unwrap();
    let mut changed_entry = first.entry.clone();
    changed_entry.title = "Owner renamed the work".into();
    changed_entry.revision = 2;
    let mut changed_cursor = first.cursor();
    changed_cursor.revision = 2;
    *first.updates.lock().await = Some(CatalogChanges {
        resume_cursor: "fixture-signed-catalog-2".into(),
        cursor: changed_cursor.clone(),
        has_more: false,
        changes: vec![CatalogChange {
            resume_cursor: "fixture-signed-catalog-2".into(),
            cursor: changed_cursor,
            kind: CatalogChangeKind::Upsert,
            work: changed_entry.work.clone(),
            entry: Some(changed_entry.clone()),
        }],
    });
    service.sync(Some(proxy)).await.unwrap();
    assert_ne!(std::fs::read(&catalog_path).unwrap(), before_entry_change);
    let persisted = private_json::<FleetCatalog>(&catalog_path)
        .unwrap()
        .unwrap();
    assert_eq!(persisted.hosts[0].entries, vec![changed_entry]);
    drop(hosts_mock);
    hosts[1].availability = HostAvailability::Offline;
    hosts[1].endpoints.clear();
    hosts[1].capabilities.clear();
    let offline_mock = Mock::given(method("GET"))
        .and(path("/v1/controller/hosts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(HostsResponse {
            revision: 2,
            hosts: hosts.clone(),
        }))
        .mount_as_scoped(&authority)
        .await;
    let mixed = service.sync(Some(proxy)).await.unwrap();
    assert!(!mixed.hosts[0].stale);
    assert!(mixed.hosts[1].stale);
    assert_eq!(mixed.hosts[1].entries.len(), 1);
    assert!(service
        .history(
            &second.entry.work,
            second.entry.store_generation,
            None,
            Some(proxy)
        )
        .await
        .is_err());
    assert_eq!(second.history_reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        authority
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path().ends_with("/auth/refresh"))
            .count(),
        1
    );
    let persisted = private_json::<FleetCatalog>(&catalog_path)
        .unwrap()
        .unwrap();
    assert!(persisted.hosts == mixed.hosts);
    drop(offline_mock);
    hosts.pop();
    hosts[0].name = "Renamed Mac".into();
    Mock::given(method("GET"))
        .and(path("/v1/controller/hosts"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(HostsResponse { revision: 3, hosts }),
        )
        .mount(&authority)
        .await;
    let removed = service.sync(Some(proxy)).await.unwrap();
    assert_eq!(removed.hosts.len(), 1);
    assert_eq!(removed.hosts[0].host.name, "Renamed Mac");
    let persisted = private_json::<FleetCatalog>(&catalog_path)
        .unwrap()
        .unwrap();
    assert!(persisted.hosts == removed.hosts);
    for server in servers {
        server.shutdown().await.unwrap();
    }
    socks_task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn outbox_storage_failure_blocks_exact_retries_until_persisted() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let authority = MockServer::start().await;
    let device = Uuid::new_v4();
    let mut fixture = Fixture::new(Uuid::new_v4(), device);
    let outbox_path = directory.path().join("desktop/outbox.json");
    fixture.required_outbox = Some(outbox_path.clone());
    let identity = ControllerTlsIdentity::load_or_create(
        directory.path().join("tls"),
        fixture.entry.work.owner_node_id,
    )
    .unwrap();
    let pin = identity.spki_sha256();
    let router = Router::new()
        .route("/v1/controller/catalog", get(catalog))
        .route("/v1/controller/work/chat/{id}/commands", post(command))
        .with_state(fixture.clone());
    let server = start_controller_tls("127.0.0.1:0".parse().unwrap(), router, identity)
        .await
        .unwrap();
    let (proxy, socks_task) = socks(HashMap::from([([100, 64, 0, 1], server.local_addr)])).await;
    Mock::given(method("GET"))
        .and(path("/v1/controller/hosts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(HostsResponse {
            revision: 1,
            hosts: vec![HostDescriptor {
                owner_node_id: fixture.entry.work.owner_node_id,
                name: "Original Mac".into(),
                device_role: DeviceRole::Host,
                protocol_version: 1,
                availability: HostAvailability::Online,
                last_seen_at: Some(Utc::now()),
                endpoints: vec![MeshEndpoint {
                    url: format!("https://100.64.0.1:{}", server.local_addr.port()),
                    tls_spki_sha256: pin,
                }],
                capabilities: vec![ControllerScope::CatalogRead, ControllerScope::WorkSend],
            }],
        }))
        .mount(&authority)
        .await;
    let service = DesktopController::load(
        directory.path().join("desktop"),
        authority.uri(),
        "fixture@example.test".into(),
        "Fixture".into(),
    )
    .unwrap();
    service
        .accept_enrollment(credentials(
            device,
            Utc::now() + chrono::Duration::minutes(10),
        ))
        .await
        .unwrap();
    service.sync(Some(proxy)).await.unwrap();
    save_json(&outbox_path, &Vec::<PendingCommand>::new()).unwrap();
    std::fs::set_permissions(&outbox_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let command = WorkCommand {
        command_id: Uuid::new_v4(),
        work: fixture.entry.work.clone(),
        expected_store_generation: fixture.entry.store_generation,
        expected_revision: 1,
        expected_run_id: None,
        action: ActionPayload::Send {
            content: vec![ControllerContent::Text {
                text: "Persist first".into(),
            }],
        },
    };
    for _ in 0..2 {
        let error = service
            .command(command.clone(), Some(proxy))
            .await
            .unwrap_err();
        assert_eq!(error.code, "controller_unavailable");
        assert_eq!(
            error.message,
            "The private desktop controller store is unreadable or unsafe"
        );
        assert!(service
            .pending_commands(&command.work)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(fixture.command_requests.load(Ordering::SeqCst), 0);
    }
    assert_eq!(std::fs::read_to_string(&outbox_path).unwrap(), "[]");
    std::fs::set_permissions(&outbox_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(service.command(command.clone(), Some(proxy)).await.is_err());
    assert_eq!(fixture.command_requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture.accepted.lock().await.get(&command.command_id),
        Some(&command)
    );
    drop(service);
    let service = DesktopController::load(
        directory.path().join("desktop"),
        authority.uri(),
        "fixture@example.test".into(),
        "Fixture".into(),
    )
    .unwrap();
    assert_eq!(
        service.pending_commands(&command.work).await.unwrap()[0].command,
        command
    );
    service.sync(Some(proxy)).await.unwrap();
    service.command(command.clone(), Some(proxy)).await.unwrap();
    assert_eq!(fixture.command_requests.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.accepted.lock().await.len(), 1);
    server.shutdown().await.unwrap();
    socks_task.abort();
}

#[test]
fn private_identity_rejects_corruption_cross_account_and_unsafe_modes() {
    let directory = tempfile::tempdir().unwrap();
    DesktopController::load(
        directory.path().into(),
        "https://authority.example".into(),
        "a@example.test".into(),
        "A".into(),
    )
    .unwrap();
    assert!(DesktopController::load(
        directory.path().into(),
        "https://authority.example".into(),
        "b@example.test".into(),
        "B".into()
    )
    .is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(directory.path().join("identity.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        std::fs::set_permissions(
            directory.path().join("identity.json"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(DesktopController::load(
            directory.path().into(),
            "https://authority.example".into(),
            "a@example.test".into(),
            "A".into()
        )
        .is_err());
        std::fs::set_permissions(
            directory.path().join("identity.json"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    std::fs::write(directory.path().join("identity.json"), "corrupt").unwrap();
    assert!(DesktopController::load(
        directory.path().into(),
        "https://authority.example".into(),
        "a@example.test".into(),
        "A".into()
    )
    .is_err());
}

#[tokio::test]
async fn authority_revocation_stays_disabled_after_restart_and_logout() {
    let directory = tempfile::tempdir().unwrap();
    let authority = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/controller/auth/refresh"))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            serde_json::json!({"error":{"code":"invalid_credential","message":"revoked"}}),
        ))
        .expect(1)
        .mount(&authority)
        .await;
    let service = DesktopController::load(
        directory.path().into(),
        authority.uri(),
        "fixture@example.test".into(),
        "Fixture".into(),
    )
    .unwrap();
    service
        .accept_enrollment(credentials(
            Uuid::new_v4(),
            Utc::now() - chrono::Duration::seconds(1),
        ))
        .await
        .unwrap();
    assert!(service.sync(None).await.is_err());
    assert!(service.enrollment().await.is_none());
    drop(service);
    let service = DesktopController::load(
        directory.path().into(),
        authority.uri(),
        "fixture@example.test".into(),
        "Fixture".into(),
    )
    .unwrap();
    assert!(service.enrollment().await.is_none());
    service.logout().await.unwrap();
    assert!(service.enrollment().await.is_none());
    let status = service.status().await;
    assert_eq!(status.state, "signed_out");
    assert!(status.device.is_none());
    assert!(service.catalog().await.hosts.is_empty());
}

#[tokio::test]
async fn refresh_lost_ack_reuses_persisted_request_id_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let authority = MockServer::start().await;
    let device = Uuid::new_v4();
    let calls = Arc::new(AtomicUsize::new(0));
    let refreshed = credentials(device, Utc::now() + chrono::Duration::minutes(10));
    let counter = calls.clone();
    Mock::given(method("POST"))
        .and(path("/v1/controller/auth/refresh"))
        .respond_with(move |_: &wiremock::Request| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200).set_body_json(&refreshed)
            }
        })
        .mount(&authority)
        .await;
    let service = DesktopController::load(
        directory.path().into(),
        authority.uri(),
        "fixture@example.test".into(),
        "Fixture".into(),
    )
    .unwrap();
    service
        .accept_enrollment(credentials(
            device,
            Utc::now() - chrono::Duration::seconds(1),
        ))
        .await
        .unwrap();
    assert!(service.credentials().await.is_err());
    drop(service);
    let service = DesktopController::load(
        directory.path().into(),
        authority.uri(),
        "fixture@example.test".into(),
        "Fixture".into(),
    )
    .unwrap();
    assert_eq!(
        service.credentials().await.unwrap().device.device_id,
        device
    );
    let requests = authority.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let first: serde_json::Value = requests[0].body_json().unwrap();
    let second: serde_json::Value = requests[1].body_json().unwrap();
    assert_eq!(first, second);
    assert!(first["request_id"]
        .as_str()
        .is_some_and(|id| Uuid::parse_str(id).is_ok()));
}
