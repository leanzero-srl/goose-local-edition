use crate::conversation::message::{Message, MessageContent};
use crate::conversation::token_usage::{ProviderUsage, Usage};
use crate::errors::ProviderError;
use crate::formats::openai::{
    extract_reasoning_effort, is_openai_responses_model, openai_reasoning_effort_for_thinking,
    OUTPUT_TRUNCATION_MARKER,
};
use crate::mcp_utils::extract_text_from_resource;
use crate::model::ModelConfig;
use anyhow::{anyhow, Error};
use async_stream::try_stream;
use chrono;
use futures::Stream;
use rmcp::model::{CallToolRequestParams, ErrorCode, ErrorData, RawContent, Role, Tool};
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ops::Deref;

const RESPONSES_REASONING_PASSBACK_PREFIX: &str = "openai-responses-reasoning:";

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponsesApiResponse {
    pub id: String,
    pub object: String,
    #[serde(deserialize_with = "deserialize_response_created_at")]
    pub created_at: i64,
    pub status: String,
    pub model: String,
    pub output: Vec<ResponseOutputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponseReasoningInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ResponseUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete_details: Option<ResponseIncompleteDetails>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseErrorInfo>,
}

fn deserialize_response_created_at<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let number = serde_json::Number::deserialize(deserializer)?;
    if let Some(seconds) = number.as_i64() {
        return Ok(seconds);
    }
    if let Some(seconds) = number.as_u64() {
        return i64::try_from(seconds).map_err(de::Error::custom);
    }

    let seconds = number
        .as_f64()
        .filter(|value| {
            value.is_finite() && *value >= i64::MIN as f64 && *value < -(i64::MIN as f64)
        })
        .ok_or_else(|| de::Error::custom(format!("invalid Responses created_at: {number}")))?;
    Ok(seconds.trunc() as i64)
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename = "summary_text")]
pub struct SummaryText {
    pub text: String,
}

fn reasoning_from_summary(summary: &[SummaryText]) -> Option<MessageContent> {
    let text: String = summary
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        None
    } else {
        Some(MessageContent::thinking(text, ""))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponsesReasoningPassback {
    #[serde(rename = "type")]
    item_type: String,
    id: String,
    #[serde(default)]
    summary: Vec<SummaryText>,
    encrypted_content: String,
}

fn reasoning_content(
    id: Option<&str>,
    summary: &[SummaryText],
    encrypted_content: Option<&str>,
) -> anyhow::Result<Vec<MessageContent>> {
    let mut content = Vec::new();
    content.extend(reasoning_from_summary(summary));

    if let Some(encrypted_content) = encrypted_content.filter(|value| !value.is_empty()) {
        let id = id.filter(|value| !value.is_empty()).ok_or_else(|| {
            anyhow!("Responses reasoning output has encrypted_content without an id")
        })?;
        let passback = ResponsesReasoningPassback {
            item_type: "reasoning".to_string(),
            id: id.to_string(),
            summary: summary.to_vec(),
            encrypted_content: encrypted_content.to_string(),
        };
        let encoded = serde_json::to_string(&passback)?;
        content.push(MessageContent::redacted_thinking(format!(
            "{RESPONSES_REASONING_PASSBACK_PREFIX}{encoded}"
        )));
    }

    Ok(content)
}

fn decode_reasoning_passback(data: &str) -> anyhow::Result<Option<ResponsesReasoningPassback>> {
    let Some(encoded) = data.strip_prefix(RESPONSES_REASONING_PASSBACK_PREFIX) else {
        return Ok(None);
    };
    let passback: ResponsesReasoningPassback = serde_json::from_str(encoded)
        .map_err(|_| anyhow!("Stored Responses reasoning passback is malformed"))?;
    if passback.item_type != "reasoning"
        || passback.id.is_empty()
        || passback.encrypted_content.is_empty()
    {
        return Err(anyhow!("Stored Responses reasoning passback is invalid"));
    }
    Ok(Some(passback))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponseOutputItem {
    Reasoning {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default)]
        summary: Vec<SummaryText>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
    },
    Message {
        // `id` and `status` are required when the OpenAI API emits these
        // items, but Codex rollout files (which reuse the same shape on
        // disk) sometimes omit them. Keep deserialization permissive.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        role: String,
        content: Vec<ResponseContentBlock>,
    },
    FunctionCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        name: String,
        arguments: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponseContentBlock {
    OutputText {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        annotations: Option<Vec<Value>>,
    },
    Refusal {
        refusal: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseReasoningInfo {
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens_details: Option<InputTokensDetails>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponseIncompleteDetails {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponseErrorInfo {
    #[serde(default)]
    pub code: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InputTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<i64>,
}

impl ResponseUsage {
    fn to_usage(&self) -> Usage {
        // input_tokens already includes cached tokens
        let cached_tokens = self
            .input_tokens_details
            .as_ref()
            .and_then(|d| d.cached_tokens);
        Usage::new(
            Some(self.input_tokens),
            Some(self.output_tokens),
            Some(self.total_tokens),
        )
        .with_cache_tokens(cached_tokens, None)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponsesStreamEvent {
    #[serde(rename = "response.created")]
    ResponseCreated {
        sequence_number: i32,
        response: ResponseMetadata,
    },
    #[serde(rename = "response.in_progress")]
    ResponseInProgress {
        sequence_number: i32,
        response: ResponseMetadata,
    },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        sequence_number: i32,
        output_index: i32,
        item: ResponseOutputItemInfo,
    },
    #[serde(rename = "response.content_part.added")]
    ContentPartAdded {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        part: ContentPart,
    },
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        delta: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        logprobs: Option<Vec<Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        obfuscation: Option<String>,
    },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        sequence_number: i32,
        output_index: i32,
        item: ResponseOutputItemInfo,
    },
    #[serde(rename = "response.content_part.done")]
    ContentPartDone {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        part: ContentPart,
    },
    #[serde(rename = "response.output_text.done")]
    OutputTextDone {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        logprobs: Option<Vec<Value>>,
    },
    #[serde(rename = "response.completed")]
    ResponseCompleted {
        sequence_number: i32,
        response: ResponseMetadata,
    },
    #[serde(rename = "response.incomplete")]
    ResponseIncomplete {
        sequence_number: i32,
        response: ResponseMetadata,
    },
    #[serde(rename = "response.failed")]
    ResponseFailed {
        sequence_number: i32,
        response: ResponseMetadata,
    },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        delta: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        obfuscation: Option<String>,
    },
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallArgumentsDone {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        arguments: String,
    },
    #[serde(rename = "response.refusal.delta")]
    RefusalDelta {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        delta: String,
    },
    #[serde(rename = "response.refusal.done")]
    RefusalDone {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        refusal: String,
    },
    #[serde(rename = "error")]
    Error {
        #[serde(default)]
        sequence_number: Option<i32>,
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        param: Option<Value>,
        #[serde(default)]
        error: Option<ResponseErrorInfo>,
    },
    #[serde(rename = "keepalive")]
    Keepalive {
        #[serde(default)]
        sequence_number: Option<i32>,
    },
}

fn is_known_responses_stream_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        "response.created"
            | "response.in_progress"
            | "response.output_item.added"
            | "response.content_part.added"
            | "response.output_text.delta"
            | "response.output_item.done"
            | "response.content_part.done"
            | "response.output_text.done"
            | "response.completed"
            | "response.incomplete"
            | "response.failed"
            | "response.function_call_arguments.delta"
            | "response.function_call_arguments.done"
            | "response.refusal.delta"
            | "response.refusal.done"
            | "error"
            | "keepalive"
    )
}

fn parse_responses_stream_event(data_line: &str) -> anyhow::Result<Option<ResponsesStreamEvent>> {
    let raw_event: Value = serde_json::from_str(data_line).map_err(|e| {
        ProviderError::stream_decode_error(format!(
            "Failed to parse Responses stream event: {}: {:?}",
            e, data_line
        ))
    })?;

    let Some(event_type) = raw_event.get("type").and_then(Value::as_str) else {
        return Ok(None);
    };

    if !is_known_responses_stream_event_type(event_type) {
        return Ok(None);
    }

    let event = serde_json::from_value(raw_event).map_err(|e| {
        ProviderError::stream_decode_error(format!(
            "Failed to parse Responses stream event: {}: {:?}",
            e, data_line
        ))
    })?;
    Ok(Some(event))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub id: String,
    pub object: String,
    #[serde(deserialize_with = "deserialize_response_created_at")]
    pub created_at: i64,
    pub status: String,
    pub model: String,
    #[serde(default)]
    pub output: Vec<ResponseOutputItemInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ResponseUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponseReasoningInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete_details: Option<ResponseIncompleteDetails>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseErrorInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponseOutputItemInfo {
    Reasoning {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default)]
        summary: Vec<SummaryText>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
    },
    Message {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        role: String,
        content: Vec<ContentPart>,
    },
    FunctionCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        name: String,
        arguments: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ContentPart {
    OutputText {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        annotations: Option<Vec<Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        logprobs: Option<Vec<Value>>,
    },
    Refusal {
        refusal: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
}

fn response_failure_error(
    context: &str,
    code: Option<&str>,
    message: Option<&str>,
) -> ProviderError {
    let code = code.filter(|value| !value.is_empty()).unwrap_or("unknown");
    let message = message
        .filter(|value| !value.is_empty())
        .unwrap_or("the provider returned no failure message");
    let details = format!("{context} (code={code}): {message}");

    match code {
        "rate_limit_exceeded" | "rate_limit" => ProviderError::RateLimitExceeded {
            details,
            retry_delay: None,
        },
        "content_filter" | "sensitive" | "bio_policy" | "image_content_policy_violation" => {
            ProviderError::Refusal {
                details,
                category: Some(code.to_string()),
            }
        }
        "context_length_exceeded" | "model_context_window_exceeded" => {
            ProviderError::ContextLengthExceeded(details)
        }
        "network_error" => ProviderError::NetworkError(details),
        "server_error" | "vector_store_timeout" | "insufficient_system_resource" => {
            ProviderError::ServerError(details)
        }
        _ => ProviderError::RequestFailed(details),
    }
}

fn invalid_tool_arguments(message: String) -> ErrorData {
    ErrorData::new(ErrorCode::INVALID_PARAMS, message, None)
}

fn json_value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn response_tool_request_from_value(
    id: String,
    name: String,
    input: Value,
    output_token_limit_reached: bool,
) -> MessageContent {
    let tool_call = if output_token_limit_reached {
        Err(invalid_tool_arguments(format!(
            "Tool arguments for {name} (id {id}) are not executable because the provider hit its output-token limit before proving this call complete"
        )))
    } else {
        match input {
            Value::Object(arguments) => {
                Ok(CallToolRequestParams::new(name).with_arguments(arguments))
            }
            other => Err(invalid_tool_arguments(format!(
                "Tool arguments for {name} (id {id}) must be a JSON object, got {}",
                json_value_kind(&other)
            ))),
        }
    };

    MessageContent::tool_request(id, tool_call)
}

fn response_tool_request_from_arguments(
    id: String,
    name: String,
    arguments: &str,
    output_token_limit_reached: bool,
) -> MessageContent {
    if output_token_limit_reached {
        return response_tool_request_from_value(id, name, Value::Null, true);
    }

    let parsed = if arguments.trim().is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        match serde_json::from_str(arguments) {
            Ok(value) => value,
            Err(error) => {
                return MessageContent::tool_request(
                    id.clone(),
                    Err(invalid_tool_arguments(format!(
                        "Tool arguments for {name} (id {id}) are not valid JSON: {error}"
                    ))),
                );
            }
        }
    };

    response_tool_request_from_value(id, name, parsed, false)
}

fn response_provider_usage(
    response: &ResponseMetadata,
    prior_model: Option<&str>,
) -> Option<ProviderUsage> {
    response.usage.as_ref().map(|usage| {
        let model = if response.model.is_empty() {
            prior_model.unwrap_or_default()
        } else {
            &response.model
        };
        ProviderUsage::new(model.to_string(), usage.to_usage())
    })
}

fn add_message_items(input_items: &mut Vec<Value>, messages: &[Message]) -> anyhow::Result<()> {
    let mut reasoning_by_id: HashMap<String, ResponsesReasoningPassback> = HashMap::new();
    for message in messages.iter().filter(|m| m.is_agent_visible()) {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };

        let mut text_items = Vec::new();

        for content in &message.content {
            match content {
                MessageContent::Text(text) if !text.text.is_empty() => {
                    let content_type = if message.role == Role::Assistant {
                        "output_text"
                    } else {
                        "input_text"
                    };
                    text_items.push(json!({
                        "type": content_type,
                        "text": text.text
                    }));
                }
                MessageContent::RedactedThinking(redacted) if message.role == Role::Assistant => {
                    let Some(passback) = decode_reasoning_passback(&redacted.data)? else {
                        continue;
                    };
                    if !text_items.is_empty() {
                        input_items.push(json!({
                            "role": role,
                            "content": text_items
                        }));
                        text_items = Vec::new();
                    }
                    if let Some(previous) = reasoning_by_id.get(&passback.id) {
                        if previous != &passback {
                            return Err(anyhow!(
                                "Stored Responses reasoning id was reused with different content"
                            ));
                        }
                        continue;
                    }
                    input_items.push(serde_json::to_value(&passback)?);
                    reasoning_by_id.insert(passback.id.clone(), passback);
                }
                MessageContent::ToolRequest(request) if message.role == Role::Assistant => {
                    if !text_items.is_empty() {
                        input_items.push(json!({
                            "role": role,
                            "content": text_items
                        }));
                        text_items = Vec::new();
                    }

                    match &request.tool_call {
                        Ok(tool_call) => {
                            let arguments_str = tool_call
                                .arguments
                                .as_ref()
                                .map(|args| {
                                    serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string())
                                })
                                .unwrap_or_else(|| "{}".to_string());

                            tracing::debug!(
                                "Replaying function_call with call_id: {}, name: {}",
                                request.id,
                                tool_call.name
                            );
                            input_items.push(json!({
                                "type": "function_call",
                                "call_id": request.id,
                                "name": tool_call.name,
                                "arguments": arguments_str
                            }));
                        }
                        Err(e) => {
                            input_items.push(json!({
                                "type": "function_call_output",
                                "call_id": request.id,
                                "output": format!("Error: {}", e.message)
                            }));
                        }
                    }
                }
                MessageContent::Image(image) => {
                    text_items.push(json!({
                        "type": "input_image",
                        "image_url": format!("data:{};base64,{}", image.mime_type, image.data)
                    }));
                }
                MessageContent::ToolResponse(response) => {
                    if !text_items.is_empty() {
                        input_items.push(json!({
                            "role": role,
                            "content": text_items
                        }));
                        text_items = Vec::new();
                    }

                    match &response.tool_result {
                        Ok(contents) => {
                            let has_images = contents
                                .content
                                .iter()
                                .any(|c| matches!(c.deref(), RawContent::Image(_)));

                            let output = if has_images {
                                json!(contents
                                    .content
                                    .iter()
                                    .map(|c| match c.deref() {
                                        RawContent::Text(t) => json!({
                                            "type": "input_text", "text": t.text
                                        }),
                                        RawContent::Resource(r) => json!({
                                            "type": "input_text",
                                            "text": extract_text_from_resource(&r.resource)
                                        }),
                                        RawContent::Image(image) => json!({
                                            "type": "input_image",
                                            "image_url": format!(
                                                "data:{};base64,{}",
                                                image.mime_type, image.data
                                            )
                                        }),
                                        RawContent::Audio(_) => json!({
                                            "type": "input_text", "text": "[Audio content]"
                                        }),
                                        RawContent::ResourceLink(_) => json!({
                                            "type": "input_text", "text": "[Resource link]"
                                        }),
                                    })
                                    .collect::<Vec<Value>>())
                            } else {
                                json!(contents
                                    .content
                                    .iter()
                                    .filter_map(|c| match c.deref() {
                                        RawContent::Text(t) => Some(t.text.clone()),
                                        RawContent::Resource(r) => {
                                            Some(extract_text_from_resource(&r.resource))
                                        }
                                        RawContent::Audio(_) => Some("[Audio content]".into()),
                                        RawContent::ResourceLink(_) => {
                                            Some("[Resource link]".into())
                                        }
                                        RawContent::Image(_) => None,
                                    })
                                    .collect::<Vec<String>>()
                                    .join("\n"))
                            };

                            input_items.push(json!({
                                "type": "function_call_output",
                                "call_id": response.id,
                                "output": output
                            }));
                        }
                        Err(error_data) => {
                            tracing::debug!(
                                "Sending function_call_output error with call_id: {}",
                                response.id
                            );
                            input_items.push(json!({
                                "type": "function_call_output",
                                "call_id": response.id,
                                "output": format!("Error: {}", error_data.message)
                            }));
                        }
                    }
                }
                MessageContent::FrontendToolRequest(request) => {
                    if !text_items.is_empty() {
                        input_items.push(json!({
                            "role": role,
                            "content": text_items
                        }));
                        text_items = Vec::new();
                    }

                    match &request.tool_call {
                        Ok(tool_call) => {
                            let arguments_str = tool_call
                                .arguments
                                .as_ref()
                                .map(|args| {
                                    serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string())
                                })
                                .unwrap_or_else(|| "{}".to_string());

                            input_items.push(json!({
                                "type": "function_call",
                                "call_id": request.id,
                                "name": tool_call.name,
                                "arguments": arguments_str
                            }));
                        }
                        Err(e) => {
                            input_items.push(json!({
                                "type": "function_call_output",
                                "call_id": request.id,
                                "output": format!("Error: {}", e.message)
                            }));
                        }
                    }
                }
                _ => {}
            }
        }

        if !text_items.is_empty() {
            input_items.push(json!({
                "role": role,
                "content": text_items
            }));
        }
    }
    Ok(())
}

pub fn create_responses_request(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
) -> anyhow::Result<Value, Error> {
    let mut input_items = Vec::new();

    if !system.is_empty() {
        input_items.push(json!({
            "role": "system",
            "content": [{
                "type": "input_text",
                "text": system
            }]
        }));
    }

    add_message_items(&mut input_items, messages)?;

    let (model_name, legacy_reasoning_effort) = extract_reasoning_effort(&model_config.model_name);
    // All models routed here are responses-capable; temperature is rejected
    // by the API for reasoning models regardless of whether an explicit
    // effort suffix was provided.
    let is_reasoning_model = is_openai_responses_model(&model_name);
    let reasoning_effort = if is_reasoning_model {
        if let Some(effort) = legacy_reasoning_effort.as_deref() {
            effort
                .parse()
                .ok()
                .and_then(|effort| openai_reasoning_effort_for_thinking(&model_name, effort))
                .or(legacy_reasoning_effort)
        } else {
            model_config
                .thinking_effort()
                .and_then(|effort| openai_reasoning_effort_for_thinking(&model_name, effort))
        }
    } else {
        None
    };

    let store = model_config.request_param::<bool>("store").unwrap_or(false);
    let mut payload = json!({
        "model": model_name,
        "input": input_items,
        "store": store,
    });

    if let Some(effort) = reasoning_effort {
        payload.as_object_mut().unwrap().insert(
            "reasoning".to_string(),
            json!({
                "effort": effort,
                "summary": "auto",
            }),
        );
    }

    if !tools.is_empty() {
        let tools_spec: Vec<Value> = tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                    "strict": false,
                })
            })
            .collect();

        payload
            .as_object_mut()
            .unwrap()
            .insert("tools".to_string(), json!(tools_spec));
    }

    if !is_reasoning_model {
        if let Some(temp) = model_config.temperature {
            payload
                .as_object_mut()
                .unwrap()
                .insert("temperature".to_string(), json!(temp));
        }
    }

    payload.as_object_mut().unwrap().insert(
        "max_output_tokens".to_string(),
        json!(model_config.max_output_tokens()),
    );

    Ok(payload)
}

pub fn responses_api_to_message(response: &ResponsesApiResponse) -> Result<Message, ProviderError> {
    let output_token_limit_reached = match response.status.as_str() {
        "completed" => false,
        "incomplete" => {
            let reason = response
                .incomplete_details
                .as_ref()
                .and_then(|details| details.reason.as_deref())
                .unwrap_or("unknown");
            match reason {
                "max_output_tokens" | "max_tokens" => true,
                "content_filter" => {
                    return Err(response_failure_error(
                        "Responses API returned an incomplete response",
                        Some(reason),
                        Some("response generation was stopped by the provider"),
                    ));
                }
                _ => {
                    return Err(response_failure_error(
                        "Responses API returned an incomplete response",
                        Some(reason),
                        Some("response generation did not complete"),
                    ));
                }
            }
        }
        "failed" => {
            let error = response.error.as_ref();
            return Err(response_failure_error(
                "Responses API failed",
                error.and_then(|value| value.code.as_deref()),
                error.map(|value| value.message.as_str()),
            ));
        }
        status => {
            return Err(response_failure_error(
                "Responses API returned a non-terminal response",
                Some(status),
                Some("response generation did not reach a successful terminal state"),
            ));
        }
    };

    let mut content = Vec::new();

    for item in &response.output {
        match item {
            ResponseOutputItem::Reasoning {
                id,
                summary,
                encrypted_content,
            } => {
                content.extend(
                    reasoning_content(id.as_deref(), summary, encrypted_content.as_deref())
                        .map_err(|error| ProviderError::ExecutionError(error.to_string()))?,
                );
            }
            ResponseOutputItem::Message {
                status,
                content: msg_content,
                ..
            } => {
                let item_incomplete = status.as_deref() == Some("incomplete");
                for block in msg_content {
                    match block {
                        ResponseContentBlock::OutputText { text, .. } => {
                            if !text.is_empty() {
                                content.push(MessageContent::text(text));
                            }
                        }
                        ResponseContentBlock::Refusal { refusal } => {
                            if !refusal.is_empty() {
                                content.push(MessageContent::text(refusal));
                            }
                        }
                        ResponseContentBlock::ToolCall { id, name, input } => {
                            content.push(response_tool_request_from_value(
                                id.clone(),
                                name.clone(),
                                input.clone(),
                                output_token_limit_reached || item_incomplete,
                            ));
                        }
                    }
                }
            }
            ResponseOutputItem::FunctionCall {
                id,
                call_id,
                name,
                arguments,
                status,
            } => {
                let request_id = call_id.clone().or_else(|| id.clone()).ok_or_else(|| {
                    ProviderError::ExecutionError(
                        "Responses function_call output missing call_id and id".to_string(),
                    )
                })?;
                let item_incomplete = status.as_deref() == Some("incomplete");
                content.push(response_tool_request_from_arguments(
                    request_id,
                    name.clone(),
                    arguments,
                    output_token_limit_reached || item_incomplete,
                ));
            }
        }
    }

    if output_token_limit_reached {
        content.push(MessageContent::text(OUTPUT_TRUNCATION_MARKER));
    }

    let mut message = Message::new(Role::Assistant, chrono::Utc::now().timestamp(), content);

    message = message.with_id(response.id.clone());
    message.metadata.output_token_limit_reached = output_token_limit_reached;

    Ok(message)
}

pub fn get_responses_usage(response: &ResponsesApiResponse) -> Usage {
    response
        .usage
        .as_ref()
        .map_or_else(Usage::default, ResponseUsage::to_usage)
}

fn process_streaming_output_items(
    output_items: Vec<ResponseOutputItemInfo>,
    is_text_response: bool,
    output_token_limit_reached: bool,
) -> anyhow::Result<Vec<MessageContent>> {
    let mut content = Vec::new();

    for item in output_items {
        match item {
            ResponseOutputItemInfo::Reasoning {
                id,
                summary,
                encrypted_content,
            } => {
                content.extend(reasoning_content(
                    id.as_deref(),
                    &summary,
                    encrypted_content.as_deref(),
                )?);
            }
            ResponseOutputItemInfo::Message {
                status,
                content: parts,
                ..
            } => {
                let item_incomplete = status.as_deref() == Some("incomplete");
                for part in parts {
                    match part {
                        ContentPart::OutputText { text, .. } => {
                            if !text.is_empty() && !is_text_response {
                                content.push(MessageContent::text(&text));
                            }
                        }
                        ContentPart::Refusal { refusal } => {
                            if !refusal.is_empty() && !is_text_response {
                                content.push(MessageContent::text(&refusal));
                            }
                        }
                        ContentPart::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            content.push(response_tool_request_from_arguments(
                                id,
                                name,
                                &arguments,
                                output_token_limit_reached || item_incomplete,
                            ));
                        }
                    }
                }
            }
            ResponseOutputItemInfo::FunctionCall {
                id,
                call_id,
                name,
                arguments,
                status,
            } => {
                let request_id = call_id.or(id).ok_or_else(|| {
                    anyhow!("Responses function_call output missing call_id and id")
                })?;
                content.push(response_tool_request_from_arguments(
                    request_id,
                    name,
                    &arguments,
                    output_token_limit_reached || status.as_deref() == Some("incomplete"),
                ));
            }
        }
    }

    if output_token_limit_reached {
        content.push(MessageContent::text(OUTPUT_TRUNCATION_MARKER));
    }

    Ok(content)
}

pub fn responses_api_to_streaming_message<S>(
    mut stream: S,
) -> impl Stream<Item = anyhow::Result<(Option<Message>, Option<ProviderUsage>)>> + 'static
where
    S: Stream<Item = anyhow::Result<String>> + Unpin + Send + 'static,
{
    try_stream! {
        use futures::StreamExt;

        let mut accumulated_text = String::new();
        let mut response_id: Option<String> = None;
        let mut model_name: Option<String> = None;
        let mut final_usage: Option<ProviderUsage> = None;
        let mut output_items: Vec<ResponseOutputItemInfo> = Vec::new();
        let mut is_text_response = false;
        let mut output_token_limit_reached = false;

        'outer: while let Some(response) = stream.next().await {
            let response_str = response?;

            // Skip empty lines
            if response_str.trim().is_empty() {
                continue;
            }
            if response_str.starts_with(':') {
                continue;
            }

            // Parse SSE format: "event: <type>\ndata: <json>"
            // For now, we only care about the data line
            // SSE spec allows both "data: value" and "data:value" (space after colon is optional)
            let data_line = if response_str.starts_with("data: ") {
                response_str.strip_prefix("data: ").unwrap()
            } else if response_str.starts_with("data:") {
                response_str.strip_prefix("data:").unwrap()
            } else if response_str.starts_with("event: ") || response_str.starts_with("event:") {
                // Skip event type lines
                continue;
            } else {
                // Try to parse as-is when there's no prefix
                &response_str
            };

            if data_line == "[DONE]" {
                break 'outer;
            }

            let Some(event) = parse_responses_stream_event(data_line)? else {
                continue;
            };

            match event {
                ResponsesStreamEvent::ResponseCreated { response, .. } |
                ResponsesStreamEvent::ResponseInProgress { response, .. } => {
                    response_id = Some(response.id);
                    model_name = Some(response.model);
                }

                ResponsesStreamEvent::OutputTextDelta { delta, .. } => {
                    is_text_response = true;
                    if !delta.is_empty() {
                        accumulated_text.push_str(&delta);

                        let mut message = Message::new(
                            Role::Assistant,
                            chrono::Utc::now().timestamp(),
                            vec![MessageContent::text(&delta)],
                        );
                        if let Some(id) = &response_id {
                            message = message.with_id(id.clone());
                        }
                        yield (Some(message), None);
                    }
                }

                ResponsesStreamEvent::OutputItemDone { item, .. } => {
                    output_items.push(item);
                }

                ResponsesStreamEvent::OutputTextDone { .. } => {
                }

                ResponsesStreamEvent::ResponseCompleted { response, .. } => {
                    let model = if response.model.is_empty() {
                        model_name.as_deref().unwrap_or_default()
                    } else {
                        &response.model
                    };
                    let usage = response.usage.as_ref().map_or_else(
                        Usage::default,
                        ResponseUsage::to_usage,
                    );
                    final_usage = Some(ProviderUsage::new(model.to_string(), usage));
                    response_id = Some(response.id.clone());

                    // For complete output, use the response output items
                    if !response.output.is_empty() {
                        output_items = response.output;
                    }

                    break 'outer;
                }

                ResponsesStreamEvent::ResponseIncomplete { response, .. } => {
                    let reason = response
                        .incomplete_details
                        .as_ref()
                        .and_then(|details| details.reason.as_deref())
                        .unwrap_or("unknown")
                        .to_string();
                    let usage = response_provider_usage(&response, model_name.as_deref());

                    match reason.as_str() {
                        "max_output_tokens" | "max_tokens" => {
                            let model = if response.model.is_empty() {
                                model_name.clone().unwrap_or_default()
                            } else {
                                response.model.clone()
                            };
                            response_id = Some(response.id.clone());
                            final_usage = Some(usage.unwrap_or_else(|| {
                                ProviderUsage::new(model, Usage::default())
                            }));
                            if !response.output.is_empty() {
                                output_items = response.output;
                            }
                            output_token_limit_reached = true;
                            break 'outer;
                        }
                        "content_filter" => {
                            if let Some(usage) = usage {
                                yield (None, Some(usage));
                            }
                            Err::<(), ProviderError>(response_failure_error(
                                "Responses API returned an incomplete response",
                                Some(&reason),
                                Some("response generation was stopped by the provider"),
                            ))?;
                        }
                        _ => {
                            if let Some(usage) = usage {
                                yield (None, Some(usage));
                            }
                            Err::<(), ProviderError>(response_failure_error(
                                "Responses API returned an incomplete response",
                                Some(&reason),
                                Some("response generation did not complete"),
                            ))?;
                        }
                    }
                }

                ResponsesStreamEvent::FunctionCallArgumentsDelta { .. } => {
                }

                ResponsesStreamEvent::FunctionCallArgumentsDone { .. } => {
                    // Arguments are complete, will be in the OutputItemDone event
                }

                ResponsesStreamEvent::RefusalDelta { delta, .. } => {
                    is_text_response = true;
                    if !delta.is_empty() {
                        accumulated_text.push_str(&delta);

                        let mut message = Message::new(
                            Role::Assistant,
                            chrono::Utc::now().timestamp(),
                            vec![MessageContent::text(&delta)],
                        );
                        if let Some(id) = &response_id {
                            message = message.with_id(id.clone());
                        }
                        yield (Some(message), None);
                    }
                }

                ResponsesStreamEvent::RefusalDone { .. } => {
                }

                ResponsesStreamEvent::ResponseFailed { response, .. } => {
                    if let Some(usage) = response_provider_usage(&response, model_name.as_deref()) {
                        yield (None, Some(usage));
                    }
                    let error = response.error.as_ref();
                    Err::<(), ProviderError>(response_failure_error(
                        "Responses API failed",
                        error.and_then(|value| value.code.as_deref()),
                        error.map(|value| value.message.as_str()),
                    ))?;
                }

                ResponsesStreamEvent::Error {
                    code,
                    message,
                    error,
                    ..
                } => {
                    let nested = error.as_ref();
                    Err::<(), ProviderError>(response_failure_error(
                        "Responses API error",
                        code.as_deref()
                            .or_else(|| nested.and_then(|value| value.code.as_deref())),
                        message
                            .as_deref()
                            .or_else(|| nested.map(|value| value.message.as_str())),
                    ))?;
                }

                _ => {
                    // Ignore other event types (OutputItemAdded, ContentPartAdded, ContentPartDone)
                }
            }
        }

        // Process final output items and yield usage data
        let content = process_streaming_output_items(
            output_items,
            is_text_response,
            output_token_limit_reached,
        )?;

        if !content.is_empty() {
            let mut message = Message::new(Role::Assistant, chrono::Utc::now().timestamp(), content);
            if let Some(id) = response_id {
                message = message.with_id(id);
            }
            message.metadata.output_token_limit_reached = output_token_limit_reached;
            yield (Some(message), final_usage);
        } else if let Some(usage) = final_usage {
            yield (None, Some(usage));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::message::MessageContent;
    use crate::model::ModelConfig;
    use futures::StreamExt;
    use rmcp::model::CallToolRequestParams;
    use rmcp::object;

    #[tokio::test]
    async fn responses_stream_accepts_numeric_created_at_from_meta() -> anyhow::Result<()> {
        let lines = vec![
            r#"data: {"type":"response.created","sequence_number":1,"response":{"id":"resp_meta_float","object":"response","created_at":1787589643.0,"status":"in_progress","model":"muse-spark-1.2","output":[]}}"#.to_string(),
            r#"data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_meta_float","output_index":0,"content_index":0,"delta":"ready"}"#.to_string(),
            r#"data: {"type":"response.completed","sequence_number":3,"response":{"id":"resp_meta_float","object":"response","created_at":1787589643.75,"status":"completed","model":"muse-spark-1.2","output":[],"usage":{"input_tokens":12,"output_tokens":3,"total_tokens":15}}}"#.to_string(),
        ];
        let stream =
            responses_api_to_streaming_message(tokio_stream::iter(lines.into_iter().map(Ok)));
        futures::pin_mut!(stream);

        let (message, usage) = stream.next().await.unwrap()?;
        assert_eq!(message.unwrap().as_concat_text(), "ready");
        assert!(usage.is_none());

        let (message, usage) = stream.next().await.unwrap()?;
        assert!(message.is_none());
        let usage = usage.expect("terminal usage must survive numeric created_at");
        assert_eq!(usage.model, "muse-spark-1.2");
        assert_eq!(usage.usage.total_tokens, Some(15));
        assert!(stream.next().await.is_none());
        Ok(())
    }

    #[test]
    fn nonstream_response_truncates_fractional_created_at_to_unix_seconds() {
        let response: ResponsesApiResponse = serde_json::from_value(json!({
            "id": "resp_meta_float",
            "object": "response",
            "created_at": 1787589643.75,
            "status": "completed",
            "model": "muse-spark-1.2",
            "output": []
        }))
        .unwrap();

        assert_eq!(response.created_at, 1787589643);
        assert_eq!(
            serde_json::to_value(response).unwrap()["created_at"],
            1787589643
        );
    }

    #[tokio::test]
    async fn test_responses_stream_ignores_keepalive_event() -> anyhow::Result<()> {
        let lines = vec![
            r#"data: {"type":"response.created","sequence_number":1,"response":{"id":"resp_1","object":"response","created_at":1737368310,"status":"in_progress","model":"gpt-5.2-pro","output":[]}}"#.to_string(),
            r#"data: {"type":"keepalive"}"#.to_string(),
            r#"data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_1","output_index":0,"content_index":0,"delta":"Hello"}"#.to_string(),
            r#"data: {"type":"response.output_text.delta","sequence_number":3,"item_id":"msg_1","output_index":0,"content_index":0,"delta":" world"}"#.to_string(),
            r#"data: {"type":"response.completed","sequence_number":4,"response":{"id":"resp_1","object":"response","created_at":1737368310,"status":"completed","model":"gpt-5.2-pro","output":[],"usage":{"input_tokens":10,"output_tokens":4,"total_tokens":14,"input_tokens_details":{"cached_tokens":6}}}}"#.to_string(),
            "data: [DONE]".to_string(),
        ];

        let response_stream = tokio_stream::iter(lines.into_iter().map(Ok));
        let messages = responses_api_to_streaming_message(response_stream);
        futures::pin_mut!(messages);

        let mut text_parts = Vec::new();
        let mut usage: Option<ProviderUsage> = None;

        while let Some(item) = messages.next().await {
            let (message, maybe_usage) = item?;
            if let Some(msg) = message {
                for content in msg.content {
                    if let MessageContent::Text(text) = content {
                        text_parts.push(text.text.clone());
                    }
                }
            }
            if let Some(final_usage) = maybe_usage {
                usage = Some(final_usage);
            }
        }

        assert_eq!(text_parts.concat(), "Hello world");
        let usage = usage.expect("usage should be present at completion");
        assert_eq!(usage.model, "gpt-5.2-pro");
        assert_eq!(usage.usage.input_tokens, Some(10));
        assert_eq!(usage.usage.output_tokens, Some(4));
        assert_eq!(usage.usage.total_tokens, Some(14));
        assert_eq!(usage.usage.cache_read_input_tokens, Some(6));
        assert_eq!(usage.usage.cache_write_input_tokens, None);

        Ok(())
    }

    #[tokio::test]
    async fn test_responses_stream_completed_allows_missing_output() -> anyhow::Result<()> {
        let lines = vec![
            r#"data: {"type":"response.created","sequence_number":1,"response":{"id":"resp_1","object":"response","created_at":1737368310,"status":"in_progress","model":"gpt-5.2-pro","output":[]}}"#.to_string(),
            r#"data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_1","output_index":0,"content_index":0,"delta":"Hello"}"#.to_string(),
            r#"data: {"type":"response.output_text.delta","sequence_number":3,"item_id":"msg_1","output_index":0,"content_index":0,"delta":" world"}"#.to_string(),
            r#"data: {"type":"response.completed","sequence_number":4,"response":{"id":"resp_1","object":"response","created_at":1737368310,"status":"completed","model":"gpt-5.2-pro","usage":{"input_tokens":10,"output_tokens":4,"total_tokens":14}}}"#.to_string(),
            "data: [DONE]".to_string(),
        ];

        let response_stream = tokio_stream::iter(lines.into_iter().map(Ok));
        let messages = responses_api_to_streaming_message(response_stream);
        futures::pin_mut!(messages);

        let mut text_parts = Vec::new();
        let mut usage: Option<ProviderUsage> = None;

        while let Some(item) = messages.next().await {
            let (message, maybe_usage) = item?;
            if let Some(msg) = message {
                for content in msg.content {
                    if let MessageContent::Text(text) = content {
                        text_parts.push(text.text.clone());
                    }
                }
            }
            if let Some(final_usage) = maybe_usage {
                usage = Some(final_usage);
            }
        }

        assert_eq!(text_parts.concat(), "Hello world");
        let usage = usage.expect("usage should be present at completion");
        assert_eq!(usage.model, "gpt-5.2-pro");
        assert_eq!(usage.usage.input_tokens, Some(10));
        assert_eq!(usage.usage.output_tokens, Some(4));
        assert_eq!(usage.usage.total_tokens, Some(14));

        Ok(())
    }

    #[tokio::test]
    async fn test_responses_stream_allows_message_output_without_id_status() -> anyhow::Result<()> {
        let lines = vec![
            r#"data: {"type":"response.created","sequence_number":1,"response":{"id":"resp_1","object":"response","created_at":1737368310,"status":"in_progress","model":"gpt-5.2-pro","output":[]}}"#.to_string(),
            r#"data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_1","output_index":0,"content_index":0,"delta":"Hello"}"#.to_string(),
            r#"data: {"type":"response.output_text.delta","sequence_number":3,"item_id":"msg_1","output_index":0,"content_index":0,"delta":" world"}"#.to_string(),
            r#"data: {"type":"response.completed","sequence_number":4,"response":{"id":"resp_1","object":"response","created_at":1737368310,"status":"completed","model":"gpt-5.2-pro","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hello world"}]}],"usage":{"input_tokens":10,"output_tokens":4,"total_tokens":14}}}"#.to_string(),
            "data: [DONE]".to_string(),
        ];

        let response_stream = tokio_stream::iter(lines.into_iter().map(Ok));
        let messages = responses_api_to_streaming_message(response_stream);
        futures::pin_mut!(messages);

        let mut text_parts = Vec::new();
        let mut usage: Option<ProviderUsage> = None;

        while let Some(item) = messages.next().await {
            let (message, maybe_usage) = item?;
            if let Some(msg) = message {
                for content in msg.content {
                    if let MessageContent::Text(text) = content {
                        text_parts.push(text.text.clone());
                    }
                }
            }
            if let Some(final_usage) = maybe_usage {
                usage = Some(final_usage);
            }
        }

        assert_eq!(text_parts.concat(), "Hello world");
        let usage = usage.expect("usage should be present at completion");
        assert_eq!(usage.model, "gpt-5.2-pro");
        assert_eq!(usage.usage.input_tokens, Some(10));
        assert_eq!(usage.usage.output_tokens, Some(4));
        assert_eq!(usage.usage.total_tokens, Some(14));

        Ok(())
    }

    #[tokio::test]
    async fn test_responses_stream_allows_function_call_without_id_status() -> anyhow::Result<()> {
        let lines = vec![
            r#"data: {"type":"response.created","sequence_number":1,"response":{"id":"resp_1","object":"response","created_at":1737368310,"status":"in_progress","model":"gpt-5.2-pro","output":[]}}"#.to_string(),
            r#"data: {"type":"response.completed","sequence_number":2,"response":{"id":"resp_1","object":"response","created_at":1737368310,"status":"completed","model":"gpt-5.2-pro","output":[{"type":"reasoning","summary":[]},{"type":"function_call","call_id":"call_abc","name":"shell","arguments":"{\"command\":\"pwd\"}"}],"usage":{"input_tokens":10,"output_tokens":4,"total_tokens":14}}}"#.to_string(),
            "data: [DONE]".to_string(),
        ];

        let response_stream = tokio_stream::iter(lines.into_iter().map(Ok));
        let messages = responses_api_to_streaming_message(response_stream);
        futures::pin_mut!(messages);

        let mut tool_request_id = None;
        let mut usage: Option<ProviderUsage> = None;

        while let Some(item) = messages.next().await {
            let (message, maybe_usage) = item?;
            if let Some(msg) = message {
                for content in msg.content {
                    if let MessageContent::ToolRequest(request) = content {
                        tool_request_id = Some(request.id);
                    }
                }
            }
            if let Some(final_usage) = maybe_usage {
                usage = Some(final_usage);
            }
        }

        assert_eq!(tool_request_id.as_deref(), Some("call_abc"));
        let usage = usage.expect("usage should be present at completion");
        assert_eq!(usage.model, "gpt-5.2-pro");
        assert_eq!(usage.usage.total_tokens, Some(14));

        Ok(())
    }

    #[test]
    fn test_responses_api_to_message_captures_reasoning_summary() -> anyhow::Result<()> {
        let response: ResponsesApiResponse = serde_json::from_value(serde_json::json!({
            "id": "resp_1",
            "object": "response",
            "created_at": 1737368310,
            "status": "completed",
            "model": "gpt-5",
            "output": [
                {
                    "type": "reasoning",
                    "id": "rs_1",
                    "summary": [
                        { "type": "summary_text", "text": "Thinking about the question..." },
                        { "type": "summary_text", "text": "The answer is straightforward." }
                    ]
                },
                {
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [
                        { "type": "output_text", "text": "The capital of France is Paris." }
                    ]
                }
            ]
        }))?;

        let message = responses_api_to_message(&response)?;

        let thinking = message.content.iter().find_map(|c| c.as_thinking());
        assert!(thinking.is_some(), "should contain thinking content");
        assert_eq!(
            thinking.unwrap().thinking,
            "Thinking about the question...\nThe answer is straightforward."
        );

        let text = message.content.iter().find_map(|c| c.as_text());
        assert_eq!(text, Some("The capital of France is Paris."));

        Ok(())
    }

    #[tokio::test]
    async fn test_responses_stream_captures_reasoning_summary() -> anyhow::Result<()> {
        let reasoning_item = serde_json::json!({
            "type": "reasoning",
            "id": "rs_1",
            "summary": [
                { "type": "summary_text", "text": "Let me think step by step." }
            ]
        });
        let message_item = serde_json::json!({
            "type": "message",
            "id": "msg_1",
            "status": "completed",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "Paris." }]
        });

        let lines = vec![
            format!(
                r#"data: {{"type":"response.created","sequence_number":1,"response":{{"id":"resp_1","object":"response","created_at":1737368310,"status":"in_progress","model":"gpt-5","output":[]}}}}"#
            ),
            format!(
                r#"data: {{"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_1","output_index":1,"content_index":0,"delta":"Paris."}}"#
            ),
            format!(
                r#"data: {{"type":"response.output_item.done","sequence_number":3,"output_index":0,"item":{}}}"#,
                serde_json::to_string(&reasoning_item)?
            ),
            format!(
                r#"data: {{"type":"response.output_item.done","sequence_number":4,"output_index":1,"item":{}}}"#,
                serde_json::to_string(&message_item)?
            ),
            format!(
                r#"data: {{"type":"response.completed","sequence_number":5,"response":{{"id":"resp_1","object":"response","created_at":1737368310,"status":"completed","model":"gpt-5","output":[{},{}],"usage":{{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}}}}"#,
                serde_json::to_string(&reasoning_item)?,
                serde_json::to_string(&message_item)?
            ),
            "data: [DONE]".to_string(),
        ];

        let response_stream = tokio_stream::iter(lines.into_iter().map(Ok));
        let messages = responses_api_to_streaming_message(response_stream);
        futures::pin_mut!(messages);

        let mut thinking_parts = Vec::new();
        let mut text_parts = Vec::new();

        while let Some(item) = messages.next().await {
            let (message, _) = item?;
            if let Some(msg) = message {
                for content in msg.content {
                    match &content {
                        MessageContent::Thinking(t) => thinking_parts.push(t.thinking.clone()),
                        MessageContent::Text(t) => text_parts.push(t.text.clone()),
                        _ => {}
                    }
                }
            }
        }

        assert!(
            !thinking_parts.is_empty(),
            "should capture thinking from stream"
        );
        assert_eq!(thinking_parts.join(""), "Let me think step by step.");
        assert!(text_parts.concat().contains("Paris."));

        Ok(())
    }

    #[tokio::test]
    async fn test_responses_stream_error_event_still_returns_error() -> anyhow::Result<()> {
        let lines = vec![
            r#"data: {"type":"error","error":{"message":"boom"}}"#.to_string(),
            "data: [DONE]".to_string(),
        ];

        let response_stream = tokio_stream::iter(lines.into_iter().map(Ok));
        let messages = responses_api_to_streaming_message(response_stream);
        futures::pin_mut!(messages);

        let first = messages
            .next()
            .await
            .expect("stream should emit an error item");

        assert!(first.is_err());
        assert!(first
            .expect_err("expected error")
            .to_string()
            .contains("Responses API error"));

        Ok(())
    }

    #[test]
    fn test_history_preserves_chronological_order() {
        let model_config = ModelConfig {
            model_name: "gpt-5.2-codex".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let messages = vec![
            Message::assistant()
                .with_text("I'll create that file.")
                .with_tool_request(
                    "call_1",
                    Ok(CallToolRequestParams::new("shell")
                        .with_arguments(object!({"command": "echo hello"}))),
                ),
            Message::assistant()
                .with_text("Now let me verify.")
                .with_tool_request(
                    "call_2",
                    Ok(CallToolRequestParams::new("shell")
                        .with_arguments(object!({"command": "cat file.txt"}))),
                ),
        ];

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        let types: Vec<&str> = input
            .iter()
            .map(|item| {
                item.get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| item["role"].as_str().unwrap())
            })
            .collect();

        assert_eq!(
            types,
            vec!["assistant", "function_call", "assistant", "function_call"]
        );
    }

    #[test]
    fn test_responses_api_to_message_uses_call_id_for_tool_request_id() {
        let response = ResponsesApiResponse {
            id: "resp_1".to_string(),
            object: "response".to_string(),
            created_at: 0,
            status: "completed".to_string(),
            model: "gpt-5.3-codex".to_string(),
            output: vec![ResponseOutputItem::FunctionCall {
                id: Some("fc_123".to_string()),
                status: Some("completed".to_string()),
                call_id: Some("call_abc".to_string()),
                name: "test__get_person_zip_code".to_string(),
                arguments: r#"{"name":"Alice Burns"}"#.to_string(),
            }],
            reasoning: None,
            usage: None,
            incomplete_details: None,
            error: None,
        };

        let message = responses_api_to_message(&response).unwrap();
        assert_eq!(message.content.len(), 1);
        let MessageContent::ToolRequest(tool_request) = &message.content[0] else {
            panic!("expected tool request content");
        };
        assert_eq!(tool_request.id, "call_abc");
    }

    #[test]
    fn test_deserialize_reasoning_info_with_null_effort() {
        let json = r#"{"effort": null}"#;
        let info: ResponseReasoningInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.effort, None);
        assert_eq!(info.summary, None);
    }

    #[test]
    fn test_deserialize_reasoning_info_with_effort() {
        let json = r#"{"effort": "high", "summary": "Thought deeply"}"#;
        let info: ResponseReasoningInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.effort.as_deref(), Some("high"));
        assert_eq!(info.summary.as_deref(), Some("Thought deeply"));
    }

    #[test]
    fn test_responses_tools_include_strict_false() {
        let model_config = ModelConfig {
            model_name: "gpt-5.4".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let tool = Tool::new(
            "shell",
            "Execute a shell command",
            object!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command to run"
                    }
                },
                "required": ["command"]
            }),
        );

        let result =
            create_responses_request(&model_config, "You are helpful.", &[], &[tool]).unwrap();
        let tools = result["tools"]
            .as_array()
            .expect("tools should be an array");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0]["strict"],
            json!(false),
            "Responses API defaults strict to true, but MCP tool schemas are not strict-compatible; must explicitly set strict: false"
        );
    }

    #[test]
    fn test_responses_request_with_explicit_effort_suffix() {
        for (model_name, expected_model, expected_effort) in [
            ("gpt-5.4-xhigh", "gpt-5.4", "xhigh"),
            ("databricks-gpt-5.4-high", "databricks-gpt-5.4", "high"),
            ("databricks-o3-none", "databricks-o3", "none"),
        ] {
            let model_config = ModelConfig {
                model_name: model_name.to_string(),
                context_limit: None,
                temperature: None,
                max_tokens: None,
                toolshim: false,
                toolshim_model: None,
                request_params: None,
                reasoning: None,
            };

            let result =
                create_responses_request(&model_config, "You are helpful.", &[], &[]).unwrap();

            assert_eq!(
                result["model"], expected_model,
                "unexpected model for {model_name}"
            );
            assert_eq!(
                result["reasoning"]["effort"], expected_effort,
                "unexpected effort for {model_name}"
            );
            assert_eq!(result["reasoning"]["summary"], "auto");
        }
    }

    #[test]
    fn test_responses_request_with_normalized_effort_suffix() {
        let model_config = ModelConfig::new("o3-mini-high");

        let result = create_responses_request(&model_config, "You are helpful.", &[], &[]).unwrap();

        assert_eq!(result["model"], "o3-mini");
        assert_eq!(result["reasoning"]["effort"], "high");
        assert_eq!(result["reasoning"]["summary"], "auto");
    }

    #[test]
    fn test_responses_request_without_effort_suffix_omits_reasoning() {
        for model_name in ["gpt-5.4", "o3", "gpt-5-nano"] {
            let model_config = ModelConfig {
                model_name: model_name.to_string(),
                context_limit: None,
                temperature: None,
                max_tokens: None,
                toolshim: false,
                toolshim_model: None,
                request_params: None,
                reasoning: None,
            };

            let result =
                create_responses_request(&model_config, "You are helpful.", &[], &[]).unwrap();

            assert_eq!(result["model"], model_name, "model should be unchanged");
            assert!(
                result.get("reasoning").is_none(),
                "reasoning should be omitted for {model_name} without explicit effort suffix"
            );
        }
    }

    #[test]
    fn test_responses_request_non_reasoning_model_ignores_global_thinking_effort() {
        let _guard = env_lock::lock_env([("GOOSE_THINKING_EFFORT", Some("high"))]);
        let model_config = ModelConfig {
            model_name: "gpt-4o".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "You are helpful.", &[], &[]).unwrap();

        assert_eq!(result["model"], "gpt-4o");
        assert!(
            result.get("reasoning").is_none(),
            "non-reasoning models should not receive reasoning config"
        );
    }

    #[test]
    fn test_request_params_override_store() {
        let model_config = ModelConfig {
            model_name: "o3".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: Some(std::collections::HashMap::from([(
                "store".to_string(),
                serde_json::json!(true),
            )])),
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &[], &[]).unwrap();

        assert_eq!(result["store"], true);
    }

    #[test]
    fn test_user_image_serialized_in_responses_request() {
        use crate::conversation::message::Message;

        let messages = vec![Message::user()
            .with_text("describe this image")
            .with_image("aW1hZ2VkYXRh", "image/png")];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result =
            create_responses_request(&model_config, "You are helpful.", &messages, &[]).unwrap();

        let input = result["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);

        assert_eq!(input[0]["role"], "system");

        assert_eq!(input[1]["role"], "user");
        let content = input[1]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);

        assert_eq!(content[0]["type"], "input_text");
        assert_eq!(content[0]["text"], "describe this image");

        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(
            content[1]["image_url"],
            "data:image/png;base64,aW1hZ2VkYXRh"
        );
    }

    #[test]
    fn test_tool_response_with_image_serializes_as_typed_array() {
        use crate::conversation::message::Message;
        use rmcp::model::{CallToolResult, Content};

        let messages = vec![Message::user().with_content(MessageContent::tool_response(
            "call_1",
            Ok(CallToolResult::success(vec![
                Content::text("caption"),
                Content::image("a+/=".to_string(), "image/png".to_string()),
            ])),
        ))];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_1");

        let output = input[0]["output"].as_array().unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], json!({"type": "input_text", "text": "caption"}));
        assert_eq!(
            output[1],
            json!({"type": "input_image", "image_url": "data:image/png;base64,a+/="})
        );
    }

    #[test]
    fn test_tool_request_serializes_function_call_with_arguments() {
        use crate::conversation::message::Message;

        let messages = vec![Message::assistant().with_tool_request(
            "call_1",
            Ok(CallToolRequestParams::new("search")
                .with_arguments(object!({"q": "rust", "limit": 2}))),
        )];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[0]["name"], "search");

        let args: serde_json::Value =
            serde_json::from_str(input[0]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["q"], "rust");
        assert_eq!(args["limit"], 2);
    }

    #[test]
    fn test_tool_request_none_arguments_serializes_empty_object() {
        use crate::conversation::message::Message;

        let messages = vec![Message::assistant()
            .with_tool_request("call_1", Ok(CallToolRequestParams::new("noop")))];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["name"], "noop");
        assert_eq!(input[0]["arguments"], "{}");
    }

    #[test]
    fn test_text_flushed_before_tool_request() {
        use crate::conversation::message::Message;

        let messages = vec![Message::assistant()
            .with_text("planning")
            .with_tool_request(
                "call_1",
                Ok(CallToolRequestParams::new("shell").with_arguments(object!({"command": "ls"}))),
            )];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "assistant");
        assert_eq!(input[0]["content"][0]["type"], "output_text");
        assert_eq!(input[0]["content"][0]["text"], "planning");
        assert_eq!(input[1]["type"], "function_call");
    }

    #[test]
    fn test_text_flushed_before_tool_response() {
        use crate::conversation::message::Message;
        use rmcp::model::{CallToolResult, Content};

        let messages =
            vec![Message::user()
                .with_text("context")
                .with_content(MessageContent::tool_response(
                    "call_1",
                    Ok(CallToolResult::success(vec![Content::text("done")])),
                ))];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "context");
        assert_eq!(input[1]["type"], "function_call_output");
        assert_eq!(input[1]["output"], "done");
    }

    #[test]
    fn test_tool_response_error_serializes_with_error_prefix() {
        use crate::conversation::message::Message;
        use rmcp::model::{ErrorCode, ErrorData};

        let messages = vec![Message::user().with_content(MessageContent::tool_response(
            "call_err",
            Err(ErrorData {
                code: ErrorCode::INTERNAL_ERROR,
                message: "file not found".into(),
                data: None,
            }),
        ))];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_err");
        assert_eq!(input[0]["output"], "Error: file not found");
    }

    #[test]
    fn test_image_only_message_serializes() {
        use crate::conversation::message::Message;

        let messages = vec![Message::user().with_image("aW1n", "image/png")];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        let content = input[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "input_image");
        assert_eq!(content[0]["image_url"], "data:image/png;base64,aW1n");
    }

    #[test]
    fn test_multiple_images_preserved_in_order() {
        use crate::conversation::message::Message;

        let messages = vec![Message::user()
            .with_text("compare")
            .with_image("img1", "image/png")
            .with_image("img2", "image/jpeg")];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input[0]["role"], "user");
        let content = input[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 3);
        assert_eq!(content[0]["type"], "input_text");
        assert_eq!(content[0]["text"], "compare");
        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(content[1]["image_url"], "data:image/png;base64,img1");
        assert_eq!(content[2]["type"], "input_image");
        assert_eq!(content[2]["image_url"], "data:image/jpeg;base64,img2");
    }

    #[test]
    fn test_assistant_text_uses_output_text_type() {
        use crate::conversation::message::Message;

        let messages = vec![Message::assistant().with_text("hello")];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input[0]["role"], "assistant");
        assert_eq!(input[0]["content"][0]["type"], "output_text");
        assert_eq!(input[0]["content"][0]["text"], "hello");
    }

    #[test]
    fn test_refusal_content_block_deserializes_in_non_streaming_response() {
        let json = r#"{
            "id": "resp_1",
            "object": "response",
            "created_at": 0,
            "status": "completed",
            "model": "gpt-5.5",
            "output": [{
                "type": "message",
                "id": "msg_1",
                "status": "completed",
                "role": "assistant",
                "content": [{"type": "refusal", "refusal": "I cannot help with that request."}]
            }]
        }"#;

        let response: ResponsesApiResponse = serde_json::from_str(json).unwrap();
        let message = responses_api_to_message(&response).unwrap();
        assert_eq!(message.content.len(), 1);
        if let MessageContent::Text(t) = &message.content[0] {
            assert_eq!(t.text, "I cannot help with that request.");
        } else {
            panic!("expected text content from refusal");
        }
    }

    #[test]
    fn test_refusal_content_part_deserializes_in_streaming_output() -> anyhow::Result<()> {
        let json = r#"{
            "type": "message",
            "id": "msg_1",
            "status": "completed",
            "role": "assistant",
            "content": [{"type": "refusal", "refusal": "I'm unable to assist."}]
        }"#;

        let item: ResponseOutputItemInfo = serde_json::from_str(json).unwrap();
        let content = process_streaming_output_items(vec![item], false, false)?;
        assert_eq!(content.len(), 1);
        if let MessageContent::Text(t) = &content[0] {
            assert_eq!(t.text, "I'm unable to assist.");
        } else {
            panic!("expected text content from refusal");
        }

        Ok(())
    }

    #[test]
    fn test_refusal_delta_stream_event_deserializes() {
        let json = r#"{"type":"response.refusal.delta","sequence_number":5,"item_id":"msg_1","output_index":0,"content_index":0,"delta":"I cannot"}"#;

        let event: ResponsesStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            ResponsesStreamEvent::RefusalDelta { delta, .. } => {
                assert_eq!(delta, "I cannot");
            }
            _ => panic!("expected RefusalDelta event"),
        }
    }

    #[test]
    fn test_streamed_refusal_not_duplicated_in_output_items() -> anyhow::Result<()> {
        let output_items = vec![ResponseOutputItemInfo::Message {
            id: Some("msg_1".to_string()),
            status: Some("completed".to_string()),
            role: "assistant".to_string(),
            content: vec![ContentPart::Refusal {
                refusal: "I cannot help with that.".to_string(),
            }],
        }];

        let content = process_streaming_output_items(output_items.clone(), true, false)?;
        assert!(
            content.is_empty(),
            "refusal should be suppressed when already streamed"
        );

        let content = process_streaming_output_items(output_items, false, false)?;
        assert_eq!(
            content.len(),
            1,
            "refusal should appear in non-streaming path"
        );

        Ok(())
    }

    #[test]
    fn test_function_call_output_requires_call_id_or_id() {
        let output_items = vec![ResponseOutputItemInfo::FunctionCall {
            id: None,
            status: None,
            call_id: None,
            name: "shell".to_string(),
            arguments: "{}".to_string(),
        }];

        let error = process_streaming_output_items(output_items, false, false).unwrap_err();
        assert!(
            error.to_string().contains("missing call_id and id"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_frontend_tool_request_serialized_in_responses_request() {
        use crate::conversation::message::Message;
        use rmcp::model::{CallToolResult, Content};

        let messages = vec![
            Message::assistant().with_frontend_tool_request(
                "call_ft1",
                Ok(CallToolRequestParams::new("browser_click")
                    .with_arguments(object!({"selector": "#btn"}))),
            ),
            Message::user().with_content(MessageContent::tool_response(
                "call_ft1",
                Ok(CallToolResult::success(vec![Content::text("clicked")])),
            )),
        ];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["call_id"], "call_ft1");
        assert_eq!(input[0]["name"], "browser_click");

        assert_eq!(input[1]["type"], "function_call_output");
        assert_eq!(input[1]["call_id"], "call_ft1");
        assert_eq!(input[1]["output"], "clicked");
    }

    #[test]
    fn test_tool_request_error_emits_function_call_output() {
        use crate::conversation::message::Message;
        use rmcp::model::{ErrorCode, ErrorData};

        let messages = vec![Message::assistant().with_tool_request(
            "call_err1",
            Err(ErrorData {
                code: ErrorCode::INTERNAL_ERROR,
                message: "invalid arguments".into(),
                data: None,
            }),
        )];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_err1");
        assert!(input[0]["output"]
            .as_str()
            .unwrap()
            .contains("invalid arguments"));
    }

    #[test]
    fn test_frontend_tool_request_error_emits_function_call_output() {
        use crate::conversation::message::Message;
        use rmcp::model::{ErrorCode, ErrorData};

        let messages = vec![Message::assistant().with_frontend_tool_request(
            "call_ft_err",
            Err(ErrorData {
                code: ErrorCode::INTERNAL_ERROR,
                message: "malformed arguments".into(),
                data: None,
            }),
        )];

        let model_config = ModelConfig {
            model_name: "gpt-5.5".to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        };

        let result = create_responses_request(&model_config, "", &messages, &[]).unwrap();
        let input = result["input"].as_array().unwrap();

        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_ft_err");
        assert!(input[0]["output"]
            .as_str()
            .unwrap()
            .contains("malformed arguments"));
    }

    #[test]
    fn responses_reasoning_round_trips_encrypted_content_with_tool_results() {
        use rmcp::model::{CallToolResult, Content};

        let response: ResponsesApiResponse = serde_json::from_value(json!({
            "id": "resp_meta_1",
            "object": "response",
            "created_at": 0,
            "status": "completed",
            "model": "muse-spark-1.2",
            "output": [
                {
                    "type": "reasoning",
                    "id": "reasoning_meta_1",
                    "summary": [{"type": "summary_text", "text": "Checked the workspace."}],
                    "encrypted_content": "opaque-encrypted-reasoning"
                },
                {
                    "type": "function_call",
                    "id": "fc_meta_1",
                    "status": "completed",
                    "call_id": "call_meta_1",
                    "name": "shell",
                    "arguments": "{\"command\":\"pwd\"}"
                }
            ]
        }))
        .unwrap();

        let assistant = responses_api_to_message(&response).unwrap();
        assert_eq!(assistant.content.len(), 3);
        assert!(matches!(assistant.content[0], MessageContent::Thinking(_)));
        assert!(matches!(
            assistant.content[1],
            MessageContent::RedactedThinking(_)
        ));
        assert!(matches!(
            assistant.content[2],
            MessageContent::ToolRequest(_)
        ));
        assert!(!assistant
            .as_concat_text()
            .contains("opaque-encrypted-reasoning"));
        assert_eq!(format!("{}", assistant.content[1]), "[RedactedThinking]");

        let tool_result = Message::user().with_tool_response(
            "call_meta_1",
            Ok(CallToolResult::success(vec![Content::text(
                "workspace path",
            )])),
        );
        let payload = create_responses_request(
            &ModelConfig::new("muse-spark-1.2"),
            "",
            &[assistant, tool_result],
            &[],
        )
        .unwrap();
        let input = payload["input"].as_array().unwrap();

        assert_eq!(input.len(), 3);
        assert_eq!(
            input[0],
            json!({
                "type": "reasoning",
                "id": "reasoning_meta_1",
                "summary": [{"type": "summary_text", "text": "Checked the workspace."}],
                "encrypted_content": "opaque-encrypted-reasoning"
            })
        );
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_meta_1");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_meta_1");
        assert_eq!(input[2]["output"], "workspace path");
    }

    #[test]
    fn responses_reasoning_replay_deduplicates_split_tool_messages() {
        let reasoning =
            reasoning_content(Some("reasoning_shared"), &[], Some("opaque-shared-content"))
                .unwrap()
                .pop()
                .unwrap();
        let messages = vec![
            Message::assistant()
                .with_content(reasoning.clone())
                .with_tool_request(
                    "call_a",
                    Ok(CallToolRequestParams::new("shell")
                        .with_arguments(object!({"command": "pwd"}))),
                ),
            Message::assistant()
                .with_content(reasoning)
                .with_tool_request(
                    "call_b",
                    Ok(CallToolRequestParams::new("shell")
                        .with_arguments(object!({"command": "ls"}))),
                ),
        ];

        let payload =
            create_responses_request(&ModelConfig::new("muse-spark-1.2"), "", &messages, &[])
                .unwrap();
        let input = payload["input"].as_array().unwrap();
        let reasoning_count = input
            .iter()
            .filter(|item| item["type"] == "reasoning")
            .count();
        let function_count = input
            .iter()
            .filter(|item| item["type"] == "function_call")
            .count();
        assert_eq!(reasoning_count, 1);
        assert_eq!(function_count, 2);
    }

    #[test]
    fn responses_reasoning_rejects_reused_id_with_different_content() {
        let first = reasoning_content(Some("reasoning_1"), &[], Some("opaque-a"))
            .unwrap()
            .pop()
            .unwrap();
        let second = reasoning_content(Some("reasoning_1"), &[], Some("opaque-b"))
            .unwrap()
            .pop()
            .unwrap();
        let error = create_responses_request(
            &ModelConfig::new("muse-spark-1.2"),
            "",
            &[
                Message::assistant().with_content(first),
                Message::assistant().with_content(second),
            ],
            &[],
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("reasoning id was reused with different content"));
    }

    #[test]
    fn responses_reasoning_rejects_encrypted_content_without_id() {
        let response: ResponsesApiResponse = serde_json::from_value(json!({
            "id": "resp_meta_missing_id",
            "object": "response",
            "created_at": 0,
            "status": "completed",
            "model": "muse-spark-1.2",
            "output": [{
                "type": "reasoning",
                "summary": [],
                "encrypted_content": "opaque-without-id"
            }]
        }))
        .unwrap();

        let error = responses_api_to_message(&response).unwrap_err();
        assert!(error
            .to_string()
            .contains("encrypted_content without an id"));
    }

    #[tokio::test]
    async fn streamed_responses_preserve_meta_reasoning_model_and_usage() -> anyhow::Result<()> {
        let lines = vec![
            r#"data: {"type":"response.created","sequence_number":1,"response":{"id":"resp_meta_stream","object":"response","created_at":0,"status":"in_progress","model":"muse-spark-1.2","output":[]}}"#.to_string(),
            r#"data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_meta_1","output_index":1,"content_index":0,"delta":"I will inspect the workspace."}"#.to_string(),
            r#"data: {"type":"response.completed","sequence_number":3,"response":{"id":"resp_meta_stream","object":"response","created_at":0,"status":"completed","model":"muse-spark-1.2","output":[{"type":"reasoning","id":"reasoning_stream_1","summary":[],"encrypted_content":"opaque-stream-content"},{"type":"message","id":"msg_meta_1","status":"completed","role":"assistant","content":[{"type":"output_text","text":"I will inspect the workspace."}]},{"type":"function_call","id":"fc_stream_1","status":"completed","call_id":"call_stream_1","name":"shell","arguments":"{\"command\":\"pwd\"}"}],"usage":{"input_tokens":120,"output_tokens":33,"total_tokens":153,"input_tokens_details":{"cached_tokens":20}}}}"#.to_string(),
            "data: [DONE]".to_string(),
        ];
        let messages =
            responses_api_to_streaming_message(tokio_stream::iter(lines.into_iter().map(Ok)));
        futures::pin_mut!(messages);
        let mut delivered = Vec::new();
        let mut final_usage = None;
        while let Some(item) = messages.next().await {
            let (message, usage) = item?;
            if let Some(message) = message {
                delivered.push(message);
            }
            if usage.is_some() {
                final_usage = usage;
            }
        }

        assert_eq!(delivered.len(), 2);
        assert_eq!(
            delivered[0].as_concat_text(),
            "I will inspect the workspace."
        );
        let message = &delivered[1];
        assert!(matches!(
            message.content[0],
            MessageContent::RedactedThinking(_)
        ));
        assert!(matches!(message.content[1], MessageContent::ToolRequest(_)));
        let usage = final_usage.expect("stream should yield provider usage");
        assert_eq!(usage.model, "muse-spark-1.2");
        assert_eq!(usage.usage.input_tokens, Some(120));
        assert_eq!(usage.usage.output_tokens, Some(33));
        assert_eq!(usage.usage.cache_read_input_tokens, Some(20));
        Ok(())
    }

    #[tokio::test]
    async fn streamed_text_only_reasoning_keeps_live_delta_and_terminal_reasoning(
    ) -> anyhow::Result<()> {
        let lines = vec![
            r#"data: {"type":"response.created","sequence_number":1,"response":{"id":"resp_meta_text","object":"response","created_at":0,"status":"in_progress","model":"muse-spark-1.2","output":[]}}"#.to_string(),
            r#"data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_meta_text","output_index":1,"content_index":0,"delta":"The result is ready."}"#.to_string(),
            r#"data: {"type":"response.completed","sequence_number":3,"response":{"id":"resp_meta_text","object":"response","created_at":0,"status":"completed","model":"muse-spark-1.2","output":[{"type":"reasoning","id":"reasoning_text_1","summary":[],"encrypted_content":"opaque-text-reasoning"},{"type":"message","id":"msg_meta_text","status":"completed","role":"assistant","content":[{"type":"output_text","text":"The result is ready."}]}],"usage":{"input_tokens":40,"output_tokens":8,"total_tokens":48}}}"#.to_string(),
            "data: [DONE]".to_string(),
        ];
        let messages =
            responses_api_to_streaming_message(tokio_stream::iter(lines.into_iter().map(Ok)));
        futures::pin_mut!(messages);
        let mut delivered = Vec::new();
        while let Some(item) = messages.next().await {
            let (message, _) = item?;
            if let Some(message) = message {
                delivered.push(message);
            }
        }

        assert_eq!(delivered.len(), 2);
        assert_eq!(delivered[0].as_concat_text(), "The result is ready.");
        assert!(matches!(
            delivered[1].content.as_slice(),
            [MessageContent::RedactedThinking(_)]
        ));
        Ok(())
    }

    fn assert_invalid_tool_request(content: &MessageContent, expected_detail: &str) {
        let MessageContent::ToolRequest(request) = content else {
            panic!("expected a tool request, got {content:?}");
        };
        let error = request
            .tool_call
            .as_ref()
            .expect_err("malformed arguments must not be executable");
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(
            error.message.contains(expected_detail),
            "unexpected tool error: {error:?}"
        );
    }

    #[test]
    fn nonstream_max_token_incomplete_preserves_partial_content_usage_and_reasoning() {
        for reason in ["max_output_tokens", "max_tokens"] {
            let response: ResponsesApiResponse = serde_json::from_value(json!({
                "id": "resp_incomplete",
                "object": "response",
                "created_at": 0,
                "status": "incomplete",
                "model": "muse-spark-1.2",
                "incomplete_details": { "reason": reason },
                "output": [
                    {
                        "type": "reasoning",
                        "id": "reasoning_incomplete",
                        "summary": [],
                        "encrypted_content": "opaque-incomplete-reasoning"
                    },
                    {
                        "type": "message",
                        "id": "msg_incomplete",
                        "status": "incomplete",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": "A useful partial answer."
                        }]
                    },
                    {
                        "type": "function_call",
                        "id": "fc_incomplete",
                        "status": "incomplete",
                        "call_id": "call_incomplete",
                        "name": "shell",
                        "arguments": "{\"command\":\"echo unfinished"
                    }
                ],
                "usage": {
                    "input_tokens": 71,
                    "output_tokens": 29,
                    "total_tokens": 100,
                    "input_tokens_details": { "cached_tokens": 11 }
                }
            }))
            .unwrap();

            let message = responses_api_to_message(&response).unwrap();
            assert!(message.metadata.output_token_limit_reached);
            let redacted = message
                .content
                .iter()
                .find_map(|content| match content {
                    MessageContent::RedactedThinking(redacted) => Some(&redacted.data),
                    _ => None,
                })
                .expect("encrypted reasoning must be retained for replay");
            let passback = decode_reasoning_passback(redacted)
                .unwrap()
                .expect("redacted reasoning must retain Responses replay data");
            assert_eq!(passback.id, "reasoning_incomplete");
            assert_eq!(passback.encrypted_content, "opaque-incomplete-reasoning");
            let text: Vec<_> = message
                .content
                .iter()
                .filter_map(MessageContent::as_text)
                .collect();
            assert_eq!(
                text,
                vec!["A useful partial answer.", OUTPUT_TRUNCATION_MARKER]
            );
            let tool = message
                .content
                .iter()
                .find(|content| matches!(content, MessageContent::ToolRequest(_)))
                .expect("partial tool call should be retained as a typed error");
            assert_invalid_tool_request(tool, "output-token limit");

            let usage = get_responses_usage(&response);
            assert_eq!(usage.input_tokens, Some(71));
            assert_eq!(usage.output_tokens, Some(29));
            assert_eq!(usage.total_tokens, Some(100));
            assert_eq!(usage.cache_read_input_tokens, Some(11));
        }
    }

    #[test]
    fn nonstream_incomplete_content_filter_and_unknown_are_typed_failures() {
        for (reason, expected_kind) in [
            ("content_filter", "refusal"),
            ("provider_specific_stop", "request"),
        ] {
            let response: ResponsesApiResponse = serde_json::from_value(json!({
                "id": "resp_incomplete_failure",
                "object": "response",
                "created_at": 0,
                "status": "incomplete",
                "model": "muse-spark-1.2",
                "incomplete_details": { "reason": reason },
                "output": [{
                    "type": "message",
                    "id": "msg_partial",
                    "status": "incomplete",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "partial"}]
                }],
                "usage": {
                    "input_tokens": 15,
                    "output_tokens": 3,
                    "total_tokens": 18
                }
            }))
            .unwrap();

            let error = responses_api_to_message(&response).unwrap_err();
            match (expected_kind, error) {
                (
                    "refusal",
                    ProviderError::Refusal {
                        category: Some(category),
                        ..
                    },
                ) => assert_eq!(category, "content_filter"),
                ("request", ProviderError::RequestFailed(details)) => {
                    assert!(details.contains("provider_specific_stop"));
                }
                (_, other) => panic!("unexpected incomplete-response error: {other:?}"),
            }
            assert_eq!(get_responses_usage(&response).total_tokens, Some(18));
        }
    }

    #[test]
    fn nonstream_failed_uses_official_nested_response_error() {
        let response: ResponsesApiResponse = serde_json::from_value(json!({
            "id": "resp_failed",
            "object": "response",
            "created_at": 0,
            "status": "failed",
            "model": "muse-spark-1.2",
            "output": [],
            "error": {
                "code": "server_error",
                "message": "provider worker failed"
            },
            "usage": {
                "input_tokens": 12,
                "output_tokens": 0,
                "total_tokens": 12
            }
        }))
        .unwrap();

        let error = responses_api_to_message(&response).unwrap_err();
        let ProviderError::ServerError(details) = error else {
            panic!("expected a server error, got {error:?}");
        };
        assert!(details.contains("provider worker failed"));
        assert_eq!(get_responses_usage(&response).total_tokens, Some(12));
    }

    #[test]
    fn malformed_and_non_object_tools_are_never_executable_nonstream_or_stream() {
        let response: ResponsesApiResponse = serde_json::from_value(json!({
            "id": "resp_bad_tools",
            "object": "response",
            "created_at": 0,
            "status": "completed",
            "model": "muse-spark-1.2",
            "output": [
                {
                    "type": "function_call",
                    "id": "fc_malformed",
                    "status": "completed",
                    "call_id": "call_malformed",
                    "name": "shell",
                    "arguments": "{not json"
                },
                {
                    "type": "function_call",
                    "id": "fc_array",
                    "status": "completed",
                    "call_id": "call_array",
                    "name": "shell",
                    "arguments": "[1,2,3]"
                },
                {
                    "type": "message",
                    "id": "msg_bad_tool",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{
                        "type": "tool_call",
                        "id": "call_nested",
                        "name": "shell",
                        "input": ["not", "an", "object"]
                    }]
                }
            ]
        }))
        .unwrap();
        let message = responses_api_to_message(&response).unwrap();
        assert_eq!(message.content.len(), 3);
        assert_invalid_tool_request(&message.content[0], "not valid JSON");
        assert_invalid_tool_request(&message.content[1], "got array");
        assert_invalid_tool_request(&message.content[2], "got array");

        let streamed = process_streaming_output_items(
            vec![
                ResponseOutputItemInfo::FunctionCall {
                    id: Some("fc_stream_malformed".to_string()),
                    status: Some("completed".to_string()),
                    call_id: Some("call_stream_malformed".to_string()),
                    name: "shell".to_string(),
                    arguments: "{not json".to_string(),
                },
                ResponseOutputItemInfo::FunctionCall {
                    id: Some("fc_stream_null".to_string()),
                    status: Some("completed".to_string()),
                    call_id: Some("call_stream_null".to_string()),
                    name: "shell".to_string(),
                    arguments: "null".to_string(),
                },
                ResponseOutputItemInfo::Message {
                    id: Some("msg_stream_bad_tool".to_string()),
                    status: Some("completed".to_string()),
                    role: "assistant".to_string(),
                    content: vec![ContentPart::ToolCall {
                        id: "call_stream_nested".to_string(),
                        name: "shell".to_string(),
                        arguments: "[]".to_string(),
                    }],
                },
            ],
            false,
            false,
        )
        .unwrap();
        assert_eq!(streamed.len(), 3);
        assert_invalid_tool_request(&streamed[0], "not valid JSON");
        assert_invalid_tool_request(&streamed[1], "got null");
        assert_invalid_tool_request(&streamed[2], "got array");
    }

    #[tokio::test]
    async fn streamed_max_token_incomplete_keeps_delta_terminal_evidence_and_usage() {
        for reason in ["max_output_tokens", "max_tokens"] {
            let lines = vec![
                r#"data: {"type":"response.created","sequence_number":1,"response":{"id":"resp_stream_incomplete","object":"response","created_at":0,"status":"in_progress","model":"muse-spark-1.2","output":[]}}"#.to_string(),
                r#"data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_partial","output_index":1,"content_index":0,"delta":"useful partial"}"#.to_string(),
                format!(
                    r#"data: {{"type":"response.incomplete","sequence_number":3,"response":{{"id":"resp_stream_incomplete","object":"response","created_at":0,"status":"incomplete","model":"muse-spark-1.2-terminal","incomplete_details":{{"reason":"{reason}"}},"output":[{{"type":"reasoning","id":"reasoning_partial","summary":[],"encrypted_content":"opaque-partial-reasoning"}},{{"type":"message","id":"msg_partial","status":"incomplete","role":"assistant","content":[{{"type":"output_text","text":"useful partial"}}]}},{{"type":"function_call","id":"fc_partial","status":"incomplete","call_id":"call_partial","name":"shell","arguments":"{{\"command\":\"unfinished"}}],"usage":{{"input_tokens":91,"output_tokens":37,"total_tokens":128,"input_tokens_details":{{"cached_tokens":13}}}}}}}}"#
                ),
                "data: [DONE]".to_string(),
            ];
            let stream =
                responses_api_to_streaming_message(tokio_stream::iter(lines.into_iter().map(Ok)));
            futures::pin_mut!(stream);

            let (delta, delta_usage) = stream.next().await.unwrap().unwrap();
            assert_eq!(delta.unwrap().as_concat_text(), "useful partial");
            assert!(delta_usage.is_none());

            let (terminal, usage) = stream.next().await.unwrap().unwrap();
            let terminal = terminal.expect("terminal evidence message should be retained");
            assert!(terminal.metadata.output_token_limit_reached);
            assert!(matches!(
                terminal.content[0],
                MessageContent::RedactedThinking(_)
            ));
            assert_invalid_tool_request(&terminal.content[1], "output-token limit");
            assert_eq!(
                terminal.content[2].as_text(),
                Some(OUTPUT_TRUNCATION_MARKER)
            );
            let usage = usage.expect("incomplete response usage must be emitted");
            assert_eq!(usage.model, "muse-spark-1.2-terminal");
            assert_eq!(usage.usage.input_tokens, Some(91));
            assert_eq!(usage.usage.output_tokens, Some(37));
            assert_eq!(usage.usage.total_tokens, Some(128));
            assert_eq!(usage.usage.cache_read_input_tokens, Some(13));
            assert!(stream.next().await.is_none());
        }
    }

    #[tokio::test]
    async fn streamed_content_filter_keeps_live_partial_text_usage_and_typed_failure() {
        let lines = vec![
            r#"data: {"type":"response.created","sequence_number":1,"response":{"id":"resp_filter","object":"response","created_at":0,"status":"in_progress","model":"muse-spark-1.2","output":[]}}"#.to_string(),
            r#"data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_filter","output_index":0,"content_index":0,"delta":"retained partial"}"#.to_string(),
            r#"data: {"type":"response.incomplete","sequence_number":3,"response":{"id":"resp_filter","object":"response","created_at":0,"status":"incomplete","model":"muse-spark-1.2","incomplete_details":{"reason":"content_filter"},"output":[],"usage":{"input_tokens":22,"output_tokens":4,"total_tokens":26}}}"#.to_string(),
        ];
        let stream =
            responses_api_to_streaming_message(tokio_stream::iter(lines.into_iter().map(Ok)));
        futures::pin_mut!(stream);

        let (partial, usage) = stream.next().await.unwrap().unwrap();
        assert_eq!(partial.unwrap().as_concat_text(), "retained partial");
        assert!(usage.is_none());

        let (message, usage) = stream.next().await.unwrap().unwrap();
        assert!(message.is_none());
        assert_eq!(usage.unwrap().usage.total_tokens, Some(26));

        let error = stream.next().await.unwrap().unwrap_err();
        let error = error
            .downcast_ref::<ProviderError>()
            .expect("stream failure should retain ProviderError type");
        assert!(matches!(
            error,
            ProviderError::Refusal {
                category: Some(category),
                ..
            } if category == "content_filter"
        ));
    }

    #[tokio::test]
    async fn streamed_unknown_incomplete_and_official_failures_are_typed() {
        let incomplete = vec![
            r#"data: {"type":"response.incomplete","sequence_number":1,"response":{"id":"resp_unknown","object":"response","created_at":0,"status":"incomplete","model":"muse-spark-1.2","incomplete_details":{"reason":"provider_specific_stop"},"output":[],"usage":{"input_tokens":8,"output_tokens":1,"total_tokens":9}}}"#.to_string(),
        ];
        let stream =
            responses_api_to_streaming_message(tokio_stream::iter(incomplete.into_iter().map(Ok)));
        futures::pin_mut!(stream);
        let (_, usage) = stream.next().await.unwrap().unwrap();
        assert_eq!(usage.unwrap().usage.total_tokens, Some(9));
        let error = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(
            error.downcast_ref::<ProviderError>(),
            Some(ProviderError::RequestFailed(details))
                if details.contains("provider_specific_stop")
        ));

        let failed = vec![
            r#"data: {"type":"response.failed","sequence_number":1,"response":{"id":"resp_failed","object":"response","created_at":0,"status":"failed","model":"muse-spark-1.2","output":[],"error":{"code":"server_error","message":"worker unavailable"},"usage":{"input_tokens":14,"output_tokens":0,"total_tokens":14}}}"#.to_string(),
        ];
        let stream =
            responses_api_to_streaming_message(tokio_stream::iter(failed.into_iter().map(Ok)));
        futures::pin_mut!(stream);
        let (_, usage) = stream.next().await.unwrap().unwrap();
        assert_eq!(usage.unwrap().usage.total_tokens, Some(14));
        let error = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(
            error.downcast_ref::<ProviderError>(),
            Some(ProviderError::ServerError(details)) if details.contains("worker unavailable")
        ));

        let top_level_error = vec![
            r#"data: {"type":"error","sequence_number":1,"code":"rate_limit_exceeded","message":"slow down","param":null}"#.to_string(),
        ];
        let stream = responses_api_to_streaming_message(tokio_stream::iter(
            top_level_error.into_iter().map(Ok),
        ));
        futures::pin_mut!(stream);
        let error = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(
            error.downcast_ref::<ProviderError>(),
            Some(ProviderError::RateLimitExceeded { details, .. })
                if details.contains("slow down")
        ));
    }
}
