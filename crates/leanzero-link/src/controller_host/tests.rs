use super::*;
use crate::{
    controller_auth::{AuthenticatedDevice, DispatchAuthorization},
    mesh::{BackendState, MeshError, MeshPeer, MeshStatus},
    peer_dial::MeshProxy,
    pubsub::PubSub,
    state::PeerRegistryConfig,
};
use futures::stream::BoxStream;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Mutex,
};
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

mod quit_lifecycle {
    use super::*;
    use crate::{
        control::ControlConfig,
        identity::IdentityStore,
        intent::IntentStore,
        manager::{AuthState, LinkManager, LinkManagerConfig, ReconnectState},
        mesh::MeshConfig,
    };

    struct ObservedMesh {
        inner: Arc<FakeMesh>,
        authority: Arc<AuthorityFixture>,
        lease_at_stop: Mutex<Vec<Option<Uuid>>>,
    }

    #[tokio::test]
    async fn quit_withdraws_even_when_the_host_task_has_failed() {
        let (_dir, server, authority, factory, mesh) = fixture().await;
        let host = PreparedHost::prepare(
            factory.clone(),
            WorkerClient::new(server.uri()).unwrap(),
            "account-fixture",
            "owner@example.test",
            "fixture-host",
        )
        .await
        .unwrap();
        host.enroll(
            mesh.as_ref(),
            host.mesh_context().await.unwrap(),
            "fixture-host",
        )
        .await
        .unwrap();
        let handle = host
            .activate(
                mesh,
                registry(),
                "100.64.0.1".into(),
                crate::control::DEFAULT_CONTROL_PORT,
                resources(),
            )
            .await
            .unwrap();
        assert!(authority.runtime.lock().unwrap().is_some());
        handle.task.as_ref().unwrap().abort();
        let error = handle.shutdown(false).await.unwrap_err().to_string();
        assert!(error.contains("cancelled"), "{error}");
        assert!(authority.runtime.lock().unwrap().is_none());
        assert!(!authority.revoked.load(Ordering::SeqCst));
        assert!(factory.directory.join("host-credential.json").exists());
    }

    #[async_trait]
    impl Mesh for ObservedMesh {
        async fn join(&self, key: &str, name: &str) -> Result<(), MeshError> {
            self.inner.join(key, name).await
        }
        async fn reauthenticate(&self, key: &str, name: &str) -> Result<(), MeshError> {
            self.inner.reauthenticate(key, name).await
        }
        async fn resume(&self) -> Result<(), MeshError> {
            self.inner.resume().await
        }
        async fn status(&self) -> Result<MeshStatus, MeshError> {
            self.inner.status().await
        }
        async fn peer_proxy(&self) -> Result<MeshProxy, MeshError> {
            self.inner.peer_proxy().await
        }
        async fn logout(&self) -> Result<(), MeshError> {
            panic!("quit must preserve mesh enrollment")
        }
        async fn shutdown(&self) {
            self.lease_at_stop
                .lock()
                .unwrap()
                .push(*self.authority.runtime.lock().unwrap());
        }
    }

    struct ObservedFactory(Arc<ObservedMesh>);
    #[async_trait]
    impl crate::manager::MeshFactory for ObservedFactory {
        async fn start(&self, _: MeshConfig) -> Result<Arc<dyn Mesh>, MeshError> {
            Ok(self.0.clone())
        }
    }

    fn manager(
        root: &std::path::Path,
        server: &MockServer,
        factory: Arc<Factory>,
        mesh: Arc<ObservedMesh>,
    ) -> LinkManager {
        let mut config = MeshConfig::new(
            "fixture-daemon".into(),
            "fixture-cli".into(),
            "fixture-host".into(),
        )
        .unwrap();
        config.state_dir = root.join("mesh");
        config.socket_path = config.state_dir.join("fixture.sock");
        let mut control = ControlConfig::new(String::new(), None);
        control.port = 0;
        control.jobs.root = root.join("jobs");
        control.jobs.heavy_lock = root.join("heavy.lock");
        LinkManager::with_mesh_factory(
            LinkManagerConfig {
                worker_base_url: server.uri(),
                identity_path: root.join("identity.json"),
                mesh: config,
                control,
            },
            Arc::new(Source),
            Arc::new(ObservedFactory(mesh)),
        )
        .unwrap()
        .with_controller_host(factory)
    }

    #[tokio::test]
    async fn quit_withdraws_before_mesh_stop_and_immediate_relaunch_keeps_login_and_enrollment() {
        let (dir, server, authority, factory, inner) = fixture().await;
        let mesh = Arc::new(ObservedMesh {
            inner,
            authority: authority.clone(),
            lease_at_stop: Mutex::new(Vec::new()),
        });
        let first = manager(dir.path(), &server, factory.clone(), mesh.clone());
        first.verify("owner@example.test", "123456").await.unwrap();
        first.connect().await.unwrap();
        let runtime = authority.runtime.lock().unwrap().expect("active lease");
        let enrollment = *authority.enrollment.lock().unwrap();
        let identity = dir.path().join("identity.json");
        let files = [
            identity.clone(),
            secret_path(&identity),
            IntentStore::beside(&identity).path().to_path_buf(),
            factory.directory.join("host-credential.json"),
        ];
        let before: Vec<_> = files.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let intent = first.status().await.intent;
        first.shutdown().await.unwrap();
        first.shutdown().await.unwrap();
        assert_eq!(*mesh.lease_at_stop.lock().unwrap(), vec![None]);
        assert!(matches!(
            first.status().await.auth,
            AuthState::LoggedIn { .. }
        ));
        assert_eq!(first.status().await.intent, intent);
        assert_eq!(
            files
                .iter()
                .map(|p| std::fs::read(p).unwrap())
                .collect::<Vec<_>>(),
            before
        );
        assert_eq!(*authority.enrollment.lock().unwrap(), enrollment);
        assert!(!authority.revoked.load(Ordering::SeqCst));
        drop(first);

        let second = manager(dir.path(), &server, factory, mesh.clone());
        assert!(matches!(
            second.auto_reconnect(Ok(()), ToString::to_string).await,
            ReconnectState::Reconnected { .. }
        ));
        assert_ne!(*authority.runtime.lock().unwrap(), Some(runtime));
        assert_eq!(mesh.inner.reauth.load(Ordering::SeqCst), 1);
        assert_eq!(mesh.inner.resume.load(Ordering::SeqCst), 1);
        second.shutdown().await.unwrap();
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.url.path() == "/v1/auth/verify")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.url.path() == "/v1/controller/hosts/withdraw")
                .count(),
            2
        );
        assert!(requests
            .iter()
            .all(|r| r.url.path() != "/v1/controller/hosts/release"));
    }

    #[tokio::test]
    async fn quit_reports_failed_withdrawal_and_still_stops_mesh_without_revoking_credentials() {
        let (dir, server, authority, factory, inner) = fixture().await;
        let mesh = Arc::new(ObservedMesh {
            inner,
            authority: authority.clone(),
            lease_at_stop: Mutex::new(Vec::new()),
        });
        let link = manager(dir.path(), &server, factory.clone(), mesh.clone());
        link.verify("owner@example.test", "123456").await.unwrap();
        link.connect().await.unwrap();
        let runtime = authority.runtime.lock().unwrap().expect("active lease");
        let intent = link.status().await.intent;
        Mock::given(path("/v1/controller/hosts/withdraw"))
            .respond_with(ResponseTemplate::new(503).set_body_json(json!({
                "error":{"code":"withdraw_unavailable","message":"withdraw unavailable","retryable":true}
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        let error = link.shutdown().await.unwrap_err().to_string();
        assert!(error.contains("withdraw_unavailable"), "{error}");
        assert_eq!(*mesh.lease_at_stop.lock().unwrap(), vec![Some(runtime)]);
        let state = link.status().await;
        assert_eq!(state.last_error.as_deref(), Some(error.as_str()));
        assert_eq!(state.intent, intent);
        assert!(matches!(state.auth, AuthState::LoggedIn { .. }));
        link.shutdown().await.unwrap();
        assert_eq!(mesh.lease_at_stop.lock().unwrap().len(), 1);
        assert_eq!(link.status().await.last_error, state.last_error);
        assert!(factory.directory.join("host-credential.json").exists());
        assert!(!authority.revoked.load(Ordering::SeqCst));
        assert!(IdentityStore::new(dir.path().join("identity.json"))
            .load()
            .unwrap()
            .is_some());
        let requests = server.received_requests().await.unwrap();
        assert!(requests
            .iter()
            .all(|r| r.url.path() != "/v1/controller/hosts/release"));
    }
}

struct Backend {
    host: HostDescriptor,
    generation: Uuid,
}
fn unused() -> ControllerError {
    ControllerError::unavailable("fixture method is not configured")
}
#[async_trait]
impl ControllerBackend for Backend {
    fn host(&self) -> HostDescriptor {
        self.host.clone()
    }
    fn allowed_scopes(&self) -> Vec<ControllerScope> {
        vec![ControllerScope::CatalogRead]
    }
    async fn catalog(
        &self,
        _: &AuthenticatedDevice,
        _: Option<&str>,
    ) -> Result<CatalogPage, ControllerError> {
        Ok(CatalogPage {
            resume_cursor: "test-signed-cursor".into(),
            watermark: CatalogCursor {
                owner_node_id: self.host.owner_node_id,
                store_generation: self.generation,
                revision: 7,
            },
            next_page_token: None,
            entries: vec![CatalogEntry {
                work: WorkRef {
                    owner_node_id: self.host.owner_node_id,
                    kind: WorkKind::Chat,
                    work_id: Uuid::new_v4(),
                },
                store_generation: self.generation,
                revision: 7,
                title: "Owner's existing work".into(),
                working_dir: None,
                state: WorkState::Running,
                current_run_id: None,
                capabilities: vec![],
                parent: None,
                related: vec![],
                updated_at: Utc::now(),
                last_event: None,
                archived: false,
            }],
        })
    }
    async fn changes(
        &self,
        _: &AuthenticatedDevice,
        _: &str,
    ) -> Result<CatalogChanges, ControllerError> {
        Err(unused())
    }
    async fn catalog_stream(
        &self,
        _: &AuthenticatedDevice,
        _: &str,
    ) -> Result<BoxStream<'static, Result<CatalogChange, ControllerError>>, ControllerError> {
        Err(unused())
    }
    async fn snapshot(
        &self,
        _: &AuthenticatedDevice,
        _: &WorkRef,
        _: Uuid,
    ) -> Result<WorkSnapshot, ControllerError> {
        Err(unused())
    }
    async fn history(
        &self,
        _: &AuthenticatedDevice,
        _: &WorkRef,
        _: Uuid,
        _: Option<&str>,
    ) -> Result<HistoryPage, ControllerError> {
        Err(unused())
    }
    async fn work_changes(
        &self,
        _: &AuthenticatedDevice,
        _: &WorkRef,
        _: Uuid,
        _: &str,
    ) -> Result<WorkChanges, ControllerError> {
        Err(unused())
    }
    async fn work_stream(
        &self,
        _: &AuthenticatedDevice,
        _: &WorkRef,
        _: &str,
    ) -> Result<BoxStream<'static, Result<WorkFrame, ControllerError>>, ControllerError> {
        Err(unused())
    }
    async fn accept_command(
        &self,
        _: &AuthenticatedDevice,
        _: WorkCommand,
        _: DispatchAuthorization,
    ) -> Result<CommandReceipt, ControllerError> {
        Err(unused())
    }
    async fn receipt(
        &self,
        _: &AuthenticatedDevice,
        _: Uuid,
    ) -> Result<CommandReceipt, ControllerError> {
        Err(unused())
    }
    async fn resources(
        &self,
        _: &AuthenticatedDevice,
    ) -> Result<ResourceSnapshot, ControllerError> {
        Err(unused())
    }
}

struct Factory {
    directory: PathBuf,
    generation: Uuid,
    revision: AtomicU64,
}
#[async_trait]
impl ControllerHostFactory for Factory {
    fn directory(&self) -> PathBuf {
        self.directory.clone()
    }
    fn listen_address(&self) -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }
    async fn watermark(&self) -> Result<HostWatermark, ControllerError> {
        Ok(HostWatermark {
            store_generation: self.generation,
            catalog_revision: self.revision.load(Ordering::SeqCst),
        })
    }
    async fn backend(
        &self,
        host: HostDescriptor,
        _: Uuid,
        _: HostResourceSampler,
    ) -> Result<Arc<dyn ControllerBackend>, ControllerError> {
        Ok(Arc::new(Backend {
            host,
            generation: self.generation,
        }))
    }
}

struct FakeMesh {
    key: Mutex<Option<String>>,
    peers: Mutex<Vec<MeshPeer>>,
    reauth: AtomicUsize,
    resume: AtomicUsize,
}
#[async_trait]
impl Mesh for FakeMesh {
    async fn join(&self, _: &str, _: &str) -> Result<(), MeshError> {
        panic!("host must never use the legacy join")
    }
    async fn reauthenticate(&self, key: &str, _: &str) -> Result<(), MeshError> {
        assert_eq!(key, "host-fixture-key");
        self.reauth.fetch_add(1, Ordering::SeqCst);
        *self.key.lock().unwrap() = Some(format!("nodekey:{}", "a".repeat(64)));
        Ok(())
    }
    async fn resume(&self) -> Result<(), MeshError> {
        self.resume.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn status(&self) -> Result<MeshStatus, MeshError> {
        Ok(MeshStatus {
            self_node_key: self.key.lock().unwrap().clone(),
            self_ip: Some("100.64.0.1".into()),
            self_hostname: Some("fixture-host".into()),
            backend_state: BackendState::Running,
            online: true,
            peers: self.peers.lock().unwrap().clone(),
        })
    }
    async fn peer_proxy(&self) -> Result<MeshProxy, MeshError> {
        Ok(MeshProxy::socks5("127.0.0.1:9".parse().unwrap()).unwrap())
    }
    async fn logout(&self) -> Result<(), MeshError> {
        Ok(())
    }
    async fn shutdown(&self) {}
}

struct AuthorityFixture {
    owner: Mutex<Option<Uuid>>,
    account: Uuid,
    lease: Uuid,
    generation: Uuid,
    runtime: Mutex<Option<Uuid>>,
    highwater: AtomicU64,
    enrollment: Mutex<Option<Uuid>>,
    confirmed: AtomicBool,
    outage: AtomicBool,
    revoked: AtomicBool,
    recovery_response: Mutex<Option<Value>>,
    confirmation_response: Mutex<Option<(u16, Value)>>,
    peer_snapshot: Mutex<Option<Value>>,
    directory: PathBuf,
}

impl wiremock::Respond for AuthorityFixture {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let body: Value = if request.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&request.body).unwrap()
        };
        let owner = || self.owner.lock().unwrap().expect("registered owner");
        let error = |status, code| {
            ResponseTemplate::new(status)
                .set_body_json(json!({"error":{"code":code,"message":code,"retryable":false}}))
        };
        if self.outage.load(Ordering::SeqCst) {
            return error(503, "registry_unavailable");
        }
        match request.url.path() {
            "/v1/auth/verify" => ResponseTemplate::new(200).set_body_json(json!({"token":"account-fixture","email":"owner@example.test","audienceSync":"skipped","nodeSecret":"a".repeat(64)})),
            "/v1/controller/hosts/register" => {
                *self.owner.lock().unwrap() = Some(serde_json::from_value(body["owner_node_id"].clone()).unwrap());
                assert!(body["controller_port"].as_u64().unwrap() > 0);
                assert_eq!(body["tls_spki_sha256"].as_str().unwrap().len(), 64);
                ResponseTemplate::new(200).set_body_json(json!({"host_credential":"host-fixture-credential","host_revision":1}))
            }
            "/v1/controller/hosts/recover-node-secret" => {
                assert_eq!(request.headers.get("Authorization").unwrap(), "Bearer account-fixture");
                assert_eq!(request.headers.get("X-Link-Host-Credential").unwrap(), "host-fixture-credential");
                assert_eq!(body, json!({}));
                if let Some(response) = self.recovery_response.lock().unwrap().as_ref() {
                    return ResponseTemplate::new(200).set_body_json(response);
                }
                ResponseTemplate::new(200).set_body_json(json!({"account_id":self.account,"owner_node_id":owner(),"email":"owner@example.test","node_secret":"a".repeat(64)}))
            }
            "/v1/controller/hosts/bootstrap" => ResponseTemplate::new(200).set_body_json(json!({"account_id":self.account,"owner_node_id":owner()})),
            "/v1/controller/hosts/mesh/context" => {
                let e = *self.enrollment.lock().unwrap();
                ResponseTemplate::new(200).set_body_json(json!({"login_server":"https://fixture.invalid", "enrollment": e.map(|id| json!({"enrollment_id":id,"status":if self.confirmed.load(Ordering::SeqCst) {"confirmed"} else {"issued"},"mesh_node_key": if self.confirmed.load(Ordering::SeqCst) {Some(format!("nodekey:{}","a".repeat(64)))} else {None}}))}))
            }
            "/v1/controller/hosts/mesh/join-key" => {
                let id: Uuid = serde_json::from_value(body["enrollment_id"].clone()).unwrap();
                assert_eq!(private_json::<Uuid>(&self.directory.join("pending-enrollment.json")).unwrap(), Some(id));
                *self.enrollment.lock().unwrap() = Some(id);
                ResponseTemplate::new(200).set_body_json(json!({"enrollment_id":id,"auth_key":"host-fixture-key","login_server":"https://fixture.invalid","expiry_seconds":60}))
            }
            "/v1/controller/hosts/mesh/confirm" => {
                assert_eq!(request.headers.get("Authorization").unwrap(), "Bearer host-fixture-credential");
                assert_eq!(body["enrollment_id"], json!(*self.enrollment.lock().unwrap()));
                if let Some((status, response)) = self.confirmation_response.lock().unwrap().as_ref() {
                    return ResponseTemplate::new(*status).set_body_json(response);
                }
                if body["mesh_node_key"] != format!("nodekey:{}","a".repeat(64)) { return error(403,"mesh_binding_unverified"); }
                self.confirmed.store(true,Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(json!({"owner_node_id":owner(),"mesh_node_key":body["mesh_node_key"],"host_revision":1}))
            }
            "/v1/controller/hosts/lease" => {
                let id: Uuid = serde_json::from_value(body["runtime_id"].clone()).unwrap();
                let mut runtime = self.runtime.lock().unwrap();
                if runtime.is_some_and(|old| old != id) { return error(409,"owner_identity_in_use"); }
                if body["store_generation"] != self.generation.to_string() { return error(409,"generation_changed"); }
                let revision = body["catalog_revision"].as_u64().unwrap();
                if revision < self.highwater.load(Ordering::SeqCst) { return error(409,"store_rollback"); }
                assert_eq!(body["capabilities"], json!(["catalog.read"]));
                self.highwater.store(revision,Ordering::SeqCst); *runtime = Some(id);
                ResponseTemplate::new(200).set_body_json(json!({"lease_id":self.lease,"lease_expires_at":Utc::now()+chrono::Duration::seconds(90),"host_revision":1}))
            }
            "/v1/controller/hosts/peers" => ResponseTemplate::new(200).set_body_json(self.peer_snapshot.lock().unwrap().clone().unwrap_or_else(|| json!({"protocol_version":1,"lease_seconds":30,"revision":2,"peers":[{"mesh_node_key":format!("nodekey:{}","a".repeat(64)),"role":"host","owner_node_id":owner()},{"mesh_node_key":format!("nodekey:{}","b".repeat(64)),"role":"controller","owner_node_id":null}]}))),
            "/v1/controller/introspect" => {
                if self.revoked.load(Ordering::SeqCst) || body["token"] != "device-fixture-token" { return ResponseTemplate::new(200).set_body_json(json!({"active":false})); }
                ResponseTemplate::new(200).set_body_json(json!({"active":true,"account_id":self.account,"device_id":Uuid::new_v4(),"device_revision":1,"role":"controller","owner_node_id":owner(),"scopes":["catalog.read"],"token_expires_at":Utc::now()+chrono::Duration::hours(1),"lease_seconds":30,"host_revision":1,"store_generation":self.generation,"host_lease_id":self.lease}))
            }
            "/v1/controller/hosts/release" => { self.revoked.store(true,Ordering::SeqCst); ResponseTemplate::new(200).set_body_json(json!({"released":true})) }
            "/v1/controller/hosts/withdraw" => { *self.runtime.lock().unwrap() = None; ResponseTemplate::new(200).set_body_json(json!({"withdrawn":true})) }
            _ => error(404,"not_found"),
        }
    }
}

async fn fixture() -> (
    tempfile::TempDir,
    MockServer,
    Arc<AuthorityFixture>,
    Arc<Factory>,
    Arc<FakeMesh>,
) {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    let generation = Uuid::new_v4();
    let directory = dir.path().join("host");
    let authority = Arc::new(AuthorityFixture {
        owner: Mutex::new(None),
        account: Uuid::new_v4(),
        lease: Uuid::new_v4(),
        generation,
        runtime: Mutex::new(None),
        highwater: AtomicU64::new(0),
        enrollment: Mutex::new(None),
        confirmed: AtomicBool::new(false),
        outage: AtomicBool::new(false),
        revoked: AtomicBool::new(false),
        recovery_response: Mutex::new(None),
        confirmation_response: Mutex::new(None),
        peer_snapshot: Mutex::new(None),
        directory: directory.clone(),
    });
    let responder = authority.clone();
    Mock::given(wiremock::matchers::any())
        .respond_with(move |req: &wiremock::Request| {
            wiremock::Respond::respond(responder.as_ref(), req)
        })
        .mount(&server)
        .await;
    let factory = Arc::new(Factory {
        directory,
        generation,
        revision: AtomicU64::new(7),
    });
    let mesh = Arc::new(FakeMesh {
        key: Mutex::new(Some(format!("nodekey:{}", "f".repeat(64)))),
        peers: Mutex::new(Vec::new()),
        reauth: AtomicUsize::new(0),
        resume: AtomicUsize::new(0),
    });
    (dir, server, authority, factory, mesh)
}
fn registry() -> PeerRegistry {
    PeerRegistry::new(
        PeerRegistryConfig {
            node_token: "fixture-node".into(),
            poll_interval: Duration::from_secs(1),
            request_timeout: Duration::from_secs(1),
            reconnect_backoff: Duration::from_secs(1),
            peer_proxy: None,
        },
        Arc::new(PubSub::new()),
    )
    .unwrap()
}
fn resources() -> HostResourceSampler {
    Arc::new(|| Box::pin(async { panic!("fixture does not request resources") }))
}

struct Source;
#[async_trait]
impl crate::state::SwarmStateSource for Source {
    async fn local_node(&self) -> crate::wire::NodeState {
        serde_json::from_value(json!({
            "node_id":"fixture-node", "hostname":"fixture-host", "mesh_ip":null,
            "status":{"type":"Idle"}, "sessions_active":0, "updated_at":Utc::now()
        }))
        .unwrap()
    }
    async fn local_sessions(&self) -> Result<Vec<crate::wire::SessionSummary>, String> {
        Ok(vec![])
    }
    fn subscribe_local_deltas(&self) -> BoxStream<'static, crate::wire::LinkEvent> {
        Box::pin(futures::stream::pending())
    }
}
struct MeshFactory {
    mesh: Arc<FakeMesh>,
    started: AtomicUsize,
}
#[async_trait]
impl crate::manager::MeshFactory for MeshFactory {
    async fn start(&self, config: crate::mesh::MeshConfig) -> Result<Arc<dyn Mesh>, MeshError> {
        assert_eq!(config.login_server, "https://fixture.invalid");
        self.started.fetch_add(1, Ordering::SeqCst);
        Ok(self.mesh.clone())
    }
}

#[tokio::test]
async fn manager_upgrades_saved_login_through_disconnect_reconnect_and_logout() {
    use crate::{
        control::ControlConfig,
        identity::{Identity, IdentityStore},
        manager::{AuthState, LinkManager, LinkManagerConfig},
        mesh::MeshConfig,
    };
    let (dir, server, authority, factory, mesh) = fixture().await;
    let identity_path = dir.path().join("identity.json");
    IdentityStore::new(&identity_path)
        .save(&Identity::new("owner@example.test", "account-fixture"))
        .unwrap();
    let mut config = MeshConfig::new(
        "fixture-daemon".into(),
        "fixture-cli".into(),
        "fixture-host".into(),
    )
    .unwrap();
    config.state_dir = dir.path().join("mesh");
    config.socket_path = config.state_dir.join("fixture.sock");
    let mut control = ControlConfig::new(String::new(), None);
    control.port = 0;
    control.jobs.root = dir.path().join("jobs");
    control.jobs.heavy_lock = dir.path().join("heavy.lock");
    let mesh_factory = Arc::new(MeshFactory {
        mesh: mesh.clone(),
        started: AtomicUsize::new(0),
    });
    let manager = LinkManager::with_mesh_factory(
        LinkManagerConfig {
            worker_base_url: server.uri(),
            identity_path: identity_path.clone(),
            mesh: config,
            control,
        },
        Arc::new(Source),
        mesh_factory.clone(),
    )
    .unwrap()
    .with_controller_host(factory.clone());
    assert!(!secret_path(&identity_path).exists());
    let saved_identity = std::fs::read(&identity_path).unwrap();
    manager.connect().await.unwrap();
    assert_eq!(std::fs::read(&identity_path).unwrap(), saved_identity);
    let saved_owner = std::fs::read(factory.directory.join("owner.json")).unwrap();
    let saved_tls = std::fs::read(factory.directory.join("tls.json")).unwrap();
    assert_eq!(
        login_secret(&identity_path, "owner@example.test").unwrap(),
        "a".repeat(64)
    );
    assert!(matches!(
        manager.status().await.auth,
        AuthState::Connected { .. }
    ));
    assert_eq!(mesh.reauth.load(Ordering::SeqCst), 1);
    assert_eq!(authority.highwater.load(Ordering::SeqCst), 7);
    assert_eq!(
        manager.node_token().await,
        Some(crate::token::node_token_from_secret(&"a".repeat(64)))
    );
    manager.disconnect().await.unwrap();
    assert!(matches!(
        manager.status().await.auth,
        AuthState::LoggedIn { .. }
    ));
    assert!(authority.runtime.lock().unwrap().is_none());
    manager.connect().await.unwrap();
    assert_eq!(mesh.reauth.load(Ordering::SeqCst), 1);
    assert_eq!(mesh.resume.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(factory.directory.join("owner.json")).unwrap(),
        saved_owner
    );
    assert_eq!(
        std::fs::read(factory.directory.join("tls.json")).unwrap(),
        saved_tls
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/v1/controller/hosts/recover-node-secret")
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/v1/controller/hosts/mesh/join-key")
            .count(),
        1
    );
    assert!(!requests
        .iter()
        .any(|r| r.url.path().starts_with("/v1/auth/")));
    manager.logout(false).await.unwrap();
    assert_eq!(manager.status().await.auth, AuthState::LoggedOut);
    assert!(authority.revoked.load(Ordering::SeqCst));
    assert!(!identity_path.exists());
    assert!(!secret_path(&identity_path).exists());
    assert!(!factory.directory.join("host-credential.json").exists());
    assert!(factory.directory.join("account.json").exists());
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|request| request.url.path() != "/v1/mesh/join-key"));
}

#[tokio::test]
async fn delayed_recovery_cannot_recreate_secret_after_logout() {
    use crate::{
        control::ControlConfig,
        identity::{Identity, IdentityStore},
        manager::{AuthState, LinkManager, LinkManagerConfig},
        mesh::MeshConfig,
    };
    let (dir, server, authority, factory, mesh) = fixture().await;
    let identity_path = dir.path().join("identity.json");
    IdentityStore::new(&identity_path)
        .save(&Identity::new("owner@example.test", "account-fixture"))
        .unwrap();
    let mut config = MeshConfig::new(
        "fixture-daemon".into(),
        "fixture-cli".into(),
        "fixture-host".into(),
    )
    .unwrap();
    config.state_dir = dir.path().join("mesh");
    config.socket_path = config.state_dir.join("fixture.sock");
    let mut control = ControlConfig::new(String::new(), None);
    control.port = 0;
    control.jobs.root = dir.path().join("jobs");
    control.jobs.heavy_lock = dir.path().join("heavy.lock");
    let mesh_factory = Arc::new(MeshFactory {
        mesh: mesh.clone(),
        started: AtomicUsize::new(0),
    });
    let manager = LinkManager::with_mesh_factory(
        LinkManagerConfig {
            worker_base_url: server.uri(),
            identity_path: identity_path.clone(),
            mesh: config,
            control,
        },
        Arc::new(Source),
        mesh_factory.clone(),
    )
    .unwrap()
    .with_controller_host(factory.clone());
    let manager = Arc::new(manager);
    let entered = Arc::new(AtomicBool::new(false));
    let arrived = entered.clone();
    let response_authority = authority.clone();
    Mock::given(path("/v1/controller/hosts/recover-node-secret"))
        .respond_with(move |request: &wiremock::Request| {
            arrived.store(true, Ordering::SeqCst);
            wiremock::Respond::respond(response_authority.as_ref(), request)
                .set_delay(Duration::from_millis(400))
        })
        .with_priority(1)
        .mount(&server)
        .await;
    let connecting = manager.clone();
    let pending = tokio::spawn(async move { connecting.connect().await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    manager.logout(false).await.unwrap();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(manager.status().await.auth, AuthState::LoggedOut);
    assert!(!identity_path.exists());
    assert!(!secret_path(&identity_path).exists());
    assert_eq!(mesh_factory.started.load(Ordering::SeqCst), 0);
}

#[test]
fn host_sign_in_secret_is_private_account_bound_and_corruption_is_loud() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.json");
    assert!(matches!(
        login_secret(&path, "owner@example.test"),
        Err(LinkError::NoNodeSecret)
    ));
    remember_login_secret(&path, "owner@example.test", &"a".repeat(64)).unwrap();
    assert_eq!(
        login_secret(&path, "owner@example.test").unwrap(),
        "a".repeat(64)
    );
    assert!(login_secret(&path, "other@example.test").is_err());
    assert!(remember_login_secret(&path, "owner@example.test", &"b".repeat(64)).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(secret_path(&path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    std::fs::write(secret_path(&path), "corrupt").unwrap();
    assert!(login_secret(&path, "owner@example.test").is_err());
    assert_eq!(std::fs::read(secret_path(&path)).unwrap(), b"corrupt");
}

#[tokio::test]
async fn recovery_rejects_wrong_account_owner_and_email_and_preserves_missing_secret_on_outage() {
    let (dir, server, authority, factory, _mesh) = fixture().await;
    let host = PreparedHost::prepare(
        factory,
        WorkerClient::new(server.uri()).unwrap(),
        "account-fixture",
        "owner@example.test",
        "fixture-host",
    )
    .await
    .unwrap();
    let path = dir.path().join("identity.json");
    std::fs::write(&path, b"saved login must survive").unwrap();
    for patch in [
        json!({"account_id":Uuid::new_v4()}),
        json!({"owner_node_id":Uuid::new_v4()}),
        json!({"email":"another@example.test"}),
        json!({"node_secret":"corrupt"}),
    ] {
        let mut response = json!({"account_id":authority.account,"owner_node_id":host.owner.owner_node_id,"email":"owner@example.test","node_secret":"a".repeat(64)});
        response
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        // Wiremock 0.6.5 priority sorting invalidates scoped guard indices.
        // One fixture responder makes each negative and its reset explicit.
        *authority.recovery_response.lock().unwrap() = Some(response);
        let error = host
            .login_secret(&path, "account-fixture", "owner@example.test")
            .await
            .unwrap_err();
        if patch.get("node_secret").is_some() {
            assert!(matches!(error, LinkError::NoNodeSecret), "{error}");
        } else {
            assert!(
                matches!(error, LinkError::ControllerHost(ref message) if message == "recovered secret belongs to another account or owner"),
                "{error}"
            );
        }
        assert!(!secret_path(&path).exists());
        *authority.recovery_response.lock().unwrap() = None;
        assert_eq!(
            host.login_secret(&path, "account-fixture", "owner@example.test")
                .await
                .unwrap(),
            "a".repeat(64)
        );
    }
    authority.outage.store(true, Ordering::SeqCst);
    let outage_error = host
        .login_secret(&path, "account-fixture", "owner@example.test")
        .await
        .unwrap_err();
    assert!(
        matches!(
            outage_error,
            LinkError::Worker(crate::worker_client::WorkerError::Unexpected { status: 503, .. })
        ),
        "{outage_error}"
    );
    assert!(!secret_path(&path).exists());
    assert_eq!(std::fs::read(&path).unwrap(), b"saved login must survive");
    authority.outage.store(false, Ordering::SeqCst);
    host.login_secret(&path, "account-fixture", "owner@example.test")
        .await
        .unwrap();
    let before = server.received_requests().await.unwrap().len();
    remember_login_secret(&path, "owner@example.test", &"a".repeat(64)).unwrap();
    std::fs::write(secret_path(&path), b"corrupt").unwrap();
    assert!(host
        .login_secret(&path, "account-fixture", "owner@example.test")
        .await
        .is_err());
    assert_eq!(server.received_requests().await.unwrap().len(), before);
    assert_eq!(std::fs::read(secret_path(&path)).unwrap(), b"corrupt");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};
        std::fs::set_permissions(secret_path(&path), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(host
            .login_secret(&path, "account-fixture", "owner@example.test")
            .await
            .is_err());
        std::fs::remove_file(secret_path(&path)).unwrap();
        let target = dir.path().join("unrelated-secret");
        std::fs::write(&target, b"do not touch").unwrap();
        symlink(&target, secret_path(&path)).unwrap();
        assert!(host
            .login_secret(&path, "account-fixture", "owner@example.test")
            .await
            .is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"do not touch");
        assert_eq!(server.received_requests().await.unwrap().len(), before);
    }
}

#[tokio::test]
async fn authority_renewal_admits_an_existing_mesh_host_without_a_mesh_change() {
    let (_dir, server, authority, factory, mesh) = fixture().await;
    let peer_key = format!("nodekey:{}", "c".repeat(64));
    let phone_key = format!("nodekey:{}", "b".repeat(64));
    *mesh.peers.lock().unwrap() = vec![
        MeshPeer {
            mesh_node_key: Some(peer_key.clone()),
            hostname: "late-host".into(),
            ip: Some("100.64.0.2".into()),
            online: true,
            last_seen: None,
        },
        MeshPeer {
            mesh_node_key: Some(phone_key.clone()),
            hostname: "phone".into(),
            ip: Some("100.64.0.3".into()),
            online: true,
            last_seen: None,
        },
    ];
    let host = PreparedHost::prepare(
        factory.clone(),
        WorkerClient::new(server.uri()).unwrap(),
        "account-fixture",
        "owner@example.test",
        "fixture-host",
    )
    .await
    .unwrap();
    host.enroll(
        mesh.as_ref(),
        host.mesh_context().await.unwrap(),
        "fixture-host",
    )
    .await
    .unwrap();
    let original_mesh = mesh.status().await.unwrap();
    let original_credentials =
        std::fs::read(factory.directory.join("host-credential.json")).unwrap();
    let peer_port = crate::control::DEFAULT_CONTROL_PORT + 1;
    assert_ne!(peer_port, host.tls.local_addr.port());
    let registry = registry();
    let handle = host
        .activate(
            mesh.clone(),
            registry.clone(),
            "100.64.0.1".into(),
            peer_port,
            resources(),
        )
        .await
        .unwrap();
    // The mesh already sees both peers, but neither has an authoritative host role yet.
    assert!(registry.peer_nodes().is_empty());
    assert!(registry.compute_peer_nodes().is_empty());
    *authority.peer_snapshot.lock().unwrap() = Some(json!({
        "protocol_version": 1,
        "lease_seconds": 30,
        "revision": 3,
        "peers": [
            {"mesh_node_key": peer_key, "role": "host", "owner_node_id": Uuid::new_v4()},
            {"mesh_node_key": phone_key, "role": "controller", "owner_node_id": null}
        ]
    }));

    // Exercise activate's actual renewal task; no direct registry reconciliation here.
    tokio::time::timeout(Duration::from_secs(8), async {
        while registry.peer_node("late-host").is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("authority renewal must admit the already observed host");
    assert!(handle.error().is_none(), "{:?}", handle.error());
    assert_eq!(mesh.status().await.unwrap(), original_mesh);
    assert_eq!(registry.peer_nodes().len(), 1);
    assert_eq!(registry.compute_peer_nodes().len(), 1);
    assert!(registry.require_compute_peer("late-host").is_ok());
    assert_eq!(
        registry.peer_base_url("late-host"),
        Some(format!("http://100.64.0.2:{peer_port}"))
    );
    assert!(registry.peer_node("phone").is_none());
    assert!(registry.require_compute_peer("phone").is_err());
    assert_eq!(mesh.reauth.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(factory.directory.join("host-credential.json")).unwrap(),
        original_credentials
    );
    handle.shutdown(false).await.unwrap();
    registry.shutdown();
}

#[tokio::test]
async fn actual_tls_opens_after_enrollment_and_lease_then_fails_closed_and_recovers() {
    let (_dir, server, authority, factory, mesh) = fixture().await;
    let worker = WorkerClient::new(server.uri()).unwrap();
    let host = PreparedHost::prepare(
        factory.clone(),
        worker,
        "account-fixture",
        "owner@example.test",
        "fixture-host",
    )
    .await
    .unwrap();
    let identity = host.identity.clone();
    let address = host.tls.local_addr;
    let client = reqwest::Client::builder()
        .no_proxy()
        .tls_certs_only([reqwest::Certificate::from_der(identity.certificate_der()).unwrap()])
        .resolve(&identity.server_name(), address)
        .build()
        .unwrap();
    let url = format!(
        "https://{}:{}/v1/controller/catalog",
        identity.server_name(),
        address.port()
    );
    assert_eq!(
        client
            .get(&url)
            .bearer_auth("device-fixture-token")
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    host.enroll(
        mesh.as_ref(),
        host.mesh_context().await.unwrap(),
        "fixture-host",
    )
    .await
    .unwrap();
    assert_eq!(mesh.reauth.load(Ordering::SeqCst), 1);
    let handle = host
        .activate(
            mesh.clone(),
            registry(),
            "100.64.0.1".into(),
            crate::control::DEFAULT_CONTROL_PORT,
            resources(),
        )
        .await
        .unwrap();
    let response = client
        .get(&url)
        .bearer_auth("device-fixture-token")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["entries"][0]["title"], "Owner's existing work");
    factory.revision.store(11, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(8), async {
        while authority.highwater.load(Ordering::SeqCst) != 11 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    authority.outage.store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(8), async {
        while handle.error().is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        client
            .get(&url)
            .bearer_auth("device-fixture-token")
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    authority.outage.store(false, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(8), async {
        while handle.error().is_some() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        client
            .get(&url)
            .bearer_auth("device-fixture-token")
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    authority.revoked.store(true, Ordering::SeqCst);
    assert_eq!(
        client
            .get(&url)
            .bearer_auth("device-fixture-token")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    handle.shutdown(true).await.unwrap();
    assert!(!factory.directory.join("host-credential.json").exists());
    assert!(factory.directory.join("account.json").exists());
}

#[tokio::test]
async fn lost_confirm_ack_resumes_exact_enrollment_without_spending_another_key() {
    let (_dir, server, authority, factory, mesh) = fixture().await;
    let worker = WorkerClient::new(server.uri()).unwrap();
    let host = PreparedHost::prepare(
        factory,
        worker,
        "account-fixture",
        "owner@example.test",
        "fixture-host",
    )
    .await
    .unwrap();
    let id = Uuid::new_v4();
    *authority.enrollment.lock().unwrap() = Some(id);
    *mesh.key.lock().unwrap() = Some(format!("nodekey:{}", "a".repeat(64)));
    host.enroll(
        mesh.as_ref(),
        host.mesh_context().await.unwrap(),
        "fixture-host",
    )
    .await
    .unwrap();
    assert!(authority.confirmed.load(Ordering::SeqCst));
    assert_eq!(mesh.reauth.load(Ordering::SeqCst), 0);
    host.enroll(
        mesh.as_ref(),
        host.mesh_context().await.unwrap(),
        "fixture-host",
    )
    .await
    .unwrap();
    assert_eq!(mesh.resume.load(Ordering::SeqCst), 1);
    assert_eq!(mesh.reauth.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn persisted_account_boundary_refuses_new_account_before_registration() {
    let (_dir, server, _authority, factory, _mesh) = fixture().await;
    let worker = WorkerClient::new(server.uri()).unwrap();
    let host = PreparedHost::prepare(
        factory.clone(),
        worker.clone(),
        "account-fixture",
        "owner@example.test",
        "fixture-host",
    )
    .await
    .unwrap();
    drop(host);
    let before = server.received_requests().await.unwrap().len();
    let result = PreparedHost::prepare(
        factory,
        worker,
        "other-account",
        "other@example.test",
        "fixture-host",
    )
    .await;
    assert!(matches!(result, Err(LinkError::OwnerIdentity(_))));
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn old_worker_is_explicit_failure_and_never_a_legacy_enrollment() {
    let (dir, server, _authority, _factory, _mesh) = fixture().await;
    server.reset().await;
    Mock::given(path("/v1/controller/hosts/register"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error":"not found"})))
        .mount(&server)
        .await;
    let factory = Arc::new(Factory {
        directory: dir.path().join("old"),
        generation: Uuid::new_v4(),
        revision: AtomicU64::new(1),
    });
    assert!(PreparedHost::prepare(
        factory,
        WorkerClient::new(server.uri()).unwrap(),
        "account-fixture",
        "owner@example.test",
        "fixture-host"
    )
    .await
    .is_err());
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| r.url.path() != "/v1/mesh/join-key"));
}

mod ambiguous_host_enrollment {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Outcome {
        Bound,
        OldBinding,
        Offline,
        MissingKey,
        StatusFailure,
        Stopped,
        ResumeStopped,
        ResumeFailure,
        ResumeChangedKey,
        BeforeCliFailure,
    }

    struct AmbiguousMesh {
        inner: Arc<FakeMesh>,
        outcome: Outcome,
    }

    #[async_trait]
    impl Mesh for AmbiguousMesh {
        async fn join(&self, _: &str, _: &str) -> Result<(), MeshError> {
            panic!("host enrollment must never enter legacy mesh join")
        }
        async fn reauthenticate(&self, key: &str, name: &str) -> Result<(), MeshError> {
            if self.outcome == Outcome::BeforeCliFailure {
                return Err(MeshError::EmptyAuthKey);
            }
            if self.outcome == Outcome::OldBinding {
                self.inner.reauth.fetch_add(1, Ordering::SeqCst);
            } else {
                self.inner.reauthenticate(key, name).await?;
            }
            Err(MeshError::JoinFailed {
                stderr: "synthetic CLI failure after issued key attempt".into(),
            })
        }
        async fn resume(&self) -> Result<(), MeshError> {
            self.inner.resume().await?;
            match self.outcome {
                Outcome::ResumeFailure => Err(MeshError::JoinFailed {
                    stderr: "resume failed".into(),
                }),
                Outcome::ResumeChangedKey => {
                    *self.inner.key.lock().unwrap() = Some(format!("nodekey:{}", "b".repeat(64)));
                    Ok(())
                }
                _ => Ok(()),
            }
        }
        async fn status(&self) -> Result<MeshStatus, MeshError> {
            if self.outcome == Outcome::StatusFailure {
                return Err(MeshError::StatusFailed {
                    stderr: "status unavailable".into(),
                });
            }
            let mut status = self.inner.status().await?;
            if self.outcome == Outcome::Stopped
                || (self.outcome == Outcome::ResumeStopped
                    && self.inner.resume.load(Ordering::SeqCst) > 0)
            {
                status.backend_state = BackendState::Stopped;
            }
            if self.outcome == Outcome::MissingKey {
                status.self_node_key = None;
            }
            if self.outcome == Outcome::Offline {
                status.online = false;
            }
            Ok(status)
        }
        async fn peer_proxy(&self) -> Result<MeshProxy, MeshError> {
            self.inner.peer_proxy().await
        }
        async fn logout(&self) -> Result<(), MeshError> {
            panic!("reconciliation must preserve the mesh identity")
        }
        async fn shutdown(&self) {}
    }

    fn requests(requests: &[wiremock::Request], suffix: &str) -> usize {
        requests
            .iter()
            .filter(|r| r.url.path().ends_with(suffix))
            .count()
    }

    #[tokio::test]
    async fn exact_provider_confirmation_recovers_cli_failure_before_lease_and_without_remint() {
        let (_dir, server, authority, factory, inner) = fixture().await;
        let mesh = Arc::new(AmbiguousMesh {
            inner: inner.clone(),
            outcome: Outcome::Offline,
        });
        let host = PreparedHost::prepare(
            factory.clone(),
            WorkerClient::new(server.uri()).unwrap(),
            "account-fixture",
            "owner@example.test",
            "fixture-host",
        )
        .await
        .unwrap();
        host.enroll(
            mesh.as_ref(),
            host.mesh_context().await.unwrap(),
            "fixture-host",
        )
        .await
        .unwrap();
        assert!(authority.confirmed.load(Ordering::SeqCst));
        assert!(authority.runtime.lock().unwrap().is_none());
        assert!(
            host.gate.read().unwrap().is_none(),
            "confirmation cannot open the receiver"
        );
        assert!(!factory.directory.join("pending-enrollment.json").exists());
        assert_eq!(inner.reauth.load(Ordering::SeqCst), 1);
        assert_eq!(inner.resume.load(Ordering::SeqCst), 1);
        assert!(!mesh.status().await.unwrap().online);
        let calls = server.received_requests().await.unwrap();
        assert_eq!(requests(&calls, "/mesh/join-key"), 1);
        assert_eq!(requests(&calls, "/mesh/confirm"), 1);
        assert_eq!(requests(&calls, "/lease"), 0);
        let handle = host
            .activate(
                mesh,
                registry(),
                "100.64.0.1".into(),
                crate::control::DEFAULT_CONTROL_PORT,
                resources(),
            )
            .await
            .unwrap();
        assert!(authority.runtime.lock().unwrap().is_some());
        assert!(handle.gate.read().unwrap().is_some());
        handle.shutdown(false).await.unwrap();
    }

    #[tokio::test]
    async fn ambiguous_cli_failure_never_trusts_old_running_state_or_failed_resume() {
        for outcome in [
            Outcome::OldBinding,
            Outcome::MissingKey,
            Outcome::StatusFailure,
            Outcome::Stopped,
            Outcome::ResumeStopped,
            Outcome::ResumeFailure,
            Outcome::ResumeChangedKey,
            Outcome::BeforeCliFailure,
        ] {
            let (_dir, server, authority, factory, inner) = fixture().await;
            let mesh = AmbiguousMesh {
                inner: inner.clone(),
                outcome,
            };
            let host = PreparedHost::prepare(
                factory.clone(),
                WorkerClient::new(server.uri()).unwrap(),
                "account-fixture",
                "owner@example.test",
                "fixture-host",
            )
            .await
            .unwrap();
            let result = host
                .enroll(&mesh, host.mesh_context().await.unwrap(), "fixture-host")
                .await;
            let error = result.expect_err("negative enrollment case must fail");
            match outcome {
                Outcome::OldBinding => assert!(matches!(error,
                    LinkError::HostEnrollmentReconciliation { ref source, .. }
                    if source.has_controller_code("mesh_binding_unverified"))),
                Outcome::MissingKey | Outcome::Stopped => assert!(matches!(
                    error,
                    LinkError::Mesh(MeshError::JoinFailed { .. })
                )),
                Outcome::StatusFailure => assert!(matches!(
                    error,
                    LinkError::Mesh(MeshError::StatusFailed { .. })
                )),
                Outcome::ResumeFailure => assert!(error.to_string().contains("resume failed")),
                Outcome::ResumeChangedKey | Outcome::ResumeStopped => {
                    assert!(error.to_string().contains("identity changed or stopped"))
                }
                Outcome::BeforeCliFailure => {
                    assert!(matches!(error, LinkError::Mesh(MeshError::EmptyAuthKey)))
                }
                Outcome::Bound | Outcome::Offline => panic!("positive case in negative table"),
            }
            assert!(
                factory.directory.join("pending-enrollment.json").exists(),
                "{outcome:?}"
            );
            assert!(host.gate.read().unwrap().is_none());
            assert!(authority.runtime.lock().unwrap().is_none());
            let calls = server.received_requests().await.unwrap();
            assert_eq!(requests(&calls, "/mesh/join-key"), 1);
            assert_eq!(requests(&calls, "/lease"), 0);
            if matches!(
                outcome,
                Outcome::MissingKey
                    | Outcome::Stopped
                    | Outcome::StatusFailure
                    | Outcome::BeforeCliFailure
            ) {
                assert_eq!(requests(&calls, "/mesh/confirm"), 0, "{outcome:?}");
            }
            if matches!(outcome, Outcome::OldBinding | Outcome::MissingKey) {
                assert_eq!(inner.resume.load(Ordering::SeqCst), 0, "{outcome:?}");
            }
        }
    }

    #[tokio::test]
    async fn ambiguous_cli_failure_requires_current_exact_authority_confirmation() {
        for (status, response) in [
            (
                503,
                json!({"error":{"code":"registry_unavailable","message":"unavailable","retryable":true}}),
            ),
            (
                409,
                json!({"error":{"code":"enrollment_expired","message":"expired","retryable":false}}),
            ),
            (
                401,
                json!({"error":{"code":"invalid_credential","message":"revoked","retryable":false}}),
            ),
            (
                403,
                json!({"error":{"code":"enrollment_mismatch","message":"another account or role","retryable":false}}),
            ),
            (
                200,
                json!({"owner_node_id":Uuid::new_v4(),"mesh_node_key":format!("nodekey:{}","a".repeat(64)),"host_revision":1}),
            ),
            (
                200,
                json!({"owner_node_id":Uuid::nil(),"mesh_node_key":format!("nodekey:{}","b".repeat(64)),"host_revision":1}),
            ),
        ] {
            let (_dir, server, authority, factory, inner) = fixture().await;
            let host = PreparedHost::prepare(
                factory.clone(),
                WorkerClient::new(server.uri()).unwrap(),
                "account-fixture",
                "owner@example.test",
                "fixture-host",
            )
            .await
            .unwrap();
            // For the key-mismatch case the owner must be valid, so only the key can reject it.
            let mut response = response;
            if response["owner_node_id"] == Uuid::nil().to_string() {
                response["owner_node_id"] = json!(host.owner.owner_node_id);
            }
            *authority.confirmation_response.lock().unwrap() = Some((status, response));
            let mesh = AmbiguousMesh {
                inner: inner.clone(),
                outcome: Outcome::Bound,
            };
            let error = host
                .enroll(&mesh, host.mesh_context().await.unwrap(), "fixture-host")
                .await
                .expect_err("authority refusal must fail enrollment");
            if status != 200 {
                let expected = authority
                    .confirmation_response
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .1["error"]["code"]
                    .as_str()
                    .unwrap()
                    .to_string();
                assert!(
                    matches!(error, LinkError::HostEnrollmentReconciliation { ref join, ref source }
                    if matches!(join.as_ref(), MeshError::JoinFailed { .. }) && source.has_controller_code(&expected))
                );
                assert!(error
                    .to_string()
                    .contains("synthetic CLI failure after issued key attempt"));
                let cause =
                    std::error::Error::source(&error).expect("typed worker cause is retained");
                if let LinkError::HostEnrollmentReconciliation { ref source, .. } = error {
                    assert_eq!(cause.to_string(), source.to_string());
                }
            } else {
                assert!(error
                    .to_string()
                    .contains("provider confirmation belongs to another host"));
            }
            assert_eq!(inner.resume.load(Ordering::SeqCst), 0);
            assert!(factory.directory.join("pending-enrollment.json").exists());
            assert!(host.gate.read().unwrap().is_none());
            assert!(authority.runtime.lock().unwrap().is_none());
            let calls = server.received_requests().await.unwrap();
            assert_eq!(requests(&calls, "/mesh/join-key"), 1);
            assert_eq!(requests(&calls, "/mesh/confirm"), 1);
            assert_eq!(requests(&calls, "/lease"), 0);
        }
    }
}
