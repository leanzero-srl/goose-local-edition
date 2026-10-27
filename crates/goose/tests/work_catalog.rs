use goose::config::GooseMode;
use goose::conversation::message::Message;
use goose::conversation::Conversation;
use goose::session::work_catalog::{
    CatalogError, CatalogPageCursor, HistoryPageCursor, StoreWatermark,
};
use goose::session::{Session, SessionManager, SessionType};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Pool, Sqlite};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

#[derive(serde::Serialize, serde::Deserialize)]
struct CatalogExtension {
    value: String,
}

impl goose::session::extension_data::ExtensionState for CatalogExtension {
    const EXTENSION_NAME: &'static str = "catalog-fixture";
    const VERSION: &'static str = "v1";
}

async fn create(manager: &SessionManager, name: &str) -> Session {
    manager
        .create_session(
            Path::new("/catalog-fixture").to_path_buf(),
            name.to_owned(),
            SessionType::User,
            GooseMode::default(),
        )
        .await
        .unwrap()
}

async fn direct_pool(root: &Path) -> Pool<Sqlite> {
    sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(root.join("sessions/sessions.db"))
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(30)),
    )
    .await
    .unwrap()
}

fn assert_error(error: anyhow::Error, expected: CatalogError) {
    assert_eq!(
        error.downcast_ref::<CatalogError>(),
        Some(&expected),
        "{error:#}"
    );
}

#[tokio::test]
async fn fresh_and_concurrent_opens_create_one_generation_and_unique_work() {
    let root = tempfile::tempdir().unwrap();
    let mut tasks = Vec::new();
    for number in 0..8 {
        let path = root.path().to_path_buf();
        tasks.push(tokio::spawn(async move {
            let manager = SessionManager::new(path);
            let session = create(&manager, &format!("session {number}")).await;
            let catalog = manager.work_catalog().await.unwrap();
            (
                catalog.watermark().await.unwrap().generation,
                catalog
                    .lookup_local_session(&session.id)
                    .await
                    .unwrap()
                    .work_id,
            )
        }));
    }
    let mut generations = std::collections::BTreeSet::new();
    let mut work_ids = std::collections::BTreeSet::new();
    for task in tasks {
        let (generation, work_id) = task.await.unwrap();
        uuid::Uuid::parse_str(&generation).unwrap();
        assert_eq!(
            uuid::Uuid::parse_str(&work_id).unwrap().get_version_num(),
            4
        );
        generations.insert(generation);
        assert!(work_ids.insert(work_id));
    }
    assert_eq!(generations.len(), 1);
    let manager = SessionManager::new(root.path().to_path_buf());
    let page = manager
        .work_catalog()
        .await
        .unwrap()
        .catalog_page(None, 20)
        .await
        .unwrap();
    assert_eq!(page.records.len(), 8);
    assert!(page.next.is_none());
}

#[tokio::test]
async fn independent_processes_can_initialize_the_same_fresh_store() {
    let root = tempfile::tempdir().unwrap();
    let executable = std::env::current_exe().unwrap();
    let mut children = Vec::new();
    for number in 0..8 {
        children.push(
            tokio::process::Command::new(&executable)
                .args(["--exact", "catalog_startup_child", "--ignored"])
                .env("GOOSE_CATALOG_STARTUP_DIR", root.path())
                .env("GOOSE_CATALOG_STARTUP_NAME", format!("process {number}"))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        let output = child.wait_with_output().await.unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let manager = SessionManager::new(root.path().to_path_buf());
    let catalog = manager.work_catalog().await.unwrap();
    let page = catalog.catalog_page(None, 20).await.unwrap();
    assert_eq!(page.records.len(), 8);
    assert_eq!(
        page.records
            .iter()
            .map(|record| &record.work_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        8
    );
    let pool = direct_pool(root.path()).await;
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "wal"
    );
}

#[tokio::test]
#[ignore = "child process for independent_processes_can_initialize_the_same_fresh_store"]
async fn catalog_startup_child() {
    let root = std::env::var_os("GOOSE_CATALOG_STARTUP_DIR").unwrap();
    let name = std::env::var("GOOSE_CATALOG_STARTUP_NAME").unwrap();
    let manager = SessionManager::new(root.into());
    let session = create(&manager, &name).await;
    let catalog = manager.work_catalog().await.unwrap();
    let record = catalog.lookup_local_session(&session.id).await.unwrap();
    assert_eq!(record.title, name);
    assert_eq!(
        uuid::Uuid::parse_str(&record.work_id)
            .unwrap()
            .get_version_num(),
        4
    );
}

#[tokio::test]
async fn migration_from_v15_is_atomic_and_backfills_without_copying_transcript_bytes() {
    let root = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    let session = create(&manager, "existing").await;
    manager
        .add_message(
            &session.id,
            &Message::user()
                .with_id("old-message")
                .with_text("preserve me"),
        )
        .await
        .unwrap();
    let pool = direct_pool(root.path()).await;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let triggers = sqlx::query_scalar::<_, String>("SELECT name FROM sqlite_master WHERE type = 'trigger' AND (name LIKE 'controller_%' OR name LIKE 'work_identity_%')").fetch_all(&mut *tx).await.unwrap();
    for trigger in triggers {
        sqlx::query(&format!("DROP TRIGGER {trigger}"))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for table in [
        "external_work_progress",
        "external_catalog_versions",
        "external_work_runs",
        "external_work_identity",
        "work_events",
        "catalog_versions",
        "controller_message_versions",
        "controller_message_rows",
        "work_identity",
        "controller_generation_resets",
        "controller_store",
    ] {
        sqlx::query(&format!("DROP TABLE {table}"))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM schema_version WHERE version >= 16")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (15)")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT MAX(version) FROM schema_version")
            .fetch_one(&pool)
            .await
            .unwrap(),
        15
    );
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let path = root.path().to_path_buf();
        let local_id = session.id.clone();
        tasks.push(tokio::spawn(async move {
            let catalog = SessionManager::new(path).work_catalog().await.unwrap();
            (
                catalog.watermark().await.unwrap(),
                catalog.lookup_local_session(&local_id).await.unwrap(),
            )
        }));
    }
    let (watermark, record) = tasks.remove(0).await.unwrap();
    for task in tasks {
        let (other_watermark, other_record) = task.await.unwrap();
        assert_eq!(other_watermark, watermark);
        assert_eq!(other_record, record);
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM controller_message_versions")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM controller_message_rows")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    let catalog = SessionManager::new(root.path().to_path_buf())
        .work_catalog()
        .await
        .unwrap();
    let history = catalog
        .history_page(&record.work_id, &watermark.generation, None, 10)
        .await
        .unwrap();
    assert_eq!(history.records[0].message_id, "old-message");
    assert!(history.records[0].content_json.contains("preserve me"));
}

#[tokio::test]
async fn local_id_reuse_and_import_copy_never_reuse_work_identity() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    let session = create(&manager, "original").await;
    let other_manager = SessionManager::new(other.path().to_path_buf());
    let same_local = create(&other_manager, "other host").await;
    assert_eq!(same_local.id, session.id);
    let catalog = manager.work_catalog().await.unwrap();
    let original = catalog.lookup_local_session(&session.id).await.unwrap();
    let other_id = other_manager
        .work_catalog()
        .await
        .unwrap()
        .lookup_local_session(&same_local.id)
        .await
        .unwrap()
        .work_id;
    assert_ne!(original.work_id, other_id);
    manager.delete_session(&session.id).await.unwrap();
    let recreated = create(&manager, "recreated").await;
    assert_eq!(recreated.id, session.id);
    let replacement = catalog.lookup_local_session(&recreated.id).await.unwrap();
    assert_ne!(replacement.work_id, original.work_id);
    let generation = catalog.watermark().await.unwrap().generation;
    let deleted = catalog
        .lookup(&original.work_id, &generation)
        .await
        .unwrap();
    assert!(deleted.deleted);
    assert_eq!(deleted.state, "deleted");
    let copy = manager
        .copy_session(&recreated.id, "copy".to_owned())
        .await
        .unwrap();
    let exported = manager.export_session(&recreated.id).await.unwrap();
    let imported = manager
        .import_session(&exported, None, Path::new("/fallback"))
        .await
        .unwrap();
    let ids = [
        original.work_id,
        replacement.work_id,
        catalog
            .lookup_local_session(&copy.id)
            .await
            .unwrap()
            .work_id,
        catalog
            .lookup_local_session(&imported.id)
            .await
            .unwrap()
            .work_id,
    ];
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        4
    );
}

#[tokio::test]
async fn catalog_pages_keep_the_original_watermark_while_rows_change_and_disappear() {
    let root = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    for name in ["one", "two", "three"] {
        create(&manager, name).await;
    }
    let catalog = manager.work_catalog().await.unwrap();
    let original = catalog.catalog_page(None, 20).await.unwrap();
    let first = catalog.catalog_page(None, 1).await.unwrap();
    assert_eq!(first.watermark, original.watermark);
    let second_record = &original.records[1];
    manager
        .update(&second_record.local_locator)
        .user_provided_name("changed")
        .working_dir(Path::new("/changed").to_path_buf())
        .archived_at(Some(chrono::Utc::now()))
        .apply()
        .await
        .unwrap();
    manager
        .delete_session(&original.records[2].local_locator)
        .await
        .unwrap();
    create(&manager, "later insert").await;
    let mut seen = first.records;
    let mut cursor = first.next;
    while let Some(token) = cursor {
        let page = catalog.catalog_page(Some(&token), 1).await.unwrap();
        assert_eq!(page.watermark, original.watermark);
        seen.extend(page.records);
        cursor = page.next;
    }
    assert_eq!(seen, original.records);
    let changes = catalog.changes(&original.watermark, 20).await.unwrap();
    assert!(changes
        .records
        .iter()
        .any(|record| record.title == "changed" && record.archived));
    assert!(changes.records.iter().any(|record| record.deleted));
    let mut cursor = original.watermark.clone();
    let mut revisions = Vec::new();
    loop {
        let changes = catalog.changes(&cursor, 1).await.unwrap();
        revisions.extend(changes.records.iter().map(|record| record.revision));
        cursor = changes.cursor;
        if !changes.has_more {
            break;
        }
    }
    assert_eq!(
        revisions,
        ((original.watermark.revision + 1)..=cursor.revision).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn history_replacement_metadata_and_deletion_replay_exact_old_bytes() {
    let root = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    let session = create(&manager, "history").await;
    let mut messages = Vec::new();
    for number in 0..3 {
        let mut message = Message::user()
            .with_id(format!("message-{number}"))
            .with_text(format!("original {number}"));
        message.created = 1_000 + number;
        messages.push(message);
    }
    manager
        .replace_conversation(
            &session.id,
            &Conversation::new_unvalidated(messages.clone()),
        )
        .await
        .unwrap();
    let catalog = manager.work_catalog().await.unwrap();
    let work = catalog.lookup_local_session(&session.id).await.unwrap();
    let generation = catalog.watermark().await.unwrap().generation;
    let baseline = catalog
        .history_page(&work.work_id, &generation, None, 20)
        .await
        .unwrap();
    let first = catalog
        .history_page(&work.work_id, &generation, None, 1)
        .await
        .unwrap();
    manager
        .update_message_metadata(&session.id, "message-1", |mut metadata| {
            metadata.user_visible = false;
            metadata
        })
        .await
        .unwrap();
    let mut replacement = Message::assistant()
        .with_id("replacement")
        .with_text("new transcript");
    replacement.created = 2_000;
    manager
        .replace_conversation(
            &session.id,
            &Conversation::new_unvalidated(vec![replacement]),
        )
        .await
        .unwrap();
    let mut records = first.records;
    let mut cursor = first.next;
    while let Some(token) = cursor {
        let page = catalog
            .history_page(&work.work_id, &generation, Some(&token), 1)
            .await
            .unwrap();
        assert_eq!(page.watermark, baseline.watermark);
        records.extend(page.records);
        cursor = page.next;
    }
    assert_eq!(records, baseline.records);
    let current = catalog
        .history_page(&work.work_id, &generation, None, 20)
        .await
        .unwrap();
    assert_eq!(current.records.len(), 1);
    assert_eq!(current.records[0].message_id, "replacement");
    let events = catalog
        .work_events(&work.work_id, &baseline.watermark, 20)
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| event.event_kind == "message_upsert"
            && event.message_id.as_deref() == Some("message-1")));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_kind == "message_deleted")
            .count(),
        3
    );
    manager
        .truncate_conversation(&session.id, 2_000)
        .await
        .unwrap();
    assert!(catalog
        .history_page(&work.work_id, &generation, None, 20)
        .await
        .unwrap()
        .records
        .is_empty());
    manager
        .replace_conversation(&session.id, &Conversation::new_unvalidated(messages))
        .await
        .unwrap();
    manager
        .truncate_conversation_from_message(&session.id, "message-1")
        .await
        .unwrap();
    assert_eq!(
        catalog
            .history_page(&work.work_id, &generation, None, 20)
            .await
            .unwrap()
            .records
            .len(),
        1
    );
}

#[tokio::test]
async fn old_process_sql_is_journaled_atomically_and_reads_do_not_write() {
    let root = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    let session = create(&manager, "before").await;
    let catalog = manager.work_catalog().await.unwrap();
    let before = catalog.watermark().await.unwrap();
    let pool = direct_pool(root.path()).await;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    sqlx::query("UPDATE sessions SET name = 'uncommitted' WHERE id = ?")
        .bind(&session.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO messages (session_id, role, content_json, created_timestamp) VALUES (?, 'user', '[{\"type\":\"text\",\"text\":\"external\"}]', 123)").bind(&session.id).execute(&mut *tx).await.unwrap();
    assert_eq!(catalog.watermark().await.unwrap(), before);
    tx.rollback().await.unwrap();
    assert_eq!(catalog.watermark().await.unwrap(), before);
    sqlx::query("UPDATE sessions SET name = 'external change' WHERE id = ?")
        .bind(&session.id)
        .execute(&pool)
        .await
        .unwrap();
    let after = catalog.watermark().await.unwrap();
    assert!(after.revision > before.revision);
    let work = catalog.lookup_local_session(&session.id).await.unwrap();
    assert_eq!(work.title, "external change");
    catalog.catalog_page(None, 20).await.unwrap();
    catalog
        .lookup(&work.work_id, &after.generation)
        .await
        .unwrap();
    catalog
        .history_page(&work.work_id, &after.generation, None, 20)
        .await
        .unwrap();
    assert_eq!(catalog.watermark().await.unwrap(), after);
    assert_eq!(
        manager
            .get_session(&session.id, false)
            .await
            .unwrap()
            .working_dir,
        Path::new("/catalog-fixture")
    );
}

#[tokio::test]
async fn generation_reset_and_impossible_cursors_fail_explicitly() {
    let root = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    let session = create(&manager, "fenced").await;
    let catalog = manager.work_catalog().await.unwrap();
    let initial = catalog.watermark().await.unwrap();
    let work = catalog.lookup_local_session(&session.id).await.unwrap();
    let mut ahead = initial.clone();
    ahead.revision += 1;
    assert_error(
        catalog.changes(&ahead, 10).await.unwrap_err(),
        CatalogError::CursorAhead,
    );
    assert_error(
        catalog
            .check_external_fence(&initial.generation, ahead.revision)
            .await
            .unwrap_err(),
        CatalogError::RollbackDetected,
    );
    let run_id = uuid::Uuid::new_v4();
    let running = catalog
        .record_runtime(&work.work_id, &initial.generation, "running", Some(run_id))
        .await
        .unwrap();
    assert_eq!(
        running.current_run_id.as_deref(),
        Some(run_id.to_string().as_str())
    );
    assert_eq!(running.state, "running");
    let reset = catalog
        .reset_generation(&initial.generation, "explicit restored-store registration")
        .await
        .unwrap();
    assert_ne!(reset.generation, initial.generation);
    assert_error(
        catalog
            .lookup(&work.work_id, &initial.generation)
            .await
            .unwrap_err(),
        CatalogError::GenerationMismatch,
    );
    assert_error(
        catalog.changes(&initial, 10).await.unwrap_err(),
        CatalogError::GenerationMismatch,
    );
    assert_error(
        catalog
            .catalog_page(
                Some(&CatalogPageCursor {
                    watermark: initial.clone(),
                    after_work_id: String::new(),
                }),
                10,
            )
            .await
            .unwrap_err(),
        CatalogError::GenerationMismatch,
    );
    assert_error(
        catalog
            .history_page(
                &work.work_id,
                &reset.generation,
                Some(&HistoryPageCursor {
                    watermark: initial,
                    work_id: work.work_id.clone(),
                    after_timestamp: 0,
                    after_row_id: 0,
                }),
                10,
            )
            .await
            .unwrap_err(),
        CatalogError::GenerationMismatch,
    );
    assert_eq!(
        catalog
            .lookup(&work.work_id, &reset.generation)
            .await
            .unwrap()
            .work_id,
        work.work_id
    );
    assert_error(
        catalog
            .changes(
                &StoreWatermark {
                    generation: reset.generation.clone(),
                    revision: -1,
                },
                10,
            )
            .await
            .unwrap_err(),
        CatalogError::InvalidCursor,
    );
    assert_error(
        catalog.catalog_page(None, 0).await.unwrap_err(),
        CatalogError::InvalidPageSize,
    );
}

#[tokio::test]
async fn shared_journal_observer_detects_local_and_other_process_commits() {
    let root = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    let session = create(&manager, "observed").await;
    let catalog = manager.work_catalog().await.unwrap();
    let mut first = catalog.subscribe(Duration::from_millis(10)).await.unwrap();
    let other_manager = SessionManager::new(root.path().to_path_buf());
    let mut second = other_manager
        .work_catalog()
        .await
        .unwrap()
        .subscribe(Duration::from_millis(10))
        .await
        .unwrap();
    let pool = direct_pool(root.path()).await;
    sqlx::query("UPDATE sessions SET name = 'external' WHERE id = ?")
        .bind(&session.id)
        .execute(&pool)
        .await
        .unwrap();
    let expected = catalog.watermark().await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), first.changed())
            .await
            .unwrap()
            .unwrap(),
        expected
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), second.changed())
            .await
            .unwrap()
            .unwrap(),
        expected
    );
    manager
        .update(&session.id)
        .user_provided_name("local")
        .apply()
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), first.changed())
            .await
            .unwrap()
            .unwrap(),
        catalog.watermark().await.unwrap()
    );
}

#[tokio::test]
async fn catalog_pagination_uses_work_identity_index_and_changes_use_revision_index() {
    let root = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    create(&manager, "query control").await;
    let pool = direct_pool(root.path()).await;
    let tables: BTreeMap<String, i64> = sqlx::query_as::<_, (String, i64)>("SELECT name, COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('catalog_versions_work', 'work_events_work', 'controller_message_rows_work', 'controller_message_versions_work') GROUP BY name").fetch_all(&pool).await.unwrap().into_iter().collect();
    assert_eq!(tables.len(), 4);
    let plan = sqlx::query_as::<_, (i64, i64, i64, String)>("EXPLAIN QUERY PLAN SELECT * FROM catalog_versions WHERE revision > 1 AND revision <= 10 ORDER BY revision LIMIT 10").fetch_all(&pool).await.unwrap();
    assert!(
        plan.iter().any(|row| row.3.contains("INTEGER PRIMARY KEY")),
        "{plan:?}"
    );
}

#[tokio::test]
async fn extension_and_tool_metadata_writes_enter_the_same_durable_journal() {
    let root = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    let session = create(&manager, "metadata").await;
    let catalog = manager.work_catalog().await.unwrap();
    let initial = catalog.watermark().await.unwrap();
    manager
        .set_extension_state(
            &session.id,
            &CatalogExtension {
                value: "first".to_owned(),
            },
        )
        .await
        .unwrap();
    manager
        .update_extension_state::<CatalogExtension, _>(&session.id, |state| {
            assert_eq!(state.unwrap().value, "first");
            Ok((
                CatalogExtension {
                    value: "second".to_owned(),
                },
                (),
            ))
        })
        .await
        .unwrap();
    let extension_changes = catalog.changes(&initial, 10).await.unwrap();
    assert_eq!(extension_changes.records.len(), 2);
    assert!(extension_changes
        .records
        .iter()
        .all(|record| record.event_kind == "session_changed"));
    manager
        .add_message(
            &session.id,
            &Message::assistant()
                .with_id("tool-message")
                .with_tool_request(
                    "tool-call",
                    Ok(rmcp::model::CallToolRequestParams::new("fixture")),
                ),
        )
        .await
        .unwrap();
    let work = catalog.lookup_local_session(&session.id).await.unwrap();
    let before = catalog.watermark().await.unwrap();
    let original = catalog
        .history_page(&work.work_id, &before.generation, None, 10)
        .await
        .unwrap();
    manager
        .update_tool_request_meta(
            &session.id,
            "tool-message",
            "tool-call",
            serde_json::json!({"title": "saved tool title"}),
        )
        .await
        .unwrap();
    let current = catalog
        .history_page(&work.work_id, &before.generation, None, 10)
        .await
        .unwrap();
    assert!(current.records[0].content_json.contains("saved tool title"));
    assert_eq!(
        current.records[0].message_id,
        original.records[0].message_id
    );
    assert!(current.records[0].revision > original.records[0].revision);
    let snapshot = HistoryPageCursor {
        watermark: before.clone(),
        work_id: work.work_id.clone(),
        after_timestamp: i64::MIN,
        after_row_id: i64::MIN,
    };
    assert_eq!(
        catalog
            .history_page(&work.work_id, &before.generation, Some(&snapshot), 10)
            .await
            .unwrap()
            .records,
        original.records
    );
    assert_eq!(
        catalog
            .work_events(&work.work_id, &before, 10)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn projected_catalog_resolves_cross_source_versions_before_visibility() {
    let root = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    let session = create(&manager, "chat").await;
    let catalog = manager.work_catalog().await.unwrap();
    let record = catalog.lookup_local_session(&session.id).await.unwrap();
    let pool = direct_pool(root.path()).await;
    sqlx::query("INSERT INTO external_work_identity(work_id,kind,local_locator) VALUES (?,'benchmark','/collision')")
        .bind(&record.work_id).execute(&pool).await.unwrap();
    let insert_external = "INSERT INTO external_catalog_versions SELECT (SELECT revision FROM controller_store),work_id,'benchmark',local_locator,title,working_dir,'external',state,current_run_id,archived,updated_at,deleted,event_kind FROM catalog_versions WHERE revision=?";
    sqlx::query("UPDATE controller_store SET revision=revision+1")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(insert_external)
        .bind(record.revision)
        .execute(&pool)
        .await
        .unwrap();
    let external = catalog.controller_catalog_page(None, 1).await.unwrap();
    assert_eq!(external.records.len(), 1);
    assert_eq!(external.records[0].kind, "benchmark");
    assert!(external.next.is_none());
    sqlx::query("UPDATE sessions SET session_type='hidden' WHERE id=?")
        .bind(&session.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(catalog
        .controller_catalog_page(None, 1)
        .await
        .unwrap()
        .records
        .is_empty());
    let raw = catalog.catalog_page(None, 1).await.unwrap();
    assert_eq!(raw.records.len(), 1);
    assert_eq!(raw.records[0].session_type, "hidden");
    assert!(raw.next.is_none());
    let frozen = catalog
        .controller_catalog_page(
            Some(&CatalogPageCursor {
                watermark: external.watermark,
                after_work_id: String::new(),
            }),
            1,
        )
        .await
        .unwrap();
    assert_eq!(frozen.records[0].kind, "benchmark");
    sqlx::query("UPDATE controller_store SET revision=revision+1")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(insert_external)
        .bind(record.revision)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        catalog
            .controller_catalog_page(None, 1)
            .await
            .unwrap()
            .records[0]
            .kind,
        "benchmark"
    );
    manager.delete_session(&session.id).await.unwrap();
    let deleted = catalog.catalog_page(None, 1).await.unwrap();
    assert!(deleted.records[0].deleted);
    sqlx::query("UPDATE controller_store SET revision=revision+1")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(insert_external)
        .bind(deleted.records[0].revision)
        .execute(&pool)
        .await
        .unwrap();
    assert!(catalog
        .controller_catalog_page(None, 1)
        .await
        .unwrap()
        .records
        .is_empty());
    assert!(catalog.catalog_page(None, 1).await.unwrap().records[0].deleted);
}

#[tokio::test]
async fn latest_changes_coalesce_both_sources_and_resume_in_revision_order() {
    let root = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(root.path().to_path_buf());
    let catalog = manager.work_catalog().await.unwrap();
    let start = catalog.watermark().await.unwrap();
    let first = create(&manager, "first").await;
    let second = create(&manager, "second").await;
    let mut external = goose::session::work_catalog::ExternalWork {
        kind: "shell_job".into(),
        locator: "/delta".into(),
        title: "external".into(),
        working_dir: "/fixture".into(),
        state: "idle".into(),
        run_key: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
        event_kind: "created".into(),
        log_bytes: None,
    };
    catalog.upsert_external(&external).await.unwrap();
    for index in 0..10 {
        manager
            .update(&first.id)
            .user_provided_name(format!("first {index}"))
            .apply()
            .await
            .unwrap();
        external.title = format!("external {index}");
        catalog.upsert_external(&external).await.unwrap();
    }
    manager.delete_session(&second.id).await.unwrap();
    let expected = catalog.latest_changes(&start, 100).await.unwrap();
    assert_eq!(expected.records.len(), 3);
    assert!(expected.records.iter().any(|r| r.title == "first 9"));
    assert!(expected.records.iter().any(|r| r.title == "external 9"));
    assert!(expected.records.iter().any(|r| r.deleted));
    let mut cursor = start;
    let mut records = Vec::new();
    loop {
        let page = catalog.latest_changes(&cursor, 1).await.unwrap();
        assert_eq!(page.records.len(), 1);
        assert!(page.cursor.revision > cursor.revision);
        records.extend(page.records);
        cursor = page.cursor;
        if !page.has_more {
            break;
        }
    }
    assert_eq!(records, expected.records);
    assert_eq!(cursor, expected.cursor);
    let empty = catalog.latest_changes(&cursor, 1).await.unwrap();
    assert!(empty.records.is_empty());
    assert!(!empty.has_more);
    assert_eq!(empty.cursor, cursor);
}
