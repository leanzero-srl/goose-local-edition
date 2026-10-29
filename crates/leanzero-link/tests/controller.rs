use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use futures::{stream::BoxStream, StreamExt};
use leanzero_link::controller::{self, ControllerBackend, ControllerContext, ControllerError};
use leanzero_link::controller_auth::{
    authorize, AuthenticatedDevice, AuthorizationError, ControllerAuthority, DispatchAuthorization,
    ReceiverBinding,
};
use leanzero_link::controller_management::*;
use leanzero_link::controller_wire::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tower::ServiceExt;
use uuid::Uuid;

struct Authority {
    revoked: AtomicBool,
    unavailable: AtomicBool,
    deny_dispatch: AtomicBool,
    calls: AtomicUsize,
    lease: AuthorizationLease,
}

#[async_trait]
impl ControllerAuthority for Authority {
    async fn introspect(
        &self,
        token: &str,
        _: &ReceiverBinding,
        _: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(AuthorizationError::Unavailable(
                "test authority offline".into(),
            ));
        }
        if token == "revoked"
            || (token == "phone" && self.revoked.load(Ordering::SeqCst))
            || (self.deny_dispatch.load(Ordering::SeqCst) && call > 0)
        {
            return Ok(None);
        }
        Ok(Some(self.lease.clone()))
    }
}

struct Backend {
    host: HostDescriptor,
    generation: Uuid,
    dispatched: AtomicUsize,
    management: Option<Arc<dyn ManagementBackend>>,
}

fn cursor(backend: &Backend) -> CatalogCursor {
    CatalogCursor {
        owner_node_id: backend.host.owner_node_id,
        store_generation: backend.generation,
        revision: 0,
    }
}

#[async_trait]
impl ControllerBackend for Backend {
    fn management(&self) -> Option<Arc<dyn ManagementBackend>> {
        self.management.clone()
    }
    fn host(&self) -> HostDescriptor {
        self.host.clone()
    }
    fn allowed_scopes(&self) -> Vec<ControllerScope> {
        self.host.capabilities.clone()
    }
    async fn catalog(
        &self,
        _: &AuthenticatedDevice,
        _: Option<&str>,
    ) -> Result<CatalogPage, ControllerError> {
        Ok(CatalogPage {
            resume_cursor: "fixture-catalog-resume".into(),
            entries: vec![],
            watermark: cursor(self),
            next_page_token: None,
        })
    }
    async fn changes(
        &self,
        _: &AuthenticatedDevice,
        _: &str,
    ) -> Result<CatalogChanges, ControllerError> {
        Err(ControllerError::unavailable(
            "fixture does not serve changes",
        ))
    }
    async fn catalog_stream(
        &self,
        _: &AuthenticatedDevice,
        _: &str,
    ) -> Result<BoxStream<'static, Result<CatalogChange, ControllerError>>, ControllerError> {
        let watermark = cursor(self);
        Ok(Box::pin(futures::stream::unfold(
            watermark,
            |cursor| async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                let change = CatalogChange {
                    resume_cursor: "fixture-catalog-resume".into(),
                    cursor: cursor.clone(),
                    kind: CatalogChangeKind::Tombstone,
                    work: WorkRef {
                        owner_node_id: cursor.owner_node_id,
                        kind: WorkKind::Chat,
                        work_id: Uuid::new_v4(),
                    },
                    entry: None,
                };
                Some((Ok(change), cursor))
            },
        )))
    }
    async fn snapshot(
        &self,
        _: &AuthenticatedDevice,
        _: &WorkRef,
        _: Uuid,
    ) -> Result<WorkSnapshot, ControllerError> {
        Err(ControllerError::unavailable(
            "fixture does not serve snapshots",
        ))
    }
    async fn history(
        &self,
        _: &AuthenticatedDevice,
        _: &WorkRef,
        _: Uuid,
        _: Option<&str>,
    ) -> Result<HistoryPage, ControllerError> {
        Err(ControllerError::unavailable(
            "fixture does not serve history",
        ))
    }
    async fn history_latest(
        &self,
        _: &AuthenticatedDevice,
        _: &WorkRef,
        _: Uuid,
        _: &str,
    ) -> Result<HistoryPage, ControllerError> {
        Err(ControllerError::unavailable(
            "fixture does not serve history",
        ))
    }
    async fn work_changes(
        &self,
        _: &AuthenticatedDevice,
        _: &WorkRef,
        _: Uuid,
        _: &str,
    ) -> Result<WorkChanges, ControllerError> {
        Err(ControllerError::unavailable(
            "fixture does not serve work changes",
        ))
    }
    async fn work_stream(
        &self,
        _: &AuthenticatedDevice,
        _: &WorkRef,
        _: &str,
    ) -> Result<BoxStream<'static, Result<WorkFrame, ControllerError>>, ControllerError> {
        Err(ControllerError::unavailable(
            "fixture does not serve work streams",
        ))
    }
    async fn accept_command(
        &self,
        device: &AuthenticatedDevice,
        command: WorkCommand,
        auth: DispatchAuthorization,
    ) -> Result<CommandReceipt, ControllerError> {
        auth.dispatch(|_| async {
            self.dispatched.fetch_add(1, Ordering::SeqCst);
        })
        .await?;
        Ok(CommandReceipt {
            command_id: command.command_id,
            work: command.work,
            authenticated_device_id: device.device_id(),
            revision: 1,
            state: CommandState::Delivered,
            run_id: Some(Uuid::new_v4()),
            message_id: None,
            input_id: None,
            checkpoint_id: None,
            reason: None,
            updated_at: Utc::now(),
        })
    }
    async fn receipt(
        &self,
        _: &AuthenticatedDevice,
        _: Uuid,
    ) -> Result<CommandReceipt, ControllerError> {
        Err(ControllerError::unavailable(
            "fixture does not serve receipts",
        ))
    }
    async fn resources(
        &self,
        _: &AuthenticatedDevice,
    ) -> Result<ResourceSnapshot, ControllerError> {
        Err(ControllerError::unavailable(
            "fixture does not serve resources",
        ))
    }
}

fn fixture() -> (ControllerContext, Arc<Authority>, Arc<Backend>) {
    let receiver = ReceiverBinding {
        owner_node_id: Uuid::new_v4(),
        runtime_id: Uuid::new_v4(),
        store_generation: Uuid::new_v4(),
        host_lease_id: Uuid::new_v4(),
        host_revision: 1,
    };
    let scopes = vec![
        ControllerScope::CatalogRead,
        ControllerScope::HistoryRead,
        ControllerScope::WorkSend,
        ControllerScope::WorkStop,
    ];
    let authority = Arc::new(Authority {
        revoked: AtomicBool::new(false),
        unavailable: AtomicBool::new(false),
        deny_dispatch: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
        lease: AuthorizationLease {
            active: true,
            account_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            device_revision: 1,
            role: DeviceRole::Controller,
            owner_node_id: receiver.owner_node_id,
            scopes: scopes.clone(),
            token_expires_at: Utc::now() + chrono::Duration::minutes(1),
            lease_seconds: 1,
            host_revision: 1,
            store_generation: receiver.store_generation,
            host_lease_id: receiver.host_lease_id,
        },
    });
    let backend = Arc::new(Backend {
        generation: receiver.store_generation,
        dispatched: AtomicUsize::new(0),
        management: None,
        host: HostDescriptor {
            owner_node_id: receiver.owner_node_id,
            name: "Fixture host".into(),
            device_role: DeviceRole::Host,
            protocol_version: 1,
            availability: HostAvailability::Online,
            last_seen_at: Some(Utc::now()),
            endpoints: vec![],
            capabilities: scopes,
        },
    });
    (
        ControllerContext {
            receiver,
            authority: authority.clone(),
            backend: backend.clone(),
        },
        authority,
        backend,
    )
}

fn get(path: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().uri(path);
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn router_rejects_origin_query_tokens_and_revocation_before_backend_reads() {
    let (ctx, authority, _) = fixture();
    let router = controller::router(ctx).unwrap();
    let mut request = get("/v1/controller/catalog", Some("phone"));
    request
        .headers_mut()
        .insert("Origin", "null".parse().unwrap());
    assert_eq!(
        router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(authority.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        router
            .clone()
            .oneshot(get("/v1/controller/catalog?token=phone", None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        router
            .clone()
            .oneshot(get("/v1/controller/catalog", Some("revoked")))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        router
            .clone()
            .oneshot(get("/v1/controller/catalog", Some("phone")))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        router
            .oneshot(get("/v1/swarm/execute", Some("phone")))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn invalid_extractors_use_the_controller_error_contract_and_no_store() {
    let (ctx, _, _) = fixture();
    let response = controller::router(ctx)
        .unwrap()
        .oneshot(get(
            "/v1/controller/work/chat/not-a-uuid?store_generation=bad",
            Some("phone"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "invalid_request");
    assert_eq!(body["error"]["retryable"], false);
}

#[tokio::test]
async fn wrong_receiver_scope_role_and_generation_fail_closed() {
    let (ctx, authority, _) = fixture();
    let mut wrong = ctx.receiver.clone();
    wrong.owner_node_id = Uuid::new_v4();
    assert!(matches!(
        authorize(
            authority.as_ref(),
            "phone",
            &wrong,
            ControllerScope::CatalogRead
        )
        .await,
        Err(AuthorizationError::BindingMismatch)
    ));
    assert!(matches!(
        authorize(
            authority.as_ref(),
            "phone",
            &ctx.receiver,
            ControllerScope::ModelsManage
        )
        .await,
        Err(AuthorizationError::BindingMismatch)
    ));
    drop(ctx);
    let mut authority = authority;
    Arc::get_mut(&mut authority).unwrap().lease.role = DeviceRole::Host;
    wrong.owner_node_id = authority.lease.owner_node_id;
    assert!(matches!(
        authorize(
            authority.as_ref(),
            "phone",
            &wrong,
            ControllerScope::CatalogRead
        )
        .await,
        Err(AuthorizationError::BindingMismatch)
    ));
}

struct DelayedAuthority(AuthorizationLease);

#[async_trait]
impl ControllerAuthority for DelayedAuthority {
    async fn introspect(
        &self,
        _: &str,
        _: &ReceiverBinding,
        _: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError> {
        tokio::time::sleep(Duration::from_millis(1100)).await;
        Ok(Some(self.0.clone()))
    }
}

#[tokio::test]
async fn introspection_latency_consumes_the_lease_instead_of_extending_it() {
    let (ctx, authority, _) = fixture();
    let delayed = DelayedAuthority(authority.lease.clone());
    assert!(matches!(
        authorize(
            &delayed,
            "phone",
            &ctx.receiver,
            ControllerScope::CatalogRead
        )
        .await,
        Err(AuthorizationError::Unavailable(_))
    ));
}

#[tokio::test]
async fn generation_and_host_lease_are_required_receiver_bindings() {
    let (ctx, authority, _) = fixture();
    let mut receiver = ctx.receiver.clone();
    receiver.store_generation = Uuid::new_v4();
    assert!(matches!(
        authorize(
            authority.as_ref(),
            "phone",
            &receiver,
            ControllerScope::CatalogRead
        )
        .await,
        Err(AuthorizationError::BindingMismatch)
    ));
    let mut receiver = ctx.receiver.clone();
    receiver.host_lease_id = Uuid::new_v4();
    assert!(matches!(
        authorize(
            authority.as_ref(),
            "phone",
            &receiver,
            ControllerScope::CatalogRead
        )
        .await,
        Err(AuthorizationError::BindingMismatch)
    ));
    receiver = ctx.receiver.clone();
    receiver.host_revision += 1;
    assert!(matches!(
        authorize(
            authority.as_ref(),
            "phone",
            &receiver,
            ControllerScope::CatalogRead
        )
        .await,
        Err(AuthorizationError::BindingMismatch)
    ));
}

#[test]
fn provider_identity_is_preserved_independently_of_same_name_and_address() {
    let status = leanzero_link::mesh::parse_status_json(
        r#"{
        "BackendState":"Running", "Self":{"PublicKey":"nodekey:self","HostName":"renamed"},
        "Peer":{
            "nodekey:host":{"HostName":"same-name","TailscaleIPs":["100.64.0.8"]},
            "nodekey:phone":{"HostName":"same-name","TailscaleIPs":["100.64.0.8"]}
        }}"#,
    )
    .unwrap();
    assert_eq!(status.self_node_key.as_deref(), Some("nodekey:self"));
    let policy = leanzero_link::controller_discovery::PeerAdmission::registered(
        RegisteredPeers {
            protocol_version: 1,
            lease_seconds: 30,
            revision: 1,
            peers: vec![
                RegisteredPeer {
                    mesh_node_key: "nodekey:host".into(),
                    role: DeviceRole::Host,
                    owner_node_id: Some(Uuid::new_v4()),
                },
                RegisteredPeer {
                    mesh_node_key: "nodekey:phone".into(),
                    role: DeviceRole::Controller,
                    owner_node_id: None,
                },
            ],
        },
        tokio::time::Instant::now(),
    )
    .unwrap();
    let admitted: Vec<_> = status
        .peers
        .iter()
        .filter(|peer| policy.admits_peer(peer))
        .collect();
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].mesh_node_key.as_deref(), Some("nodekey:host"));
}

#[tokio::test]
async fn accepted_command_author_is_rechecked_before_dispatch() {
    let (ctx, authority, backend) = fixture();
    let work = WorkRef {
        owner_node_id: ctx.receiver.owner_node_id,
        kind: WorkKind::Chat,
        work_id: Uuid::new_v4(),
    };
    let command = WorkCommand {
        command_id: Uuid::new_v4(),
        work: work.clone(),
        expected_store_generation: ctx.receiver.store_generation,
        expected_revision: 0,
        expected_run_id: None,
        action: ActionPayload::Send {
            content: vec![ControllerContent::Text {
                text: "hello".into(),
            }],
        },
    };
    authority.deny_dispatch.store(true, Ordering::SeqCst);
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/v1/controller/work/chat/{}/commands",
            work.work_id
        ))
        .header("Authorization", "Bearer phone")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&command).unwrap()))
        .unwrap();
    let response = controller::router(ctx)
        .unwrap()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(backend.dispatched.load(Ordering::SeqCst), 0);
    assert_eq!(authority.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn wire_preserves_action_payload_and_rejects_filesystem_paths() {
    let command = WorkCommand {
        command_id: Uuid::new_v4(),
        work: WorkRef {
            owner_node_id: Uuid::new_v4(),
            kind: WorkKind::Chat,
            work_id: Uuid::new_v4(),
        },
        expected_store_generation: Uuid::new_v4(),
        expected_revision: 2,
        expected_run_id: None,
        action: ActionPayload::Send {
            content: vec![ControllerContent::Resource {
                resource_id: Uuid::new_v4(),
            }],
        },
    };
    let value = serde_json::to_value(&command).unwrap();
    assert_eq!(value["action"], "send");
    assert!(value["payload"]["content"].is_array());
    assert_eq!(
        serde_json::from_value::<WorkCommand>(value).unwrap(),
        command
    );
    assert!(serde_json::from_value::<ControllerContent>(
        serde_json::json!({"type":"resource","path":"/tmp/private"})
    )
    .is_err());
}

async fn socket(
    address: std::net::SocketAddr,
    token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = format!("ws://{address}/v1/controller/catalog/stream?cursor=fixture")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    tokio_tungstenite::connect_async(request).await.unwrap().0
}

async fn close_code(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> u16 {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(Ok(tokio_tungstenite::tungstenite::Message::Close(Some(close)))) =
                socket.next().await
            {
                return u16::from(close.code);
            }
        }
    })
    .await
    .expect("authorization deadline closes stream")
}

#[tokio::test]
async fn revoked_stream_closes_while_other_device_survives_and_outage_closes_that_one() {
    let (ctx, authority, _) = fixture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = controller::router(ctx).unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    let mut phone = socket(address, "phone").await;
    let mut other = socket(address, "other").await;
    assert!(phone.next().await.unwrap().unwrap().is_text());
    authority.revoked.store(true, Ordering::SeqCst);
    assert_eq!(close_code(&mut phone).await, 4403);
    assert!(other.next().await.unwrap().unwrap().is_text());
    authority.unavailable.store(true, Ordering::SeqCst);
    assert_eq!(close_code(&mut other).await, 4413);
    server.abort();
}

#[tokio::test]
async fn latest_history_route_requires_credentials_generation_and_snapshot_cursor() {
    let (ctx, _, _) = fixture();
    let url = format!(
        "/v1/controller/work/chat/{}/history/latest?store_generation={}&cursor=snapshot",
        Uuid::new_v4(),
        ctx.receiver.store_generation
    );
    let wrong_generation = format!(
        "/v1/controller/work/chat/{}/history/latest?store_generation={}&cursor=snapshot",
        Uuid::new_v4(),
        Uuid::new_v4()
    );
    let missing_cursor = url.replace("&cursor=snapshot", "");
    let router = controller::router(ctx).unwrap();
    for (url, token, expected) in [
        (url.as_str(), None, StatusCode::UNAUTHORIZED),
        (
            wrong_generation.as_str(),
            Some("phone"),
            StatusCode::CONFLICT,
        ),
        (
            missing_cursor.as_str(),
            Some("phone"),
            StatusCode::BAD_REQUEST,
        ),
        (url.as_str(), Some("phone"), StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let response = router.clone().oneshot(get(url, token)).await.unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
}

struct NodeManagementProbe(AtomicUsize);
#[async_trait]
impl ManagementBackend for NodeManagementProbe {
    fn scopes(&self) -> Vec<ControllerScope> {
        vec![ControllerScope::ModelsRead, ControllerScope::ModelsManage]
    }
    async fn nodes(&self, device: &AuthenticatedDevice) -> Result<ManagedNodes, ControllerError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ManagedNodes {
            nodes: vec![ManagedNode {
                id: "fixture".into(),
                name: "Fixture".into(),
                kind: ManagedNodeKind::Endpoint,
                model: Some("fixture-model".into()),
                provider: Some("fixture-provider".into()),
                macs: vec![],
                editable: true,
                definition_revision: "fixture-revision".into(),
            }],
            providers: vec![],
            local_models: vec![],
            local_models_error: None,
            capabilities: if device.scopes().contains(&ControllerScope::ModelsManage) {
                vec!["node_create".into(), "node_remove".into()]
            } else {
                vec![]
            },
            supported_kinds: vec![ManagedNodeKind::Cloud, ManagedNodeKind::Endpoint],
        })
    }
    async fn models(&self, _: &AuthenticatedDevice) -> Result<serde_json::Value, ControllerError> {
        Err(ControllerError::unavailable("unused fixture path"))
    }
    async fn download(
        &self,
        _: &AuthenticatedDevice,
        _: &str,
    ) -> Result<serde_json::Value, ControllerError> {
        Err(ControllerError::unavailable("unused fixture path"))
    }
    async fn providers(
        &self,
        _: &AuthenticatedDevice,
    ) -> Result<ManagedProviders, ControllerError> {
        Err(ControllerError::unavailable("unused fixture path"))
    }
    async fn receipt(
        &self,
        _: &AuthenticatedDevice,
        _: Uuid,
    ) -> Result<ManagementReceipt, ControllerError> {
        Err(ControllerError::unavailable("unused fixture path"))
    }
    async fn accept(
        &self,
        device: &AuthenticatedDevice,
        command: ManagementCommand,
        authorization: DispatchAuthorization,
    ) -> Result<ManagementReceipt, ControllerError> {
        authorization.recheck().await?;
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ManagementReceipt {
            command_id: command.command_id,
            owner_node_id: command.owner_node_id,
            authenticated_device_id: device.device_id(),
            action: command.action.name().into(),
            state: CommandState::Accepted,
            target: command.action.target().into(),
            reason: None,
            updated_at: Utc::now(),
        })
    }
}
fn node_management_fixture(
    scopes: Vec<ControllerScope>,
) -> (ControllerContext, Arc<NodeManagementProbe>) {
    let (mut ctx, authority, backend) = fixture();
    let mut lease = authority.lease.clone();
    lease.scopes.extend(scopes.clone());
    ctx.authority = Arc::new(Authority {
        revoked: AtomicBool::new(false),
        unavailable: AtomicBool::new(false),
        deny_dispatch: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
        lease,
    });
    let mut host = backend.host.clone();
    host.capabilities.extend(scopes);
    let probe = Arc::new(NodeManagementProbe(AtomicUsize::new(0)));
    ctx.backend = Arc::new(Backend {
        host,
        generation: backend.generation,
        dispatched: AtomicUsize::new(0),
        management: Some(probe.clone()),
    });
    (ctx, probe)
}
#[tokio::test]
async fn compute_node_get_route_passes_authenticated_scope_and_allowlisted_projection() {
    for manage in [false, true] {
        let mut scopes = vec![ControllerScope::ModelsRead];
        if manage {
            scopes.push(ControllerScope::ModelsManage);
        }
        let (ctx, probe) = node_management_fixture(scopes);
        let router = controller::router(ctx).unwrap();
        let path = "/v1/controller/management/nodes";
        for (token, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("revoked"), StatusCode::FORBIDDEN),
        ] {
            assert_eq!(
                router
                    .clone()
                    .oneshot(get(path, token))
                    .await
                    .unwrap()
                    .status(),
                expected
            );
        }
        assert_eq!(probe.0.load(Ordering::SeqCst), 0);
        let response = router.oneshot(get(path, Some("phone"))).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["nodes"][0]["definition_revision"], "fixture-revision");
        assert_eq!(value["nodes"][0]["kind"], "endpoint");
        assert_eq!(
            value["capabilities"].as_array().unwrap().len(),
            if manage { 2 } else { 0 }
        );
        assert!(value.get("config").is_none());
        assert_eq!(probe.0.load(Ordering::SeqCst), 1);
    }
    let (ctx, probe) = node_management_fixture(vec![]);
    assert_eq!(
        controller::router(ctx)
            .unwrap()
            .oneshot(get("/v1/controller/management/nodes", Some("phone")))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(probe.0.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn compute_node_commands_route_checks_write_scope_generation_and_typed_payloads() {
    for manage in [false, true] {
        let scopes = if manage {
            vec![ControllerScope::ModelsManage]
        } else {
            vec![ControllerScope::ModelsRead]
        };
        let (ctx, probe) = node_management_fixture(scopes);
        let envelope = serde_json::json!({"command_id":Uuid::new_v4(), "owner_node_id":ctx.receiver.owner_node_id,
            "expected_store_generation":ctx.receiver.store_generation, "action":"node_create",
            "payload":{"id":"fixture", "name":"Fixture", "kind":"endpoint", "model":"fixture-model", "provider":"fixture-provider"}});
        let router = controller::router(ctx).unwrap();
        let send = |body: serde_json::Value| {
            Request::builder()
                .method("POST")
                .uri("/v1/controller/management/commands")
                .header("Authorization", "Bearer phone")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap()
        };
        for action in ["node_create", "node_remove"] {
            let mut body = envelope.clone();
            body["action"] = serde_json::json!(action);
            if action == "node_remove" {
                body["payload"] = serde_json::json!({"id":"fixture", "expected_definition_revision":"fixture-revision"});
            }
            assert_eq!(
                router.clone().oneshot(send(body)).await.unwrap().status(),
                if manage {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::FORBIDDEN
                }
            );
        }
        let calls = probe.0.load(Ordering::SeqCst);
        assert_eq!(calls, if manage { 2 } else { 0 });
        let mut extra = envelope.clone();
        extra["payload"]["keep_loaded"] = serde_json::json!(true);
        assert_eq!(
            router.clone().oneshot(send(extra)).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
        let mut remove = envelope.clone();
        remove["action"] = serde_json::json!("node_remove");
        remove["payload"] = serde_json::json!({"id":"fixture"});
        assert_eq!(
            router.clone().oneshot(send(remove)).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
        if manage {
            let mut stale = envelope;
            stale["expected_store_generation"] = serde_json::json!(Uuid::new_v4());
            assert_eq!(
                router.oneshot(send(stale)).await.unwrap().status(),
                StatusCode::CONFLICT
            );
        }
        assert_eq!(probe.0.load(Ordering::SeqCst), calls);
    }
}
