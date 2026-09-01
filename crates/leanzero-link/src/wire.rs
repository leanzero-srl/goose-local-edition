//! The `/v1/swarm` wire contract consumed by the desktop UI, the companion app,
//! and peer LeanZero Link nodes. Field and tag names here are load-bearing —
//! changing any of them breaks every consumer at once.
//!
//! Idiom mirrors goose-server's `MessageEvent` (`crates/goose-server/src/routes/reply.rs`):
//! internally tagged enums (`#[serde(tag = "type")]`), snake_case field names as written.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One node's identity + state as served by `GET /v1/swarm/nodes` and carried in
/// [`LinkEvent::NodeStateChanged`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeState {
    pub node_id: String,
    pub hostname: String,
    pub mesh_ip: Option<String>,
    pub status: NodeStatus,
    pub sessions_active: u32,
    pub updated_at: DateTime<Utc>,
    /// Set by the node POLLING this one. An HTTP-status or unparseable-body failure
    /// (a `401` from a token mismatch, a `503` from an unreadable session index) means
    /// the peer is ALIVE but not answering as a LeanZero Link node should: the text
    /// lands here and `status` keeps its last known value — never a fabricated
    /// `Offline`. A transport failure (refused, timeout) flips `status` to `Offline`
    /// and carries its text here too. `None` after a clean poll, and always `None` in a
    /// node's own `self` report. Omitted on the wire when `None` (additive: older peers
    /// parse; every constructor in every crate must now name it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_poll_error: Option<String>,
}

/// Wire: `{"type":"Idle"}`, `{"type":"Busy","session_id":"..."}`, `{"type":"Offline"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeStatus {
    Idle,
    Busy { session_id: String },
    Offline,
}

impl NodeStatus {
    /// The honest Busy/Idle signal: >=1 live session means Busy, carrying the most
    /// recently updated live session's id; Idle otherwise. Offline is never derived
    /// here — it is a mesh-level verdict owned by the control service.
    pub fn from_sessions(sessions: &[SessionSummary]) -> Self {
        sessions
            .iter()
            .filter(|s| s.live)
            .max_by_key(|s| s.updated_at)
            .map(|s| NodeStatus::Busy {
                session_id: s.session_id.clone(),
            })
            .unwrap_or(NodeStatus::Idle)
    }
}

/// One session in the mirror index, served by `GET /v1/swarm/sessions` and carried
/// in [`LinkEvent::SessionUpserted`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub origin_node_id: String,
    pub working_dir: String,
    pub name: String,
    pub updated_at: DateTime<Utc>,
    pub message_count: u64,
    pub live: bool,
}

/// Body of `GET /v1/swarm/nodes`. The JSON key for the local node is `self`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwarmNodesResponse {
    #[serde(rename = "self")]
    pub self_node: NodeState,
    pub peers: Vec<NodeState>,
}

/// The real-time event contract on `GET /v1/swarm/stream`.
///
/// `SessionDelta` mirrors goose-server's `MessageEvent` transport: the payload is
/// opaque JSON passed through untouched (a serialized `MessageEvent`-shaped object
/// once the goose-server adapter feeds it), and `seq` is the ORIGIN node's delta
/// sequence for that session — never rewritten by relaying nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LinkEvent {
    NodeStateChanged(NodeState),
    SessionUpserted(SessionSummary),
    SessionDelta {
        session_id: String,
        seq: u64,
        kind: SessionDeltaKind,
        payload: serde_json::Value,
    },
}

/// Wire values: `message`, `tool_call`, `tool_update`, `finish`, `error` — the
/// delta classes of goose-server's `MessageEvent` (`Message`, tool `Notification`s,
/// `Finish`, `Error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionDeltaKind {
    Message,
    ToolCall,
    ToolUpdate,
    Finish,
    Error,
}

/// One `/v1/swarm/stream` WebSocket text frame: `{"seq":N,"event":{...}}`.
///
/// `seq` is THIS node's monotonic stream cursor (the `?since=` replay cursor). It
/// wraps the event rather than being flattened into it because `SessionDelta`
/// carries its own origin-scoped `seq` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamFrame {
    pub seq: u64,
    pub event: LinkEvent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn summary(id: &str, live: bool, updated: i64) -> SessionSummary {
        SessionSummary {
            session_id: id.to_string(),
            origin_node_id: "node-a".to_string(),
            working_dir: "/tmp/w".to_string(),
            name: format!("session {id}"),
            updated_at: ts(updated),
            message_count: 3,
            live,
        }
    }

    #[test]
    fn node_status_wire_shape_is_exact() {
        assert_eq!(
            serde_json::to_value(NodeStatus::Idle).unwrap(),
            serde_json::json!({"type": "Idle"})
        );
        assert_eq!(
            serde_json::to_value(NodeStatus::Busy {
                session_id: "s1".into()
            })
            .unwrap(),
            serde_json::json!({"type": "Busy", "session_id": "s1"})
        );
        assert_eq!(
            serde_json::to_value(NodeStatus::Offline).unwrap(),
            serde_json::json!({"type": "Offline"})
        );
    }

    #[test]
    fn link_event_wire_shape_is_exact() {
        let delta = LinkEvent::SessionDelta {
            session_id: "s1".to_string(),
            seq: 7,
            kind: SessionDeltaKind::ToolCall,
            payload: serde_json::json!({"anything": true}),
        };
        assert_eq!(
            serde_json::to_value(&delta).unwrap(),
            serde_json::json!({
                "type": "SessionDelta",
                "session_id": "s1",
                "seq": 7,
                "kind": "tool_call",
                "payload": {"anything": true}
            })
        );

        let node = NodeState {
            node_id: "node-a".to_string(),
            hostname: "a-host".to_string(),
            mesh_ip: Some("100.64.0.1".to_string()),
            status: NodeStatus::Idle,
            sessions_active: 0,
            updated_at: ts(1_700_000_000),
            last_poll_error: None,
        };
        let value = serde_json::to_value(LinkEvent::NodeStateChanged(node.clone())).unwrap();
        assert_eq!(value["type"], "NodeStateChanged");
        assert_eq!(value["node_id"], "node-a");
        assert_eq!(value["status"], serde_json::json!({"type": "Idle"}));

        let back: LinkEvent = serde_json::from_value(value).unwrap();
        assert_eq!(back, LinkEvent::NodeStateChanged(node));
    }

    /// `last_poll_error` is additive: omitted when `None`, defaulted when absent (an
    /// older peer's JSON), carried verbatim when set.
    #[test]
    fn last_poll_error_is_omitted_when_none_and_defaults_when_absent() {
        let node = NodeState {
            node_id: "node-a".to_string(),
            hostname: "a-host".to_string(),
            mesh_ip: None,
            status: NodeStatus::Offline,
            sessions_active: 0,
            updated_at: ts(1),
            last_poll_error: None,
        };
        let value = serde_json::to_value(&node).unwrap();
        assert!(
            value.get("last_poll_error").is_none(),
            "None is omitted on the wire: {value}"
        );

        let older_peer_json = serde_json::json!({
            "node_id": "node-b", "hostname": "b", "mesh_ip": null,
            "status": {"type": "Idle"}, "sessions_active": 0,
            "updated_at": "2023-11-14T22:13:20Z"
        });
        let parsed: NodeState = serde_json::from_value(older_peer_json).unwrap();
        assert_eq!(parsed.last_poll_error, None);

        let with_error = NodeState {
            last_poll_error: Some("peer answered 401 Unauthorized: token mismatch".into()),
            ..node
        };
        let value = serde_json::to_value(&with_error).unwrap();
        assert_eq!(
            value["last_poll_error"],
            "peer answered 401 Unauthorized: token mismatch"
        );
        let back: NodeState = serde_json::from_value(value).unwrap();
        assert_eq!(back, with_error);
    }

    #[test]
    fn nodes_response_uses_self_key() {
        let response = SwarmNodesResponse {
            self_node: NodeState {
                node_id: "node-a".to_string(),
                hostname: "a-host".to_string(),
                mesh_ip: None,
                status: NodeStatus::Offline,
                sessions_active: 0,
                updated_at: ts(1),
                last_poll_error: None,
            },
            peers: Vec::new(),
        };
        let value = serde_json::to_value(&response).unwrap();
        assert!(value.get("self").is_some());
        assert!(value.get("self_node").is_none());
        assert_eq!(value["peers"], serde_json::json!([]));
    }

    #[test]
    fn busy_carries_most_recent_live_session() {
        let sessions = vec![summary("old", true, 100), summary("new", true, 200)];
        assert_eq!(
            NodeStatus::from_sessions(&sessions),
            NodeStatus::Busy {
                session_id: "new".to_string()
            }
        );
        let idle = vec![summary("dead", false, 300)];
        assert_eq!(NodeStatus::from_sessions(&idle), NodeStatus::Idle);
        assert_eq!(NodeStatus::from_sessions(&[]), NodeStatus::Idle);
    }
}
