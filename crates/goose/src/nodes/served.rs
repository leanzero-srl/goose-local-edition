//! Which node served a session's turn (design §7.1): written by the router for EVERY lease —
//! MLX, LM Studio and cloud — and read by the chip at the end of each turn through
//! `nodes/servedLast`. This process keeps each session's last record in memory, and the session
//! keeps it in its `extension_data` (`nodes.served`) so the chip survives a reload. Only the last
//! record is kept: its one reader reads the turn just served, and after a reload earlier turn
//! lines are not reconstructed (nothing pretends they were).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use anyhow::Result;
use goose_sdk_types::custom_requests::NodeServedTurnDto;
use serde::{Deserialize, Serialize};

use crate::session::extension_data::ExtensionState;
use crate::session::SessionManager;

/// The persisted record, as the session's extension state `nodes.served`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServedState {
    pub last: NodeServedTurnDto,
}

impl ExtensionState for ServedState {
    const EXTENSION_NAME: &'static str = "nodes";
    const VERSION: &'static str = "served";
}

fn memory() -> &'static Mutex<HashMap<String, NodeServedTurnDto>> {
    static LAST: OnceLock<Mutex<HashMap<String, NodeServedTurnDto>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record the node that served `session_id`'s turn: in memory at once, then in the session.
pub async fn record(
    session_manager: &SessionManager,
    session_id: &str,
    turn: NodeServedTurnDto,
) -> Result<()> {
    remember(session_id, turn.clone());
    session_manager
        .set_extension_state(session_id, &ServedState { last: turn })
        .await
}

/// This process's record only (the router's in-process write, before it is persisted).
pub fn remember(session_id: &str, turn: NodeServedTurnDto) {
    memory()
        .lock()
        .expect("served-turn map poisoned")
        .insert(session_id.to_string(), turn);
}

/// The session's last record: this process's, else the one persisted in the session. `Ok(None)` =
/// no turn of this session was served through a node yet.
pub async fn last(
    session_manager: &SessionManager,
    session_id: &str,
) -> Result<Option<NodeServedTurnDto>> {
    if let Some(turn) = memory()
        .lock()
        .expect("served-turn map poisoned")
        .get(session_id)
        .cloned()
    {
        return Ok(Some(turn));
    }
    let session = session_manager.get_session(session_id, false).await?;
    // Read the state directly: the trait's `from_extension_data` turns a record that no longer
    // parses into "none", and a broken record is named here instead.
    session
        .extension_data
        .get_extension_state(ServedState::EXTENSION_NAME, ServedState::VERSION)
        .map(|value| ServedState::from_value(value).map(|s| s.last))
        .transpose()
}
