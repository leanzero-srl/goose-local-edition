//! How a session's LAST turn ended, durable in its `extension_data` (`turn_outcome.v0`), so a
//! session whose answer was cut ("The split across your Macs stopped mid-answer") reads FAILED in
//! every list instead of "8h ago". Written at the end of every turn of a non-swarm agent: a later
//! turn that completes clears it.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::session::extension_data::ExtensionState;
use crate::session::{SessionManager, SessionType};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnOutcomeState {
    pub failed: bool,
    pub at: DateTime<Utc>,
    /// The failure as the person saw it in the chat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ExtensionState for TurnOutcomeState {
    const EXTENSION_NAME: &'static str = "turn_outcome";
    const VERSION: &'static str = "v0";
}

#[derive(Debug, Clone)]
pub struct FailedSession {
    pub session_id: String,
    pub session_name: String,
    pub working_dir: PathBuf,
    pub at: DateTime<Utc>,
    pub reason: Option<String>,
}

/// A turn that ended on a failure; `shown` is the failure text the chat showed, when it had one.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub shown: Option<String>,
}

pub async fn record(
    session_manager: &SessionManager,
    session_id: &str,
    failure: Option<Failure>,
) -> Result<()> {
    let state = TurnOutcomeState {
        failed: failure.is_some(),
        at: Utc::now(),
        reason: failure.and_then(|failure| failure.shown),
    };
    session_manager
        .set_extension_state(session_id, &state)
        .await
}

/// User and scheduled sessions whose last turn failed.
pub async fn failed_sessions(session_manager: &SessionManager) -> Result<Vec<FailedSession>> {
    Ok(session_manager
        .sessions_with_extension_state::<TurnOutcomeState>()
        .await?
        .into_iter()
        .filter(|row| {
            row.state.failed
                && matches!(row.session_type, SessionType::User | SessionType::Scheduled)
        })
        .map(|row| FailedSession {
            session_id: row.session_id,
            session_name: row.name,
            working_dir: row.working_dir,
            at: row.state.at,
            reason: row.state.reason,
        })
        .collect())
}
