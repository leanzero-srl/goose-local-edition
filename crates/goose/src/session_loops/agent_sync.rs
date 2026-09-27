//! The `loop` extension's life on a session's agent (design §4.4, §9 L3).
//!
//! The agent carries the `loop_report` tool exactly while its chat's loop has not ended. The
//! process-wide runner holds no agent, so it never adds or removes the tool; the places that DO
//! hold the session's agent call [`sync_on`] (through the seam: the `loops/start` and
//! `loops/control` handlers, and `on_prompt` before `agent.reply`; directly: the `/loop` command).
//! A loop the runner ends on its own keeps the tool until the next sync, and the tool refuses
//! outside a tick meanwhile. `add_extension` persists the extension set with the session, so a
//! reload carries it — and a fork must not ([`strip_for_fork`]).

use std::sync::Arc;

use async_trait::async_trait;
use goose_sdk_types::custom_requests::{LoopRecord, LoopStatus};

use super::{record, seam};
use crate::agents::extension::{ExtensionConfig, PLATFORM_EXTENSIONS};
use crate::agents::platform_extensions::loop_report;
use crate::agents::Agent;
use crate::config::extensions::name_to_key;
use crate::session::extension_data::{EnabledExtensionsState, ExtensionData, ExtensionState};

/// The `loop` extension as the session's extension set stores it.
pub fn loop_extension_config() -> ExtensionConfig {
    let def = PLATFORM_EXTENSIONS
        .get(loop_report::EXTENSION_NAME)
        .expect("the loop platform extension is registered");
    ExtensionConfig::Platform {
        name: def.name.to_string(),
        description: def.description.to_string(),
        display_name: Some(def.display_name.to_string()),
        bundled: Some(true),
        available_tools: Vec::new(),
    }
}

/// The agent carries the `loop` extension iff the record's loop has not ended. Idempotent.
pub async fn sync_on(agent: &Agent, session_id: &str, record: &LoopRecord) -> Result<(), String> {
    let wanted = record.status != LoopStatus::Ended;
    let carried = agent
        .extension_manager
        .is_extension_enabled(loop_report::EXTENSION_NAME)
        .await;
    match (wanted, carried) {
        (true, false) => agent
            .add_extension(loop_extension_config(), session_id)
            .await
            .map_err(|e| format!("The loop report tool could not be added to this chat: {e}")),
        (false, true) => agent
            .remove_extension(loop_report::EXTENSION_NAME, session_id)
            .await
            .map_err(|e| format!("The loop report tool could not be removed from this chat: {e}")),
        _ => Ok(()),
    }
}

struct AgentSync;

#[async_trait]
impl seam::LoopExtensionSync for AgentSync {
    async fn sync(
        &self,
        agent: Arc<Agent>,
        session_id: &str,
        record: &LoopRecord,
    ) -> Result<(), String> {
        sync_on(&agent, session_id, record).await
    }
}

/// Install this process's extension sync into the seam. Idempotent: the first install wins.
pub fn install() {
    seam::install_extension_sync(Arc::new(AgentSync));
}

fn is_loop_entry(entry: &serde_json::Value) -> bool {
    entry
        .get("name")
        .and_then(|name| name.as_str())
        .is_some_and(|name| name_to_key(name) == loop_report::EXTENSION_NAME)
}

/// A fork (`copy_session`) starts with no loop: the copy drops `loop.v0` and the `loop` entry of
/// the persisted extension set. The other entries are kept as stored, byte for byte — they are
/// edited as JSON, never re-parsed into `ExtensionConfig`, which would drop what this build cannot
/// read.
pub fn strip_for_fork(extension_data: &mut ExtensionData) {
    extension_data.extension_states.remove(&format!(
        "{}.{}",
        record::EXTENSION_NAME,
        record::VERSION
    ));
    let enabled_key = format!(
        "{}.{}",
        EnabledExtensionsState::EXTENSION_NAME,
        EnabledExtensionsState::VERSION
    );
    if let Some(extensions) = extension_data
        .extension_states
        .get_mut(&enabled_key)
        .and_then(|state| state.get_mut("extensions"))
        .and_then(|extensions| extensions.as_array_mut())
    {
        extensions.retain(|entry| !is_loop_entry(entry));
    }
}
