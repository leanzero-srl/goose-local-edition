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

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::BoxStream;
use leanzero_link::control::ControlConfig;
use leanzero_link::identity::{Identity, IdentityStore};
use leanzero_link::manager::{AuthState, LinkManager, LinkManagerConfig, Mesh, MeshFactory};
use leanzero_link::mesh::{BackendState, MeshConfig, MeshError, MeshStatus};
use leanzero_link::state::SwarmStateSource;
use leanzero_link::wire::{LinkEvent, NodeState, NodeStatus, SessionSummary};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ── fakes ────────────────────────────────────────────────────────────────

#[derive(Default)]
struct MeshCalls {
    joined: Vec<(String, String)>,
    logout_count: u32,
    shutdown_count: u32,
}

struct FakeMesh {
    calls: Arc<StdMutex<MeshCalls>>,
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
        if self.join_fails {
            Err(MeshError::JoinFailed {
                stderr: "fake join failure".to_string(),
            })
        } else {
            Ok(())
        }
    }
    async fn status(&self) -> Result<MeshStatus, MeshError> {
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
            join_fails: self.join_fails,
            status: self.status.clone(),
        }))
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
        }
    }
    async fn local_sessions(&self) -> Vec<SessionSummary> {
        Vec::new()
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

// ── harness ────────────────────────────────────────────────────────────────

struct Harness {
    _tmp: tempfile::TempDir,
    identity_path: std::path::PathBuf,
    state_dir: std::path::PathBuf,
    calls: Arc<StdMutex<MeshCalls>>,
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

        let mut control = ControlConfig::new("shared-node-token".to_string(), None);
        control.port = 0; // ephemeral loopback; peers use the shared fixed port in prod
        control.poll_interval = Duration::from_millis(50);
        control.heartbeat_interval = Duration::from_millis(200);
        control.reconnect_backoff = Duration::from_millis(50);

        let config = LinkManagerConfig {
            worker_base_url: server.uri(),
            identity_path: self.identity_path.clone(),
            mesh,
            control,
        };
        let factory = Arc::new(FakeFactory {
            calls: self.calls.clone(),
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
        json!({"authKey": "tskey-auth-xyz", "expirySeconds": 600}),
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
        json!({"authKey": "tskey-auth-ok", "expirySeconds": 600}),
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
