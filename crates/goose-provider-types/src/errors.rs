use reqwest::StatusCode;
use std::time::Duration;
use thiserror::Error;

use crate::request_log::LogError;

/// The name of the error a stream parser raises when the body ends before its protocol's
/// completion marker — see [`ProviderError::stream_truncated`].
pub const STREAM_TRUNCATED: &str = "stream ended before completion";

#[derive(Error, Debug, Clone, PartialEq)]
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

impl ProviderError {
    pub fn stream_decode_error(error: impl std::fmt::Display) -> Self {
        ProviderError::NetworkError(format!("Stream decode error: {error}"))
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

/// An HTTP failure as goose frames it: `"<what> at <url>: <what the server said>"`.
pub fn http_failure_text(what: &str, url: &str, said: &str) -> String {
    format!("{what}{HTTP_FAILURE_AT}{url}{HTTP_FAILURE_SAID}{said}")
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
    let msg = if details.is_empty() {
        error.to_string()
    } else {
        format!("{} ({})", error, details.join(", "))
    };
    ProviderError::RequestFailed(msg)
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
