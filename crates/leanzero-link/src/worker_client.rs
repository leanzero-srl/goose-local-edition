//! HTTP client for the LeanZero Link auth worker — the ONLY backend the desktop talks
//! to for identity. Four endpoints, matching `leanzero-link/worker/README.md` exactly:
//! `POST /v1/auth/request-code`, `POST /v1/auth/verify`, `POST /v1/mesh/join-key`,
//! `GET /v1/health`.
//!
//! Every non-2xx is mapped to a typed [`WorkerError`] whose `error` field carries the
//! worker's response body verbatim — nothing is flattened or swallowed (loud absence).
//! The base URL is injected; it defaults to the LeanZero-hosted deployment but is ALWAYS
//! overridable (tests point it at a mock server).

use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

/// The live LeanZero Link auth worker — the self-hosted Node deployment on the Mac
/// Studio, reachable over Tailscale Funnel at this path on `:443`. A different
/// deployment overrides this via [`WorkerClient::new`] /
/// `LinkManagerConfig::worker_base_url`, and the `LEANZERO_LINK_WORKER_URL` env var
/// always wins over this default; baking the real URL here makes login work after a
/// reboot without the launchctl env being set.
pub const DEFAULT_WORKER_BASE_URL: &str = "https://worksmacstudio.tailfc4700.ts.net/leanzero-link";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// `200` body of `POST /v1/auth/request-code`.
#[derive(Debug, Clone, Deserialize)]
pub struct RequestCodeResult {
    /// The worker-normalized email the code was sent to.
    pub email: String,
    #[serde(rename = "expiresInSeconds")]
    pub expires_in_seconds: u64,
}

/// `200` body of `POST /v1/auth/verify`.
#[derive(Debug, Clone, Deserialize)]
pub struct VerifyResult {
    pub token: String,
    /// The worker-normalized account email (the JWT `sub`).
    pub email: String,
    /// `"synced" | "skipped" | "failed"` — carried as a string so a value the worker
    /// adds later never fails an otherwise-successful sign-in. The UI reads `"failed"`
    /// to note an honest contact-sync failure.
    #[serde(rename = "audienceSync")]
    pub audience_sync: String,
}

/// `200` body of `POST /v1/mesh/join-key`.
#[derive(Debug, Clone, Deserialize)]
pub struct JoinKeyResult {
    #[serde(rename = "authKey")]
    pub auth_key: String,
    /// The control-plane URL this key must be joined against. Present for the Headscale
    /// (self-hosted, per-account-isolated) path — a Headscale preauth key only works
    /// against its own server, so the two travel together. Absent for the Tailscale
    /// hosted path, where the mesh keeps its configured default login server.
    #[serde(rename = "loginServer", default)]
    pub login_server: Option<String>,
    /// The per-account node secret (a stable 32-byte hex value the worker derives per
    /// account). Every device of the account derives the SAME `/v1/swarm` bearer from it
    /// via [`crate::token::node_token_from_secret`]; the secret itself never goes on the
    /// wire between nodes. `None` when the worker predates this field — the manager then
    /// refuses to connect loudly rather than derive a token from anything else.
    #[serde(rename = "nodeSecret", default)]
    pub node_secret: Option<String>,
    #[serde(rename = "expirySeconds")]
    pub expiry_seconds: u64,
}

/// `200` body of `GET /v1/health`.
#[derive(Debug, Clone, Deserialize)]
pub struct Health {
    pub ok: bool,
    pub version: String,
    pub capabilities: Capabilities,
}

/// Which worker capabilities the deployment has configured (derived from env presence).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Capabilities {
    pub mail: bool,
    pub audience: bool,
    pub mesh: bool,
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("cannot build the worker HTTP client: {source}")]
    BuildClient { source: reqwest::Error },
    #[error("worker request to {url} failed to send: {source}")]
    Transport { url: String, source: reqwest::Error },
    #[error("worker response for {what} at {url} was not valid JSON: {source}")]
    Decode {
        what: &'static str,
        url: String,
        source: serde_json::Error,
    },

    // POST /v1/auth/request-code
    #[error(
        "rate limited on {scope}; retry after {} (worker said: {error})",
        retry_after_text(.retry_after_seconds)
    )]
    RateLimited {
        scope: String,
        /// `None` when the worker's body carried no `retryAfterSeconds` — reported as
        /// absent, never as a fabricated `0`.
        retry_after_seconds: Option<u64>,
        error: String,
    },
    #[error("mail is not configured on this worker deployment (worker said: {error})")]
    MailNotConfigured { error: String },

    // POST /v1/auth/verify
    #[error("invalid or expired code (worker said: {error})")]
    InvalidCode { error: String },
    #[error("too many verify attempts; request a new code (worker said: {error})")]
    TooManyAttempts { error: String },

    // POST /v1/mesh/join-key
    #[error("identity token expired — sign in again (worker reason: {reason}; said: {error})")]
    AuthExpired { reason: String, error: String },
    #[error("identity token rejected (worker reason: {reason}; said: {error})")]
    AuthInvalid { reason: String, error: String },
    #[error("mesh keys are not configured on this worker deployment (worker said: {error})")]
    MeshNotConfigured { error: String },

    /// Any other non-2xx — carries the status and the worker's body verbatim. Includes a
    /// `401` on join-key whose body is NOT the worker's own dead-token verdict (a
    /// proxy's HTML page, a truncated body, an unknown `reason`): it is never a reason
    /// to clear the stored identity.
    #[error("worker returned {status} for {what} at {url}: {error}")]
    Unexpected {
        status: u16,
        what: &'static str,
        url: String,
        error: String,
    },
}

fn retry_after_text(retry_after_seconds: &Option<u64>) -> String {
    match retry_after_seconds {
        Some(secs) => format!("{secs}s"),
        None => "an unspecified interval (the worker sent no retryAfterSeconds)".to_string(),
    }
}

/// The `reason` values the worker's JWT check emits for a token it has judged dead
/// (`leanzero-link/worker/src/jwt.ts`): the only 401 bodies that move the manager to
/// `LoggedOut` and clear the credential. Any other 401 body stays [`WorkerError::Unexpected`].
pub const IDENTITY_DEAD_REASONS: &[&str] = &["expired", "malformed", "bad_signature", "bad_claims"];

/// The structured fields a worker error body may carry that this client acts on. The
/// human-readable `error` string is not modelled here — it is carried verbatim as the
/// raw body ([`ErrorEnvelope::raw`]) so nothing is flattened.
#[derive(Debug, Default, Deserialize)]
struct ErrorBody {
    scope: Option<String>,
    #[serde(rename = "retryAfterSeconds")]
    retry_after_seconds: Option<u64>,
    reason: Option<String>,
}

struct ErrorEnvelope {
    /// The response body verbatim — always carried into the typed error (no flattening).
    raw: String,
    parsed: ErrorBody,
}

impl ErrorEnvelope {
    fn scope(&self) -> String {
        self.parsed
            .scope
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    }
    fn retry_after(&self) -> Option<u64> {
        self.parsed.retry_after_seconds
    }
    fn reason(&self) -> Option<&str> {
        self.parsed.reason.as_deref()
    }
}

#[derive(Debug, Clone)]
pub struct WorkerClient {
    http: reqwest::Client,
    base_url: String,
}

impl WorkerClient {
    /// A client against `base_url` (trailing slash trimmed) with the default timeout.
    pub fn new(base_url: impl Into<String>) -> Result<Self, WorkerError> {
        Self::with_timeout(base_url, DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(
        base_url: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, WorkerError> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|source| WorkerError::BuildClient { source })?;
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Ok(Self { http, base_url })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// `POST /v1/auth/request-code` — email a fresh OTP.
    pub async fn request_code(&self, email: &str) -> Result<RequestCodeResult, WorkerError> {
        let url = self.url("/v1/auth/request-code");
        let response = self
            .http
            .post(&url)
            .json(&serde_json::json!({ "email": email }))
            .send()
            .await
            .map_err(|source| WorkerError::Transport {
                url: url.clone(),
                source,
            })?;
        let status = response.status().as_u16();
        if response.status().is_success() {
            return decode("request-code", &url, response).await;
        }
        let envelope = error_envelope(response).await;
        Err(match status {
            429 => WorkerError::RateLimited {
                scope: envelope.scope(),
                retry_after_seconds: envelope.retry_after(),
                error: envelope.raw,
            },
            501 => WorkerError::MailNotConfigured {
                error: envelope.raw,
            },
            _ => WorkerError::Unexpected {
                status,
                what: "request-code",
                url,
                error: envelope.raw,
            },
        })
    }

    /// `POST /v1/auth/verify` — exchange the OTP for an identity token.
    pub async fn verify(&self, email: &str, code: &str) -> Result<VerifyResult, WorkerError> {
        let url = self.url("/v1/auth/verify");
        let response = self
            .http
            .post(&url)
            .json(&serde_json::json!({ "email": email, "code": code }))
            .send()
            .await
            .map_err(|source| WorkerError::Transport {
                url: url.clone(),
                source,
            })?;
        let status = response.status().as_u16();
        if response.status().is_success() {
            return decode("verify", &url, response).await;
        }
        let envelope = error_envelope(response).await;
        Err(match status {
            401 => WorkerError::InvalidCode {
                error: envelope.raw,
            },
            429 => WorkerError::TooManyAttempts {
                error: envelope.raw,
            },
            _ => WorkerError::Unexpected {
                status,
                what: "verify",
                url,
                error: envelope.raw,
            },
        })
    }

    /// `POST /v1/mesh/join-key` — mint an ephemeral mesh join key (plus the account's
    /// node secret). A `401` is a verdict on the stored identity token ONLY when the
    /// worker says so: a JSON body whose `reason` is one of [`IDENTITY_DEAD_REASONS`] —
    /// `expired` (the 180-day lifetime elapsed) → [`WorkerError::AuthExpired`],
    /// `malformed` / `bad_signature` / `bad_claims` → [`WorkerError::AuthInvalid`]; the
    /// manager clears the identity on those and only those. A `401` with any other body
    /// — a proxy's HTML page, a truncated body, a reason this client does not know — is
    /// [`WorkerError::Unexpected`], so a middlebox can never sign the user out.
    pub async fn join_key(&self, token: &str) -> Result<JoinKeyResult, WorkerError> {
        let url = self.url("/v1/mesh/join-key");
        let response = self
            .http
            .post(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|source| WorkerError::Transport {
                url: url.clone(),
                source,
            })?;
        let status = response.status().as_u16();
        if response.status().is_success() {
            return decode("join-key", &url, response).await;
        }
        let envelope = error_envelope(response).await;
        let reason = envelope.reason().map(str::to_string);
        Err(match (status, reason.as_deref()) {
            (401, Some("expired")) => WorkerError::AuthExpired {
                reason: "expired".to_string(),
                error: envelope.raw,
            },
            (401, Some(reason)) if IDENTITY_DEAD_REASONS.contains(&reason) => {
                WorkerError::AuthInvalid {
                    reason: reason.to_string(),
                    error: envelope.raw,
                }
            }
            (501, _) => WorkerError::MeshNotConfigured {
                error: envelope.raw,
            },
            _ => WorkerError::Unexpected {
                status,
                what: "join-key",
                url,
                error: envelope.raw,
            },
        })
    }

    /// `GET /v1/health` — what the deployment supports.
    pub async fn health(&self) -> Result<Health, WorkerError> {
        let url = self.url("/v1/health");
        let response =
            self.http
                .get(&url)
                .send()
                .await
                .map_err(|source| WorkerError::Transport {
                    url: url.clone(),
                    source,
                })?;
        let status = response.status().as_u16();
        if response.status().is_success() {
            return decode("health", &url, response).await;
        }
        let envelope = error_envelope(response).await;
        Err(WorkerError::Unexpected {
            status,
            what: "health",
            url,
            error: envelope.raw,
        })
    }
}

async fn decode<T: serde::de::DeserializeOwned>(
    what: &'static str,
    url: &str,
    response: reqwest::Response,
) -> Result<T, WorkerError> {
    let text = response
        .text()
        .await
        .map_err(|source| WorkerError::Transport {
            url: url.to_string(),
            source,
        })?;
    serde_json::from_str(&text).map_err(|source| WorkerError::Decode {
        what,
        url: url.to_string(),
        source,
    })
}

async fn error_envelope(response: reqwest::Response) -> ErrorEnvelope {
    let raw = match response.text().await {
        Ok(text) => text,
        // The read failure IS the body the user sees — never an empty string that
        // reads as "the worker said nothing".
        Err(err) => format!("<body unreadable: {err}>"),
    };
    // An unparseable body yields NO structured hints (all `None`); every caller treats
    // an absent hint as "unknown" — a 401 without a named reason is `Unexpected`, a 429
    // without `retryAfterSeconds` reports `None` — never as a verdict.
    let parsed = serde_json::from_str(&raw).unwrap_or_default();
    ErrorEnvelope { raw, parsed }
}
