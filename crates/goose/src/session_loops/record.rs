//! The loop record, `extension_data["loop.v0"]` (design §4.2). Written ONLY through
//! `SessionManager::update_extension_state` (one key, read-modify-write inside `BEGIN IMMEDIATE`):
//! two writers touch it (the runner, and the `loop_report` tool inside a tick), and two goose
//! processes may race an owner claim, so an overwrite that does not read first would let the later
//! writer erase the earlier one's change. Reads never write.

use anyhow::Result;
use chrono::{DateTime, SecondsFormat, Utc};
use goose_sdk_types::custom_requests::{LoopEdit, LoopRecord, LoopStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::LOOP_ID_PREFIX;
use crate::session::extension_data::ExtensionState;
use crate::session::SessionManager;

pub const EXTENSION_NAME: &str = "loop";
pub const VERSION: &str = "v0";

impl ExtensionState for LoopRecord {
    const EXTENSION_NAME: &'static str = EXTENSION_NAME;
    const VERSION: &'static str = VERSION;
}

/// The stored value as it is, parsed per row by the reader, so one unreadable record is named on
/// its own row instead of failing every other chat's.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RawLoopValue(pub Value);

impl ExtensionState for RawLoopValue {
    const EXTENSION_NAME: &'static str = EXTENSION_NAME;
    const VERSION: &'static str = VERSION;
}

/// The canonical time format of the record: RFC 3339, UTC, whole seconds.
pub fn fmt_time(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn parse_time(text: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(text)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| format!("\"{text}\" is not an RFC 3339 time ({e})"))
}

pub fn new_loop_id() -> String {
    let hex: String = Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect();
    format!("{LOOP_ID_PREFIX}{hex}")
}

/// A new loop from a validated start (`rules::validate_loop`), before the runner claims and
/// schedules it: no owner, no next tick, no ticks.
pub fn new_record(id: String, start: LoopEdit, now: DateTime<Utc>) -> LoopRecord {
    let at = fmt_time(now);
    LoopRecord {
        id,
        goal: start.goal,
        template: start.template,
        steps: start.steps,
        cadence: start.cadence,
        state_file: start.state_file,
        check: start.check,
        stop_after_ticks: start.stop_after_ticks,
        status: LoopStatus::Waiting,
        status_reason: None,
        next_tick: None,
        offer: None,
        owner: None,
        created_at: at.clone(),
        started_at: at,
        ended_at: None,
        ticks: Vec::new(),
    }
}

/// The chat's loop: `Ok(None)` = the chat has no loop; an unreadable record is `Err` with its
/// words, never "no loop".
pub fn parse_stored(value: Option<&Value>) -> Result<Option<LoopRecord>, String> {
    value
        .map(|v| {
            serde_json::from_value::<LoopRecord>(v.clone())
                .map_err(|e| format!("The loop record could not be read: {e}"))
        })
        .transpose()
}

/// A pure read of one chat's loop.
pub async fn read(
    session_manager: &SessionManager,
    session_id: &str,
) -> Result<Result<Option<LoopRecord>, String>> {
    let session = session_manager.get_session(session_id, false).await?;
    Ok(parse_stored(
        session
            .extension_data
            .get_extension_state(EXTENSION_NAME, VERSION),
    ))
}

/// Every chat that carries a loop record, each parsed on its own.
pub async fn read_all(
    session_manager: &SessionManager,
) -> Result<Vec<(String, Result<LoopRecord, String>)>> {
    Ok(session_manager
        .sessions_with_extension_state::<RawLoopValue>()
        .await?
        .into_iter()
        .map(|row| {
            let parsed = parse_stored(Some(&row.state.0))
                .and_then(|r| r.ok_or_else(|| "The loop record is empty".to_string()));
            (row.session_id, parsed)
        })
        .collect())
}

/// Read-modify-write the chat's loop inside one transaction. `modify` receives the current record
/// (`None` = no loop yet) and returns the record to store and a value for the caller.
pub async fn update<T>(
    session_manager: &SessionManager,
    session_id: &str,
    modify: impl FnOnce(Option<LoopRecord>) -> Result<(LoopRecord, T)>,
) -> Result<T> {
    session_manager
        .update_extension_state::<LoopRecord, T>(session_id, modify)
        .await
}
