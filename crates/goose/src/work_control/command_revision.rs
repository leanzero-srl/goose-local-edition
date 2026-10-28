use super::{conflict, unavailable};
use crate::session::work_catalog::VERIFIED_LOG_APPEND;
use leanzero_link::controller::ControllerError;
use leanzero_link::controller_wire::{ActionPayload, WorkCommand, WorkKind};
use sqlx::SqliteConnection;

pub(super) async fn check_revision(
    connection: &mut SqliteConnection,
    command: &WorkCommand,
    current_revision: i64,
) -> Result<(), ControllerError> {
    if u64::try_from(current_revision).ok() == Some(command.expected_revision) {
        return Ok(());
    }
    let stale = || {
        conflict(
            "stale_revision",
            "work changed; refresh before issuing this command",
        )
    };
    let observed = i64::try_from(command.expected_revision).map_err(|_| stale())?;
    if observed >= current_revision {
        return Err(stale());
    }
    let (table, kind, event, external) = match (&command.work.kind, &command.action) {
        (WorkKind::Chat, ActionPayload::Steer { .. } | ActionPayload::Stop {}) => {
            ("catalog_versions", "chat", "partial_replacement", false)
        }
        (
            WorkKind::AgentWork,
            ActionPayload::Note { .. }
            | ActionPayload::Stop {}
            | ActionPayload::Pause {}
            | ActionPayload::Resume {}
            | ActionPayload::Tick {},
        ) => (
            "external_catalog_versions",
            "agent_work",
            VERIFIED_LOG_APPEND,
            true,
        ),
        (WorkKind::Benchmark, ActionPayload::Note { .. } | ActionPayload::Stop {}) => (
            "external_catalog_versions",
            "benchmark",
            VERIFIED_LOG_APPEND,
            true,
        ),
        (WorkKind::ShellJob, ActionPayload::Stop {}) => (
            "external_catalog_versions",
            "shell_job",
            VERIFIED_LOG_APPEND,
            true,
        ),
        _ => return Err(stale()),
    };
    let run = command.expected_run_id.ok_or_else(stale)?.to_string();
    // Only proved output can bridge a control snapshot: chat chunks need their durable
    // payload records; external appends need the writer's metadata-and-growth attestation.
    let sql = format!("SELECT EXISTS (
            SELECT 1 FROM {table} observed
            JOIN {table} current ON current.work_id=observed.work_id AND current.revision=?3
            WHERE observed.work_id=?1 AND observed.revision=?2
              AND observed.kind=?5 AND observed.deleted=0 AND observed.current_run_id=?4
              AND current.current_run_id=?4
              AND EXISTS (SELECT 1 FROM {table} v
                          WHERE v.work_id=observed.work_id AND v.revision>?2 AND v.revision<=?3)
              AND NOT EXISTS (
                SELECT 1 FROM {table} v
                LEFT JOIN work_events e ON e.revision=v.revision AND e.work_id=v.work_id
                LEFT JOIN owner_partial_events p ON p.revision=v.revision AND p.work_id=v.work_id
                WHERE v.work_id=observed.work_id AND v.revision>?2 AND v.revision<=?3 AND (
                    v.event_kind IS NOT ?7
                    OR (NOT ?6 AND (e.revision IS NULL OR p.revision IS NULL
                        OR e.event_kind IS NOT ?7 OR p.run_id IS NOT ?4))
                    OR v.kind IS NOT observed.kind OR v.local_locator IS NOT observed.local_locator
                    OR v.title IS NOT observed.title OR v.working_dir IS NOT observed.working_dir
                    OR v.session_type IS NOT observed.session_type OR v.state IS NOT observed.state
                    OR v.current_run_id IS NOT observed.current_run_id OR v.archived IS NOT observed.archived
                    OR (NOT ?6 AND v.updated_at IS NOT observed.updated_at) OR v.deleted IS NOT observed.deleted
                )
              )
        )");
    let only_output: bool = sqlx::query_scalar(&sql)
        .bind(command.work.work_id.to_string())
        .bind(observed)
        .bind(current_revision)
        .bind(run)
        .bind(kind)
        .bind(external)
        .bind(event)
        .fetch_one(connection)
        .await
        .map_err(unavailable)?;
    if only_output {
        Ok(())
    } else {
        Err(stale())
    }
}
