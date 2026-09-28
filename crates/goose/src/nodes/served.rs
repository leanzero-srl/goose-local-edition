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

/// The load a session's CURRENT reply paid, by node: the router writes one record per model call,
/// and only the call that demanded the load measures it — a reply's later calls on the same node
/// carry it, so the reply's last record still says what it loaded (Q-434: a delegate's card read
/// its final call's record and never said "loaded for this delegate in …"). A new user reply
/// (`reply_began`) starts empty; a delegate's session is one reply for its whole run.
fn reply_loads() -> &'static Mutex<HashMap<String, (String, u64)>> {
    static LOADS: OnceLock<Mutex<HashMap<String, (String, u64)>>> = OnceLock::new();
    LOADS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A prompt opened a new reply in `session_id`: what an earlier reply loaded is not this one's.
pub fn reply_began(session_id: &str) {
    reply_loads()
        .lock()
        .expect("reply-load map poisoned")
        .remove(session_id);
}

/// Record the node that served `session_id`'s turn: in memory at once, then in the session.
pub async fn record(
    session_manager: &SessionManager,
    session_id: &str,
    turn: NodeServedTurnDto,
) -> Result<()> {
    let turn = remember(session_id, turn);
    persist(session_manager, session_id, turn).await
}

/// The session's copy of a record `remember` already took (it survives a reload).
pub async fn persist(
    session_manager: &SessionManager,
    session_id: &str,
    turn: NodeServedTurnDto,
) -> Result<()> {
    session_manager
        .set_extension_state(session_id, &ServedState { last: turn })
        .await
}

/// This process's record (the router's in-process write, before it is persisted), with the load
/// its reply paid on that node carried onto it; the record as kept is returned.
pub fn remember(session_id: &str, mut turn: NodeServedTurnDto) -> NodeServedTurnDto {
    {
        let mut loads = reply_loads().lock().expect("reply-load map poisoned");
        match turn.loaded_ms {
            Some(ms) => {
                loads.insert(session_id.to_string(), (turn.node.clone(), ms));
            }
            None => {
                if let Some((node, ms)) = loads.get(session_id) {
                    if *node == turn.node {
                        turn.loaded_ms = Some(*ms);
                    }
                }
            }
        }
    }
    memory()
        .lock()
        .expect("served-turn map poisoned")
        .insert(session_id.to_string(), turn.clone());
    turn
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
