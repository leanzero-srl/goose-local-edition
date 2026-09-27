use crate::custom_requests::{CustomMethodSchema, LoopRecord};
use agent_client_protocol::{JsonRpcMessage, JsonRpcNotification};
use schemars::{JsonSchema, SchemaGenerator};
use serde::{Deserialize, Serialize};

/// Goose-custom session update notification — a parallel to ACP's
/// `session/update` carrying goose-specific update variants.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcNotification)]
#[notification(method = "_goose/unstable/session/update")]
#[serde(rename_all = "camelCase")]
pub struct GooseSessionNotification {
    pub session_id: String,
    pub update: GooseSessionUpdate,
}

/// Discriminated union of goose-specific session update payloads.
/// Variant tag matches ACP's convention (`sessionUpdate: "<snake_case>"`).
///
/// `discriminator.mapping` is what makes TS codegen (`@hey-api/openapi-ts`)
/// emit the correct snake_case tag value even when this enum has a single
/// variant. Add a mapping entry per variant.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
#[schemars(extend("discriminator" = {
    "propertyName": "sessionUpdate",
    "mapping": {
        "usage_update": "#/$defs/SessionUsageUpdate",
        "status_message": "#/$defs/StatusMessageUpdate"
    }
}))]
pub enum GooseSessionUpdate {
    UsageUpdate(SessionUsageUpdate),
    StatusMessage(StatusMessageUpdate),
}

impl Default for GooseSessionUpdate {
    fn default() -> Self {
        GooseSessionUpdate::UsageUpdate(SessionUsageUpdate::default())
    }
}

/// Streaming context-window usage update for a session.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsageUpdate {
    pub used: u64,
    pub context_limit: u64,
    pub accumulated_input_tokens: u64,
    pub accumulated_output_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accumulated_cost: Option<f64>,
}

/// Live UI/session status. This is not conversation transcript content, and
/// should not be persisted or replayed as history.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusMessageUpdate {
    pub status: StatusMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StatusMessage {
    #[serde(rename_all = "camelCase")]
    Notice {
        message: String,
        /// The turn the person stopped, when this notice says so (Q-169) — the chat renders the
        /// line from these numbers. Absent for every other notice.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stopped: Option<StoppedTurnStatus>,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        message: String,
        /// A response still forming tool calls, as received so far — what the chat lists behind
        /// its status line. Absent for every other progress status.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        forming: Option<FormingStatus>,
    },
}

/// A turn the person stopped: how long it ran and the output tokens the model had written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoppedTurnStatus {
    pub elapsed_ms: u64,
    /// Absent when goose could not count them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

/// What the decoder has received of a response whose tool calls are still forming (Q-151): each
/// call with its tool and argument characters, and the text that arrived beside them, which the
/// chat does not place in the conversation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormingStatus {
    pub calls: Vec<FormingCallStatus>,
    pub argument_chars: u64,
    pub reasoning_chars: u64,
    pub text: String,
    /// Finished calls that copy an earlier call of the response word for word (Q-159). goose runs
    /// such a copy once; the line says how many there are while they form.
    #[serde(default)]
    pub repeated_calls: u64,
    /// The tool, as the chat names it, when every copy repeats that ONE earlier call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeated_title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormingCallStatus {
    /// The tool as goose names it (`extension__tool`).
    pub name: String,
    /// The tool as the chat names it (goose's `extension: tool`).
    pub title: String,
    pub argument_chars: u64,
}

/// A loop tick is due in this chat (session loops, design DESIGN-SESSION-LOOPS.md §5.1): the
/// renderer submits `prompt` as a user message with id `messageId` through the same door a typed
/// message uses, carrying `_meta.goose.loopTick = {loopId, n, messageId}`, or answers
/// `loops/tickRefused`. The offer stands until goosed accepts it; a repeat of the same
/// `(loopId, n, messageId)` is the same offer, never a second tick.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcNotification)]
#[notification(method = "_goose/unstable/loops/tickDue")]
#[serde(rename_all = "camelCase")]
pub struct LoopsTickDueNotification {
    pub session_id: String,
    pub loop_id: String,
    pub n: u32,
    pub message_id: String,
    pub prompt: String,
}

/// A chat's loop record changed (the rail and the pills update on this event, never on a poll).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcNotification)]
#[notification(method = "_goose/unstable/loops/changed")]
#[serde(rename_all = "camelCase")]
pub struct LoopsChangedNotification {
    pub session_id: String,
    #[serde(rename = "loop")]
    pub record: LoopRecord,
}

fn notification_schema<T>(generator: &mut SchemaGenerator) -> CustomMethodSchema
where
    T: Default + JsonRpcMessage + JsonSchema,
{
    let dummy = T::default();
    let type_name = std::any::type_name::<T>()
        .rsplit("::")
        .next()
        .unwrap_or(std::any::type_name::<T>())
        .to_string();
    CustomMethodSchema {
        method: dummy.method().to_string(),
        params_schema: Some(generator.subschema_for::<T>()),
        params_type_name: Some(type_name),
        response_schema: None,
        response_type_name: None,
    }
}

/// Schemas for every goose-custom outbound notification. To register a new
/// notification, define the struct above (with `JsonRpcNotification` +
/// `Default`) and add one line below.
pub fn custom_notification_schemas(generator: &mut SchemaGenerator) -> Vec<CustomMethodSchema> {
    vec![
        notification_schema::<GooseSessionNotification>(generator),
        notification_schema::<LoopsTickDueNotification>(generator),
        notification_schema::<LoopsChangedNotification>(generator),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_message_serializes_to_expected_wire_shape() {
        let notification = GooseSessionNotification {
            session_id: "s1".to_string(),
            update: GooseSessionUpdate::StatusMessage(StatusMessageUpdate {
                status: StatusMessage::Notice {
                    message: "Compaction complete".to_string(),
                    stopped: None,
                },
            }),
        };

        let value = serde_json::to_value(notification).unwrap();

        assert_eq!(
            value,
            json!({
                "sessionId": "s1",
                "update": {
                    "sessionUpdate": "status_message",
                    "status": {
                        "type": "notice",
                        "message": "Compaction complete"
                    }
                }
            })
        );
    }
}
