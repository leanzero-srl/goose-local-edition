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
use super::base::{stream_from_single_message, MessageStream, Provider};
use super::retry::ProviderRetry;
use crate::conversation::message::Message;
use crate::errors::ProviderError;
use crate::formats::openai::{
    create_request, get_usage, response_to_message, response_to_streaming_message,
};
use crate::formats::openai_responses::responses_api_to_streaming_message;
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

    fn endpoint(&self) -> Option<&str> {
        Some(self.api_client.host())
    }

    fn sharing_origin(&self) -> Option<String> {
        self.api_client
            .sharing_origin(&format!("{}chat/completions", self.completions_prefix))
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        let response = self.api_client.response_get("models").await.map_err(|e| {
            ProviderError::RequestFailed(super::http_status::redact_relay_capability(
                &e.to_string(),
            ))
        })?;
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
                message.id.as_deref(),
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

/// How a call's stream ended, as its telemetry line states it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CallEnd {
    /// The stream was read to its end.
    Completed,
    /// The consumer dropped the stream first (abort, stall kill, a mid-stream provider error).
    /// The provider may still bill the call, so the line is written with its `response_id`; it
    /// carries `usage: false` and no token counts, so the decode-rate readers
    /// (`telemetry_node_rates`, score_sb6/score_sb7 `telemetry_summary`, which read only
    /// `usage: true` lines) measure exactly the completed calls they measured before.
    Incomplete,
}

fn telemetry_path() -> Option<std::path::PathBuf> {
    std::env::var_os("GOOSE_SWARM_TELEMETRY_FILE")
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
}

fn telemetry_line(
    model: &str,
    ttft: std::time::Duration,
    total: std::time::Duration,
    usage: Option<crate::conversation::token_usage::Usage>,
    chunks: u64,
    response_id: Option<&str>,
    ended: CallEnd,
) -> Value {
    let usage = usage.filter(|_| ended == CallEnd::Completed);
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
        // The provider's own id for this call (OpenRouter: the generation id its
        // /api/v1/generation endpoint bills under). Null when no message carried one.
        "response_id": response_id,
        "ended": match ended {
            CallEnd::Completed => "completed",
            CallEnd::Incomplete => "incomplete",
        },
    });
    if !has_usage && chunks > 0 {
        line["approx_completion_chunks"] = serde_json::json!(chunks);
    }
    line
}

fn telemetry_append(path: &std::path::Path, line: &Value) {
    let payload = format!("{line}\n");
    static APPEND: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = APPEND.lock().unwrap_or_else(|e| e.into_inner());
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(payload.as_bytes())
        });
    // Never fails the call, but never silent: a lost line is a call a harness cannot bill.
    if let Err(error) = written {
        tracing::warn!(path = %path.display(), %error, "telemetry line not written");
    }
}

pub(crate) fn telemetry_record(
    model: &str,
    ttft: std::time::Duration,
    total: std::time::Duration,
    usage: Option<crate::conversation::token_usage::Usage>,
    chunks: u64,
    response_id: Option<&str>,
) {
    let Some(path) = telemetry_path() else {
        return;
    };
    let line = telemetry_line(
        model,
        ttft,
        total,
        usage,
        chunks,
        response_id,
        CallEnd::Completed,
    );
    telemetry_append(&path, &line);
}

/// One streamed call's telemetry, written exactly once: when the stream is read to its end, or
/// on drop when the consumer let go first. It is built before the stream is first polled, so a
/// stream dropped unpolled (the request was sent; the provider may bill it) still writes its line.
struct CallTelemetry {
    path: Option<std::path::PathBuf>,
    model: String,
    t0: std::time::Instant,
    ttft: Option<std::time::Duration>,
    usage: Option<crate::conversation::token_usage::Usage>,
    chunks: u64,
    response_id: Option<String>,
    written: bool,
}

impl CallTelemetry {
    fn new(path: Option<std::path::PathBuf>, model: String, t0: std::time::Instant) -> Self {
        Self {
            path,
            model,
            t0,
            ttft: None,
            usage: None,
            chunks: 0,
            response_id: None,
            written: false,
        }
    }

    fn observe(&mut self, message: Option<&Message>, usage: Option<&ProviderUsage>) {
        self.ttft.get_or_insert_with(|| self.t0.elapsed());
        self.chunks += 1;
        if self.response_id.is_none() {
            self.response_id = message.and_then(|m| m.id.clone());
        }
        if let Some(u) = usage {
            self.usage = Some(u.usage);
        }
    }

    fn write(&mut self, ended: CallEnd) {
        self.written = true;
        let Some(path) = &self.path else {
            return;
        };
        let total = self.t0.elapsed();
        let line = telemetry_line(
            &self.model,
            self.ttft.unwrap_or(total),
            total,
            self.usage,
            self.chunks,
            self.response_id.as_deref(),
            ended,
        );
        telemetry_append(path, &line);
    }
}

impl Drop for CallTelemetry {
    fn drop(&mut self) {
        if !self.written {
            self.write(CallEnd::Incomplete);
        }
    }
}

/// `stream_openai_compat` plus per-call telemetry: TTFT at the first streamed message, usage from
/// the final chunk, the provider's response id from the first message that carries one. Behaviour
/// of the yielded stream is byte-identical to the untimed variant. Every call writes one line:
/// `ended: "completed"` when read to the end, `ended: "incomplete"` (no usage) when dropped first.
pub fn stream_openai_compat_timed(
    response: Response,
    log: Option<Box<dyn RequestLogHandle>>,
    t0: std::time::Instant,
    model_name: String,
) -> Result<MessageStream, ProviderError> {
    stream_openai_compat_recorded(
        response,
        log,
        CallTelemetry::new(telemetry_path(), model_name, t0),
    )
}

fn stream_openai_compat_recorded(
    response: Response,
    mut log: Option<Box<dyn RequestLogHandle>>,
    mut call: CallTelemetry,
) -> Result<MessageStream, ProviderError> {
    let stall = crate::stream_stall::StallWatch::of(&response);
    let stream = response
        .bytes_stream()
        .inspect_ok(stall.data_lines())
        .map_err(crate::errors::body_read_error);

    Ok(crate::stream_stall::guard(
        Box::pin(try_stream! {
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
                call.observe(message.as_ref(), usage.as_ref());
                log.write(&message, usage.as_ref().map(|f| f.usage).as_ref())?;
                yield (message, usage);
            }
            call.write(CallEnd::Completed);
        }),
        stall,
    ))
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
    log: Option<Box<dyn RequestLogHandle>>,
) -> Result<MessageStream, ProviderError> {
    let stall = crate::stream_stall::StallWatch::of(&response);
    stream_openai_compat_watched(response, log, stall)
}

fn stream_openai_compat_watched(
    response: Response,
    mut log: Option<Box<dyn RequestLogHandle>>,
    stall: crate::stream_stall::StallWatch,
) -> Result<MessageStream, ProviderError> {
    let stream = response
        .bytes_stream()
        .inspect_ok(stall.data_lines())
        .map_err(crate::errors::body_read_error);

    Ok(crate::stream_stall::guard(
        Box::pin(try_stream! {
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
        }),
        stall,
    ))
}

pub fn stream_responses_compat(
    response: Response,
    mut log: Option<Box<dyn RequestLogHandle>>,
) -> Result<MessageStream, ProviderError> {
    let stall = crate::stream_stall::StallWatch::of(&response);
    let stream = response
        .bytes_stream()
        .inspect_ok(stall.data_lines())
        .map_err(crate::errors::body_read_error);

    Ok(crate::stream_stall::guard(
        Box::pin(try_stream! {
            let stream_reader = StreamReader::new(stream);
            let framed = FramedRead::new(stream_reader, LinesCodec::new())
                .map_err(Error::from);

            let message_stream = responses_api_to_streaming_message(framed);
            pin!(message_stream);
            while let Some(message) = message_stream.next().await {
                let (message, usage) = message.map_err(|e|
                    e.downcast::<ProviderError>()
                        .unwrap_or_else(ProviderError::stream_decode_error)
                )?;
                log.write(&message, usage.as_ref().map(|f| f.usage).as_ref())?;
                yield (message, usage);
            }
        }),
        stall,
    ))
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

    const GEN_ID: &str = "gen-1790911486-OyqP2p7UE9dNJc1weFAo";

    /// A real streamed OpenRouter response (SSE), served over HTTP so the stream reads real bytes.
    async fn openrouter_stream_response() -> (wiremock::MockServer, Response) {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let chunk = |delta: Value, finish: Value, usage: Option<Value>| {
            let mut c = json!({"id": GEN_ID, "object": "chat.completion.chunk", "created": 1,
                               "model": "deepseek/deepseek-v4.1-flash",
                               "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]});
            if let Some(u) = usage {
                c["usage"] = u;
            }
            format!("data: {c}\n\n")
        };
        let body = [
            chunk(
                json!({"role": "assistant", "content": "Hello"}),
                Value::Null,
                None,
            ),
            chunk(
                json!({"content": " there"}),
                json!("stop"),
                Some(json!({"prompt_tokens": 2527, "completion_tokens": 89, "total_tokens": 2616})),
            ),
            "data: [DONE]\n\n".to_string(),
        ]
        .concat();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;
        let response = reqwest::Client::new()
            .post(server.uri())
            .send()
            .await
            .unwrap();
        (server, response)
    }

    fn telemetry_lines(path: &std::path::Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn recorded_call(path: &std::path::Path) -> CallTelemetry {
        CallTelemetry::new(
            Some(path.to_path_buf()),
            "deepseek/deepseek-v4.1-flash".to_string(),
            std::time::Instant::now(),
        )
    }

    #[tokio::test]
    async fn a_completed_stream_writes_one_line_with_the_providers_response_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("telemetry.jsonl");
        let (_server, response) = openrouter_stream_response().await;
        let mut stream =
            stream_openai_compat_recorded(response, None, recorded_call(&path)).unwrap();
        while let Some(item) = stream.next().await {
            item.unwrap();
        }
        drop(stream);
        let lines = telemetry_lines(&path);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0]["response_id"], GEN_ID);
        assert_eq!(lines[0]["ended"], "completed");
        assert_eq!(lines[0]["usage"], true);
        assert_eq!(lines[0]["prompt_tokens"], 2527);
        assert_eq!(lines[0]["completion_tokens"], 89);
    }

    #[tokio::test]
    async fn a_dropped_stream_still_writes_its_line_without_usage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("telemetry.jsonl");
        let (_server, response) = openrouter_stream_response().await;
        let mut stream =
            stream_openai_compat_recorded(response, None, recorded_call(&path)).unwrap();
        stream.next().await.unwrap().unwrap();
        drop(stream);
        let lines = telemetry_lines(&path);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0]["response_id"], GEN_ID);
        assert_eq!(lines[0]["ended"], "incomplete");
        // usage:false keeps the decode-rate readers on completed calls only.
        assert_eq!(lines[0]["usage"], false);
        assert_eq!(lines[0]["prompt_tokens"], Value::Null);
        assert_eq!(lines[0]["approx_completion_chunks"], 1);
    }

    #[tokio::test]
    async fn a_stream_dropped_before_its_first_poll_still_writes_its_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("telemetry.jsonl");
        let (_server, response) = openrouter_stream_response().await;
        drop(stream_openai_compat_recorded(response, None, recorded_call(&path)).unwrap());
        let lines = telemetry_lines(&path);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0]["ended"], "incomplete");
        assert_eq!(lines[0]["response_id"], Value::Null);
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

    /// A one-connection SSE server: `frames` are written with the given delay before each, then
    /// the connection is held open (neither a byte nor a close) for an hour.
    async fn hold_after(
        frames: Vec<(std::time::Duration, String)>,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 8192];
            let _ = socket.read(&mut request).await.unwrap();
            let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
            socket.write_all(head.as_bytes()).await.unwrap();
            for (delay, frame) in frames {
                tokio::time::sleep(delay).await;
                let chunk = format!("{:x}\r\n{frame}\r\n", frame.len());
                socket.write_all(chunk.as_bytes()).await.unwrap();
                socket.flush().await.unwrap();
            }
            tokio::time::sleep(std::time::Duration::from_secs(3_600)).await;
            drop(socket);
        });
        (addr, server)
    }

    fn sse(choice: Value) -> String {
        let c = json!({"id": GEN_ID, "object": "chat.completion.chunk", "created": 1,
                       "model": "elorian/glimpse", "choices": [choice]});
        format!("data: {c}\n\n")
    }

    /// The response from `addr` under a public-looking name (the peer is still loopback).
    async fn public_named(addr: std::net::SocketAddr) -> Response {
        reqwest::Client::builder()
            .resolve("stalls.example.com", addr)
            .build()
            .unwrap()
            .post(format!(
                "http://stalls.example.com:{}/v1/chat/completions",
                addr.port()
            ))
            .send()
            .await
            .unwrap()
    }

    /// Reads `stream` to its end (or 30 s): the texts, the tool calls and the error it ended on.
    async fn drain(mut stream: MessageStream) -> (String, Vec<String>, Option<ProviderError>) {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            let (mut text, mut calls) = (String::new(), Vec::new());
            loop {
                match stream.next().await {
                    Some(Ok((Some(message), _))) => {
                        text.push_str(&message.as_concat_text());
                        for content in &message.content {
                            if let crate::conversation::message::MessageContent::ToolRequest(r) =
                                content
                            {
                                calls.push(match &r.tool_call {
                                    Ok(call) => format!("{}:{:?}", call.name, call.arguments),
                                    Err(e) => format!("ERR {}", e.message),
                                });
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return (text, calls, Some(error)),
                    None => return (text, calls, None),
                }
            }
        })
        .await
        .expect("the stream was waited on forever (I-33)")
    }

    /// I-33 (Glimpse, 2026-10-19): a remote stream that delivers deltas and then neither sends a
    /// byte nor closes. Before the stall guard this read waited forever; now it ends with the
    /// transient "stream stalled" error the reply loop resends.
    #[tokio::test]
    async fn a_remote_stream_that_goes_silent_mid_answer_ends_as_a_resendable_error() {
        let zero = std::time::Duration::ZERO;
        let (addr, server) = hold_after(vec![
            (
                zero,
                sse(json!({"index": 0, "delta": {"content": "The plan"}, "finish_reason": null})),
            ),
            (
                zero,
                sse(json!({"index": 0, "delta": {"content": " /in"}, "finish_reason": null})),
            ),
        ])
        .await;
        let response = public_named(addr).await;
        // The peer is loopback, so the rule an ordinary call gets never judges it (review 2026-10-19).
        assert_eq!(
            crate::stream_stall::StallWatch::of(&response).window(),
            None
        );
        let watch = crate::stream_stall::StallWatch::judging(
            std::time::Duration::from_secs(2),
            "stalls.example.com",
        );
        let (text, _, error) =
            drain(stream_openai_compat_watched(response, None, watch).unwrap()).await;
        server.abort();
        assert_eq!(text, "The plan /in");
        let error =
            error.expect("a stalled stream must end with an error, not as a finished answer");
        assert!(
            error
                .to_string()
                .contains(crate::stream_stall::STREAM_STALLED),
            "{error}"
        );
        assert!(error.is_transient(), "{error:?}");
    }

    /// Review 2026-10-19: text, then a tool call whose arguments arrive every 0.9 s. The parser
    /// yields nothing until the call closes; with a 2 s window the first guard ended it as
    /// "stream stalled". The arguments are progress: the call arrives whole.
    #[tokio::test]
    async fn tool_call_arguments_streaming_slowly_are_not_a_stall() {
        let gap = std::time::Duration::from_millis(900);
        let zero = std::time::Duration::ZERO;
        let mut frames = vec![
            (
                zero,
                sse(
                    json!({"index": 0, "delta": {"content": "Writing it."}, "finish_reason": null}),
                ),
            ),
            (
                gap,
                sse(
                    json!({"index": 0, "delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function",
                "function": {"name": "write", "arguments": ""}}]}, "finish_reason": null}),
                ),
            ),
        ];
        for piece in [
            "{\"path\": ",
            "\"a.txt\", ",
            "\"content\": ",
            "\"hello\"",
            "}",
        ] {
            frames.push((
                gap,
                sse(json!({"index": 0, "delta": {"tool_calls": [{"index": 0,
                "function": {"arguments": piece}}]}, "finish_reason": null})),
            ));
        }
        frames.push((
            gap,
            sse(json!({"index": 0, "delta": {}, "finish_reason": "tool_calls"})),
        ));
        frames.push((zero, "data: [DONE]\n\n".to_string()));
        let (addr, server) = hold_after(frames).await;
        let watch = crate::stream_stall::StallWatch::judging(
            std::time::Duration::from_secs(2),
            "stalls.example.com",
        );
        let response = public_named(addr).await;
        let (text, calls, error) =
            drain(stream_openai_compat_watched(response, None, watch).unwrap()).await;
        server.abort();
        assert!(error.is_none(), "{error:?}");
        assert_eq!(text, "Writing it.");
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert!(
            calls[0].starts_with("write:") && calls[0].contains("hello"),
            "{calls:?}"
        );
    }

    /// An SSE comment is not progress: a relay that keeps the connection alive with comments over a
    /// dead upstream still stalls.
    #[tokio::test]
    async fn keepalive_comments_alone_are_a_stall() {
        let gap = std::time::Duration::from_millis(500);
        let zero = std::time::Duration::ZERO;
        let mut frames = vec![(
            zero,
            sse(json!({"index": 0, "delta": {"content": "thinking"}, "finish_reason": null})),
        )];
        for _ in 0..20 {
            frames.push((gap, ": OPENROUTER PROCESSING\n\n".to_string()));
        }
        let (addr, server) = hold_after(frames).await;
        let watch = crate::stream_stall::StallWatch::judging(
            std::time::Duration::from_secs(2),
            "stalls.example.com",
        );
        let response = public_named(addr).await;
        let (_, _, error) =
            drain(stream_openai_compat_watched(response, None, watch).unwrap()).await;
        server.abort();
        let error = error.expect("comments over a dead upstream must still stall");
        assert!(
            error
                .to_string()
                .contains(crate::stream_stall::STREAM_STALLED),
            "{error}"
        );
    }
}
