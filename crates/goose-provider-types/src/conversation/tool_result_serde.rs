use crate::conversation::message::ToolResult;
use rmcp::model::{CallToolRequestParams, ErrorCode, ErrorData, JsonObject};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;

pub fn serialize<T, S>(value: &ToolResult<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    match value {
        Ok(val) => {
            let mut state = serializer.serialize_struct("ToolResult", 2)?;
            state.serialize_field("status", "success")?;
            state.serialize_field("value", val)?;
            state.end()
        }
        Err(err) => {
            let mut state = serializer.serialize_struct("ToolResult", 2)?;
            state.serialize_field("status", "error")?;
            state.serialize_field("error", &err.to_string())?;
            state.end()
        }
    }
}

/// The error a stored tool result reads back as. It was stored as its Display — rmcp's
/// `ErrorData` writes "<code>: <message>" (and "(<data>)" after the message) — so the code comes
/// back from that text and the message is the rest, and the error displays exactly as it did
/// before it was stored. Q-294: read back as INTERNAL_ERROR with the whole text as its message, a
/// result stored as "-32002: Tool 'read_file' not found…" displayed as "-32603: -32002: Tool
/// 'read_file' not found…", so a conversation reloaded at the next turn sent the model a different
/// tool message than the turn itself had sent, and the provider's prompt cache stopped matching
/// there (E2E #3o turn 3: 0 of 76,117 prompt tokens read, the previous 74,913-token prefix cached).
/// A stored text with no leading code is read as before: INTERNAL_ERROR, the text as the message.
fn stored_error(error: String) -> ErrorData {
    let coded = error
        .split_once(": ")
        .and_then(|(code, message)| Some((code.parse::<i32>().ok()?, message.to_string())));
    match coded {
        Some((code, message)) => ErrorData {
            code: ErrorCode(code),
            message: Cow::from(message),
            data: None,
        },
        None => ErrorData {
            code: ErrorCode::INTERNAL_ERROR,
            message: Cow::from(error),
            data: None,
        },
    }
}

#[derive(Deserialize)]
struct ToolCallWithValueArguments {
    name: String,
    arguments: serde_json::Value,
}

impl ToolCallWithValueArguments {
    fn into_call_tool_request_param(self) -> CallToolRequestParams {
        let arguments = match self.arguments {
            serde_json::Value::Object(map) => Some(map),
            serde_json::Value::Null => None,
            other => {
                let mut map = JsonObject::new();
                map.insert("value".to_string(), other);
                Some(map)
            }
        };
        {
            let mut params = CallToolRequestParams::new(self.name);
            if let Some(args) = arguments {
                params = params.with_arguments(args);
            }
            params
        }
    }
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<ToolResult<CallToolRequestParams>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ResultFormat {
        SuccessWithCallToolRequestParams {
            status: String,
            value: CallToolRequestParams,
        },
        SuccessWithToolCallValueArguments {
            status: String,
            value: ToolCallWithValueArguments,
        },
        Error {
            status: String,
            error: String,
        },
    }

    let format = ResultFormat::deserialize(deserializer)?;

    match format {
        ResultFormat::SuccessWithCallToolRequestParams { status, value } => {
            if status == "success" {
                Ok(Ok(value))
            } else {
                Err(serde::de::Error::custom(format!(
                    "Expected status 'success', got '{}'",
                    status
                )))
            }
        }
        ResultFormat::SuccessWithToolCallValueArguments { status, value } => {
            if status == "success" {
                Ok(Ok(value.into_call_tool_request_param()))
            } else {
                Err(serde::de::Error::custom(format!(
                    "Expected status 'success', got '{}'",
                    status
                )))
            }
        }
        ResultFormat::Error { status, error } => {
            if status == "error" {
                Ok(Err(stored_error(error)))
            } else {
                Err(serde::de::Error::custom(format!(
                    "Expected status 'error', got '{}'",
                    status
                )))
            }
        }
    }
}

pub mod call_tool_result {
    use super::*;
    use rmcp::model::{CallToolResult, Content};

    pub fn serialize<S>(
        value: &ToolResult<CallToolResult>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        super::serialize(value, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ToolResult<CallToolResult>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ResultFormat {
            SuccessWithCallToolResult {
                status: String,
                value: CallToolResult,
            },
            SuccessWithContentVec {
                status: String,
                value: Vec<Content>,
            },
            Error {
                status: String,
                error: String,
            },
        }

        let format = ResultFormat::deserialize(deserializer)?;

        match format {
            ResultFormat::SuccessWithCallToolResult { status, value } => {
                if status == "success" {
                    Ok(Ok(value))
                } else {
                    Err(serde::de::Error::custom(format!(
                        "Expected status 'success', got '{}'",
                        status
                    )))
                }
            }
            ResultFormat::SuccessWithContentVec { status, value } => {
                if status == "success" {
                    Ok(Ok(CallToolResult::success(value)))
                } else {
                    Err(serde::de::Error::custom(format!(
                        "Expected status 'success', got '{}'",
                        status
                    )))
                }
            }
            ResultFormat::Error { status, error } => {
                if status == "error" {
                    Ok(Err(stored_error(error)))
                } else {
                    Err(serde::de::Error::custom(format!(
                        "Expected status 'error', got '{}'",
                        status
                    )))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::message::{Message, MessageContent};
    use rmcp::model::CallToolResult;

    fn reloaded(message: &Message) -> Message {
        serde_json::from_str(&serde_json::to_string(message).unwrap()).unwrap()
    }

    fn response_error(message: &Message) -> String {
        match &message.content[0] {
            MessageContent::ToolResponse(response) => {
                response.tool_result.as_ref().unwrap_err().to_string()
            }
            other => panic!("not a tool response: {other:?}"),
        }
    }

    /// Q-294, E2E #3o turn 2: the missing-tool error goose returned for `read_file`, stored at
    /// 04:41:44 as "-32002: Tool 'read_file' not found. …" and sent back at turn 3 (and every
    /// later turn) as "-32603: -32002: Tool 'read_file' not found. …".
    #[test]
    fn a_stored_tool_error_reads_back_as_it_was_sent() {
        let sent = ErrorData {
            code: ErrorCode(-32002),
            message: Cow::from("Tool 'read_file' not found. Available tools: [write, edit, shell]"),
            data: None,
        };
        let turn =
            Message::user().with_tool_response("8352e563", Err::<CallToolResult, _>(sent.clone()));
        let stored = serde_json::to_value(&turn).unwrap();
        assert_eq!(
            stored["content"][0]["toolResult"]["error"],
            "-32002: Tool 'read_file' not found. Available tools: [write, edit, shell]",
            "the stored shape is unchanged"
        );
        let back = reloaded(&turn);
        assert_eq!(response_error(&back), sent.to_string());
        assert_eq!(
            response_error(&reloaded(&back)),
            sent.to_string(),
            "and again"
        );

        let with_data = ErrorData {
            code: ErrorCode::INVALID_PARAMS,
            message: Cow::from("bad path: a: b"),
            data: Some(serde_json::json!({"path": "x"})),
        };
        let turn =
            Message::user().with_tool_response("c", Err::<CallToolResult, _>(with_data.clone()));
        assert_eq!(response_error(&reloaded(&turn)), with_data.to_string());

        let request = Message::assistant().with_tool_request("r", Err(sent.clone()));
        let back = reloaded(&request);
        match &back.content[0] {
            MessageContent::ToolRequest(req) => {
                assert_eq!(
                    req.tool_call.as_ref().unwrap_err().to_string(),
                    sent.to_string()
                )
            }
            other => panic!("not a tool request: {other:?}"),
        }
    }

    #[test]
    fn a_stored_error_without_a_code_reads_as_an_internal_error() {
        let mut stored = serde_json::to_value(Message::user().with_tool_response(
            "c",
            Err::<CallToolResult, _>(ErrorData::internal_error("unused", None)),
        ))
        .unwrap();
        stored["content"][0]["toolResult"]["error"] = "the tool crashed".into();
        let legacy: Message = serde_json::from_value(stored).unwrap();
        assert_eq!(response_error(&legacy), "-32603: the tool crashed");
    }
}
