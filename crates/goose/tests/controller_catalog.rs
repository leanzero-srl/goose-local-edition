use async_trait::async_trait;
use axum::http::StatusCode;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use futures::StreamExt;
use goose::config::GooseMode;
use goose::conversation::message::Message;
use goose::session::work_catalog::WorkCatalog;
use goose::session::{Session, SessionManager, SessionType};
use goose::work_control::backend::ResourceSampler;
use goose::work_control::{
    ControllerBackendConfig, CursorPolicy, GooseControllerBackend, OwnerRunRegistry,
};
use leanzero_link::controller::{ControllerBackend, ControllerError};
use leanzero_link::controller_auth::{
    authorize, AuthenticatedDevice, AuthorizationError, ControllerAuthority, ReceiverBinding,
};
use leanzero_link::controller_wire::*;
use serde_json::{json, Value};
use sqlx::{Pool, Sqlite};
use std::{path::Path, sync::Arc, time::Duration};
use uuid::Uuid;

struct Authority(AuthorizationLease);

#[tokio::test]
async fn catalog_metadata_coalesces_wire_bytes_without_losing_selected_events() {
    let f = Fixture::new(128, 256).await;
    let (_, work) = f.create("A real visible session", SessionType::User).await;
    let initial = f.backend.catalog(&f.device, None).await.unwrap();
    let mut cursor = initial.resume_cursor.clone();
    let mut separate_bytes = 0;
    for index in 0..64 {
        f.catalog
            .record_runtime(
                &work.work_id.to_string(),
                &f.generation.to_string(),
                if index % 2 == 0 { "running" } else { "idle" },
                None,
            )
            .await
            .unwrap();
        let change = f.backend.changes(&f.device, &cursor).await.unwrap();
        separate_bytes += serde_json::to_vec(&change).unwrap().len();
        cursor = change.resume_cursor;
    }
    let coalesced = f
        .backend
        .changes(&f.device, &initial.resume_cursor)
        .await
        .unwrap();
    assert_eq!(coalesced.changes.len(), 1);
    let preview = coalesced.changes[0]
        .entry
        .as_ref()
        .unwrap()
        .last_event
        .as_ref()
        .unwrap();
    assert_eq!(preview.kind, "run_state");
    assert!(!preview.text.is_empty());
    let bytes = serde_json::to_vec(&coalesced).unwrap().len();
    assert!(bytes * 60 < separate_bytes);
    let events = f
        .catalog
        .work_events(
            &work.work_id.to_string(),
            &goose::session::work_catalog::StoreWatermark {
                generation: f.generation.to_string(),
                revision: initial.watermark.revision as i64,
            },
            128,
        )
        .await
        .unwrap();
    assert_eq!(events.len(), 64);
    eprintln!("controller metadata wire: {separate_bytes} bytes/64 replies -> {bytes} bytes/1 reply; selected replay keeps 64 events");
}

#[async_trait]
impl ControllerAuthority for Authority {
    async fn introspect(
        &self,
        _: &str,
        _: &ReceiverBinding,
        _: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError> {
        Ok(Some(self.0.clone()))
    }
}

struct Fixture {
    root: tempfile::TempDir,
    manager: Arc<SessionManager>,
    catalog: WorkCatalog,
    runs: Arc<OwnerRunRegistry>,
    backend: GooseControllerBackend,
    owner: Uuid,
    account: Uuid,
    device: AuthenticatedDevice,
    generation: Uuid,
    policy: CursorPolicy,
}

impl Fixture {
    async fn new(page_size: usize, stream_buffer: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        let manager = Arc::new(SessionManager::new(root.path().to_path_buf()));
        let catalog = manager.work_catalog().await.unwrap();
        let generation = Uuid::parse_str(&catalog.watermark().await.unwrap().generation).unwrap();
        let owner = Uuid::new_v4();
        let account = Uuid::new_v4();
        let policy = CursorPolicy {
            page_size,
            stream_buffer,
            journal_poll_interval: Duration::from_millis(10),
            ..CursorPolicy::default()
        };
        let runs = OwnerRunRegistry::for_store(root.path().to_path_buf(), manager.clone());
        let config = config(root.path(), owner, account, policy.clone());
        let backend = GooseControllerBackend::new(config, catalog.clone(), runs.clone(), None)
            .await
            .unwrap();
        let device = device(owner, account, generation, Uuid::new_v4()).await;
        Self {
            root,
            manager,
            catalog,
            runs,
            backend,
            owner,
            account,
            device,
            generation,
            policy,
        }
    }
    async fn create(&self, name: &str, kind: SessionType) -> (Session, WorkRef) {
        let session = self
            .manager
            .create_session(
                Path::new("/controller-fixture").into(),
                name.into(),
                kind,
                GooseMode::default(),
            )
            .await
            .unwrap();
        let record = self
            .catalog
            .lookup_local_session(&session.id)
            .await
            .unwrap();
        let work = WorkRef {
            owner_node_id: self.owner,
            kind: WorkKind::Chat,
            work_id: Uuid::parse_str(&record.work_id).unwrap(),
        };
        (session, work)
    }
    async fn pool(&self) -> Pool<Sqlite> {
        sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(self.root.path().join("sessions/sessions.db"))
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(10)),
        )
        .await
        .unwrap()
    }
    async fn reopen(
        &self,
        resources: Option<ResourceSampler>,
    ) -> Result<GooseControllerBackend, ControllerError> {
        GooseControllerBackend::new(
            config(
                self.root.path(),
                self.owner,
                self.account,
                self.policy.clone(),
            ),
            self.catalog.clone(),
            self.runs.clone(),
            resources,
        )
        .await
    }
}

fn config(
    root: &Path,
    owner: Uuid,
    account: Uuid,
    policy: CursorPolicy,
) -> ControllerBackendConfig {
    ControllerBackendConfig {
        host: HostDescriptor {
            owner_node_id: owner,
            name: "owner fixture".into(),
            device_role: DeviceRole::Host,
            protocol_version: 1,
            availability: HostAvailability::Online,
            last_seen_at: None,
            endpoints: Vec::new(),
            capabilities: vec![ControllerScope::ModelsManage],
        },
        account_id: account,
        cursor_key_path: root.join("controller-cursor.key"),
        policy,
    }
}

async fn device(owner: Uuid, account: Uuid, generation: Uuid, id: Uuid) -> AuthenticatedDevice {
    let binding = ReceiverBinding {
        owner_node_id: owner,
        runtime_id: Uuid::new_v4(),
        store_generation: generation,
        host_lease_id: Uuid::new_v4(),
        host_revision: 1,
    };
    let lease = AuthorizationLease {
        active: true,
        account_id: account,
        device_id: id,
        device_revision: 1,
        role: DeviceRole::Controller,
        owner_node_id: owner,
        scopes: vec![
            ControllerScope::CatalogRead,
            ControllerScope::HistoryRead,
            ControllerScope::ResourcesRead,
            ControllerScope::WorkSend,
            ControllerScope::WorkStop,
        ],
        token_expires_at: Utc::now() + chrono::Duration::minutes(10),
        lease_seconds: 30,
        host_revision: 1,
        store_generation: generation,
        host_lease_id: binding.host_lease_id,
    };
    authorize(
        &Authority(lease),
        "test-only-token",
        &binding,
        ControllerScope::CatalogRead,
    )
    .await
    .unwrap()
}

fn error_code<T>(result: Result<T, ControllerError>, expected: &str) {
    match result {
        Ok(_) => panic!("expected error {expected}"),
        Err(error) => assert_eq!(error.body.code, expected, "{error}"),
    }
}

async fn next_frame<T>(
    stream: &mut futures::stream::BoxStream<'static, Result<T, ControllerError>>,
) -> Result<T, ControllerError> {
    tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("stream stalled")
        .expect("stream closed")
}

#[tokio::test]
async fn remote_reads_never_activate_an_agent_or_write_the_store() {
    let f = Fixture::new(4, 8).await;
    let (session, work) = f.create("existing chat", SessionType::Acp).await;
    f.manager
        .add_message(
            &session.id,
            &Message::user().with_id("message").with_text("hello"),
        )
        .await
        .unwrap();
    let pool = f.pool().await;
    let mut observer = pool.acquire().await.unwrap();
    let before: i64 = sqlx::query_scalar("PRAGMA data_version")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    let watermark = f.catalog.watermark().await.unwrap();
    assert!(f.runs.active_agent(&session.id).await.is_none());
    let catalog = f.backend.catalog(&f.device, None).await.unwrap();
    let snapshot = f
        .backend
        .snapshot(&f.device, &work, f.generation)
        .await
        .unwrap();
    let history = f
        .backend
        .history(&f.device, &work, f.generation, None)
        .await
        .unwrap();
    assert_eq!(history.items.len(), 1);
    assert_eq!(history.items[0].message["content"][0]["text"], "hello");
    assert!(snapshot.pending_inputs.is_empty());
    assert!(snapshot.entry.capabilities.is_empty());
    assert!(f
        .backend
        .changes(&f.device, &catalog.resume_cursor)
        .await
        .unwrap()
        .changes
        .is_empty());
    drop(
        f.backend
            .catalog_stream(&f.device, &catalog.resume_cursor)
            .await
            .unwrap(),
    );
    drop(
        f.backend
            .work_stream(&f.device, &work, &snapshot.work_cursor)
            .await
            .unwrap(),
    );
    assert!(f.runs.current_run(&session.id).await.is_none());
    assert!(f.runs.active_agent(&session.id).await.is_none());
    assert_eq!(f.catalog.watermark().await.unwrap(), watermark);
    let after: i64 = sqlx::query_scalar("PRAGMA data_version")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    assert_eq!(before, after, "read APIs performed SQL writes");
    assert_eq!(
        f.backend.host().capabilities,
        vec![ControllerScope::CatalogRead, ControllerScope::HistoryRead]
    );
}

#[tokio::test]
async fn cursors_bind_owner_principal_purpose_work_and_generation_and_survive_reopen() {
    let f = Fixture::new(1, 8).await;
    let (a, work) = f.create("a", SessionType::User).await;
    let (_, other_work) = f.create("b", SessionType::User).await;
    for n in 0..2 {
        f.manager
            .add_message(
                &a.id,
                &Message::user()
                    .with_id(format!("m{n}"))
                    .with_text(format!("hello{n}")),
            )
            .await
            .unwrap();
    }
    let page = f.backend.catalog(&f.device, None).await.unwrap();
    let token = page.next_page_token.as_ref().unwrap();
    let second = f
        .reopen(None)
        .await
        .unwrap()
        .catalog(&f.device, Some(token))
        .await
        .unwrap();
    assert_eq!(second.watermark, page.watermark);
    let wrong_owner = device(
        Uuid::new_v4(),
        f.account,
        f.generation,
        f.device.device_id(),
    )
    .await;
    error_code(
        f.backend.catalog(&wrong_owner, Some(token)).await,
        "wrong_owner",
    );
    let wrong_account = device(f.owner, Uuid::new_v4(), f.generation, f.device.device_id()).await;
    error_code(
        f.backend.catalog(&wrong_account, Some(token)).await,
        "wrong_owner",
    );
    let other_device = device(f.owner, f.account, f.generation, Uuid::new_v4()).await;
    error_code(
        f.backend.catalog(&other_device, Some(token)).await,
        "cursor_binding_mismatch",
    );
    error_code(f.backend.changes(&f.device, token).await, "invalid_cursor");
    error_code(
        f.backend
            .catalog(&f.device, Some(&page.resume_cursor))
            .await,
        "invalid_cursor",
    );
    error_code(
        f.backend
            .catalog(
                &f.device,
                Some(&URL_SAFE_NO_PAD.encode(b"{\"revision\":0}")),
            )
            .await,
        "invalid_cursor",
    );
    let mut tampered = token.clone().into_bytes();
    tampered[5] = if tampered[5] == b'A' { b'B' } else { b'A' };
    error_code(
        f.backend
            .catalog(&f.device, Some(std::str::from_utf8(&tampered).unwrap()))
            .await,
        "invalid_cursor",
    );
    let history = f
        .backend
        .history(&f.device, &work, f.generation, None)
        .await
        .unwrap();
    error_code(
        f.backend
            .history(
                &f.device,
                &other_work,
                f.generation,
                history.next_page_token.as_deref(),
            )
            .await,
        "invalid_cursor",
    );
    error_code(
        f.backend
            .work_stream(&f.device, &other_work, &history.work_cursor)
            .await,
        "invalid_cursor",
    );
    error_code(
        f.backend.snapshot(&f.device, &work, Uuid::new_v4()).await,
        "snapshot_required",
    );
    let mut wrong_path = work.clone();
    wrong_path.owner_node_id = Uuid::new_v4();
    error_code(
        f.backend
            .snapshot(&f.device, &wrong_path, f.generation)
            .await,
        "wrong_owner",
    );
    wrong_path = work.clone();
    wrong_path.kind = WorkKind::AgentWork;
    error_code(
        f.backend
            .snapshot(&f.device, &wrong_path, f.generation)
            .await,
        "unsupported_action",
    );
    wrong_path = work.clone();
    wrong_path.work_id = Uuid::new_v4();
    error_code(
        f.backend
            .snapshot(&f.device, &wrong_path, f.generation)
            .await,
        "work_not_found",
    );
    // A correctly authenticated future cursor must still fail the storage watermark fence.
    let (payload, _) = page.resume_cursor.split_once('.').unwrap();
    let mut payload: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap();
    payload["revision"] = json!(i64::MAX);
    let bytes = serde_json::to_vec(&payload).unwrap();
    let key: [u8; 32] = std::fs::read(f.root.path().join("controller-cursor.key"))
        .unwrap()
        .try_into()
        .unwrap();
    let future = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(&bytes),
        URL_SAFE_NO_PAD.encode(blake3::keyed_hash(&key, &bytes).as_bytes())
    );
    error_code(f.backend.changes(&f.device, &future).await, "cursor_ahead");
    f.catalog
        .reset_generation(&f.generation.to_string(), "test restore fence")
        .await
        .unwrap();
    error_code(
        f.backend.catalog(&f.device, None).await,
        "snapshot_required",
    );
}

#[tokio::test]
async fn fixed_catalog_pages_and_visibility_changes_do_not_leak_internal_sessions() {
    let f = Fixture::new(1, 16).await;
    let (_, a) = f.create("visible a", SessionType::User).await;
    let (_, b) = f.create("visible b", SessionType::Scheduled).await;
    let (_, hidden) = f.create("private hidden title", SessionType::Hidden).await;
    let (_, child) = f.create("private child title", SessionType::SubAgent).await;
    let original = f.catalog.catalog_page(None, 20).await.unwrap();
    let first = f.backend.catalog(&f.device, None).await.unwrap();
    let target = original
        .records
        .iter()
        .find(|r| r.work_id == b.work_id.to_string())
        .unwrap();
    f.manager
        .update(&target.local_locator)
        .user_provided_name("renamed")
        .apply()
        .await
        .unwrap();
    let pool = f.pool().await;
    sqlx::query("UPDATE sessions SET session_type='hidden' WHERE id=?")
        .bind(&target.local_locator)
        .execute(&pool)
        .await
        .unwrap();
    f.create("another internal", SessionType::Hidden).await;
    let mut entries = first.entries;
    let mut next = first.next_page_token;
    while let Some(token) = next {
        let page = f.backend.catalog(&f.device, Some(&token)).await.unwrap();
        assert_eq!(page.watermark, first.watermark);
        entries.extend(page.entries);
        next = page.next_page_token;
    }
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| entry.work == a));
    assert!(entries.iter().any(|entry| entry.title == "visible b"));
    let mut cursor = first.resume_cursor;
    let mut changes = Vec::new();
    loop {
        let page = f.backend.changes(&f.device, &cursor).await.unwrap();
        cursor = page.resume_cursor;
        changes.extend(page.changes);
        if !page.has_more {
            break;
        }
    }
    assert!(changes
        .iter()
        .any(|change| change.work == b && matches!(change.kind, CatalogChangeKind::Tombstone)));
    assert!(changes
        .iter()
        .all(|change| change.work != hidden && change.work != child));
    assert!(!serde_json::to_string(&changes).unwrap().contains("private"));
    error_code(
        f.backend.snapshot(&f.device, &b, f.generation).await,
        "work_not_found",
    );
    error_code(
        f.backend
            .history(&f.device, &hidden, f.generation, None)
            .await,
        "work_not_found",
    );
}

#[tokio::test]
async fn history_uses_fixed_versions_and_user_audience_without_transport_secrets() {
    let f = Fixture::new(1, 16).await;
    let (session, work) = f.create("history", SessionType::User).await;
    for n in 0..3 {
        let mut message = Message::assistant()
            .with_id(format!("m{n}"))
            .with_thinking("private thought", "private signature")
            .with_text(format!("original{n}"));
        message.created = 100 + n;
        f.manager.add_message(&session.id, &message).await.unwrap();
    }
    let pool = f.pool().await;
    let mixed = json!([
        {"type":"text","text":"assistant only","annotations":{"audience":["assistant"]}},
        {"type":"text","text":"user only","annotations":{"audience":["user"]},"_meta":{"api_key":"SECRET"}},
        {"type":"toolRequest","id":"t1","toolCall":{"status":"success","value":{"name":"test","arguments":{}}},"metadata":{"api_key":"SECRET"},"_meta":{"api_key":"SECRET"}},
        {"type":"toolResponse","id":"t1","toolResult":{"status":"success","value":{"content":[{"type":"text","text":"private tool","annotations":{"audience":["assistant"]}},{"type":"text","text":"public tool"}],"_meta":{"api_key":"SECRET"}}},"metadata":{"api_key":"SECRET"}}
    ]);
    sqlx::query("UPDATE messages SET content_json=?,metadata_json=? WHERE message_id='m2' AND session_id=?")
        .bind(mixed.to_string()).bind(json!({"userVisible":true,"agentVisible":true,"providerError":{"class":"auth","transient":false,"said":"error","detail":"SECRET"},"inference":{"provider":"private-provider","requestedModel":"private-model"}}).to_string()).bind(&session.id).execute(&pool).await.unwrap();
    f.manager
        .add_message(
            &session.id,
            &Message::user()
                .with_id("hidden")
                .with_text("hidden message")
                .with_visibility(false, true),
        )
        .await
        .unwrap();
    let first = f
        .backend
        .history(&f.device, &work, f.generation, None)
        .await
        .unwrap();
    assert_eq!(
        first.items[0].message["content"].as_array().unwrap().len(),
        1
    );
    f.manager
        .update_message_metadata(&session.id, "m1", |mut metadata| {
            metadata.user_visible = false;
            metadata
        })
        .await
        .unwrap();
    sqlx::query("UPDATE messages SET content_json=? WHERE message_id='m2' AND session_id=?")
        .bind(json!([{"type":"text","text":"edited later"}]).to_string())
        .bind(&session.id)
        .execute(&pool)
        .await
        .unwrap();
    let mut items = first.items;
    let mut next = first.next_page_token;
    while let Some(token) = next {
        let page = f
            .backend
            .history(&f.device, &work, f.generation, Some(&token))
            .await
            .unwrap();
        assert_eq!(page.revision, first.revision);
        items.extend(page.items);
        next = page.next_page_token;
    }
    assert_eq!(items.len(), 3);
    assert_eq!(items[1].message["content"][0]["text"], "original1");
    let serialized = serde_json::to_string(&items).unwrap();
    for private in [
        "SECRET",
        "private thought",
        "private signature",
        "assistant only",
        "private tool",
        "private-provider",
        "private-model",
        "hidden message",
        "edited later",
    ] {
        assert!(!serialized.contains(private), "leaked {private}");
    }
    assert!(serialized.contains("user only"));
    assert!(serialized.contains("public tool"));
    assert!(items
        .iter()
        .all(|item| !item.message.to_string().contains("_meta")));
    // Present malformed metadata cannot turn into a public default message.
    sqlx::query(
        "UPDATE messages SET metadata_json='not json' WHERE message_id='m0' AND session_id=?",
    )
    .bind(&session.id)
    .execute(&pool)
    .await
    .unwrap();
    error_code(
        f.backend
            .history(&f.device, &work, f.generation, None)
            .await,
        "owner_unavailable",
    );
}

#[tokio::test]
async fn selected_stream_replays_edits_visibility_deletions_and_reconnect_without_other_work() {
    let f = Fixture::new(16, 32).await;
    let (session, work) = f.create("selected", SessionType::User).await;
    let (other, _) = f.create("other", SessionType::User).await;
    f.manager
        .add_message(
            &session.id,
            &Message::user().with_id("m1").with_text("before"),
        )
        .await
        .unwrap();
    f.manager
        .add_message(
            &session.id,
            &Message::assistant().with_id("m2").with_text("will hide"),
        )
        .await
        .unwrap();
    let snapshot = f
        .backend
        .snapshot(&f.device, &work, f.generation)
        .await
        .unwrap();
    let pool = f.pool().await;
    sqlx::query("UPDATE messages SET content_json=? WHERE message_id='m1' AND session_id=?")
        .bind(json!([{"type":"text","text":"after"}]).to_string())
        .bind(&session.id)
        .execute(&pool)
        .await
        .unwrap();
    f.manager
        .update_message_metadata(&session.id, "m2", |mut metadata| {
            metadata.user_visible = false;
            metadata
        })
        .await
        .unwrap();
    sqlx::query("DELETE FROM messages WHERE message_id='m1' AND session_id=?")
        .bind(&session.id)
        .execute(&pool)
        .await
        .unwrap();
    f.manager
        .add_message(
            &other.id,
            &Message::assistant()
                .with_id("other")
                .with_text("other private transcript"),
        )
        .await
        .unwrap();
    let mut stream = f
        .backend
        .work_stream(&f.device, &work, &snapshot.work_cursor)
        .await
        .unwrap();
    let first = next_frame(&mut stream).await.unwrap();
    assert_eq!(first.kind, "message_upsert");
    assert_eq!(
        first.payload["item"]["message"]["content"][0]["text"],
        "after"
    );
    assert_eq!(first.work, work);
    assert_eq!(first.stream_epoch, f.generation);
    drop(stream);
    let mut resumed = f
        .backend
        .work_stream(&f.device, &work, &first.resume_cursor)
        .await
        .unwrap();
    let hide = next_frame(&mut resumed).await.unwrap();
    assert_eq!(hide.kind, "message_deleted");
    assert_eq!(hide.payload["message_id"], "m2");
    let delete = next_frame(&mut resumed).await.unwrap();
    assert_eq!(delete.kind, "message_deleted");
    assert_eq!(delete.payload["message_id"], "m1");
    assert!(hide.sequence > first.sequence && delete.sequence > hide.sequence);
    assert!(!serde_json::to_string(&[first, hide, delete])
        .unwrap()
        .contains("other private"));
    // Subscribe before this write; the observer closes the snapshot/subscribe race.
    f.manager
        .add_message(
            &session.id,
            &Message::assistant()
                .with_id("live")
                .with_text("arrived live"),
        )
        .await
        .unwrap();
    let live = next_frame(&mut resumed).await.unwrap();
    assert_eq!(live.payload["item"]["message_id"], "live");
    sqlx::query("UPDATE sessions SET session_type='hidden' WHERE id=?")
        .bind(&session.id)
        .execute(&pool)
        .await
        .unwrap();
    let mut hidden = next_frame(&mut resumed).await.unwrap();
    if hidden.kind == "session_changed" {
        hidden = next_frame(&mut resumed).await.unwrap();
    }
    assert_eq!(hidden.kind, "tombstone");
    assert!(tokio::time::timeout(Duration::from_secs(5), resumed.next())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn backpressure_reports_a_gap_and_last_cursor_replays_the_committed_events() {
    let f = Fixture::new(16, 1).await;
    let (session, work) = f.create("slow viewer", SessionType::User).await;
    let snapshot = f
        .backend
        .snapshot(&f.device, &work, f.generation)
        .await
        .unwrap();
    for n in 0..4 {
        f.manager
            .add_message(
                &session.id,
                &Message::assistant()
                    .with_id(format!("m{n}"))
                    .with_text(format!("message{n}")),
            )
            .await
            .unwrap();
    }
    let mut stream = f
        .backend
        .work_stream(&f.device, &work, &snapshot.work_cursor)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let first = next_frame(&mut stream).await.unwrap();
    let gap = next_frame(&mut stream).await.unwrap_err();
    assert_eq!(gap.body.code, "stream_gap");
    assert_eq!(gap.status, StatusCode::GONE);
    let mut resumed = f
        .backend
        .work_stream(&f.device, &work, &first.resume_cursor)
        .await
        .unwrap();
    let next = next_frame(&mut resumed).await.unwrap();
    assert!(next.sequence > first.sequence);
    assert_eq!(next.kind, "session_changed");
    drop(resumed);
    let mut resumed = f
        .backend
        .work_stream(&f.device, &work, &next.resume_cursor)
        .await
        .unwrap();
    let next = next_frame(&mut resumed).await.unwrap();
    assert_eq!(next.payload["item"]["message_id"], "m1");
}

#[tokio::test]
async fn finite_work_changes_match_stream_replay_and_empty_cursor_stays_usable() {
    let f = Fixture::new(2, 32).await;
    let (session, work) = f.create("selected", SessionType::User).await;
    let (other, other_work) = f.create("other", SessionType::User).await;
    let snapshot = f
        .backend
        .snapshot(&f.device, &work, f.generation)
        .await
        .unwrap();
    for (id, text, visible) in [
        ("one", "first", true),
        ("hidden", "internal only", false),
        ("two", "second", true),
    ] {
        f.manager
            .add_message(
                &session.id,
                &Message::assistant()
                    .with_id(id)
                    .with_text(text)
                    .with_visibility(visible, true),
            )
            .await
            .unwrap();
    }
    f.manager
        .add_message(
            &other.id,
            &Message::assistant()
                .with_id("other")
                .with_text("other private"),
        )
        .await
        .unwrap();
    let mut cursor = snapshot.work_cursor.clone();
    let mut frames = Vec::new();
    loop {
        let page = f
            .backend
            .work_changes(&f.device, &work, f.generation, &cursor)
            .await
            .unwrap();
        assert_ne!(page.resume_cursor, cursor);
        cursor = page.resume_cursor;
        frames.extend(page.frames);
        if !page.has_more {
            break;
        }
    }
    assert!(frames
        .iter()
        .any(|frame| frame.payload["item"]["message_id"] == "one"));
    assert!(frames
        .iter()
        .any(|frame| frame.payload["item"]["message_id"] == "two"));
    assert!(!serde_json::to_string(&frames)
        .unwrap()
        .contains("internal only"));
    assert!(!serde_json::to_string(&frames)
        .unwrap()
        .contains("other private"));
    let mut stream = f
        .backend
        .work_stream(&f.device, &work, &snapshot.work_cursor)
        .await
        .unwrap();
    for expected in &frames {
        let actual = next_frame(&mut stream).await.unwrap();
        assert_eq!(actual.sequence, expected.sequence);
        assert_eq!(actual.kind, expected.kind);
        assert_eq!(actual.payload, expected.payload);
    }
    let empty = tokio::time::timeout(
        Duration::from_secs(1),
        f.backend
            .work_changes(&f.device, &work, f.generation, &cursor),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(empty.frames.is_empty());
    assert!(!empty.has_more);
    f.manager
        .add_message(
            &session.id,
            &Message::assistant()
                .with_id("later")
                .with_text("after empty poll"),
        )
        .await
        .unwrap();
    let later = f
        .backend
        .work_changes(&f.device, &work, f.generation, &empty.resume_cursor)
        .await
        .unwrap();
    assert!(later
        .frames
        .iter()
        .any(|frame| frame.payload["item"]["message_id"] == "later"));
    error_code(
        f.backend
            .work_changes(&f.device, &other_work, f.generation, &cursor)
            .await,
        "invalid_cursor",
    );
    let other_device = device(f.owner, f.account, f.generation, Uuid::new_v4()).await;
    error_code(
        f.backend
            .work_changes(&other_device, &work, f.generation, &cursor)
            .await,
        "cursor_binding_mismatch",
    );
    f.catalog
        .reset_generation(&f.generation.to_string(), "test replacement")
        .await
        .unwrap();
    error_code(
        f.backend
            .work_changes(&f.device, &work, f.generation, &cursor)
            .await,
        "snapshot_required",
    );
}

#[tokio::test]
async fn catalog_stream_delivers_committed_changes_and_visibility_tombstones() {
    let f = Fixture::new(4, 8).await;
    let page = f.backend.catalog(&f.device, None).await.unwrap();
    let mut stream = f
        .backend
        .catalog_stream(&f.device, &page.resume_cursor)
        .await
        .unwrap();
    let (session, work) = f.create("new chat", SessionType::User).await;
    let change = next_frame(&mut stream).await.unwrap();
    assert_eq!(change.work, work);
    assert!(matches!(change.kind, CatalogChangeKind::Upsert));
    f.manager.delete_session(&session.id).await.unwrap();
    let tombstone = next_frame(&mut stream).await.unwrap();
    assert!(matches!(tombstone.kind, CatalogChangeKind::Tombstone));
    assert!(tombstone.entry.is_none());
    assert_eq!(tombstone.work, work);
}

#[tokio::test]
async fn resource_sample_retains_its_real_timestamp_and_units_and_requires_a_sampler() {
    let f = Fixture::new(4, 8).await;
    error_code(f.backend.resources(&f.device).await, "scope_denied");
    let sampled_at = Utc::now() - chrono::Duration::seconds(3);
    let sample = leanzero_link::load::NodeLoad {
        sampled_at: Some(sampled_at),
        own: leanzero_link::load::OwnLoad {
            cpu_load_per_core: Some(1.7),
            mem_free_pct: Some(65.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let sampler: ResourceSampler = Arc::new(move || {
        let sample = sample.clone();
        Box::pin(async move { Ok(sample) })
    });
    let backend = f.reopen(Some(sampler)).await.unwrap();
    let result = backend.resources(&f.device).await.unwrap();
    assert_eq!(result.sampled_at, sampled_at);
    assert_eq!(result.load.own.cpu_load_per_core, Some(1.7));
    assert_eq!(result.load.own.mem_free_pct, Some(65.0));
    let unavailable: ResourceSampler =
        Arc::new(|| Box::pin(async { Ok(leanzero_link::load::NodeLoad::default()) }));
    error_code(
        f.reopen(Some(unavailable))
            .await
            .unwrap()
            .resources(&f.device)
            .await,
        "owner_unavailable",
    );
}

#[tokio::test]
async fn cursor_expiry_corruption_and_private_file_require_an_explicit_resnapshot_or_repair() {
    let mut f = Fixture::new(1, 8).await;
    f.create("a", SessionType::User).await;
    f.create("b", SessionType::User).await;
    f.policy.lifetime = Duration::from_secs(1);
    let backend = f.reopen(None).await.unwrap();
    let page = backend.catalog(&f.device, None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    error_code(
        backend
            .catalog(&f.device, page.next_page_token.as_deref())
            .await,
        "snapshot_required",
    );
    let key_path = f.root.path().join("controller-cursor.key");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        error_code(f.reopen(None).await, "owner_unavailable");
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(&key_path, b"corrupt").unwrap();
    error_code(f.reopen(None).await, "owner_unavailable");
    assert_eq!(std::fs::read(&key_path).unwrap(), b"corrupt");
}
