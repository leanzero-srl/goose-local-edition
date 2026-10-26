use reqwest::StatusCode;
use std::time::Duration;
use thiserror::Error;

use crate::redact::redact_relay_capability;
use crate::request_log::LogError;
use once_cell::sync::Lazy;
use regex::Regex;

/// The name of the error a stream parser raises when the body ends before its protocol's
/// completion marker — see [`ProviderError::stream_truncated`].
pub const STREAM_TRUNCATED: &str = "stream ended before completion";

/// `Debug` is written out ([`DebugShape`]) so that no dump carries a relay capability (Q-402).
#[derive(Error, Clone, PartialEq)]
pub enum ProviderError {
    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Context length exceeded: {0}")]
    ContextLengthExceeded(String),

    #[error("Rate limit exceeded: {details}")]
    RateLimitExceeded {
        details: String,
        retry_delay: Option<Duration>,
    },

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Usage data error: {0}")]
    UsageError(String),

    #[error("Unsupported operation: {0}")]
    NotImplemented(String),

    #[error("Endpoint not found (404): {0}")]
    EndpointNotFound(String),

    /// The serving engine holds new requests until its memory recovers — goose's distributed
    /// engine answers 503 with `code: "memory_hold"` (Q-397). A known engine state with its own
    /// end, so it is waited out on `admission_url` (`engine_hold::wait_for_admission`), never
    /// retried on a clock and never counted against the retries.
    #[error("Server error: {details}")]
    EngineHold {
        details: String,
        /// The engine's own words for why it holds; `None` when its refusal carried none.
        reason: Option<String>,
        /// Where the lift is awaited: the refusal's `admission` path on the refusing origin.
        /// `None` when the engine named no path — the hold cannot be waited out, and says so.
        admission_url: Option<String>,
    },

    #[error("Credits exhausted: {details}")]
    CreditsExhausted {
        details: String,
        top_up_url: Option<String>,
    },

    #[error("Provider refused request: {details}")]
    Refusal {
        details: String,
        category: Option<String>,
    },
}

/// The derived `Debug` shape, borrowed: [`ProviderError`]'s `Debug` prints exactly this, then
/// redacts — `EngineHold::admission_url` holds a relay's capability URL (it must, to be waited
/// on) and any text field may quote one.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "every field is read by the derived Debug, which dead-code analysis ignores"
)]
enum DebugShape<'a> {
    Authentication(&'a String),
    ContextLengthExceeded(&'a String),
    RateLimitExceeded {
        details: &'a String,
        retry_delay: &'a Option<Duration>,
    },
    ServerError(&'a String),
    NetworkError(&'a String),
    RequestFailed(&'a String),
    ExecutionError(&'a String),
    UsageError(&'a String),
    NotImplemented(&'a String),
    EndpointNotFound(&'a String),
    EngineHold {
        details: &'a String,
        reason: &'a Option<String>,
        admission_url: &'a Option<String>,
    },
    CreditsExhausted {
        details: &'a String,
        top_up_url: &'a Option<String>,
    },
    Refusal {
        details: &'a String,
        category: &'a Option<String>,
    },
}

impl<'a> From<&'a ProviderError> for DebugShape<'a> {
    fn from(error: &'a ProviderError) -> Self {
        match error {
            ProviderError::Authentication(s) => Self::Authentication(s),
            ProviderError::ContextLengthExceeded(s) => Self::ContextLengthExceeded(s),
            ProviderError::RateLimitExceeded {
                details,
                retry_delay,
            } => Self::RateLimitExceeded {
                details,
                retry_delay,
            },
            ProviderError::ServerError(s) => Self::ServerError(s),
            ProviderError::NetworkError(s) => Self::NetworkError(s),
            ProviderError::RequestFailed(s) => Self::RequestFailed(s),
            ProviderError::ExecutionError(s) => Self::ExecutionError(s),
            ProviderError::UsageError(s) => Self::UsageError(s),
            ProviderError::NotImplemented(s) => Self::NotImplemented(s),
            ProviderError::EndpointNotFound(s) => Self::EndpointNotFound(s),
            ProviderError::EngineHold {
                details,
                reason,
                admission_url,
            } => Self::EngineHold {
                details,
                reason,
                admission_url,
            },
            ProviderError::CreditsExhausted {
                details,
                top_up_url,
            } => Self::CreditsExhausted {
                details,
                top_up_url,
            },
            ProviderError::Refusal { details, category } => Self::Refusal { details, category },
        }
    }
}

impl std::fmt::Debug for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let shape = DebugShape::from(self);
        let raw = if f.alternate() {
            format!("{shape:#?}")
        } else {
            format!("{shape:?}")
        };
        f.write_str(&redact_relay_capability(&raw))
    }
}

impl ProviderError {
    pub fn stream_decode_error(error: impl std::fmt::Display) -> Self {
        ProviderError::NetworkError(redact_relay_capability(&format!(
            "Stream decode error: {error}"
        )))
    }

    /// The server closed a streamed response before its protocol's completion marker (OpenAI
    /// chat: a `finish_reason` or `[DONE]`; Anthropic: `message_stop`; Responses:
    /// `response.completed`/`response.incomplete`; Gemini: a `finishReason`; Bedrock:
    /// `messageStop`). The partial answer must not pass as a finished one: a serving process
    /// that dies mid-generation can still close the body cleanly on an HTTP 200, and the only
    /// evidence of the cut is the missing marker.
    ///
    /// It rides `stream_decode_error` on purpose: a clean close mid-answer is the same dropped
    /// body as a reset one, so it inherits that error's transient retry class and every reader
    /// already keyed on it (the swarm's mid-stream body-drop re-dispatch).
    pub fn stream_truncated(detail: impl std::fmt::Display) -> Self {
        Self::stream_decode_error(format!("{STREAM_TRUNCATED}: {detail}"))
    }

    pub fn telemetry_type(&self) -> &'static str {
        match self {
            ProviderError::Authentication(_) => "auth",
            ProviderError::ContextLengthExceeded(_) => "context_length",
            ProviderError::RateLimitExceeded { .. } => "rate_limit",
            ProviderError::ServerError(_) => "server",
            ProviderError::NetworkError(_) => "network",
            ProviderError::RequestFailed(_) => "request",
            ProviderError::ExecutionError(_) => "execution",
            ProviderError::UsageError(_) => "usage",
            ProviderError::NotImplemented(_) => "not_implemented",
            ProviderError::EndpointNotFound(_) => "endpoint_not_found",
            ProviderError::EngineHold { .. } => "engine_hold",
            ProviderError::CreditsExhausted { .. } => "credits_exhausted",
            ProviderError::Refusal { .. } => "refusal",
        }
    }

    pub fn is_endpoint_not_found(&self) -> bool {
        matches!(self, ProviderError::EndpointNotFound(_))
    }

    /// Recover a typed `ProviderError` from a streaming decode error, falling
    /// back to a retryable stream decode error for errors that did not
    /// originate as one.
    pub fn from_stream_error(error: anyhow::Error) -> Self {
        error
            .downcast()
            .unwrap_or_else(ProviderError::stream_decode_error)
    }

    /// The classes a resend can outlive — a rate limit, a server fault, a dropped connection —
    /// the set `RetryConfig::transient_only` retries. Every other class (a 4xx refusal, bad
    /// credentials, an unsupported request) fails the same way when the same request is resent.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimitExceeded { .. }
                | ProviderError::ServerError(_)
                | ProviderError::NetworkError(_)
        )
    }

    /// The error's own text, without the class prefix `Display` adds.
    pub fn inner_text(&self) -> &str {
        match self {
            ProviderError::Authentication(s)
            | ProviderError::ContextLengthExceeded(s)
            | ProviderError::ServerError(s)
            | ProviderError::NetworkError(s)
            | ProviderError::RequestFailed(s)
            | ProviderError::ExecutionError(s)
            | ProviderError::UsageError(s)
            | ProviderError::NotImplemented(s)
            | ProviderError::EndpointNotFound(s) => s,
            ProviderError::RateLimitExceeded { details, .. }
            | ProviderError::EngineHold { details, .. }
            | ProviderError::CreditsExhausted { details, .. }
            | ProviderError::Refusal { details, .. } => details,
        }
    }

    /// What the serving engine itself said: the body's words when the text is an HTTP failure
    /// goose framed with its endpoint (`http_failure_text`), else the error's own text whole.
    pub fn engine_words(&self) -> &str {
        let inner = self.inner_text();
        split_http_failure_text(inner).map_or(inner, |parts| parts.said)
    }
}

/// An HTTP failure as goose frames it: `"<what> at <url>: <what the server said>"`, a relay
/// capability redacted whoever built `url` (Q-402).
pub fn http_failure_text(what: &str, url: &str, said: &str) -> String {
    redact_relay_capability(&format!(
        "{what}{HTTP_FAILURE_AT}{url}{HTTP_FAILURE_SAID}{said}"
    ))
}

/// A refused credential as goose frames it (a 401/403 answer): the URL, the status and the
/// provider's own words, in the shape [`split_authentication_failure_text`] reads back (Q-478).
pub fn authentication_failure_text(
    url: &str,
    status: impl std::fmt::Display,
    said: &str,
) -> String {
    format!("{AUTH_FAILURE_FOR}{url}{AUTH_FAILURE_STATUS}{status}{AUTH_FAILURE_SAID}{said}")
}

const AUTH_FAILURE_FOR: &str = "Authentication failed for ";
const AUTH_FAILURE_STATUS: &str = ". Status: ";
const AUTH_FAILURE_SAID: &str = ". Response: ";

#[derive(Debug, PartialEq, Eq)]
pub struct AuthenticationFailureParts<'a> {
    pub url: &'a str,
    pub status: &'a str,
    pub said: &'a str,
}

/// `authentication_failure_text` read back; `None` for any other text (an `Authentication`
/// error a provider built itself, with no status).
pub fn split_authentication_failure_text(text: &str) -> Option<AuthenticationFailureParts<'_>> {
    let rest = text.strip_prefix(AUTH_FAILURE_FOR)?;
    let (url, rest) = rest.split_once(AUTH_FAILURE_STATUS)?;
    let (status, said) = rest.split_once(AUTH_FAILURE_SAID)?;
    Some(AuthenticationFailureParts { url, status, said })
}

const HTTP_FAILURE_AT: &str = " at ";
const HTTP_FAILURE_SAID: &str = ": ";

#[derive(Debug, PartialEq, Eq)]
pub struct HttpFailureParts<'a> {
    pub what: &'a str,
    pub url: &'a str,
    pub said: &'a str,
}

/// `http_failure_text` read back. A sanitized URL carries no `": "` (its port colon is never
/// followed by a space), so the first one after `" at "` ends it; `None` for any other text.
pub fn split_http_failure_text(text: &str) -> Option<HttpFailureParts<'_>> {
    let (what, rest) = text.split_once(HTTP_FAILURE_AT)?;
    let (url, said) = rest.split_once(HTTP_FAILURE_SAID)?;
    url.contains("://")
        .then_some(HttpFailureParts { what, url, said })
}

fn is_network_error(err: &reqwest::Error) -> bool {
    err.is_connect() || err.is_timeout() || (err.status().is_none() && err.is_request())
}

fn provider_error_from_reqwest(error: &reqwest::Error) -> ProviderError {
    if is_network_error(error) {
        let msg = if error.is_timeout() {
            "Request timed out — check your network connection and try again.".to_string()
        } else if error.is_connect() {
            if let Some(url) = error.url() {
                if let Some(host) = url.host_str() {
                    let port_info = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
                    format!(
                        "Could not connect to {}{} — check your network connection and try again.",
                        host, port_info
                    )
                } else {
                    "Could not connect to the provider — check your network connection and try again.".to_string()
                }
            } else {
                "Could not connect to the provider — check your network connection and try again."
                    .to_string()
            }
        } else {
            "Network error — check your network connection and try again.".to_string()
        };
        return ProviderError::NetworkError(msg);
    }

    let mut details = vec![];
    if let Some(status) = error.status() {
        details.push(format!("status: {}", status));
    }
    // A `reqwest::Error` says the request's whole URL ("… for url (<url>)").
    let msg = if details.is_empty() {
        error.to_string()
    } else {
        format!("{} ({})", error, details.join(", "))
    };
    ProviderError::RequestFailed(redact_relay_capability(&msg))
}

impl From<anyhow::Error> for ProviderError {
    fn from(error: anyhow::Error) -> Self {
        if let Some(reqwest_err) = error.downcast_ref::<reqwest::Error>() {
            return provider_error_from_reqwest(reqwest_err);
        }
        ProviderError::ExecutionError(error.to_string())
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(error: reqwest::Error) -> Self {
        provider_error_from_reqwest(&error)
    }
}

/// A failed read of a streamed response body, as the `io::Error` a line reader carries, its words
/// the WHOLE cause chain. reqwest names every cut body "error decoding response body" and keeps
/// why (a reset connection, an early EOF) only in `source()`, and the line codec wrapping the read
/// drops `source()` — so a connection reset mid-answer reached the chat as a decoding problem
/// (Q-392: "Stream decode error: error decoding response body" for an engine's RST).
pub fn body_read_error(error: reqwest::Error) -> std::io::Error {
    let mut said = error.to_string();
    let mut cause = std::error::Error::source(&error);
    while let Some(reason) = cause {
        let words = reason.to_string();
        if !said.contains(&words) {
            said.push_str(": ");
            said.push_str(&words);
        }
        cause = reason.source();
    }
    std::io::Error::other(said)
}

/// Added by `http_status` to a context-length refusal whose number is NOT the model's window:
/// Rapid-MLX's D-METAL-CAP refusal (`lz_reason: "metal_cap_exceeds_device"`) names the largest
/// prompt that fits beside THIS request's own max_tokens reservation — with an unset max_tokens
/// that is the window minus the prompt, so a few thousand tokens (review 2026-10-19, I-17
/// numbers: ~6k). Read as a window it would shrink the session's compaction to it.
pub const NOT_A_CONTEXT_WINDOW: &str =
    "[this number is the prompt that fits beside the request's output reservation, not the context window]";

/// The context window a context-length refusal names, in tokens (I-9): Rapid-MLX's memory guard
/// ("the maximum context here is 35328 tokens", plus `n_ctx` in its payload), the OpenAI shape
/// ("maximum context length is 262144 tokens"), Anthropic ("210000 tokens > 200000 maximum"),
/// llama.cpp ("available context size (4096 tokens)") and the `(n_ctx N)` that
/// `http_status` adds when only the payload carried the number. `None` when the refusal names no
/// usable window (a zero is "nothing fits", never a window to size to).
pub fn refused_context_window(text: &str) -> Option<usize> {
    if text.contains(NOT_A_CONTEXT_WINDOW) {
        return None;
    }
    static PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
        [
            r"(?i)maximum context(?: length| size| window)?(?: here)?(?: is| of)?\s+(\d[\d,]*)",
            r"(?i)\(n_ctx (\d+)\)",
            r#"(?i)"n_ctx"\s*:\s*(\d+)"#,
            r"(?i)tokens\s*>\s*(\d[\d,]*)\s*maximum",
            r"(?i)context size \((\d[\d,]*) tokens\)",
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).expect("a valid pattern"))
        .collect()
    });
    PATTERNS.iter().find_map(|pattern| {
        let digits = pattern.captures(text)?.get(1)?.as_str().replace(',', "");
        digits.parse::<usize>().ok().filter(|window| *window > 0)
    })
}

/// The output tokens a context-length refusal says the request reserved beside its prompt, when
/// it says so: OpenRouter's refusal counts them inside the window it names — "This endpoint's
/// maximum context length is 131072 tokens. However, you requested about 272464 tokens (258978 of
/// text input, 3486 of tool input, 10000 in the output)" (aider-desk#361) — so the prompt a later
/// request may carry is the window minus this reservation, not the whole window.
pub fn refused_output_reservation(text: &str) -> Option<usize> {
    static PATTERN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)(\d[\d,]*) in the output").expect("a valid pattern"));
    let digits = PATTERN.captures(text)?.get(1)?.as_str().replace(',', "");
    digits.parse::<usize>().ok()
}

impl From<LogError> for ProviderError {
    fn from(value: LogError) -> Self {
        ProviderError::ExecutionError(value.to_string())
    }
}

#[derive(Debug)]
pub enum GoogleErrorCode {
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    TooManyRequests = 429,
    InternalServerError = 500,
    ServiceUnavailable = 503,
}

impl GoogleErrorCode {
    pub fn to_status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    pub fn from_code(code: u64) -> Option<Self> {
        match code {
            400 => Some(Self::BadRequest),
            401 => Some(Self::Unauthorized),
            403 => Some(Self::Forbidden),
            404 => Some(Self::NotFound),
            429 => Some(Self::TooManyRequests),
            500 => Some(Self::InternalServerError),
            503 => Some(Self::ServiceUnavailable),
            _ => Some(Self::InternalServerError),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn an_authentication_failure_reads_back_into_its_parts() {
        let text = authentication_failure_text(
            "http://127.0.0.1:8899/v1/models",
            StatusCode::UNAUTHORIZED,
            "Incorrect API key provided: sk-bad. You can find your API key at https://x.y/keys.",
        );
        assert_eq!(
            text,
            "Authentication failed for http://127.0.0.1:8899/v1/models. Status: 401 Unauthorized. \
             Response: Incorrect API key provided: sk-bad. You can find your API key at https://x.y/keys."
        );
        assert_eq!(
            split_authentication_failure_text(&text),
            Some(AuthenticationFailureParts {
                url: "http://127.0.0.1:8899/v1/models",
                status: "401 Unauthorized",
                said: "Incorrect API key provided: sk-bad. You can find your API key at https://x.y/keys.",
            })
        );
        assert_eq!(split_authentication_failure_text("invalid x-api-key"), None);
    }

    #[test]
    fn an_http_failure_reads_back_into_its_parts() {
        let text = http_failure_text(
            "Resource not found (404)",
            "http://127.0.0.1:8091/v1/chat/completions",
            "Only 'text' content type is supported: see: docs",
        );
        assert_eq!(
            split_http_failure_text(&text),
            Some(HttpFailureParts {
                what: "Resource not found (404)",
                url: "http://127.0.0.1:8091/v1/chat/completions",
                said: "Only 'text' content type is supported: see: docs",
            })
        );
    }

    /// Q-402: the hold keeps the relay's real admission URL to wait on, and no dump of it — nor
    /// a framed failure at a relay — says the capability.
    #[test]
    fn no_dump_or_framed_failure_says_a_relays_capability() {
        let capability = "3f".repeat(32);
        let relay = format!("http://127.0.0.1:61001/relay/{capability}");
        let hold = ProviderError::EngineHold {
            details: http_failure_text(
                "Server error (503 Service Unavailable)",
                &format!("{relay}/v1/chat/completions"),
                "goose distributed engine is not admitting new requests",
            ),
            reason: Some("memory is low".into()),
            admission_url: Some(format!("{relay}/goose/admission")),
        };
        for said in [hold.to_string(), format!("{hold:?}"), format!("{hold:#?}")] {
            assert!(!said.contains(&capability), "{said}");
        }
        assert_eq!(
            format!("{hold:?}"),
            "EngineHold { details: \"Server error (503 Service Unavailable) at \
             http://127.0.0.1:61001/relay/…/v1/chat/completions: goose distributed engine is \
             not admitting new requests\", reason: Some(\"memory is low\"), admission_url: \
             Some(\"http://127.0.0.1:61001/relay/…/goose/admission\") }"
        );
        let ProviderError::EngineHold { admission_url, .. } = &hold else {
            unreachable!()
        };
        assert!(admission_url.as_deref().unwrap().contains(&capability));
    }

    /// NEGATIVE CONTROL: with no relay in it, the written-out `Debug` is the derived one.
    #[test]
    fn debug_is_the_derived_shape() {
        assert_eq!(
            format!("{:?}", ProviderError::ServerError("boom".into())),
            "ServerError(\"boom\")"
        );
        assert_eq!(
            format!(
                "{:?}",
                ProviderError::RateLimitExceeded {
                    details: "slow down".into(),
                    retry_delay: Some(Duration::from_secs(2)),
                }
            ),
            "RateLimitExceeded { details: \"slow down\", retry_delay: Some(2s) }"
        );
        assert_eq!(
            format!("{:#?}", ProviderError::Authentication("key revoked".into())),
            "Authentication(\n    \"key revoked\",\n)"
        );
    }

    #[test]
    fn text_goose_did_not_frame_is_its_own_words() {
        let bad = ProviderError::RequestFailed("Bad request (400): look at this: no".into());
        assert_eq!(bad.engine_words(), "Bad request (400): look at this: no");
        let auth = ProviderError::Authentication("key revoked".into());
        assert_eq!(auth.engine_words(), "key revoked");
        assert!(!auth.is_transient());
        let limited = ProviderError::RateLimitExceeded {
            details: "slow down".into(),
            retry_delay: None,
        };
        assert_eq!(limited.engine_words(), "slow down");
        assert!(limited.is_transient());
    }
}

#[cfg(test)]
mod refused_window_tests {
    use super::{refused_context_window, refused_output_reservation};

    /// OpenRouter's refusal as it reached clients (aider-desk#361; the 2026 wording names the
    /// context-compression plugin, multica#1796): the window and the output share inside it.
    #[test]
    fn an_openrouter_refusal_names_its_window_and_its_output_reservation() {
        for (text, window, output) in [
            (
                "This endpoint's maximum context length is 131072 tokens. However, you requested \
                 about 272464 tokens (258978 of text input, 3486 of tool input, 10000 in the \
                 output). Please reduce the length of either one, or use the \"middle-out\" \
                 transform to compress your prompt automatically.",
                131_072,
                10_000,
            ),
            (
                "This endpoint's maximum context length is 204800 tokens. However, you requested \
                 about 208281 tokens (158694 of text input, 17587 of tool input, 32000 in the \
                 output). Please reduce the length of either one, or use the context-compression \
                 plugin to compress your prompt automatically.",
                204_800,
                32_000,
            ),
        ] {
            assert_eq!(refused_context_window(text), Some(window), "{text}");
            assert_eq!(refused_output_reservation(text), Some(output), "{text}");
        }
        assert_eq!(
            refused_output_reservation("maximum context length is 262144 tokens"),
            None
        );
    }

    #[test]
    fn a_refusal_names_its_window() {
        for (text, window) in [
            (
                "This prompt (82684 tokens) needs 131.8 GB but this Mac can hold 75.1 GB; the \
                 maximum context here is 35328 tokens.",
                Some(35_328),
            ),
            (
                "This model's maximum context length is 262144 tokens. However, you requested \
                 270000 tokens (269000 prompt + 1000 completion).",
                Some(262_144),
            ),
            (
                "prompt is too long: 210000 tokens > 200000 maximum",
                Some(200_000),
            ),
            (
                "the request exceeds the available context size (4,096 tokens)",
                Some(4_096),
            ),
            (
                "the request exceeds the available context size (n_ctx 8192)",
                Some(8_192),
            ),
            (
                r#"{"error":{"n_prompt_tokens":9000,"n_ctx":8192}}"#,
                Some(8_192),
            ),
            ("the maximum context here is 0 tokens.", None),
            ("Too many tool responses: 5 > 2", None),
            ("context_length_exceeded", None),
            (
                &format!(
                    "This prompt (40000 tokens) needs 42.6 GB but this Mac can hold 38.0 GB; the \
                     maximum context here is 6123 tokens. {}",
                    super::NOT_A_CONTEXT_WINDOW
                ),
                None,
            ),
        ] {
            assert_eq!(refused_context_window(text), window, "{text}");
        }
    }
}
