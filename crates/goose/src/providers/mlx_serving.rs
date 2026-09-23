//! Who this goose process is sending work to right now, for the surfaces that show the local MLX
//! engine's live state (the desktop's state tile and its menu-bar tray).
//!
//! Rapid-MLX cannot tell its clients apart: its `/v1/status` lists requests by an internal id.
//! goose can, at the two doors its own work leaves through:
//!
//! - the swarm router's lease on an `mlx-sidecar` node (`swarm_router::Router::leased`) — every
//!   `swarm` chat turn, session title and OpenAI-compatible request routed to the local engine,
//!   carrying the session id the agent's reply loop scoped (`session_context::SESSION_ID`);
//! - the OpenAI-compatible route (`api::openai_compat::chat_completions`) — one entry per external
//!   request for the life of its turn, whatever provider it names.
//!
//! An entry lives exactly as long as its guard: the router's guard rides the lease (held by the
//! stream until it ends or is dropped), the API guard rides the spawned turn. Nothing here is
//! sampled, estimated or kept past its guard, so the list is what is in flight, not what was.
//! Work that leaves through neither door — a `goose swarm run` child process, another app on the
//! port — is not listed; the reader counts it as unattributed against the engine's own requests.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServingVia {
    /// The swarm router leased a slot on the local `mlx-sidecar` node for this turn.
    SwarmRouter,
    /// An external client's `POST /v1/chat/completions` is running its turn.
    OpenaiApi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServingEntry {
    pub id: u64,
    pub via: ServingVia,
    /// The goose session the work belongs to; `None` when the call ran outside a session scope.
    pub session_id: Option<String>,
    /// The goose provider the work was dispatched with (`omlx` for a router lease on the engine).
    pub provider: String,
    pub model: String,
    /// The swarm pool device the router leased (router entries only).
    pub node_id: Option<String>,
    pub started_at: DateTime<Utc>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static IN_FLIGHT: LazyLock<Mutex<BTreeMap<u64, ServingEntry>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Removes its entry when dropped — the only way an entry leaves the registry.
#[derive(Debug)]
pub struct ServingGuard(u64);

impl Drop for ServingGuard {
    fn drop(&mut self) {
        IN_FLIGHT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

pub fn register(
    via: ServingVia,
    session_id: Option<String>,
    provider: &str,
    model: &str,
    node_id: Option<&str>,
) -> ServingGuard {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner()).insert(
        id,
        ServingEntry {
            id,
            via,
            session_id,
            provider: provider.to_string(),
            model: model.to_string(),
            node_id: node_id.map(str::to_string),
            started_at: Utc::now(),
        },
    );
    ServingGuard(id)
}

/// Every entry in flight, oldest first.
pub fn snapshot() -> Vec<ServingEntry> {
    IN_FLIGHT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ours(ids: &[u64]) -> Vec<ServingEntry> {
        snapshot()
            .into_iter()
            .filter(|e| ids.contains(&e.id))
            .collect()
    }

    #[test]
    fn an_entry_lives_exactly_as_long_as_its_guard() {
        let router = register(
            ServingVia::SwarmRouter,
            Some("20260923_7".to_string()),
            "omlx",
            "mihai-qwen3.8-27b-atlassian-q8-mlx",
            Some("mihai-mlx"),
        );
        let api = register(
            ServingVia::OpenaiApi,
            Some("20260923_8".to_string()),
            "omlx",
            "mihai-qwen3.8-27b-atlassian-q8-mlx",
            None,
        );
        let ids = [router.0, api.0];
        let live = ours(&ids);
        assert_eq!(live.len(), 2);
        assert_eq!(live[0].via, ServingVia::SwarmRouter);
        assert_eq!(live[0].node_id.as_deref(), Some("mihai-mlx"));
        assert_eq!(live[1].session_id.as_deref(), Some("20260923_8"));

        drop(router);
        let live = ours(&ids);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].via, ServingVia::OpenaiApi);
        drop(api);
        assert!(ours(&ids).is_empty());
    }

    #[test]
    fn entries_serialize_in_the_desktops_camel_case() {
        let guard = register(ServingVia::OpenaiApi, None, "anthropic", "claude", None);
        let entry = ours(&[guard.0]).remove(0);
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["via"], "openaiApi");
        assert_eq!(json["sessionId"], serde_json::Value::Null);
        assert_eq!(json["provider"], "anthropic");
        assert!(json["startedAt"].is_string());
    }
}
