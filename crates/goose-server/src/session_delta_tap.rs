//! The process-wide per-message delta tap: goose-server's half of the LeanZero Link
//! dependency inversion.
//!
//! `crates/goose` hosts `GoosedSwarmStateSource` (the `leanzero_link::SwarmStateSource`)
//! but cannot see the per-session `MessageEvent` buses, which live here in goose-server
//! (a crate that depends on `goose`). So goose-server taps every session's reply-loop
//! `MessageEvent`s ADDITIVELY (beside — never instead of — the per-session bus), classifies
//! each into a [`SessionDeltaKind`] + opaque payload here (the only place `MessageEvent` is
//! visible), and injects the tap into `goose` as a [`DeltaSource`] at boot. Once real
//! `SessionDelta`s flow out of `subscribe_local_deltas`, the control service broadcasts them
//! over `/v1/swarm/stream` to peers unchanged.

use crate::routes::reply::MessageEvent;
use futures::stream::BoxStream;
use goose::acp::server::{DeltaInput, DeltaSource, SessionDeltaKind};
use rmcp::model::ServerNotification;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;

/// One tapped reply-loop event, carried on the broadcast tap. It keeps the whole
/// `MessageEvent` (classification happens on the consumer side, so the reply-loop send
/// stays a single non-fallible line) plus the origin `seq` that `bus.publish` assigned —
/// the ORIGIN node's per-session delta sequence the wire contract stamps onto
/// `SessionDelta.seq`.
#[derive(Clone, Debug)]
pub struct SessionDeltaMsg {
    pub session_id: String,
    pub seq: u64,
    pub event: MessageEvent,
}

/// Map a reply-loop `MessageEvent` to its wire delta class, or `None` when it is not a
/// per-message delta. `None` is a deliberate drop (logged by [`to_delta_input`]) — never
/// emitted under a wrong kind.
///
/// `SessionDeltaKind::ToolCall` is intentionally never produced here: the goose-server
/// `MessageEvent` stream has a single `Notification` variant (an MCP `ServerNotification`,
/// which goose's own ACP layer treats uniformly as a tool-call *update*), while an actual
/// tool invocation reaches the tap as `ToolRequest` content INSIDE a `Message` event —
/// classified `message`, with the full tool-request payload. The `ToolCall` wire kind
/// stays reserved for a future dedicated ACP tool-call tap rather than being faked from a
/// notification it does not match.
pub fn classify_delta(event: &MessageEvent) -> Option<SessionDeltaKind> {
    match event {
        MessageEvent::Message { .. } => Some(SessionDeltaKind::Message),
        MessageEvent::Finish { .. } => Some(SessionDeltaKind::Finish),
        MessageEvent::Error { .. } => Some(SessionDeltaKind::Error),
        MessageEvent::Notification { message, .. } => classify_notification(message),
        // Not per-message deltas: a full-conversation replace (`SessionUpserted` already
        // covers index changes), the stream-attach hint, and the heartbeat.
        MessageEvent::UpdateConversation { .. }
        | MessageEvent::ActiveRequests { .. }
        | MessageEvent::Ping => None,
    }
}

/// Classify an MCP `ServerNotification` exactly as goose's own ACP layer does
/// (`goose::acp::server::tool_notifications`): the variants it surfaces as tool-call
/// updates become `tool_update`; every other variant is not tool-related and is dropped.
fn classify_notification(notification: &ServerNotification) -> Option<SessionDeltaKind> {
    match notification {
        ServerNotification::LoggingMessageNotification(_)
        | ServerNotification::ProgressNotification(_) => Some(SessionDeltaKind::ToolUpdate),
        ServerNotification::CustomNotification(n) if n.method == "platform_event" => {
            Some(SessionDeltaKind::ToolUpdate)
        }
        _ => None,
    }
}

/// Classify + serialize one tapped message into a [`DeltaInput`], or drop it (with a
/// debug log) when it is not a per-message delta or fails to serialize.
fn to_delta_input(msg: SessionDeltaMsg) -> Option<DeltaInput> {
    let Some(kind) = classify_delta(&msg.event) else {
        tracing::debug!(
            session_id = %msg.session_id,
            event = ?msg.event,
            "leanzeroLink delta tap: unclassifiable MessageEvent dropped"
        );
        return None;
    };
    let payload = match serde_json::to_value(&msg.event) {
        Ok(value) => value,
        Err(error) => {
            tracing::debug!(
                %error,
                session_id = %msg.session_id,
                "leanzeroLink delta tap: MessageEvent failed to serialize; dropped"
            );
            return None;
        }
    };
    Some(DeltaInput {
        session_id: msg.session_id,
        seq: msg.seq,
        kind,
        payload,
    })
}

/// Wraps the process-wide broadcast tap as a [`DeltaSource`] for `goose`. Each `subscribe`
/// spawns a pump that classifies tapped events and forwards the deltas over an mpsc; the
/// pump exits when its receiver (the control service's local delta stream) is dropped, so
/// nothing leaks past a disconnect. A lagged pump drops the oldest deltas (best-effort
/// mirror) rather than blocking.
pub struct SessionDeltaTapSource {
    tap: broadcast::Sender<SessionDeltaMsg>,
}

impl SessionDeltaTapSource {
    pub fn new(tap: broadcast::Sender<SessionDeltaMsg>) -> Self {
        Self { tap }
    }
}

impl DeltaSource for SessionDeltaTapSource {
    fn subscribe(&self) -> BoxStream<'static, DeltaInput> {
        let mut rx = self.tap.subscribe();
        let (out_tx, out_rx) = mpsc::channel::<DeltaInput>(256);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(msg) => {
                        if let Some(input) = to_delta_input(msg) {
                            if out_tx.send(input).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        tracing::debug!(
                            dropped,
                            "leanzeroLink delta tap: pump lagged; peers reconcile via poll"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Box::pin(ReceiverStream::new(out_rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use goose::conversation::message::{Message, TokenState};
    use rmcp::model::{
        CancelledNotificationParam, LoggingLevel, LoggingMessageNotificationParam, Notification,
        NumberOrString,
    };
    use serde_json::json;
    use std::sync::Arc;

    fn logging_notification() -> ServerNotification {
        ServerNotification::LoggingMessageNotification(Notification::new(
            LoggingMessageNotificationParam::new(LoggingLevel::Info, json!({ "line": "x" })),
        ))
    }

    fn cancelled_notification() -> ServerNotification {
        ServerNotification::CancelledNotification(Notification::new(CancelledNotificationParam {
            request_id: NumberOrString::String(Arc::from("r1")),
            reason: None,
        }))
    }

    #[test]
    fn classify_maps_each_delta_variant_and_drops_the_rest() {
        assert_eq!(
            classify_delta(&MessageEvent::Message {
                message: Message::assistant().with_text("hi"),
                token_state: TokenState::default(),
            }),
            Some(SessionDeltaKind::Message)
        );
        assert_eq!(
            classify_delta(&MessageEvent::Error {
                error: "boom".to_string()
            }),
            Some(SessionDeltaKind::Error)
        );
        assert_eq!(
            classify_delta(&MessageEvent::Finish {
                reason: "stop".to_string(),
                token_state: TokenState::default(),
            }),
            Some(SessionDeltaKind::Finish)
        );
        assert_eq!(
            classify_delta(&MessageEvent::Notification {
                request_id: "r1".to_string(),
                message: logging_notification(),
            }),
            Some(SessionDeltaKind::ToolUpdate)
        );

        // A non-tool notification and the non-delta variants are dropped, never mislabeled.
        assert_eq!(
            classify_delta(&MessageEvent::Notification {
                request_id: "r1".to_string(),
                message: cancelled_notification(),
            }),
            None
        );
        assert_eq!(
            classify_delta(&MessageEvent::ActiveRequests {
                request_ids: vec!["r1".to_string()]
            }),
            None
        );
        assert_eq!(classify_delta(&MessageEvent::Ping), None);
    }

    #[tokio::test]
    async fn publishing_to_the_tap_reaches_a_subscriber_as_a_classified_delta() {
        let (tap, _) = broadcast::channel::<SessionDeltaMsg>(16);
        let source = SessionDeltaTapSource::new(tap.clone());
        let mut stream = source.subscribe();

        // The pump has to run before the send is buffered; a Message publishes as a delta.
        let _ = tap.send(SessionDeltaMsg {
            session_id: "s1".to_string(),
            seq: 42,
            event: MessageEvent::Message {
                message: Message::assistant().with_text("mirror me"),
                token_state: TokenState::default(),
            },
        });

        let input = stream.next().await.expect("a delta reaches the subscriber");
        assert_eq!(input.session_id, "s1");
        assert_eq!(input.seq, 42);
        assert_eq!(input.kind, SessionDeltaKind::Message);
        assert_eq!(input.payload["type"], "Message");
    }

    #[tokio::test]
    async fn dropped_variants_never_surface_as_deltas() {
        let (tap, _) = broadcast::channel::<SessionDeltaMsg>(16);
        let source = SessionDeltaTapSource::new(tap.clone());
        let mut stream = source.subscribe();

        // A heartbeat is dropped; the following Message is the first thing to surface.
        let _ = tap.send(SessionDeltaMsg {
            session_id: "s1".to_string(),
            seq: 1,
            event: MessageEvent::Ping,
        });
        let _ = tap.send(SessionDeltaMsg {
            session_id: "s1".to_string(),
            seq: 2,
            event: MessageEvent::Error {
                error: "boom".to_string(),
            },
        });

        let input = stream.next().await.expect("the Error delta surfaces");
        assert_eq!(input.seq, 2, "the Ping at seq 1 was dropped, not emitted");
        assert_eq!(input.kind, SessionDeltaKind::Error);
    }

    #[test]
    fn send_with_no_receiver_errors_but_never_panics() {
        // The reply loop ignores this Err — a tap with no subscriber (mesh not connected)
        // must not block or fail the reply path.
        let (tap, _) = broadcast::channel::<SessionDeltaMsg>(16);
        let result = tap.send(SessionDeltaMsg {
            session_id: "s1".to_string(),
            seq: 1,
            event: MessageEvent::Ping,
        });
        assert!(
            result.is_err(),
            "send with no receiver reports no-subscriber"
        );
    }
}
