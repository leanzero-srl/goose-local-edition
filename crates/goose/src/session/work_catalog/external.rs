use super::{notify_commit, read_watermark, CatalogRecord, WorkCatalog};
use anyhow::{ensure, Result};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ExternalWork {
    pub kind: String,
    pub locator: String,
    pub title: String,
    pub working_dir: String,
    pub state: String,
    pub run_key: Option<String>,
    pub updated_at: String,
    pub event_kind: String,
}

pub(crate) async fn create_objects(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS external_work_identity (work_id TEXT PRIMARY KEY, kind TEXT NOT NULL, local_locator TEXT NOT NULL, UNIQUE(kind,local_locator))",
        "CREATE TABLE IF NOT EXISTS external_work_runs (work_id TEXT NOT NULL REFERENCES external_work_identity(work_id), run_key TEXT NOT NULL, run_id TEXT NOT NULL UNIQUE, PRIMARY KEY(work_id,run_key))",
        "CREATE TABLE IF NOT EXISTS external_catalog_versions (revision INTEGER PRIMARY KEY, work_id TEXT NOT NULL REFERENCES external_work_identity(work_id), kind TEXT NOT NULL, local_locator TEXT NOT NULL, title TEXT NOT NULL, working_dir TEXT NOT NULL, session_type TEXT NOT NULL, state TEXT NOT NULL, current_run_id TEXT, archived INTEGER NOT NULL, updated_at TEXT NOT NULL, deleted INTEGER NOT NULL, event_kind TEXT NOT NULL)",
        "CREATE INDEX IF NOT EXISTS external_versions_work ON external_catalog_versions(work_id,revision DESC)",
    ] {
        sqlx::query(statement).execute(&mut **tx).await?;
    }
    Ok(())
}

impl WorkCatalog {
    pub async fn remove_missing_external(&self, kind: &str, present: &[String]) -> Result<()> {
        let records: Vec<CatalogRecord> = sqlx::query_as(
            "SELECT v.* FROM external_catalog_versions v WHERE v.kind=? AND v.deleted=0 AND v.revision=(SELECT MAX(revision) FROM external_catalog_versions WHERE work_id=v.work_id)")
            .bind(kind).fetch_all(&self.pool).await?;
        for record in records {
            if !present.contains(&record.local_locator) {
                self.upsert_external(&ExternalWork {
                    kind: record.kind,
                    locator: record.local_locator,
                    title: record.title,
                    working_dir: record.working_dir,
                    state: "deleted".into(),
                    run_key: None,
                    updated_at: chrono::Utc::now().to_rfc3339(),
                    event_kind: "deleted".into(),
                })
                .await?;
            }
        }
        Ok(())
    }

    /// Metadata changes alone advance the shared journal. Re-reading an unchanged job is read-only.
    pub async fn upsert_external(&self, work: &ExternalWork) -> Result<CatalogRecord> {
        ensure!(
            matches!(work.kind.as_str(), "agent_work" | "benchmark" | "shell_job"),
            "unsupported external work kind"
        );
        ensure!(!work.locator.is_empty(), "external work needs a locator");
        ensure!(
            matches!(
                work.state.as_str(),
                "idle"
                    | "running"
                    | "waiting_input"
                    | "paused"
                    | "finished"
                    | "failed"
                    | "interrupted"
                    | "deleted"
            ),
            "invalid external work state"
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let work_id: Option<String> = sqlx::query_scalar(
            "SELECT work_id FROM external_work_identity WHERE kind=? AND local_locator=?",
        )
        .bind(&work.kind)
        .bind(&work.locator)
        .fetch_optional(&mut *tx)
        .await?;
        let work_id = match work_id {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO external_work_identity(work_id,kind,local_locator) VALUES (?,?,?)",
                )
                .bind(&id)
                .bind(&work.kind)
                .bind(&work.locator)
                .execute(&mut *tx)
                .await?;
                id
            }
        };
        let current_run_id = if let Some(key) = &work.run_key {
            sqlx::query(
                "INSERT OR IGNORE INTO external_work_runs(work_id,run_key,run_id) VALUES (?,?,?)",
            )
            .bind(&work_id)
            .bind(key)
            .bind(Uuid::new_v4().to_string())
            .execute(&mut *tx)
            .await?;
            Some(
                sqlx::query_scalar::<_, String>(
                    "SELECT run_id FROM external_work_runs WHERE work_id=? AND run_key=?",
                )
                .bind(&work_id)
                .bind(key)
                .fetch_one(&mut *tx)
                .await?,
            )
        } else {
            None
        };
        let previous: Option<CatalogRecord> = sqlx::query_as("SELECT * FROM external_catalog_versions WHERE work_id=? ORDER BY revision DESC LIMIT 1")
            .bind(&work_id).fetch_optional(&mut *tx).await?;
        let mut record = CatalogRecord {
            revision: previous.as_ref().map_or(0, |p| p.revision),
            work_id,
            kind: work.kind.clone(),
            local_locator: work.locator.clone(),
            title: work.title.clone(),
            working_dir: work.working_dir.clone(),
            session_type: "external".into(),
            state: work.state.clone(),
            current_run_id,
            archived: false,
            updated_at: work.updated_at.clone(),
            deleted: work.state == "deleted",
            event_kind: work.event_kind.clone(),
        };
        if previous.as_ref() == Some(&record) {
            tx.commit().await?;
            return Ok(record);
        }
        sqlx::query("UPDATE controller_store SET revision=revision+1 WHERE singleton=1")
            .execute(&mut *tx)
            .await?;
        record.revision = read_watermark(&mut tx).await?.revision;
        sqlx::query("INSERT INTO external_catalog_versions(revision,work_id,kind,local_locator,title,working_dir,session_type,state,current_run_id,archived,updated_at,deleted,event_kind) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(record.revision).bind(&record.work_id).bind(&record.kind).bind(&record.local_locator).bind(&record.title)
            .bind(&record.working_dir).bind(&record.session_type).bind(&record.state).bind(&record.current_run_id)
            .bind(record.archived).bind(&record.updated_at).bind(record.deleted).bind(&record.event_kind).execute(&mut *tx).await?;
        tx.commit().await?;
        notify_commit(&self.path);
        Ok(record)
    }

    pub async fn external_run_key(&self, work_id: &str, run_id: &str) -> Result<String> {
        Ok(sqlx::query_scalar(
            "SELECT run_key FROM external_work_runs WHERE work_id=? AND run_id=?",
        )
        .bind(work_id)
        .bind(run_id)
        .fetch_one(&self.pool)
        .await?)
    }
}
