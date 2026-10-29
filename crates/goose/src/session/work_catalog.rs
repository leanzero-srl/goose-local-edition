//! Durable storage records for owner adapters. These are not controller wire DTOs.
//!
//! SQLite triggers cover every session writer, including older Goose processes. Catalog
//! versions are immutable. Message before-images preserve fixed-watermark history without
//! duplicating the existing transcript on migration. No versions are pruned yet.
//! A restored/cloned database cannot identify itself: the host must compare its generation
//! and high-water with the external registry and acquire an exclusive runtime lease.

mod external;
pub(crate) use external::create_objects as create_external_objects;
pub use external::ExternalWork;
pub(crate) use external::VERIFIED_LOG_APPEND;

use anyhow::{bail, Result};
use sqlx::{Pool, Sqlite, Transaction};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{watch, Notify};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct StoreWatermark {
    pub generation: String,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct CatalogRecord {
    pub revision: i64,
    pub work_id: String,
    pub kind: String,
    pub local_locator: String,
    pub title: String,
    pub working_dir: String,
    pub session_type: String,
    pub state: String,
    pub current_run_id: Option<String>,
    pub archived: bool,
    pub updated_at: String,
    pub deleted: bool,
    pub event_kind: String,
}

const CONTROLLER_WORK_KINDS: &[&str] = &["agent_work", "benchmark", "shell_job"];
const CONTROLLER_CHAT_TYPES: &[&str] = &["user", "scheduled", "acp"];

impl CatalogRecord {
    pub fn is_controller_visible(&self) -> bool {
        !self.deleted
            && (CONTROLLER_WORK_KINDS.contains(&self.kind.as_str())
                || self.kind == "chat"
                    && CONTROLLER_CHAT_TYPES.contains(&self.session_type.as_str()))
    }
}

static CATALOG_PAGE_SQL: LazyLock<String> = LazyLock::new(|| {
    let branch = |table| {
        format!(
        "SELECT v.* FROM {table} v WHERE v.work_id > ?2 AND v.revision = \
         (SELECT MAX(revision) FROM ( \
           SELECT MAX(revision) AS revision FROM catalog_versions WHERE work_id = v.work_id AND revision <= ?1 \
           UNION ALL \
           SELECT MAX(revision) AS revision FROM external_catalog_versions WHERE work_id = v.work_id AND revision <= ?1))"
    )
    };
    // Resolve the latest version across both sources before visibility, so an older visible
    // version cannot reappear after a hide/delete. The same policy powers the boundary check.
    format!(
        "SELECT * FROM ({} UNION ALL {}) \
         WHERE (?4 = 0 OR (deleted = 0 AND (kind IN ('{}') OR (kind = 'chat' AND session_type IN ('{}'))))) \
         ORDER BY work_id LIMIT ?3",
        branch("catalog_versions"), branch("external_catalog_versions"),
        CONTROLLER_WORK_KINDS.join("','"), CONTROLLER_CHAT_TYPES.join("','")
    )
});

const LATEST_CHANGES_SQL: &str =
    "WITH changed AS ( \
       SELECT revision, work_id FROM catalog_versions WHERE revision > ?1 AND revision <= ?2 \
       UNION ALL \
       SELECT revision, work_id FROM external_catalog_versions WHERE revision > ?1 AND revision <= ?2 \
     ), latest AS (SELECT MAX(revision) AS revision FROM changed GROUP BY work_id ORDER BY revision LIMIT ?3) \
     SELECT v.* FROM latest JOIN catalog_versions v USING (revision) \
     UNION ALL \
     SELECT v.* FROM latest JOIN external_catalog_versions v USING (revision) ORDER BY revision";

// Both branches walk ordering indexes; SQLite merges them and stops at the page limit.
const HISTORY_BEFORE_SQL: &str =
    "SELECT m.id AS row_id, r.message_id, r.valid_from AS revision, m.role, m.content_json, m.created_timestamp, m.metadata_json \
     FROM messages m INDEXED BY controller_messages_order CROSS JOIN controller_message_rows r \
     WHERE m.session_id = ?1 AND (m.created_timestamp, m.id) < (?4, ?5) \
       AND r.row_id = m.id AND r.work_id = ?2 AND r.valid_from <= ?3 \
     UNION ALL \
     SELECT row_id, message_id, valid_from AS revision, role, content_json, created_timestamp, metadata_json \
     FROM controller_message_versions INDEXED BY controller_message_versions_order \
     WHERE work_id = ?2 AND (created_timestamp, row_id) < (?4, ?5) AND valid_from <= ?3 AND valid_to >= ?3 \
     ORDER BY created_timestamp DESC, row_id DESC LIMIT ?6";

static HISTORY_LATEST_SQL: LazyLock<String> = LazyLock::new(|| {
    HISTORY_BEFORE_SQL
        .replace(" AND (m.created_timestamp, m.id) < (?4, ?5)", "")
        .replace(" AND (created_timestamp, row_id) < (?4, ?5)", "")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPageCursor {
    pub watermark: StoreWatermark,
    pub after_work_id: String,
}

#[derive(Debug)]
pub struct CatalogPage {
    pub watermark: StoreWatermark,
    pub records: Vec<CatalogRecord>,
    pub next: Option<CatalogPageCursor>,
}

#[derive(Debug)]
pub struct CatalogChanges {
    pub cursor: StoreWatermark,
    pub records: Vec<CatalogRecord>,
    pub has_more: bool,
}

/// Raw persisted transcript bytes. The transport adapter must enforce user-visible message
/// and content policies; these records must never be serialized directly to a controller.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct HistoryRecord {
    pub row_id: i64,
    pub message_id: String,
    pub revision: i64,
    pub role: String,
    pub content_json: String,
    pub created_timestamp: i64,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPageCursor {
    pub watermark: StoreWatermark,
    pub work_id: String,
    pub after_timestamp: i64,
    pub after_row_id: i64,
}

#[derive(Debug)]
pub struct HistoryPage {
    pub watermark: StoreWatermark,
    pub records: Vec<HistoryRecord>,
    pub next: Option<HistoryPageCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryBeforeCursor {
    pub before_timestamp: i64,
    pub before_row_id: i64,
}

#[derive(Debug)]
pub struct HistoryBeforePage {
    pub records: Vec<HistoryRecord>,
    pub next: Option<HistoryBeforeCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct WorkEventRecord {
    pub revision: i64,
    pub work_id: String,
    pub event_kind: String,
    pub message_id: Option<String>,
    pub row_id: Option<i64>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error("store_generation_mismatch: snapshot required")]
    GenerationMismatch,
    #[error("cursor_ahead: local catalog is behind the supplied watermark")]
    CursorAhead,
    #[error("work_not_found")]
    WorkNotFound,
    #[error("invalid_catalog_cursor")]
    InvalidCursor,
    #[error("invalid_page_size")]
    InvalidPageSize,
    #[error("store_rollback: local revision is below the registered high-water")]
    RollbackDetected,
}

#[derive(Clone)]
pub struct WorkCatalog {
    pool: Pool<Sqlite>,
    path: PathBuf,
}

impl WorkCatalog {
    pub(crate) fn new(pool: Pool<Sqlite>, path: PathBuf) -> Self {
        Self { pool, path }
    }

    pub async fn watermark(&self) -> Result<StoreWatermark> {
        Ok(
            sqlx::query_as("SELECT generation, revision FROM controller_store WHERE singleton = 1")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn check_external_fence(
        &self,
        registered_generation: &str,
        registered_revision: i64,
    ) -> Result<StoreWatermark> {
        let current = self.watermark().await?;
        if current.generation != registered_generation {
            bail!(CatalogError::GenerationMismatch);
        }
        if current.revision < registered_revision {
            bail!(CatalogError::RollbackDetected);
        }
        Ok(current)
    }

    /// An explicit administrative reset, never an automatic response to a failed fence.
    /// The caller must revoke its external lease and register the new generation before
    /// accepting commands. Work UUIDs and the monotonic local journal are preserved.
    pub async fn reset_generation(
        &self,
        expected_generation: &str,
        reason: &str,
    ) -> Result<StoreWatermark> {
        if reason.trim().is_empty() {
            bail!("generation reset requires a reason");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = read_watermark(&mut tx).await?;
        if current.generation != expected_generation {
            bail!(CatalogError::GenerationMismatch);
        }
        let generation = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO controller_generation_resets (previous_generation, generation, revision, reason) VALUES (?, ?, ?, ?)")
            .bind(&current.generation).bind(&generation).bind(current.revision).bind(reason)
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE controller_store SET generation = ? WHERE singleton = 1")
            .bind(&generation)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        notify_commit(&self.path);
        Ok(StoreWatermark {
            generation,
            revision: current.revision,
        })
    }

    pub async fn catalog_page(
        &self,
        cursor: Option<&CatalogPageCursor>,
        page_size: usize,
    ) -> Result<CatalogPage> {
        self.catalog_page_projected(cursor, page_size, false).await
    }

    pub async fn controller_catalog_page(
        &self,
        cursor: Option<&CatalogPageCursor>,
        page_size: usize,
    ) -> Result<CatalogPage> {
        self.catalog_page_projected(cursor, page_size, true).await
    }

    async fn catalog_page_projected(
        &self,
        cursor: Option<&CatalogPageCursor>,
        page_size: usize,
        controller_visible: bool,
    ) -> Result<CatalogPage> {
        let limit = page_limit(page_size)?;
        let mut tx = self.pool.begin().await?;
        let current = read_watermark(&mut tx).await?;
        let (watermark, after) = match cursor {
            Some(cursor) => {
                validate_cursor(&current, &cursor.watermark)?;
                (cursor.watermark.clone(), cursor.after_work_id.as_str())
            }
            None => (current, ""),
        };
        let mut records = sqlx::query_as::<_, CatalogRecord>(&CATALOG_PAGE_SQL)
            .bind(watermark.revision)
            .bind(after)
            .bind(limit)
            .bind(controller_visible)
            .fetch_all(&mut *tx)
            .await?;
        let has_more = records.len() > page_size;
        records.truncate(page_size);
        let next = if has_more {
            records.last().map(|last| CatalogPageCursor {
                watermark: watermark.clone(),
                after_work_id: last.work_id.clone(),
            })
        } else {
            None
        };
        tx.commit().await?;
        Ok(CatalogPage {
            watermark,
            records,
            next,
        })
    }

    pub async fn changes(
        &self,
        cursor: &StoreWatermark,
        page_size: usize,
    ) -> Result<CatalogChanges> {
        let limit = page_limit(page_size)?;
        let mut tx = self.pool.begin().await?;
        let current = read_watermark(&mut tx).await?;
        validate_cursor(&current, cursor)?;
        let mut records = sqlx::query_as::<_, CatalogRecord>(
            "SELECT * FROM (SELECT * FROM catalog_versions UNION ALL SELECT * FROM external_catalog_versions) WHERE revision > ? AND revision <= ? ORDER BY revision LIMIT ?")
            .bind(cursor.revision).bind(current.revision).bind(limit).fetch_all(&mut *tx).await?;
        let has_more = records.len() > page_size;
        records.truncate(page_size);
        let revision = if has_more {
            records.last().expect("nonempty page").revision
        } else {
            current.revision
        };
        tx.commit().await?;
        Ok(CatalogChanges {
            cursor: StoreWatermark {
                generation: current.generation,
                revision,
            },
            records,
            has_more,
        })
    }

    pub async fn latest_changes(
        &self,
        cursor: &StoreWatermark,
        page_size: usize,
    ) -> Result<CatalogChanges> {
        let limit = page_limit(page_size)?;
        let mut tx = self.pool.begin().await?;
        let current = read_watermark(&mut tx).await?;
        validate_cursor(&current, cursor)?;
        let mut records = sqlx::query_as::<_, CatalogRecord>(LATEST_CHANGES_SQL)
            .bind(cursor.revision)
            .bind(current.revision)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await?;
        let has_more = records.len() > page_size;
        records.truncate(page_size);
        let revision = if has_more {
            records.last().expect("nonempty page").revision
        } else {
            current.revision
        };
        tx.commit().await?;
        Ok(CatalogChanges {
            cursor: StoreWatermark {
                generation: current.generation,
                revision,
            },
            records,
            has_more,
        })
    }

    pub async fn lookup(&self, work_id: &str, generation: &str) -> Result<CatalogRecord> {
        let mut tx = self.pool.begin().await?;
        let current = read_watermark(&mut tx).await?;
        require_generation(&current, generation)?;
        let record = lookup_at(&mut tx, work_id, current.revision).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Read the immutable catalog version used by a snapshot or journal event.
    pub async fn lookup_at(
        &self,
        work_id: &str,
        watermark: &StoreWatermark,
    ) -> Result<CatalogRecord> {
        let mut tx = self.pool.begin().await?;
        validate_cursor(&read_watermark(&mut tx).await?, watermark)?;
        let record = lookup_at(&mut tx, work_id, watermark.revision).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Fetch one journal message at its event revision without replaying the transcript.
    pub async fn message_at(
        &self,
        work_id: &str,
        watermark: &StoreWatermark,
        row_id: i64,
    ) -> Result<Option<HistoryRecord>> {
        let mut tx = self.pool.begin().await?;
        validate_cursor(&read_watermark(&mut tx).await?, watermark)?;
        lookup_at(&mut tx, work_id, watermark.revision).await?;
        let record = sqlx::query_as::<_, HistoryRecord>(
            "SELECT r.row_id, r.message_id, r.valid_from AS revision, m.role, m.content_json, m.created_timestamp, m.metadata_json \
             FROM controller_message_rows r JOIN messages m ON m.id = r.row_id \
             WHERE r.row_id = ? AND r.work_id = ? AND r.valid_from <= ? \
             UNION ALL \
             SELECT row_id, message_id, valid_from AS revision, role, content_json, created_timestamp, metadata_json \
             FROM controller_message_versions WHERE row_id = ? AND work_id = ? AND valid_from <= ? AND valid_to >= ?")
            .bind(row_id).bind(work_id).bind(watermark.revision)
            .bind(row_id).bind(work_id).bind(watermark.revision).bind(watermark.revision)
            .fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        Ok(record)
    }

    pub async fn lookup_local_session(&self, local_id: &str) -> Result<CatalogRecord> {
        Ok(sqlx::query_as(
            "SELECT v.* FROM work_identity i JOIN catalog_versions v ON v.revision = i.revision \
             WHERE i.local_locator = ? AND i.deleted = 0",
        )
        .bind(local_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(CatalogError::WorkNotFound)?)
    }

    pub async fn history_page(
        &self,
        work_id: &str,
        generation: &str,
        cursor: Option<&HistoryPageCursor>,
        page_size: usize,
    ) -> Result<HistoryPage> {
        let limit = page_limit(page_size)?;
        let mut tx = self.pool.begin().await?;
        let current = read_watermark(&mut tx).await?;
        require_generation(&current, generation)?;
        let (watermark, after_timestamp, after_row_id) = match cursor {
            Some(cursor) => {
                validate_cursor(&current, &cursor.watermark)?;
                if cursor.work_id != work_id {
                    bail!(CatalogError::InvalidCursor);
                }
                (
                    cursor.watermark.clone(),
                    cursor.after_timestamp,
                    cursor.after_row_id,
                )
            }
            None => (current, i64::MIN, i64::MIN),
        };
        lookup_at(&mut tx, work_id, watermark.revision).await?;
        let mut records = sqlx::query_as::<_, HistoryRecord>(
            "SELECT * FROM ( \
               SELECT r.row_id, r.message_id, r.valid_from AS revision, m.role, m.content_json, m.created_timestamp, m.metadata_json \
               FROM controller_message_rows r JOIN messages m ON m.id = r.row_id \
               WHERE r.work_id = ? AND r.valid_from <= ? \
               UNION ALL \
               SELECT row_id, message_id, valid_from AS revision, role, content_json, created_timestamp, metadata_json \
               FROM controller_message_versions WHERE work_id = ? AND valid_from <= ? AND valid_to >= ? \
             ) WHERE (created_timestamp, row_id) > (?, ?) ORDER BY created_timestamp, row_id LIMIT ?")
            .bind(work_id).bind(watermark.revision).bind(work_id).bind(watermark.revision).bind(watermark.revision)
            .bind(after_timestamp).bind(after_row_id).bind(limit).fetch_all(&mut *tx).await?;
        let has_more = records.len() > page_size;
        records.truncate(page_size);
        let next = if has_more {
            records.last().map(|last| HistoryPageCursor {
                watermark: watermark.clone(),
                work_id: work_id.to_owned(),
                after_timestamp: last.created_timestamp,
                after_row_id: last.row_id,
            })
        } else {
            None
        };
        tx.commit().await?;
        Ok(HistoryPage {
            watermark,
            records,
            next,
        })
    }

    pub async fn history_before(
        &self,
        work_id: &str,
        watermark: &StoreWatermark,
        before: Option<&HistoryBeforeCursor>,
        page_size: usize,
    ) -> Result<HistoryBeforePage> {
        let limit = page_limit(page_size)?;
        let mut tx = self.pool.begin().await?;
        validate_cursor(&read_watermark(&mut tx).await?, watermark)?;
        let work = lookup_at(&mut tx, work_id, watermark.revision).await?;
        let query = if before.is_some() {
            HISTORY_BEFORE_SQL
        } else {
            HISTORY_LATEST_SQL.as_str()
        };
        let mut records = sqlx::query_as::<_, HistoryRecord>(query)
            .bind(&work.local_locator)
            .bind(work_id)
            .bind(watermark.revision)
            .bind(before.map(|cursor| cursor.before_timestamp))
            .bind(before.map(|cursor| cursor.before_row_id))
            .bind(limit)
            .fetch_all(&mut *tx)
            .await?;
        let has_more = records.len() > page_size;
        records.truncate(page_size);
        let next = if has_more {
            records.last().map(|record| HistoryBeforeCursor {
                before_timestamp: record.created_timestamp,
                before_row_id: record.row_id,
            })
        } else {
            None
        };
        records.reverse();
        tx.commit().await?;
        Ok(HistoryBeforePage { records, next })
    }

    pub async fn work_events(
        &self,
        work_id: &str,
        cursor: &StoreWatermark,
        page_size: usize,
    ) -> Result<Vec<WorkEventRecord>> {
        let limit = page_limit(page_size)? - 1;
        let mut tx = self.pool.begin().await?;
        let current = read_watermark(&mut tx).await?;
        validate_cursor(&current, cursor)?;
        lookup_at(&mut tx, work_id, current.revision).await?;
        let records = sqlx::query_as(
            "SELECT * FROM (SELECT revision, work_id, event_kind, message_id, row_id FROM work_events UNION ALL SELECT revision, work_id, event_kind, NULL AS message_id, NULL AS row_id FROM external_catalog_versions) WHERE work_id = ? AND revision > ? AND revision <= ? ORDER BY revision LIMIT ?")
            .bind(work_id).bind(cursor.revision).bind(current.revision).bind(limit).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(records)
    }

    pub async fn record_runtime(
        &self,
        work_id: &str,
        generation: &str,
        state: &str,
        run_id: Option<Uuid>,
    ) -> Result<CatalogRecord> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_generation(&read_watermark(&mut tx).await?, generation)?;
        let changed = sqlx::query(
            "UPDATE work_identity SET state = ?, current_run_id = ?, event_kind = 'run_state', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE work_id = ? AND deleted = 0")
            .bind(state).bind(run_id.map(|id| id.to_string())).bind(work_id).execute(&mut *tx).await?;
        if changed.rows_affected() == 0 {
            bail!(CatalogError::WorkNotFound);
        }
        let revision = read_watermark(&mut tx).await?.revision;
        let record = lookup_at(&mut tx, work_id, revision).await?;
        tx.commit().await?;
        notify_commit(&self.path);
        Ok(record)
    }

    /// One process-wide, indexed journal observer per database. Polling detects commits from
    /// older processes; local writers wake the same observer immediately after commit.
    pub async fn subscribe(&self, poll_interval: Duration) -> Result<CatalogSubscription> {
        if poll_interval.is_zero() {
            bail!("journal polling interval must be positive");
        }
        let initial = self.watermark().await?;
        let path = std::fs::canonicalize(&self.path)?;
        let mut watchers = WATCHERS
            .lock()
            .map_err(|_| anyhow::anyhow!("catalog watcher registry poisoned"))?;
        let watcher = match watchers.get(&path).and_then(Weak::upgrade) {
            Some(watcher) => watcher,
            None => {
                let (sender, _) = watch::channel(Ok(initial));
                let watcher = Arc::new(JournalWatcher {
                    sender,
                    notify: Arc::new(Notify::new()),
                });
                watchers.insert(path, Arc::downgrade(&watcher));
                let weak = Arc::downgrade(&watcher);
                let pool = self.pool.clone();
                let notify = watcher.notify.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::select! { _ = tokio::time::sleep(poll_interval) => {}, _ = notify.notified() => {} }
                        let Some(watcher) = weak.upgrade() else {
                            break;
                        };
                        let next = sqlx::query_as(
                            "SELECT generation, revision FROM controller_store WHERE singleton = 1",
                        )
                        .fetch_one(&pool)
                        .await
                        .map_err(|error| error.to_string());
                        watcher.sender.send_if_modified(|previous| {
                            if *previous == next {
                                false
                            } else {
                                *previous = next;
                                true
                            }
                        });
                    }
                });
                watcher
            }
        };
        Ok(CatalogSubscription {
            receiver: watcher.sender.subscribe(),
            _watcher: watcher,
        })
    }
}

async fn read_watermark(tx: &mut Transaction<'_, Sqlite>) -> Result<StoreWatermark> {
    Ok(
        sqlx::query_as("SELECT generation, revision FROM controller_store WHERE singleton = 1")
            .fetch_one(&mut **tx)
            .await?,
    )
}

fn require_generation(current: &StoreWatermark, generation: &str) -> Result<()> {
    if current.generation != generation {
        bail!(CatalogError::GenerationMismatch);
    }
    Ok(())
}

fn validate_cursor(current: &StoreWatermark, cursor: &StoreWatermark) -> Result<()> {
    require_generation(current, &cursor.generation)?;
    if cursor.revision < 0 {
        bail!(CatalogError::InvalidCursor);
    }
    if cursor.revision > current.revision {
        bail!(CatalogError::CursorAhead);
    }
    Ok(())
}

fn page_limit(size: usize) -> Result<i64> {
    if size == 0 {
        bail!(CatalogError::InvalidPageSize);
    }
    i64::try_from(size)
        .ok()
        .and_then(|size| size.checked_add(1))
        .ok_or_else(|| CatalogError::InvalidPageSize.into())
}

async fn lookup_at(
    tx: &mut Transaction<'_, Sqlite>,
    work_id: &str,
    revision: i64,
) -> Result<CatalogRecord> {
    Ok(sqlx::query_as("SELECT * FROM (SELECT * FROM catalog_versions UNION ALL SELECT * FROM external_catalog_versions) WHERE work_id = ? AND revision <= ? ORDER BY revision DESC LIMIT 1")
        .bind(work_id).bind(revision).fetch_optional(&mut **tx).await?.ok_or(CatalogError::WorkNotFound)?)
}

type JournalNotice = std::result::Result<StoreWatermark, String>;
struct JournalWatcher {
    sender: watch::Sender<JournalNotice>,
    notify: Arc<Notify>,
}
static WATCHERS: LazyLock<Mutex<HashMap<PathBuf, Weak<JournalWatcher>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct CatalogSubscription {
    receiver: watch::Receiver<JournalNotice>,
    _watcher: Arc<JournalWatcher>,
}

impl CatalogSubscription {
    pub fn current(&self) -> JournalNotice {
        self.receiver.borrow().clone()
    }

    pub async fn changed(&mut self) -> Result<StoreWatermark> {
        self.receiver.changed().await?;
        self.receiver
            .borrow_and_update()
            .clone()
            .map_err(anyhow::Error::msg)
    }
}

pub(crate) fn notify_commit(path: &Path) {
    // Notification is a latency optimization. The durable journal remains the authority and
    // the observer's poll also sees commits made by processes without this notification.
    if let Ok(path) = std::fs::canonicalize(path) {
        if let Ok(watchers) = WATCHERS.lock() {
            if let Some(watcher) = watchers.get(&path).and_then(Weak::upgrade) {
                watcher.notify.notify_one();
            }
        }
    }
}

pub(crate) async fn create_objects(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    external::create_objects(tx).await?;
    let uuid_sql = "lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-' || substr('89ab', 1 + (random() & 3), 1) || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6)))";
    for statement in [
        "CREATE TABLE IF NOT EXISTS controller_store (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), generation TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision >= 0))",
        "CREATE TABLE IF NOT EXISTS controller_generation_resets (id INTEGER PRIMARY KEY, previous_generation TEXT NOT NULL, generation TEXT NOT NULL UNIQUE, revision INTEGER NOT NULL, reason TEXT NOT NULL, reset_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')))",
        "CREATE TABLE IF NOT EXISTS work_identity (work_id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('chat','agent_work')), local_locator TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 0, title TEXT NOT NULL, working_dir TEXT NOT NULL, session_type TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'idle' CHECK(state IN ('idle','running','waiting_input','paused','finished','failed','interrupted','deleted')), current_run_id TEXT, archived INTEGER NOT NULL, updated_at TEXT NOT NULL, deleted INTEGER NOT NULL DEFAULT 0 CHECK(deleted IN (0,1)), event_kind TEXT NOT NULL)",
        "CREATE UNIQUE INDEX IF NOT EXISTS work_active_locator ON work_identity(local_locator) WHERE deleted = 0",
        "CREATE TABLE IF NOT EXISTS catalog_versions (revision INTEGER PRIMARY KEY, work_id TEXT NOT NULL REFERENCES work_identity(work_id), kind TEXT NOT NULL, local_locator TEXT NOT NULL, title TEXT NOT NULL, working_dir TEXT NOT NULL, session_type TEXT NOT NULL, state TEXT NOT NULL, current_run_id TEXT, archived INTEGER NOT NULL, updated_at TEXT NOT NULL, deleted INTEGER NOT NULL, event_kind TEXT NOT NULL)",
        "CREATE INDEX IF NOT EXISTS catalog_versions_work ON catalog_versions(work_id, revision DESC)",
        "CREATE TABLE IF NOT EXISTS work_events (revision INTEGER PRIMARY KEY REFERENCES catalog_versions(revision), work_id TEXT NOT NULL REFERENCES work_identity(work_id), event_kind TEXT NOT NULL, message_id TEXT, row_id INTEGER)",
        "CREATE INDEX IF NOT EXISTS work_events_work ON work_events(work_id, revision)",
        "CREATE TABLE IF NOT EXISTS controller_message_rows (row_id INTEGER PRIMARY KEY, work_id TEXT NOT NULL REFERENCES work_identity(work_id), message_id TEXT NOT NULL, valid_from INTEGER NOT NULL)",
        "CREATE INDEX IF NOT EXISTS controller_message_rows_work ON controller_message_rows(work_id, valid_from)",
        "CREATE TABLE IF NOT EXISTS controller_message_versions (row_id INTEGER NOT NULL, work_id TEXT NOT NULL REFERENCES work_identity(work_id), message_id TEXT NOT NULL, valid_from INTEGER NOT NULL, valid_to INTEGER NOT NULL, role TEXT NOT NULL, content_json TEXT NOT NULL, created_timestamp INTEGER NOT NULL, metadata_json TEXT, PRIMARY KEY(row_id, valid_from))",
        "CREATE INDEX IF NOT EXISTS controller_message_versions_work ON controller_message_versions(work_id, valid_to, valid_from)",
    ] { sqlx::query(statement).execute(&mut **tx).await?; }
    sqlx::query(
        "INSERT OR IGNORE INTO controller_store (singleton, generation, revision) VALUES (1, ?, 0)",
    )
    .bind(Uuid::new_v4().to_string())
    .execute(&mut **tx)
    .await?;

    let columns = "revision, work_id, kind, local_locator, title, working_dir, session_type, state, current_run_id, archived, updated_at, deleted, event_kind";
    let journal = format!(
        "UPDATE controller_store SET revision = revision + 1 WHERE singleton = 1; \
         UPDATE work_identity SET revision = (SELECT revision FROM controller_store WHERE singleton = 1) WHERE work_id = NEW.work_id; \
         INSERT INTO catalog_versions ({columns}) SELECT {columns} FROM work_identity WHERE work_id = NEW.work_id; \
         INSERT INTO work_events (revision, work_id, event_kind) SELECT revision, work_id, event_kind FROM work_identity WHERE work_id = NEW.work_id;");
    for statement in [
        format!("CREATE TRIGGER IF NOT EXISTS work_identity_insert AFTER INSERT ON work_identity BEGIN {journal} END"),
        format!("CREATE TRIGGER IF NOT EXISTS work_identity_update AFTER UPDATE OF title, working_dir, session_type, state, current_run_id, archived, updated_at, deleted, event_kind ON work_identity BEGIN {journal} END"),
        "CREATE TRIGGER IF NOT EXISTS work_identity_immutable BEFORE UPDATE OF work_id, kind, local_locator ON work_identity BEGIN SELECT RAISE(ABORT, 'work identity is immutable'); END".to_owned(),
        "CREATE TRIGGER IF NOT EXISTS work_identity_no_delete BEFORE DELETE ON work_identity BEGIN SELECT RAISE(ABORT, 'work tombstones must be retained'); END".to_owned(),
        "CREATE TRIGGER IF NOT EXISTS controller_session_locator_immutable BEFORE UPDATE OF id ON sessions WHEN OLD.id IS NOT NEW.id BEGIN SELECT RAISE(ABORT, 'session locator is immutable'); END".to_owned(),
        format!("CREATE TRIGGER IF NOT EXISTS controller_session_insert AFTER INSERT ON sessions BEGIN \
          INSERT INTO work_identity (work_id, kind, local_locator, title, working_dir, session_type, archived, updated_at, event_kind) \
          VALUES ({uuid_sql}, 'chat', NEW.id, NEW.name, NEW.working_dir, NEW.session_type, NEW.archived_at IS NOT NULL, strftime('%Y-%m-%dT%H:%M:%fZ', NEW.updated_at), 'created'); END"),
        "CREATE TRIGGER IF NOT EXISTS controller_session_update AFTER UPDATE ON sessions BEGIN \
          UPDATE work_identity SET title = NEW.name, working_dir = NEW.working_dir, session_type = NEW.session_type, archived = NEW.archived_at IS NOT NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', NEW.updated_at), event_kind = 'session_changed' WHERE local_locator = NEW.id AND deleted = 0; END".to_owned(),
        "CREATE TRIGGER IF NOT EXISTS controller_session_delete AFTER DELETE ON sessions BEGIN \
          UPDATE work_identity SET deleted = 1, state = 'deleted', current_run_id = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), event_kind = 'deleted' WHERE local_locator = OLD.id AND deleted = 0; END".to_owned(),
        "CREATE TRIGGER IF NOT EXISTS controller_message_locator_immutable BEFORE UPDATE OF id, session_id ON messages WHEN OLD.id IS NOT NEW.id OR OLD.session_id IS NOT NEW.session_id BEGIN SELECT RAISE(ABORT, 'message locator is immutable'); END".to_owned(),
    ] { sqlx::query(&statement).execute(&mut **tx).await?; }

    sqlx::query(&format!(
        "INSERT INTO work_identity (work_id, kind, local_locator, title, working_dir, session_type, archived, updated_at, event_kind) \
         SELECT {uuid_sql}, 'chat', s.id, s.name, s.working_dir, s.session_type, s.archived_at IS NOT NULL, strftime('%Y-%m-%dT%H:%M:%fZ',s.updated_at), 'created' \
         FROM sessions s WHERE NOT EXISTS (SELECT 1 FROM work_identity w WHERE w.local_locator = s.id AND w.deleted = 0)"))
        .execute(&mut **tx).await?;
    sqlx::query(
        "INSERT OR IGNORE INTO controller_message_rows (row_id, work_id, message_id, valid_from) \
         SELECT m.id, w.work_id, COALESCE(m.message_id, 'row_' || m.id), w.revision FROM messages m \
         JOIN work_identity w ON w.local_locator = m.session_id AND w.deleted = 0")
        .execute(&mut **tx).await?;

    for (operation, row, event) in [
        ("INSERT", "NEW", "message_upsert"),
        ("UPDATE", "NEW", "message_upsert"),
        ("DELETE", "OLD", "message_deleted"),
    ] {
        let preserve = if operation == "INSERT" {
            String::new()
        } else {
            "INSERT INTO controller_message_versions (row_id, work_id, message_id, valid_from, valid_to, role, content_json, created_timestamp, metadata_json) \
             SELECT r.row_id, r.work_id, r.message_id, r.valid_from, (SELECT revision - 1 FROM controller_store WHERE singleton = 1), OLD.role, OLD.content_json, OLD.created_timestamp, OLD.metadata_json \
             FROM controller_message_rows r WHERE r.row_id = OLD.id; \
             DELETE FROM controller_message_rows WHERE row_id = OLD.id;".to_owned()
        };
        let current = if operation == "DELETE" {
            String::new()
        } else {
            "INSERT INTO controller_message_rows (row_id, work_id, message_id, valid_from) \
             SELECT NEW.id, work_id, COALESCE(NEW.message_id, 'row_' || NEW.id), revision FROM work_identity WHERE local_locator = NEW.session_id AND deleted = 0;".to_owned()
        };
        sqlx::query(&format!(
            "CREATE TRIGGER IF NOT EXISTS controller_message_{} AFTER {operation} ON messages BEGIN \
             UPDATE work_identity SET event_kind = '{event}', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE local_locator = {row}.session_id AND deleted = 0; \
             {preserve} {current} \
             UPDATE work_events SET message_id = COALESCE({row}.message_id, 'row_' || {row}.id), row_id = {row}.id WHERE revision = (SELECT revision FROM controller_store WHERE singleton = 1); END", operation.to_lowercase()))
            .execute(&mut **tx).await?;
    }
    create_history_indexes(tx).await?;
    Ok(())
}

pub(crate) async fn create_history_indexes(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    for statement in [
        "CREATE INDEX IF NOT EXISTS controller_messages_order ON messages(session_id, created_timestamp, id)",
        "CREATE INDEX IF NOT EXISTS controller_message_versions_order ON controller_message_versions(work_id, created_timestamp, row_id, valid_from, valid_to)",
    ] {
        sqlx::query(statement).execute(&mut **tx).await?;
    }
    Ok(())
}

#[cfg(test)]
mod query_plan_tests {
    use super::*;
    use sqlx::Row;

    #[tokio::test]
    async fn history_before_uses_ordered_seeks_without_sorting_the_transcript() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        for statement in [
            "CREATE TABLE messages(id INTEGER PRIMARY KEY,session_id TEXT,role TEXT,content_json TEXT,created_timestamp INTEGER,metadata_json TEXT)",
            "CREATE TABLE controller_message_rows(row_id INTEGER PRIMARY KEY,work_id TEXT,message_id TEXT,valid_from INTEGER)",
            "CREATE TABLE controller_message_versions(row_id INTEGER,work_id TEXT,message_id TEXT,valid_from INTEGER,valid_to INTEGER,role TEXT,content_json TEXT,created_timestamp INTEGER,metadata_json TEXT)",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        let mut tx = pool.begin().await.unwrap();
        create_history_indexes(&mut tx).await.unwrap();
        tx.commit().await.unwrap();
        let rows = sqlx::query(&format!("EXPLAIN QUERY PLAN {HISTORY_BEFORE_SQL}"))
            .bind("session")
            .bind("work")
            .bind(1000_i64)
            .bind(500_i64)
            .bind(500_i64)
            .bind(3_i64)
            .fetch_all(&pool)
            .await
            .unwrap();
        let plan = rows
            .into_iter()
            .map(|row| row.get::<String, _>("detail"))
            .collect::<Vec<_>>()
            .join("\n");
        eprintln!("Latest history query plan:\n{plan}");
        assert!(plan.contains("MERGE (UNION ALL)"), "{plan}");
        assert!(
            plan.contains("controller_messages_order (session_id=? AND created_timestamp<?)"),
            "{plan}"
        );
        assert!(
            plan.contains("controller_message_versions_order (work_id=? AND "),
            "{plan}"
        );
        assert!(
            plan.contains("SEARCH r USING INTEGER PRIMARY KEY (rowid=?)"),
            "{plan}"
        );
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
        assert!(!plan.contains("SCAN "), "{plan}");
    }

    #[tokio::test]
    async fn catalog_queries_use_bounded_indexes_on_bundled_sqlite() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        for (table, index) in [
            ("catalog_versions", "catalog_versions_work"),
            ("external_catalog_versions", "external_versions_work"),
        ] {
            sqlx::query(&format!("CREATE TABLE {table}(revision INTEGER PRIMARY KEY, work_id TEXT, deleted INTEGER, kind TEXT, session_type TEXT)"))
                .execute(&pool).await.unwrap();
            sqlx::query(&format!(
                "CREATE INDEX {index} ON {table}(work_id, revision DESC)"
            ))
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(&format!("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000) INSERT INTO {table} SELECT x,printf('%06d', x/10),0,'chat','hidden' FROM n"))
                .execute(&pool).await.unwrap();
        }
        let version: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&pool)
            .await
            .unwrap();
        let catalog = sqlx::query(&format!("EXPLAIN QUERY PLAN {}", *CATALOG_PAGE_SQL))
            .bind(1000_i64)
            .bind("000050")
            .bind(3_i64)
            .bind(true)
            .fetch_all(&pool)
            .await
            .unwrap();
        let delta = sqlx::query(&format!("EXPLAIN QUERY PLAN {LATEST_CHANGES_SQL}"))
            .bind(999_i64)
            .bind(1000_i64)
            .bind(3_i64)
            .fetch_all(&pool)
            .await
            .unwrap();
        let details = |rows: Vec<sqlx::sqlite::SqliteRow>| {
            rows.into_iter()
                .map(|r| r.get::<String, _>("detail"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let catalog = details(catalog);
        let delta = details(delta);
        eprintln!("SQLite {version} catalog plan:\n{catalog}\ndelta plan:\n{delta}");
        for index in ["catalog_versions_work", "external_versions_work"] {
            assert!(
                catalog.contains(&format!("USING INDEX {index} (work_id>?)")),
                "{catalog}"
            );
            assert!(
                catalog.contains(&format!(
                    "USING COVERING INDEX {index} (work_id=? AND revision<?)"
                )),
                "{catalog}"
            );
        }
        for table in ["catalog_versions", "external_catalog_versions"] {
            assert!(
                delta.contains(&format!(
                    "SEARCH {table} USING INTEGER PRIMARY KEY (rowid>? AND rowid<?)"
                )),
                "{delta}"
            );
        }
        assert!(!catalog.contains("MATERIALIZE"), "{catalog}");
        assert!(!delta.contains("MATERIALIZE versions"), "{delta}");
        assert!(
            delta.contains("SEARCH v USING INTEGER PRIMARY KEY (rowid=?)"),
            "{delta}"
        );
    }
}
