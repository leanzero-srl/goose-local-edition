//! `LinkManager` state machine, driven with a mock worker (wiremock) + a FAKE mesh
//! (injected via the `MeshFactory` seam, so no `tailscaled` ever starts) + a
//! `FakeStateSource`. The real `ControlService` DOES start (on ephemeral loopback) —
//! that is in-process and hermetic. No tailnet contact.
//!
//! Covered transitions:
//! - LoggedOut → CodeSent → LoggedIn → Connecting → Connected, then logout → LoggedOut.
//! - join-key 401 → LoggedOut + identity cleared, mesh never started.
//! - verify failure → stays LoggedOut with `last_error`.
//! - connect failure (mesh join) → back to LoggedIn, mesh torn down per-pid.
//! - logout while merely LoggedIn → LoggedOut, identity cleared.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::BoxStream;
use leanzero_link::control::{ControlConfig, ControlService};
use leanzero_link::identity::{Identity, IdentityStore};
use leanzero_link::manager::{
    AuthState, LinkError, LinkManager, LinkManagerConfig, Mesh, MeshFactory,
};
use leanzero_link::mesh::{BackendState, MeshConfig, MeshError, MeshStatus};
use leanzero_link::state::{
    ExecuteAccepted, ExecuteError, ExecuteRequest, MlxControl, MlxControlError, MlxOp, PeerTarget,
    RemoteExecutor, SwarmStateSource,
};
use leanzero_link::token::node_token_from_secret;
use leanzero_link::wire::{LinkEvent, NodeState, NodeStatus, SessionSummary};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The per-account node secret the join-key mocks issue; every node of the account
/// derives the same `/v1/swarm` bearer from it (`node_token_from_secret`).
const SECRET: &str = "test-secret";

// ── fakes ────────────────────────────────────────────────────────────────

#[derive(Default)]
struct MeshCalls {
    joined: Vec<(String, String)>,
    logout_count: u32,
    shutdown_count: u32,
}

/// Runtime knobs the tests flip on the fake mesh mid-flight (all shared with the
/// harness). Default: a healthy daemon.
#[derive(Default)]
struct MeshScript {
    /// `status()` answers `DaemonExited` — the supervised tailscaled is dead.
    daemon_dead: AtomicBool,
    /// `join()` parks (yielding) until this is cleared — lets a test act mid-connect.
    hold_join: AtomicBool,
    /// Set by `join()` on entry so a test knows the connect is inside the mesh step.
    join_entered: AtomicBool,
}

struct FakeMesh {
    calls: Arc<StdMutex<MeshCalls>>,
    script: Arc<MeshScript>,
    join_fails: bool,
    status: MeshStatus,
}

#[async_trait]
impl Mesh for FakeMesh {
    async fn join(&self, auth_key: &str, hostname: &str) -> Result<(), MeshError> {
        self.calls
            .lock()
            .unwrap()
            .joined
            .push((auth_key.to_string(), hostname.to_string()));
        self.script.join_entered.store(true, Ordering::SeqCst);
        while self.script.hold_join.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if self.join_fails {
            Err(MeshError::JoinFailed {
                stderr: "fake join failure".to_string(),
            })
        } else {
            Ok(())
        }
    }
    async fn status(&self) -> Result<MeshStatus, MeshError> {
        if self.script.daemon_dead.load(Ordering::SeqCst) {
            return Err(MeshError::DaemonExited {
                status: "exit status: 1".to_string(),
                stderr_tail: "fake tailscaled crashed".to_string(),
            });
        }
        Ok(self.status.clone())
    }
    async fn logout(&self) -> Result<(), MeshError> {
        self.calls.lock().unwrap().logout_count += 1;
        Ok(())
    }
    async fn shutdown(&self) {
        self.calls.lock().unwrap().shutdown_count += 1;
    }
}

struct FakeFactory {
    calls: Arc<StdMutex<MeshCalls>>,
    script: Arc<MeshScript>,
    start_count: Arc<AtomicU32>,
    join_fails: bool,
    status: MeshStatus,
}

#[async_trait]
impl MeshFactory for FakeFactory {
    async fn start(&self, _config: MeshConfig) -> Result<Arc<dyn Mesh>, MeshError> {
        self.start_count.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(FakeMesh {
            calls: self.calls.clone(),
            script: self.script.clone(),
            join_fails: self.join_fails,
            status: self.status.clone(),
        }))
    }
}

async fn wait_until<F, Fut>(what: &str, mut probe: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if probe().await {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

struct FakeStateSource;

#[async_trait]
impl SwarmStateSource for FakeStateSource {
    async fn local_node(&self) -> NodeState {
        NodeState {
            node_id: "self-node".to_string(),
            hostname: "self-host".to_string(),
            mesh_ip: None,
            status: NodeStatus::Idle,
            sessions_active: 0,
            updated_at: Utc::now(),
            last_poll_error: None,
        }
    }
    async fn local_sessions(&self) -> Result<Vec<SessionSummary>, String> {
        Ok(Vec::new())
    }
    fn subscribe_local_deltas(&self) -> BoxStream<'static, LinkEvent> {
        Box::pin(futures::stream::pending())
    }
}

fn connected_status() -> MeshStatus {
    MeshStatus {
        self_ip: Some("100.64.0.7".to_string()),
        self_hostname: Some("self-host".to_string()),
        backend_state: BackendState::Running,
        online: true,
        peers: Vec::new(),
    }
}

/// An always-Idle source with a chosen node id — node B's state in the peer-execute test.
struct NamedIdleSource {
    node_id: String,
}

#[async_trait]
impl SwarmStateSource for NamedIdleSource {
    async fn local_node(&self) -> NodeState {
        NodeState {
            node_id: self.node_id.clone(),
            hostname: format!("{}-host", self.node_id),
            mesh_ip: None,
            status: NodeStatus::Idle,
            sessions_active: 0,
            updated_at: Utc::now(),
            last_poll_error: None,
        }
    }
    async fn local_sessions(&self) -> Result<Vec<SessionSummary>, String> {
        Ok(Vec::new())
    }
    fn subscribe_local_deltas(&self) -> BoxStream<'static, LinkEvent> {
        Box::pin(futures::stream::pending())
    }
}

/// A scripted [`RemoteExecutor`] that records what it is asked to run and returns a fixed
/// session id — the same stand-in the control tests use, no agent involved.
struct RecordingExecutor {
    recorded: StdMutex<Vec<ExecuteRequest>>,
    session_id: String,
}

impl RecordingExecutor {
    fn new(session_id: &str) -> Arc<Self> {
        Arc::new(Self {
            recorded: StdMutex::new(Vec::new()),
            session_id: session_id.to_string(),
        })
    }
}

#[async_trait]
impl RemoteExecutor for RecordingExecutor {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteAccepted, ExecuteError> {
        self.recorded.lock().unwrap().push(req);
        Ok(ExecuteAccepted {
            session_id: self.session_id.clone(),
        })
    }
}

/// A scripted [`MlxControl`] that records the (op, request) it is handed and returns a
/// fixed outcome — the same stand-in the control tests use, no engine involved.
struct RecordingMlxControl {
    recorded: StdMutex<Vec<(MlxOp, serde_json::Value)>>,
    outcome: Result<serde_json::Value, MlxControlError>,
    /// How long the op "works" before answering — a slow peer (model delete, HF fetch).
    delay: Duration,
}

impl RecordingMlxControl {
    fn returning(value: serde_json::Value) -> Arc<Self> {
        Arc::new(Self {
            recorded: StdMutex::new(Vec::new()),
            outcome: Ok(value),
            delay: Duration::ZERO,
        })
    }

    fn returning_after(value: serde_json::Value, delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            recorded: StdMutex::new(Vec::new()),
            outcome: Ok(value),
            delay,
        })
    }

    fn failing(error: MlxControlError) -> Arc<Self> {
        Arc::new(Self {
            recorded: StdMutex::new(Vec::new()),
            outcome: Err(error),
            delay: Duration::ZERO,
        })
    }
}

#[async_trait]
impl MlxControl for RecordingMlxControl {
    async fn dispatch(
        &self,
        op: MlxOp,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, MlxControlError> {
        self.recorded.lock().unwrap().push((op, request));
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.outcome.clone()
    }
}

// ── harness ────────────────────────────────────────────────────────────────

struct Harness {
    _tmp: tempfile::TempDir,
    identity_path: std::path::PathBuf,
    state_dir: std::path::PathBuf,
    calls: Arc<StdMutex<MeshCalls>>,
    script: Arc<MeshScript>,
    start_count: Arc<AtomicU32>,
}

impl Harness {
    fn new(tmp: tempfile::TempDir) -> Self {
        let identity_path = tmp.path().join("identity.json");
        let state_dir = tmp.path().join("tailscale");
        Self {
            _tmp: tmp,
            identity_path,
            state_dir,
            calls: Arc::new(StdMutex::new(MeshCalls::default())),
            script: Arc::new(MeshScript::default()),
            start_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn manager(&self, server: &MockServer, join_fails: bool) -> LinkManager {
        let mut mesh = MeshConfig::new(
            "/nonexistent/tailscaled".into(),
            "/nonexistent/tailscale".into(),
            "placeholder".to_string(),
        )
        .expect("mesh config");
        // Keep the fake wholly off the real ~/.leanzero: a `wipe` logout would remove
        // this dir, so point it inside the temp dir.
        mesh.state_dir = self.state_dir.clone();
        mesh.socket_path = self.state_dir.join("tailscaled.sock");

        // The template token is EMPTY on purpose: connect() must replace it with the
        // secret-derived token before the service starts (an empty token reaching
        // `ControlService::start` is refused as `EmptyToken`).
        let mut control = ControlConfig::new(String::new(), None);
        control.port = 0; // ephemeral loopback; peers use the shared fixed port in prod
        control.poll_interval = Duration::from_millis(50);
        control.heartbeat_interval = Duration::from_millis(200);
        control.reconnect_backoff = Duration::from_millis(50);
        // The fabric's TOTAL poll timeout, deliberately short: a proxy POST that
        // (wrongly) shared it would fail against the slow peer below.
        control.request_timeout = Duration::from_millis(200);

        let config = LinkManagerConfig {
            worker_base_url: server.uri(),
            identity_path: self.identity_path.clone(),
            mesh,
            control,
        };
        let factory = Arc::new(FakeFactory {
            calls: self.calls.clone(),
            script: self.script.clone(),
            start_count: self.start_count.clone(),
            join_fails,
            status: connected_status(),
        });
        LinkManager::with_mesh_factory(config, Arc::new(FakeStateSource), factory)
            .expect("manager constructs")
    }

    fn seed_identity(&self, email: &str, token: &str) {
        IdentityStore::new(self.identity_path.clone())
            .save(&Identity::new(email, token))
            .expect("seed identity");
    }

    fn identity_present(&self) -> bool {
        IdentityStore::new(self.identity_path.clone())
            .load()
            .expect("load identity")
            .is_some()
    }
}

async fn mount(server: &MockServer, m: &str, p: &str, status: u16, body: serde_json::Value) {
    Mock::given(method(m))
        .and(path(p))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

// ── tests ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn full_flow_loggedout_to_connected_then_logout() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/auth/request-code",
        200,
        json!({"ok": true, "email": "a@example.com", "expiresInSeconds": 600}),
    )
    .await;
    mount(
        &server,
        "POST",
        "/v1/auth/verify",
        200,
        json!({"token": "identity-jwt", "email": "a@example.com", "audienceSync": "synced"}),
    )
    .await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-xyz", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;

    let h = Harness::new(tempfile::tempdir().unwrap());
    let manager = h.manager(&server, false);

    // Fresh: no identity → LoggedOut.
    assert!(matches!(manager.status().await.auth, AuthState::LoggedOut));

    // request_code → CodeSent{normalized email}.
    manager.request_code("A@Example.com").await.unwrap();
    match manager.status().await.auth {
        AuthState::CodeSent { email, .. } => assert_eq!(email, "a@example.com"),
        other => panic!("expected CodeSent, got {other:?}"),
    }

    // verify → LoggedIn + identity persisted; audience_sync surfaced honestly.
    let verify = manager.verify("a@example.com", "123456").await.unwrap();
    assert_eq!(verify.audience_sync, "synced");
    assert!(matches!(
        manager.status().await.auth,
        AuthState::LoggedIn { .. }
    ));
    assert!(h.identity_present(), "verify persisted the identity");

    // connect → Connected{mesh_ip}. The minted key + computed hostname reached join().
    manager.connect().await.unwrap();
    match manager.status().await.auth {
        AuthState::Connected { email, mesh_ip } => {
            assert_eq!(email, "a@example.com");
            assert_eq!(mesh_ip, "100.64.0.7");
        }
        other => panic!("expected Connected, got {other:?}"),
    }
    assert_eq!(h.start_count.load(Ordering::SeqCst), 1, "mesh started once");
    {
        let calls = h.calls.lock().unwrap();
        assert_eq!(calls.joined.len(), 1);
        assert_eq!(
            calls.joined[0].0, "tskey-auth-xyz",
            "the minted key was used"
        );
        // Hostname scheme: <machine>-<6-hex suffix> persisted at <dir>/node-id.
        let suffix = std::fs::read_to_string(h._tmp.path().join("node-id"))
            .expect("node-id persisted")
            .trim()
            .to_string();
        assert_eq!(suffix.len(), 6, "suffix is 6 hex chars");
        assert!(
            calls.joined[0].1.ends_with(&format!("-{suffix}")),
            "mesh hostname carries the stable suffix: {}",
            calls.joined[0].1
        );
    }

    // Composed status: mesh present, node_count = self.
    let state = manager.status().await;
    assert!(state.mesh.is_some());
    assert_eq!(state.node_count, 1);

    // logout → LoggedOut, identity cleared, mesh logged out.
    manager.logout(false).await.unwrap();
    assert!(matches!(manager.status().await.auth, AuthState::LoggedOut));
    assert!(!h.identity_present(), "logout cleared the identity");
    assert_eq!(
        h.calls.lock().unwrap().logout_count,
        1,
        "mesh.logout() called"
    );
}

#[tokio::test]
async fn join_key_401_expired_logs_out_and_clears_identity() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        401,
        json!({"error": "invalid token", "reason": "expired"}),
    )
    .await;

    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "dead-180d-token");
    let manager = h.manager(&server, false);
    assert!(matches!(
        manager.status().await.auth,
        AuthState::LoggedIn { .. }
    ));

    let err = manager
        .connect()
        .await
        .expect_err("expired token cannot connect");
    assert!(err.to_string().contains("expired"), "loud cause: {err}");

    let state = manager.status().await;
    assert!(
        matches!(state.auth, AuthState::LoggedOut),
        "dropped to LoggedOut"
    );
    assert!(state.last_error.is_some(), "error surfaced, not swallowed");
    assert!(!h.identity_present(), "dead identity cleared");
    assert_eq!(
        h.start_count.load(Ordering::SeqCst),
        0,
        "mesh never started"
    );
}

/// R-M1: `logout()` lands while `connect()` is inside the mesh join. The connect must
/// NOT install `Connected` over a cleared identity; the fresh connection is logged out
/// of the tailnet and torn down, and logout's `LoggedOut` stands.
#[tokio::test]
async fn logout_during_connecting_aborts_the_connect_and_stays_logged_out() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let manager = h.manager(&server, false);
    h.script.hold_join.store(true, Ordering::SeqCst);

    let (connect_result, ()) = tokio::join!(manager.connect(), async {
        wait_until("connect to reach the mesh join", || async {
            h.script.join_entered.load(Ordering::SeqCst)
        })
        .await;
        assert!(
            matches!(manager.status().await.auth, AuthState::Connecting { .. }),
            "mid-connect the state is Connecting"
        );
        manager.logout(false).await.expect("logout mid-connect");
        assert!(matches!(manager.status().await.auth, AuthState::LoggedOut));
        assert!(!h.identity_present(), "logout cleared the credential");
        h.script.hold_join.store(false, Ordering::SeqCst);
    });

    let err = connect_result.expect_err("the raced connect must not report success");
    assert!(matches!(err, LinkError::ConnectAborted), "got {err:?}");

    let state = manager.status().await;
    assert!(
        matches!(state.auth, AuthState::LoggedOut),
        "logout's state stands, got {:?}",
        state.auth
    );
    assert!(state.mesh.is_none());
    assert_eq!(state.node_count, 0);
    assert!(
        manager.active_registry().await.is_none(),
        "nothing installed"
    );
    assert!(manager.node_token().await.is_none());
    assert!(!h.identity_present(), "the credential stays cleared");
    let calls = h.calls.lock().unwrap();
    assert_eq!(
        calls.logout_count, 1,
        "the fresh mesh was logged out of the tailnet (node key expired), not just killed"
    );
}

/// R-M1, failing-connect variant: a logout that races a connect which then FAILS must
/// not flip auth back to `LoggedIn` over a credential logout already deleted.
#[tokio::test]
async fn logout_during_a_failing_connect_keeps_logged_out() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let manager = h.manager(&server, /* join_fails */ true);
    h.script.hold_join.store(true, Ordering::SeqCst);

    let (connect_result, ()) = tokio::join!(manager.connect(), async {
        wait_until("connect to reach the mesh join", || async {
            h.script.join_entered.load(Ordering::SeqCst)
        })
        .await;
        manager.logout(false).await.expect("logout mid-connect");
        h.script.hold_join.store(false, Ordering::SeqCst);
    });

    let err = connect_result.expect_err("the join fails");
    assert!(err.to_string().contains("fake join failure"), "{err}");
    let state = manager.status().await;
    assert!(
        matches!(state.auth, AuthState::LoggedOut),
        "must not resurrect LoggedIn over a cleared identity, got {:?}",
        state.auth
    );
    assert!(!h.identity_present());
    assert_eq!(
        h.calls.lock().unwrap().shutdown_count,
        1,
        "the failed mesh was still torn down per-pid"
    );
}

/// R-M7: a 401 from a proxy (HTML body, no worker reason) is an ordinary connect
/// failure — the credential stays on disk and auth stays `LoggedIn`.
#[tokio::test]
async fn join_key_401_without_a_worker_reason_keeps_the_identity() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mesh/join-key"))
        .respond_with(
            ResponseTemplate::new(401)
                .insert_header("content-type", "text/html")
                .set_body_string("<html><body>401 Authorization Required</body></html>"),
        )
        .mount(&server)
        .await;

    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "still-good-token");
    let manager = h.manager(&server, false);

    let err = manager
        .connect()
        .await
        .expect_err("the proxy 401 fails the connect");
    assert!(
        matches!(
            err,
            LinkError::Worker(leanzero_link::worker_client::WorkerError::Unexpected {
                status: 401,
                ..
            })
        ),
        "got {err:?}"
    );

    let state = manager.status().await;
    assert!(
        matches!(state.auth, AuthState::LoggedIn { .. }),
        "no worker verdict → still LoggedIn, got {:?}",
        state.auth
    );
    assert!(state
        .last_error
        .as_deref()
        .is_some_and(|e| e.contains("401") && e.contains("<html>")));
    assert!(h.identity_present(), "a proxy can never sign the user out");
    assert_eq!(
        h.start_count.load(Ordering::SeqCst),
        0,
        "mesh never started"
    );
}

#[tokio::test]
async fn connect_without_a_node_secret_is_loud_and_starts_no_mesh() {
    let server = MockServer::start().await;
    // A worker that predates `nodeSecret`: the key is fine, the secret is absent.
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "expirySeconds": 600}),
    )
    .await;

    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let manager = h.manager(&server, false);

    let err = manager
        .connect()
        .await
        .expect_err("no node secret → no connection");
    assert!(matches!(err, LinkError::NoNodeSecret), "got {err:?}");
    assert!(
        err.to_string().contains("did not issue a node secret"),
        "loud cause: {err}"
    );

    let state = manager.status().await;
    assert!(
        matches!(state.auth, AuthState::LoggedIn { .. }),
        "auth is fine; only the connect is refused"
    );
    assert!(state.last_error.is_some());
    assert!(h.identity_present(), "identity untouched");
    assert_eq!(
        h.start_count.load(Ordering::SeqCst),
        0,
        "refused BEFORE any daemon is spawned"
    );
    assert!(
        manager.node_token().await.is_none(),
        "no token is ever derived from anything but the worker's secret"
    );
}

#[tokio::test]
async fn node_token_is_none_until_connected_then_the_secret_derived_value() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": "  test-secret  ", "expirySeconds": 600}),
    )
    .await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let manager = h.manager(&server, false);
    assert!(manager.node_token().await.is_none(), "not connected yet");

    manager.connect().await.expect("connects");
    let token = manager.node_token().await.expect("connected → token");
    assert_eq!(
        token,
        node_token_from_secret("test-secret"),
        "the bearer is the secret-derived value (whitespace trimmed), not the template"
    );
    assert_ne!(token, "", "the empty template token never survives connect");

    // The control service this node serves accepts exactly that token — the loopback
    // proxy on the host must present it.
    let registry = manager.active_registry().await.expect("live registry");
    drop(registry);
    manager.logout(false).await.unwrap();
    assert!(
        manager.node_token().await.is_none(),
        "gone with the connection"
    );
}

/// FH#1: the supervised tailscaled dies under a live connection. The poll loop must
/// drop the connection per-pid, keep the credential, and return auth to `LoggedIn` so
/// `connect()` re-arms — never a calm `Connected`/`Stopped`, never a wiped identity.
#[tokio::test]
async fn dead_daemon_drops_to_logged_in_keeps_identity_and_re_arms() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let manager = h.manager(&server, false);
    manager.connect().await.expect("connects");
    assert!(matches!(
        manager.status().await.auth,
        AuthState::Connected { .. }
    ));

    h.script.daemon_dead.store(true, Ordering::SeqCst);

    // Observed through `active_registry()` only — it never polls the mesh, so the
    // demotion seen here is the POLL LOOP's, not a status read's.
    wait_until("the poll loop to drop the dead connection", || {
        let manager = &manager;
        async move { manager.active_registry().await.is_none() }
    })
    .await;

    let state = manager.status().await;
    match &state.auth {
        AuthState::LoggedIn { email } => assert_eq!(email, "a@example.com"),
        other => panic!("expected LoggedIn after the daemon died, got {other:?}"),
    }
    let err = state.last_error.expect("the death is recorded, not erased");
    assert!(
        err.contains("tailscaled exited") && err.contains("fake tailscaled crashed"),
        "last_error carries the daemon's exit + stderr tail: {err}"
    );
    assert!(state.mesh.is_none());
    assert_eq!(state.node_count, 0);
    assert!(
        h.identity_present(),
        "a daemon crash never clears the credential"
    );
    assert!(manager.node_token().await.is_none());
    {
        let calls = h.calls.lock().unwrap();
        assert_eq!(calls.shutdown_count, 1, "torn down per-pid");
        assert_eq!(
            calls.logout_count, 0,
            "no `tailscale logout` against a dead daemon, and no account logout"
        );
    }

    // Re-arm: the same identity connects again once the daemon can come back.
    h.script.daemon_dead.store(false, Ordering::SeqCst);
    manager
        .connect()
        .await
        .expect("re-connects with the kept identity");
    assert!(matches!(
        manager.status().await.auth,
        AuthState::Connected { .. }
    ));
    assert_eq!(
        h.start_count.load(Ordering::SeqCst),
        2,
        "a fresh daemon was started"
    );
    manager.logout(false).await.unwrap();
}

/// The same death seen first by a `status()` read (the UI's poll) instead of the loop:
/// that read demotes in place and reports the demoted state, never `Connected` over a
/// dead daemon.
#[tokio::test]
async fn status_read_that_finds_the_daemon_dead_demotes_in_place() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let manager = h.manager(&server, false);
    manager.connect().await.expect("connects");

    h.script.daemon_dead.store(true, Ordering::SeqCst);
    let state = manager.status().await;
    assert!(
        matches!(state.auth, AuthState::LoggedIn { .. }),
        "the very read that saw the death reports LoggedIn, got {:?}",
        state.auth
    );
    assert!(state.mesh.is_none());
    assert_eq!(state.node_count, 0);
    assert!(state
        .last_error
        .as_deref()
        .is_some_and(|e| e.contains("tailscaled exited")));
    assert!(h.identity_present());
    assert!(manager.active_registry().await.is_none());
}

#[tokio::test]
async fn verify_failure_stays_logged_out_with_error() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/auth/verify",
        401,
        json!({"error": "invalid or expired code"}),
    )
    .await;

    let h = Harness::new(tempfile::tempdir().unwrap());
    let manager = h.manager(&server, false);

    let err = manager
        .verify("a@example.com", "000000")
        .await
        .expect_err("wrong code fails");
    assert!(
        err.to_string().contains("invalid or expired code"),
        "loud: {err}"
    );

    let state = manager.status().await;
    assert!(
        matches!(state.auth, AuthState::LoggedOut),
        "stays LoggedOut"
    );
    assert!(state.last_error.is_some());
    assert!(!h.identity_present(), "no identity persisted on failure");
}

#[tokio::test]
async fn connect_failure_returns_to_logged_in_and_tears_down_mesh() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;

    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let manager = h.manager(&server, /* join_fails */ true);

    let err = manager.connect().await.expect_err("mesh join fails");
    assert!(
        err.to_string().contains("fake join failure"),
        "loud cause: {err}"
    );

    let state = manager.status().await;
    assert!(
        matches!(state.auth, AuthState::LoggedIn { .. }),
        "still authed after a mesh failure"
    );
    assert!(state.last_error.is_some());
    assert_eq!(state.node_count, 0, "no active connection");
    assert!(state.mesh.is_none());
    assert!(h.identity_present(), "auth intact — identity kept");

    let calls = h.calls.lock().unwrap();
    assert_eq!(h.start_count.load(Ordering::SeqCst), 1, "mesh was started");
    assert_eq!(
        calls.shutdown_count, 1,
        "mesh torn down per-pid after the failed join"
    );
}

#[tokio::test]
async fn logout_while_only_logged_in_clears_identity() {
    let server = MockServer::start().await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "token");
    let manager = h.manager(&server, false);
    assert!(matches!(
        manager.status().await.auth,
        AuthState::LoggedIn { .. }
    ));

    manager.logout(false).await.unwrap();
    assert!(matches!(manager.status().await.auth, AuthState::LoggedOut));
    assert!(!h.identity_present());
    // No connection existed, so the mesh was never touched.
    assert_eq!(h.calls.lock().unwrap().logout_count, 0);
}

#[tokio::test]
async fn remote_execute_self_short_circuits_to_the_local_executor() {
    let server = MockServer::start().await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    let executor = RecordingExecutor::new("local-sess-1");
    // FakeStateSource reports node_id "self-node"; targeting it takes the no-network path.
    let manager = h.manager(&server, false).with_executor(executor.clone());

    let accepted = manager
        .remote_execute(
            "self-node",
            ExecuteRequest {
                prompt: "do the thing locally".to_string(),
                working_dir: Some("/tmp/here".to_string()),
                session_id: None,
            },
        )
        .await
        .expect("self execute runs the local executor");
    assert_eq!(accepted.session_id, "local-sess-1");

    let recorded = executor.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 1, "no network hop; the local executor ran");
    assert_eq!(recorded[0].prompt, "do the thing locally");
    assert_eq!(recorded[0].working_dir.as_deref(), Some("/tmp/here"));
}

#[tokio::test]
async fn remote_execute_self_without_executor_is_loud() {
    let server = MockServer::start().await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    let manager = h.manager(&server, false); // no executor wired

    let err = manager
        .remote_execute(
            "self-node",
            ExecuteRequest {
                prompt: "x".to_string(),
                working_dir: None,
                session_id: None,
            },
        )
        .await
        .expect_err("no executor is a loud typed error, not a fake accept");
    assert!(matches!(err, LinkError::ExecutorUnavailable), "got {err:?}");
}

#[tokio::test]
async fn remote_execute_posts_to_a_peer_execute_route() {
    // Node B: a real control service with a recording executor, on ephemeral loopback.
    let b_executor = RecordingExecutor::new("sess-on-b");
    let mut b_config = ControlConfig::new(node_token_from_secret(SECRET), None);
    b_config.port = 0;
    b_config.allow_remote_execution = true; // B opts in to being acted on
    let b = ControlService::start(
        b_config,
        Arc::new(NamedIdleSource {
            node_id: "node-b".to_string(),
        }),
        Some(b_executor.clone()),
        None,
    )
    .await
    .expect("node B starts");
    let b_port = b.local_addr().port();

    // Node A: a connected manager (fake mesh reports zero peers of its own).
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let a_executor = RecordingExecutor::new("unused-on-a");
    let manager = h.manager(&server, false).with_executor(a_executor.clone());
    manager.connect().await.expect("A connects");

    let registry = manager
        .active_registry()
        .await
        .expect("A has a live peer registry while connected");
    let target = PeerTarget {
        hostname: "node-b".to_string(),
        mesh_ip: Some("127.0.0.1".to_string()),
        port: b_port,
    };

    // The peer-sync loop reconciles mesh-derived peers to [] exactly once at connect; re-seed
    // B until the POST lands (after that first reconcile it never wipes again, since the fake
    // mesh keeps reporting zero peers).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let accepted = loop {
        registry.set_peers(vec![target.clone()]);
        match manager
            .remote_execute(
                "node-b",
                ExecuteRequest {
                    prompt: "run this on B".to_string(),
                    working_dir: None,
                    session_id: None,
                },
            )
            .await
        {
            Ok(accepted) => break accepted,
            Err(LinkError::UnknownPeer(_)) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(other) => panic!("unexpected remote_execute error: {other:?}"),
        }
    };

    assert_eq!(
        accepted.session_id, "sess-on-b",
        "A got B's session id back"
    );
    {
        let recorded = b_executor.recorded.lock().unwrap();
        assert_eq!(
            recorded.len(),
            1,
            "the prompt reached B's executor over HTTP"
        );
        assert_eq!(recorded[0].prompt, "run this on B");
        assert!(
            a_executor.recorded.lock().unwrap().is_empty(),
            "A's local executor must not run for a peer target"
        );
    }

    b.shutdown();
    manager.logout(false).await.unwrap();
}

// ── mlx_proxy: self short-circuit + peer forwarding over /v1/swarm/mlx ────

#[tokio::test]
async fn mlx_proxy_self_short_circuits_to_the_local_control() {
    let server = MockServer::start().await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    let payload = json!({"settings": {"modelsDir": "~/models", "port": 8090, "spawnCommand": []}});
    let control = RecordingMlxControl::returning(payload.clone());
    // FakeStateSource reports node_id "self-node"; targeting it takes the no-network path.
    let manager = h.manager(&server, false).with_mlx_control(control.clone());

    let out = manager
        .mlx_proxy(
            "self-node",
            MlxOp::SettingsRead,
            json!({"nodeId": "self-node"}),
        )
        .await
        .expect("self mlx op runs the local control");
    assert_eq!(
        out, payload,
        "the local control's payload is returned verbatim"
    );

    let recorded = control.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 1, "no network hop; the local control ran");
    assert_eq!(recorded[0].0, MlxOp::SettingsRead);
}

#[tokio::test]
async fn mlx_proxy_self_without_control_is_loud() {
    let server = MockServer::start().await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    let manager = h.manager(&server, false); // no mlx control wired

    let err = manager
        .mlx_proxy("self-node", MlxOp::Status, json!({}))
        .await
        .expect_err("no mlx control is a loud typed error, not a fabricated result");
    assert!(
        matches!(err, LinkError::MlxControlUnavailable),
        "got {err:?}"
    );
}

/// Node A forwards a mlx op to node B over `POST /v1/swarm/mlx/<op>` and returns B's
/// payload; a peer's own failure surfaces verbatim as a typed error.
#[tokio::test]
async fn mlx_proxy_posts_to_a_peer_mlx_route_and_surfaces_its_payload_and_errors() {
    // Node B: a real control service with a recording mlx control, on ephemeral loopback.
    let b_payload = json!({
        "models": [{"id": "org/model", "sizeBytes": 42, "complete": true, "missingFiles": 0}],
        "diskAvailableBytes": 100,
        "diskTotalBytes": 200
    });
    let b_control = RecordingMlxControl::returning(b_payload.clone());
    let mut b_config = ControlConfig::new(node_token_from_secret(SECRET), None);
    b_config.port = 0;
    b_config.allow_remote_execution = true; // B opts in to being acted on
    let b = ControlService::start(
        b_config,
        Arc::new(NamedIdleSource {
            node_id: "node-b".to_string(),
        }),
        None,
        Some(b_control.clone()),
    )
    .await
    .expect("node B starts");
    let b_port = b.local_addr().port();

    // Node A: a connected manager (fake mesh reports zero peers of its own).
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let a_control = RecordingMlxControl::returning(json!({"unused": true}));
    let manager = h
        .manager(&server, false)
        .with_mlx_control(a_control.clone());
    manager.connect().await.expect("A connects");

    let registry = manager
        .active_registry()
        .await
        .expect("A has a live peer registry while connected");
    let target = PeerTarget {
        hostname: "node-b".to_string(),
        mesh_ip: Some("127.0.0.1".to_string()),
        port: b_port,
    };

    // Re-seed B until the POST lands (the peer-sync loop reconciles to [] once at connect).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let out = loop {
        registry.set_peers(vec![target.clone()]);
        match manager
            .mlx_proxy("node-b", MlxOp::ModelsList, json!({"nodeId": "node-b"}))
            .await
        {
            Ok(value) => break value,
            Err(LinkError::UnknownPeer(_)) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(other) => panic!("unexpected mlx_proxy error: {other:?}"),
        }
    };
    assert_eq!(
        out, b_payload,
        "A got B's models-list payload back over the mesh"
    );
    {
        let recorded = b_control.recorded.lock().unwrap();
        assert_eq!(recorded.len(), 1, "the op reached B's control over HTTP");
        assert_eq!(recorded[0].0, MlxOp::ModelsList);
        assert!(
            a_control.recorded.lock().unwrap().is_empty(),
            "A's local control must not run for a peer target"
        );
    }

    b.shutdown();
    manager.logout(false).await.unwrap();
}

#[tokio::test]
async fn mlx_proxy_surfaces_a_peer_failure_verbatim() {
    // Node B refuses a mount with a memory-gate BLOCK (the local invalid_params class).
    let b_control = RecordingMlxControl::failing(MlxControlError::BadRequest(
        "memory gate BLOCK: model needs 40GB, 12GB free".to_string(),
    ));
    let mut b_config = ControlConfig::new(node_token_from_secret(SECRET), None);
    b_config.port = 0;
    b_config.allow_remote_execution = true; // B opts in to being acted on
    let b = ControlService::start(
        b_config,
        Arc::new(NamedIdleSource {
            node_id: "node-b".to_string(),
        }),
        None,
        Some(b_control.clone()),
    )
    .await
    .expect("node B starts");
    let b_port = b.local_addr().port();

    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let manager = h
        .manager(&server, false)
        .with_mlx_control(RecordingMlxControl::returning(json!({})));
    manager.connect().await.expect("A connects");

    let registry = manager.active_registry().await.expect("live registry");
    let target = PeerTarget {
        hostname: "node-b".to_string(),
        mesh_ip: Some("127.0.0.1".to_string()),
        port: b_port,
    };

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let err = loop {
        registry.set_peers(vec![target.clone()]);
        match manager
            .mlx_proxy("node-b", MlxOp::Mount, json!({"modelId": "org/big"}))
            .await
        {
            Err(LinkError::UnknownPeer(_)) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(other) => break other,
            Ok(v) => panic!("expected a BLOCK error, got a payload: {v:?}"),
        }
    };
    match err {
        LinkError::MlxControl(MlxControlError::BadRequest(text)) => {
            assert_eq!(text, "memory gate BLOCK: model needs 40GB, 12GB free");
        }
        other => panic!("expected a verbatim BadRequest, got {other:?}"),
    }

    b.shutdown();
    manager.logout(false).await.unwrap();
}

/// R-M3: a peer whose op takes longer than the fabric's poll timeout still answers the
/// proxy — the proxy POST has a connect timeout only, never a total cap.
#[tokio::test]
async fn mlx_proxy_waits_past_the_fabric_request_timeout_for_a_slow_peer() {
    let b_payload = json!({"deleted": "org/huge-model", "freedBytes": 40_000_000_000u64});
    // 3x the harness's 200 ms `request_timeout`.
    let b_control =
        RecordingMlxControl::returning_after(b_payload.clone(), Duration::from_millis(600));
    let mut b_config = ControlConfig::new(node_token_from_secret(SECRET), None);
    b_config.port = 0;
    b_config.allow_remote_execution = true;
    let b = ControlService::start(
        b_config,
        Arc::new(NamedIdleSource {
            node_id: "node-b".to_string(),
        }),
        None,
        Some(b_control.clone()),
    )
    .await
    .expect("node B starts");
    let b_port = b.local_addr().port();

    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let manager = h
        .manager(&server, false)
        .with_mlx_control(RecordingMlxControl::returning(json!({})));
    manager.connect().await.expect("A connects");

    let registry = manager.active_registry().await.expect("live registry");
    let target = PeerTarget {
        hostname: "node-b".to_string(),
        mesh_ip: Some("127.0.0.1".to_string()),
        port: b_port,
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let started = tokio::time::Instant::now();
    let out = loop {
        registry.set_peers(vec![target.clone()]);
        match manager
            .mlx_proxy(
                "node-b",
                MlxOp::ModelDelete,
                json!({"modelId": "org/huge-model"}),
            )
            .await
        {
            Ok(value) => break value,
            Err(LinkError::UnknownPeer(_)) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(other) => panic!("a slow peer must not time out the proxy: {other:?}"),
        }
    };
    assert_eq!(out, b_payload, "the slow peer's payload arrived intact");
    assert!(
        started.elapsed() >= Duration::from_millis(600),
        "the proxy actually waited for the peer's work"
    );

    b.shutdown();
    manager.logout(false).await.unwrap();
}

#[tokio::test]
async fn mlx_proxy_unreachable_peer_is_a_typed_error() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/v1/mesh/join-key",
        200,
        json!({"authKey": "tskey-auth-ok", "nodeSecret": SECRET, "expirySeconds": 600}),
    )
    .await;
    let h = Harness::new(tempfile::tempdir().unwrap());
    h.seed_identity("a@example.com", "good-token");
    let manager = h
        .manager(&server, false)
        .with_mlx_control(RecordingMlxControl::returning(json!({})));
    manager.connect().await.expect("A connects");

    let registry = manager.active_registry().await.expect("live registry");
    // A port that answers nothing: bind, take the number, drop the listener.
    let dead_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let target = PeerTarget {
        hostname: "node-b".to_string(),
        mesh_ip: Some("127.0.0.1".to_string()),
        port: dead_port,
    };

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let err = loop {
        registry.set_peers(vec![target.clone()]);
        match manager.mlx_proxy("node-b", MlxOp::Status, json!({})).await {
            Err(LinkError::UnknownPeer(_)) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(other) => break other,
            Ok(v) => panic!("an unreachable peer must not yield a payload: {v:?}"),
        }
    };
    assert!(
        matches!(err, LinkError::MlxProxy(_)),
        "an unreachable peer is a loud typed error, got {err:?}"
    );

    manager.logout(false).await.unwrap();
}
