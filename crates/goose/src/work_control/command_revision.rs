use super::{conflict, unavailable};
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
    if observed >= current_revision
        || command.work.kind != WorkKind::Chat
        || !matches!(
            command.action,
            ActionPayload::Steer { .. } | ActionPayload::Stop {}
        )
    {
        return Err(stale());
    }
    let run = command.expected_run_id.ok_or_else(stale)?.to_string();
    // Streaming advances the catalog on every chunk. Only a complete same-run partial
    // journal can bridge a stale control snapshot; an event label alone proves nothing.
    let only_partials: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM catalog_versions observed
            JOIN catalog_versions current ON current.work_id=observed.work_id AND current.revision=?3
            WHERE observed.work_id=?1 AND observed.revision=?2
              AND observed.kind='chat' AND observed.deleted=0 AND observed.current_run_id=?4
              AND current.current_run_id=?4
              AND EXISTS (SELECT 1 FROM catalog_versions v
                          WHERE v.work_id=observed.work_id AND v.revision>?2 AND v.revision<=?3)
              AND NOT EXISTS (
                SELECT 1 FROM catalog_versions v
                LEFT JOIN work_events e ON e.revision=v.revision AND e.work_id=v.work_id
                LEFT JOIN owner_partial_events p ON p.revision=v.revision AND p.work_id=v.work_id
                WHERE v.work_id=observed.work_id AND v.revision>?2 AND v.revision<=?3 AND (
                    e.revision IS NULL OR p.revision IS NULL
                    OR v.event_kind IS NOT 'partial_replacement'
                    OR e.event_kind IS NOT 'partial_replacement' OR p.run_id IS NOT ?4
                    OR v.kind IS NOT observed.kind OR v.local_locator IS NOT observed.local_locator
                    OR v.title IS NOT observed.title OR v.working_dir IS NOT observed.working_dir
                    OR v.session_type IS NOT observed.session_type OR v.state IS NOT observed.state
                    OR v.current_run_id IS NOT observed.current_run_id OR v.archived IS NOT observed.archived
                    OR v.updated_at IS NOT observed.updated_at OR v.deleted IS NOT observed.deleted
                )
              )
        )",
    )
    .bind(command.work.work_id.to_string())
    .bind(observed)
    .bind(current_revision)
    .bind(run)
    .fetch_one(connection)
    .await
    .map_err(unavailable)?;
    if only_partials {
        Ok(())
    } else {
        Err(stale())
    }
}
