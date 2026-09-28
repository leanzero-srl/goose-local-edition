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

/// Records and chats the L3 tests share (the report tool, `/loop`, the fork strip).
#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;
    use std::sync::Arc;

    use goose_sdk_types::custom_requests::{
        LoopCadence, LoopEdit, LoopRecord, LoopStatus, LoopTemplateId, LoopTickOrigin,
        LoopTickRecord,
    };

    use crate::agents::{Agent, AgentConfig, GoosePlatform};
    use crate::config::permission::PermissionManager;
    use crate::config::GooseMode;
    use crate::session::{SessionManager, SessionType};
    use crate::session_loops::record;

    pub const GOAL: &str = "Make every test pass";
    pub const T0: &str = "2026-09-27T22:00:00Z";

    /// A loop with `ended_ticks` finished ticks and, when `in_flight`, one more started and not ended.
    pub fn a_loop(cadence: LoopCadence, ended_ticks: u32, in_flight: bool) -> LoopRecord {
        let mut loop_record = record::new_record(
            "lp_0a1b2c3d".into(),
            LoopEdit {
                goal: GOAL.into(),
                template: LoopTemplateId::Blank,
                steps: String::new(),
                cadence,
                state_file: ".goose/loops/make-every-test-pass/NOW.md".into(),
                check: None,
                stop_after_ticks: None,
            },
            record::parse_time(T0).unwrap(),
        );
        let tick = |n: u32, ended: bool| LoopTickRecord {
            n,
            origin: if n == 1 {
                LoopTickOrigin::First
            } else {
                LoopTickOrigin::Cadence
            },
            started_at: T0.into(),
            ended_at: ended.then(|| T0.to_string()),
            first_message_id: format!("looptick_lp_0a1b2c3d_{n}_0000"),
            report: None,
            outcome: None,
            wrote: Vec::new(),
            check: None,
            served: None,
            tokens: None,
        };
        loop_record.ticks = (1..=ended_ticks).map(|n| tick(n, true)).collect();
        if in_flight {
            loop_record.ticks.push(tick(ended_ticks + 1, false));
            loop_record.status = LoopStatus::Running;
        }
        loop_record
    }

    pub async fn a_chat() -> (tempfile::TempDir, Arc<SessionManager>, String) {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(SessionManager::new(dir.path().to_path_buf()));
        let session = manager
            .create_session(
                PathBuf::from(dir.path()),
                "loop chat".into(),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();
        (dir, manager, session.id)
    }

    pub async fn store(manager: &SessionManager, session_id: &str, loop_record: LoopRecord) {
        record::update(manager, session_id, |_| Ok((loop_record, ())))
            .await
            .unwrap();
    }

    pub async fn stored(manager: &SessionManager, session_id: &str) -> Option<LoopRecord> {
        record::read(manager, session_id).await.unwrap().unwrap()
    }

    pub fn an_agent(manager: &Arc<SessionManager>) -> Agent {
        Agent::with_config(AgentConfig::new(
            Arc::clone(manager),
            PermissionManager::instance(),
            None,
            GooseMode::Auto,
            true,
            GoosePlatform::GooseDesktop,
        ))
    }

    pub async fn tool_names(agent: &Agent, session_id: &str) -> Vec<String> {
        agent
            .list_tools(session_id, None)
            .await
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use goose_sdk_types::custom_requests::{LoopCadence, LoopStatus};
    use serde_json::json;

    use super::testing::*;
    use super::*;
    use crate::agents::platform_extensions::loop_report::LOOP_REPORT_TOOL_NAME;

    fn persisted_names(extension_data: &ExtensionData) -> Vec<String> {
        EnabledExtensionsState::from_extension_data(extension_data)
            .expect("the extension set is persisted")
            .extensions
            .iter()
            .map(|e| e.name())
            .collect()
    }

    fn carries_the_tool(names: &[String]) -> bool {
        names.iter().any(|n| n == LOOP_REPORT_TOOL_NAME)
    }

    #[tokio::test]
    async fn the_tool_lives_exactly_as_long_as_the_loop() {
        let (_dir, manager, id) = a_chat().await;
        let agent = an_agent(&manager);
        assert!(
            !carries_the_tool(&tool_names(&agent, &id).await),
            "a chat with no loop never carries loop_report"
        );

        let running = a_loop(LoopCadence::SelfPaced, 0, true);
        sync_on(&agent, &id, &running).await.unwrap();
        sync_on(&agent, &id, &running).await.unwrap();
        let names = tool_names(&agent, &id).await;
        assert_eq!(
            names.iter().filter(|n| *n == LOOP_REPORT_TOOL_NAME).count(),
            1,
            "present once, unprefixed, after two syncs: {names:?}"
        );
        let session = manager.get_session(&id, false).await.unwrap();
        assert!(persisted_names(&session.extension_data).contains(&"loop".to_string()));

        let mut paused = running.clone();
        paused.status = LoopStatus::Paused;
        sync_on(&agent, &id, &paused).await.unwrap();
        assert!(
            carries_the_tool(&tool_names(&agent, &id).await),
            "a paused loop keeps the tool, so the tool list does not flip between ticks"
        );

        let mut ended = running;
        ended.status = LoopStatus::Ended;
        sync_on(&agent, &id, &ended).await.unwrap();
        assert!(!carries_the_tool(&tool_names(&agent, &id).await));
        let session = manager.get_session(&id, false).await.unwrap();
        assert!(!persisted_names(&session.extension_data).contains(&"loop".to_string()));
    }

    #[test]
    fn the_fork_strip_drops_the_loop_and_keeps_every_other_entry_as_stored() {
        let unknown = json!({"type": "from_a_later_build", "name": "future", "knob": [1, 2]});
        let developer = json!({"type": "platform", "name": "developer", "description": "Write and edit files", "display_name": "Developer", "bundled": true});
        let mut data = ExtensionData::new();
        data.set_extension_state("loop", "v0", json!({"id": "lp_0a1b2c3d"}));
        data.set_extension_state("todo", "v0", json!({"content": "keep me"}));
        data.set_extension_state(
            "enabled_extensions",
            "v0",
            json!({"extensions": [
                developer,
                serde_json::to_value(loop_extension_config()).unwrap(),
                unknown,
            ]}),
        );

        strip_for_fork(&mut data);

        assert!(data.get_extension_state("loop", "v0").is_none());
        assert_eq!(
            data.get_extension_state("todo", "v0"),
            Some(&json!({"content": "keep me"}))
        );
        assert_eq!(
            data.get_extension_state("enabled_extensions", "v0"),
            Some(&json!({"extensions": [developer, unknown]}))
        );
    }
}
