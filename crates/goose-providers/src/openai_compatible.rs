use crate::conversation::token_usage::ProviderUsage;
use crate::images::ImageFormat;
use anyhow::Error;
use async_stream::try_stream;
use futures::TryStreamExt;
use reqwest::Response;
#[cfg(test)]
use reqwest::StatusCode;
use serde_json::Value;
use tokio::pin;
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, LinesCodec};
use tokio_util::io::StreamReader;

use super::api_client::ApiClient;
use super::base::{
    stream_from_single_message, MessageStream, Provider, SingleAttemptTerminalProof,
    SingleAttemptTerminalReporter,
};
use super::retry::ProviderRetry;
use crate::conversation::message::Message;
use crate::errors::ProviderError;
use crate::formats::openai::{
    create_request, get_usage, response_to_message, response_to_streaming_message,
    response_to_streaming_message_with_terminal_proof,
};
use crate::formats::openai_responses::responses_api_to_streaming_message_with_terminal_proof;
use crate::model::ModelConfig;
use crate::request_log::{start_log, LoggerHandleExt, RequestLogHandle};
use rmcp::model::Tool;

pub struct OpenAiCompatibleProvider {
    name: String,
    /// Client targeted at the base URL (e.g. `https://api.x.ai/v1`)
    api_client: ApiClient,
    /// Path prefix prepended to `chat/completions` (e.g. `"deployments/{name}/"` for Azure).
    completions_prefix: String,
    supports_streaming: bool,
}

impl OpenAiCompatibleProvider {
    pub fn new(name: String, api_client: ApiClient, completions_prefix: String) -> Self {
        Self {
            name,
            api_client,
            completions_prefix,
            supports_streaming: true,
        }
    }

    pub fn with_supports_streaming(mut self, supports_streaming: bool) -> Self {
        self.supports_streaming = supports_streaming;
        self
    }

    fn build_request(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
        for_streaming: bool,
    ) -> Result<Value, ProviderError> {
        create_request(
            model_config,
            system,
            messages,
            tools,
            &ImageFormat::OpenAi,
            for_streaming,
        )
        .map_err(|e| ProviderError::RequestFailed(format!("Failed to create request: {}", e)))
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        let response = self
            .api_client
            .response_get("models")
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;
        let json = handle_response_openai_compat(response).await?;

        if let Some(err_obj) = json.get("error") {
            let msg = err_obj
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(ProviderError::Authentication(msg.to_string()));
        }

        let arr = json.get("data").and_then(|v| v.as_array()).ok_or_else(|| {
            ProviderError::RequestFailed("Missing 'data' array in models response".to_string())
        })?;
        let mut models: Vec<String> = arr
            .iter()
            .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(str::to_string))
            .collect();
        models.sort();
        Ok(models)
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let payload = self.build_request(
            model_config,
            system,
            messages,
            tools,
            self.supports_streaming,
        )?;
        let mut log = start_log(model_config, &payload)?;

        let telemetry_t0 = std::time::Instant::now();
        let completions_path = format!("{}chat/completions", self.completions_prefix);
        let response = self
            .with_retry(|| async {
                let resp = self
                    .api_client
                    .response_post(&completions_path, &payload)
                    .await?;
                handle_status(resp).await
            })
            .await
            .inspect_err(|e| {
                let _ = log.error(e);
            })?;

        if self.supports_streaming {
            stream_openai_compat_timed(response, log, telemetry_t0, model_config.model_name.clone())
        } else {
            let json: serde_json::Value = response.json().await.map_err(|e| {
                ProviderError::RequestFailed(format!("Failed to parse JSON: {}", e))
            })?;

            let message = response_to_message(&json).map_err(|e| {
                ProviderError::RequestFailed(format!("Failed to parse message: {}", e))
            })?;

            let usage_data = get_usage(json.get("usage").unwrap_or(&serde_json::Value::Null));
            let usage = ProviderUsage::new(model_config.model_name.clone(), usage_data);

            log.write(
                &serde_json::to_value(&message).unwrap_or_default(),
                Some(&usage.usage),
            )?;

            let elapsed = telemetry_t0.elapsed();
            telemetry_record(
                &model_config.model_name,
                elapsed,
                elapsed,
                Some(usage.usage),
                0,
            );
            Ok(stream_from_single_message(message, usage))
        }
    }
}

/// GOOSE_SWARM_TELEMETRY_FILE (Mihai 2026-08-19, adopted): one JSON line per completion call —
/// model, node (the fleet's model-id prefix), ttft_ms, total_ms, token usage from the backend's
/// final chunk (`stream_options.include_usage` is always set by the request builder). Env unset
/// = no work, no file handle. A write failure never fails or slows the call. The same line shape
/// is written by `goose::providers::swarm` for its planner-side calls; this site covers the
/// per-task fleet streams, which is where the wall clock actually goes.
fn telemetry_node(model: &str) -> Option<String> {
    // NODE-FIRST (mirrors the engine's device_from_lms_id): per-host aliases carry the node at
    // the START (`mihai-qwen/qwen3.8-27b`); post-slash only when the first segment has no dash.
    let first = model.split('/').next().unwrap_or(model);
    let seg = if first.contains('-') {
        first
    } else {
        model.rsplit('/').next().unwrap_or(model)
    };
    let (node, rest) = seg.split_once('-')?;
    (!node.is_empty() && !rest.is_empty()).then(|| node.to_string())
}

pub(crate) fn telemetry_record(
    model: &str,
    ttft: std::time::Duration,
    total: std::time::Duration,
    usage: Option<crate::conversation::token_usage::Usage>,
    chunks: u64,
) {
    let Some(path) = std::env::var_os("GOOSE_SWARM_TELEMETRY_FILE").filter(|p| !p.is_empty())
    else {
        return;
    };
    let has_usage = usage.is_some_and(|u| u.output_tokens.is_some() || u.input_tokens.is_some());
    let mut line = serde_json::json!({
        "t": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        "model": model,
        "node": telemetry_node(model),
        "ttft_ms": ttft.as_millis() as u64,
        "total_ms": total.as_millis() as u64,
        "prompt_tokens": usage.and_then(|u| u.input_tokens),
        "completion_tokens": usage.and_then(|u| u.output_tokens),
        "usage": has_usage,
    });
    if !has_usage && chunks > 0 {
        line["approx_completion_chunks"] = serde_json::json!(chunks);
    }
    let payload = format!("{line}\n");
    static APPEND: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = APPEND.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = f.write_all(payload.as_bytes());
    }
}

/// `stream_openai_compat` plus per-call telemetry: TTFT at the first streamed message, usage from
/// the final chunk. Behaviour of the yielded stream is byte-identical to the untimed variant.
/// A consumer that drops the stream mid-way (abort, stall kill) writes NO line — decode-rate
/// stats deliberately measure only completed calls.
pub(crate) fn stream_openai_compat_timed(
    response: Response,
    log: Option<Box<dyn RequestLogHandle>>,
    t0: std::time::Instant,
    model_name: String,
) -> Result<MessageStream, ProviderError> {
    let (_, reporter) = SingleAttemptTerminalProof::channel();
    stream_openai_compat_timed_with_terminal_proof(response, log, t0, model_name, reporter)
}

pub(crate) fn stream_openai_compat_timed_with_terminal_proof(
    response: Response,
    mut log: Option<Box<dyn RequestLogHandle>>,
    t0: std::time::Instant,
    model_name: String,
    terminal: SingleAttemptTerminalReporter,
) -> Result<MessageStream, ProviderError> {
    let stream = response.bytes_stream().map_err(std::io::Error::other);

    Ok(Box::pin(try_stream! {
        let stream_reader = StreamReader::new(stream);
        let framed = FramedRead::new(stream_reader, LinesCodec::new())
            .map_err(Error::from);

        let message_stream = response_to_streaming_message_with_terminal_proof(framed, terminal);
        pin!(message_stream);
        let mut ttft: Option<std::time::Duration> = None;
        let mut last_usage: Option<crate::conversation::token_usage::Usage> = None;
        let mut chunks: u64 = 0;
        while let Some(message) = message_stream.next().await {
            let (message, usage) = message.map_err(|e|
                e.downcast::<ProviderError>()
                    .unwrap_or_else(ProviderError::stream_decode_error)
            )?;
            ttft.get_or_insert_with(|| t0.elapsed());
            chunks += 1;
            if let Some(u) = usage.as_ref() {
                last_usage = Some(u.usage);
            }
            log.write(&message, usage.as_ref().map(|f| f.usage).as_ref())?;
            yield (message, usage);
        }
        let total = t0.elapsed();
        telemetry_record(&model_name, ttft.unwrap_or(total), total, last_usage, chunks);
    }))
}

// Re-exported from the dedicated `http_status` module — these helpers are
// format-agnostic and used across all provider families.
pub use super::http_status::{
    handle_response, handle_status, map_http_error_to_provider_error, sanitize_url,
};

// Legacy alias kept for callers that haven't migrated their import path yet.
pub use super::http_status::handle_response as handle_response_openai_compat;

pub fn stream_openai_compat(
    response: Response,
    mut log: Option<Box<dyn RequestLogHandle>>,
) -> Result<MessageStream, ProviderError> {
    let stream = response.bytes_stream().map_err(std::io::Error::other);

    Ok(Box::pin(try_stream! {
        let stream_reader = StreamReader::new(stream);
        let framed = FramedRead::new(stream_reader, LinesCodec::new())
            .map_err(Error::from);

        let message_stream = response_to_streaming_message(framed);
        pin!(message_stream);
        while let Some(message) = message_stream.next().await {
            let (message, usage) = message.map_err(|e|
                e.downcast::<ProviderError>()
                    .unwrap_or_else(ProviderError::stream_decode_error)
            )?;
            log.write(&message, usage.as_ref().map(|f| f.usage).as_ref())?;
            yield (message, usage);
        }
    }))
}

pub fn stream_responses_compat(
    response: Response,
    log: Option<Box<dyn RequestLogHandle>>,
) -> Result<MessageStream, ProviderError> {
    let (_, reporter) = SingleAttemptTerminalProof::channel();
    stream_responses_compat_with_terminal_proof(response, log, reporter)
}

pub fn stream_responses_compat_with_terminal_proof(
    response: Response,
    mut log: Option<Box<dyn RequestLogHandle>>,
    terminal: SingleAttemptTerminalReporter,
) -> Result<MessageStream, ProviderError> {
    let stream = response.bytes_stream().map_err(std::io::Error::other);

    Ok(Box::pin(try_stream! {
        let stream_reader = StreamReader::new(stream);
        let framed = FramedRead::new(stream_reader, LinesCodec::new())
            .map_err(Error::from);

        let message_stream =
            responses_api_to_streaming_message_with_terminal_proof(framed, terminal);
        pin!(message_stream);
        while let Some(message) = message_stream.next().await {
            let (message, usage) = message.map_err(|e|
                e.downcast::<ProviderError>()
                    .unwrap_or_else(ProviderError::stream_decode_error)
            )?;
            log.write(&message, usage.as_ref().map(|f| f.usage).as_ref())?;
            yield (message, usage);
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelConfig;
    use serde_json::json;
    use test_case::test_case;

    #[test]
    fn telemetry_node_is_the_dash_bounded_model_prefix() {
        assert_eq!(
            telemetry_node("workhorse-qwen3.6-27b").as_deref(),
            Some("workhorse")
        );
        assert_eq!(telemetry_node("ns/gabee-qwen3.6").as_deref(), Some("gabee"));
        assert_eq!(telemetry_node("gpt4"), None, "no dash -> no node identity");
        assert_eq!(telemetry_node("-qwen"), None, "empty prefix is not a node");
        assert_eq!(telemetry_node("gabee-"), None, "empty rest is not a model");
    }

    #[test_case(
        StatusCode::PAYMENT_REQUIRED,
        Some(json!({"error": {"message": "Insufficient credits to complete this request"}})),
        "CreditsExhausted"
        ; "402 with payload"
    )]
    #[test_case(
        StatusCode::PAYMENT_REQUIRED,
        None,
        "CreditsExhausted"
        ; "402 without payload"
    )]
    #[test_case(
        StatusCode::TOO_MANY_REQUESTS,
        Some(json!({"error": {"message": "Rate limit exceeded"}})),
        "RateLimitExceeded"
        ; "429 rate limit"
    )]
    #[test_case(
        StatusCode::UNAUTHORIZED,
        None,
        "Authentication"
        ; "401 unauthorized"
    )]
    #[test_case(
        StatusCode::BAD_REQUEST,
        Some(json!({"error": {"message": "This request exceeds the maximum context length"}})),
        "ContextLengthExceeded"
        ; "400 context length"
    )]
    #[test_case(
        StatusCode::INTERNAL_SERVER_ERROR,
        None,
        "ServerError"
        ; "500 server error"
    )]
    #[test_case(
        StatusCode::NOT_FOUND,
        None,
        "RequestFailed"
        ; "404 not found"
    )]
    #[test_case(
        StatusCode::NOT_FOUND,
        Some(json!({"error": {"message": "model not available"}})),
        "RequestFailed"
        ; "404 with error payload"
    )]
    fn http_status_maps_to_expected_error(
        status: StatusCode,
        payload: Option<Value>,
        expected_variant: &str,
    ) {
        let err = map_http_error_to_provider_error(status, payload, "http://test/endpoint");
        let actual = err.telemetry_type();
        let expected_telemetry = match expected_variant {
            "CreditsExhausted" => "credits_exhausted",
            "RateLimitExceeded" => "rate_limit",
            "Authentication" => "auth",
            "ContextLengthExceeded" => "context_length",
            "ServerError" => "server",
            "RequestFailed" => "request",
            other => panic!("Unknown variant: {other}"),
        };
        assert_eq!(
            actual, expected_telemetry,
            "Expected {expected_variant}, got error: {err:?}"
        );
    }

    #[test]
    fn build_request_respects_non_streaming_mode() {
        let provider = OpenAiCompatibleProvider::new(
            "test".to_string(),
            ApiClient::new_with_tls(
                "http://localhost".to_string(),
                super::super::api_client::AuthMethod::NoAuth,
                None,
            )
            .unwrap(),
            String::new(),
        )
        .with_supports_streaming(false);

        let model = ModelConfig::new("test-model");
        let payload = provider
            .build_request(&model, "", &[], &[], provider.supports_streaming)
            .unwrap();

        assert_eq!(payload.get("stream"), None);
        assert_eq!(payload.get("stream_options"), None);
    }
}
