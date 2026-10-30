use super::api_client::ApiClient;
use super::base::{ConfigKey, ModelInfo, Provider, ProviderMetadata};
use super::retry::ProviderRetry;
use crate::api_client::{AuthMethod, TlsConfig};
use crate::conversation::message::{MediaContent, MediaKind, Message, MessageContent};
use crate::conversation::token_usage::ProviderUsage;
use crate::declarative::{DeclarativeProviderConfig, KeyResolver};
use crate::errors::ProviderError;
use crate::formats::openai::is_openai_responses_model;
use crate::formats::openai::{
    create_request_reporting_images, get_usage, response_to_message, turn_context_tail_suffix,
    OpenAiFormatOptions,
};
use crate::formats::openai_responses::{
    create_responses_request, get_responses_usage, responses_api_to_message, ResponsesApiResponse,
};
use crate::http_status::sanitize_url;
use crate::images::{
    announced, convert_image, convert_media, split_pcm_wav, withheld_images_notice, ImageFormat,
    ImageInput,
};
use crate::openai_compatible::{
    handle_response_openai_compat, handle_status, stream_responses_compat,
};
use crate::request_log::{start_log, LoggerHandleExt};
use crate::thinking::ThinkingEffort;
use anyhow::Result;
use async_trait::async_trait;
use reqwest::StatusCode;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use crate::base::{MessageStream, ProviderDescriptor};
use crate::model::ModelConfig;
use rmcp::model::Tool;

pub const OPEN_AI_PROVIDER_NAME: &str = "openai";
pub const OPEN_AI_DEFAULT_BASE_PATH: &str = "v1/chat/completions";
pub const OPEN_AI_VERSIONLESS_BASE_PATH: &str = "chat/completions";
const OPEN_AI_DEFAULT_RESPONSES_PATH: &str = "v1/responses";
const OPEN_AI_DEFAULT_MODELS_PATH: &str = "v1/models";
/// LM Studio's native models endpoint. Its `/v1/models` carries only `id`/`object`/`owned_by`
/// (measured 2026-09-02 on the local node); the loaded window is served here as
/// `loaded_context_length`, beside `max_context_length` and `state`.
const LM_STUDIO_NATIVE_MODELS_PATH: &str = "api/v0/models";
pub const OPEN_AI_DEFAULT_MODEL: &str = "gpt-4o";
pub const OPEN_AI_DEFAULT_FAST_MODEL: &str = "gpt-4o-mini";
pub const OPEN_AI_KNOWN_MODELS: &[(&str, usize)] = &[
    ("gpt-4o", 128_000),
    ("gpt-4o-mini", 128_000),
    ("gpt-4.1", 128_000),
    ("gpt-4.1-mini", 128_000),
    ("o1", 200_000),
    ("o3", 200_000),
    ("gpt-3.5-turbo", 16_385),
    ("gpt-4-turbo", 128_000),
    ("o4-mini", 128_000),
    ("gpt-5", 400_000),
    ("gpt-5-mini", 400_000),
    ("gpt-5-nano", 400_000),
    ("gpt-5-pro", 400_000),
    ("gpt-5-codex", 400_000),
    ("gpt-5.1", 400_000),
    ("gpt-5.1-codex", 400_000),
    ("gpt-5.2", 400_000),
    ("gpt-5.2-codex", 400_000),
    ("gpt-5.2-pro", 400_000),
    ("gpt-5.3-codex", 400_000),
    ("gpt-5.4", 1_050_000),
    ("gpt-5.4-mini", 400_000),
    ("gpt-5.4-nano", 400_000),
    ("gpt-5.4-pro", 1_050_000),
];

pub const OPEN_AI_DOC_URL: &str = "https://platform.openai.com/docs/models";
const DEFAULT_TIMEOUT_SECONDS: u64 = 600;

type OpenAiBaseUrlParts = (String, Vec<(String, String)>, bool);

/// Ensure a base URL has an explicit scheme.
///
/// Users frequently enter hosts like `localhost:1234` without a scheme. The
/// `url` crate parses such input as `scheme="localhost"`, `path="1234"`,
/// silently dropping both the host and the port. When no `://` is present we
/// prepend a sensible scheme (`http://` for local hosts, `https://`
/// otherwise) so the host and port survive parsing.
pub fn ensure_url_scheme(raw_url: &str) -> String {
    let trimmed = raw_url.trim();
    if trimmed.contains("://") {
        return trimmed.to_string();
    }

    let host_part = trimmed.split(['/', '?']).next().unwrap_or(trimmed);
    let bare_host = if let Some(rest) = host_part.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else {
        host_part.split(':').next().unwrap_or(host_part)
    };
    let is_local = bare_host == "localhost"
        || bare_host == "127.0.0.1"
        || bare_host == "0.0.0.0"
        || bare_host == "::1";

    let scheme = if is_local { "http" } else { "https" };
    format!("{}://{}", scheme, trimmed)
}

pub fn parse_openai_base_url(raw_url: &str) -> Result<OpenAiBaseUrlParts> {
    let raw_url = ensure_url_scheme(raw_url);
    let raw_url = raw_url.as_str();
    let parsed = url::Url::parse(raw_url).map_err(|e| {
        anyhow::anyhow!("Invalid OPENAI_BASE_URL '{}': {}", sanitize_url(raw_url), e)
    })?;

    let authority = parsed[..url::Position::BeforePath].to_string();
    let query_params: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let path = parsed.path().trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return Ok((authority, query_params, true));
    }

    if path == "/v1" {
        return Ok((authority, query_params, true));
    }
    if let Some(prefix) = path.strip_suffix("/v1") {
        return Ok((format!("{}{}", authority, prefix), query_params, true));
    }

    Ok((format!("{}{}", authority, path), query_params, false))
}

#[derive(Debug, serde::Serialize)]
pub struct OpenAiProvider {
    #[serde(skip)]
    api_client: ApiClient,
    base_path: String,
    organization: Option<String>,
    project: Option<String>,
    custom_headers: Option<HashMap<String, String>>,
    supports_streaming: bool,
    name: String,
    custom_models: Option<Vec<String>>,
    dynamic_models: Option<bool>,
    skip_canonical_filtering: bool,
    preserve_thinking_context: bool,
    #[serde(skip)]
    n_ctx_cache: Arc<Mutex<HashMap<String, Option<usize>>>>,
    /// Per model: whether the endpoint's `/v1/models` entry declared
    /// `request_extensions: ["rapid_mlx_transient_tail"]`. Only answered probes are cached.
    #[serde(skip)]
    transient_tail_cache: Arc<Mutex<HashMap<String, bool>>>,
}

/// Builder for [`OpenAiProvider`].
///
/// Exposes every field of the provider so that constructors living outside
/// `openai.rs` (e.g. in `openai_def.rs`) can assemble a provider without
/// needing direct access to the struct's private fields.
pub struct OpenAiProviderBuilder {
    api_client: ApiClient,
    base_path: String,
    organization: Option<String>,
    project: Option<String>,
    custom_headers: Option<HashMap<String, String>>,
    supports_streaming: bool,
    name: String,
    custom_models: Option<Vec<String>>,
    dynamic_models: Option<bool>,
    skip_canonical_filtering: bool,
    preserve_thinking_context: bool,
}

impl OpenAiProviderBuilder {
    pub fn new(api_client: ApiClient) -> Self {
        Self {
            api_client,
            base_path: OPEN_AI_DEFAULT_BASE_PATH.to_string(),
            organization: None,
            project: None,
            custom_headers: None,
            supports_streaming: true,
            name: OPEN_AI_PROVIDER_NAME.to_string(),
            custom_models: None,
            dynamic_models: None,
            skip_canonical_filtering: false,
            preserve_thinking_context: false,
        }
    }

    pub fn api_client(mut self, api_client: ApiClient) -> Self {
        self.api_client = api_client;
        self
    }

    pub fn map_api_client(mut self, f: impl FnOnce(ApiClient) -> ApiClient) -> Self {
        self.api_client = f(self.api_client);
        self
    }

    pub fn try_map_api_client(
        mut self,
        f: impl FnOnce(ApiClient) -> Result<ApiClient>,
    ) -> Result<Self> {
        self.api_client = f(self.api_client)?;
        Ok(self)
    }

    pub fn base_path(mut self, base_path: impl Into<String>) -> Self {
        self.base_path = base_path.into();
        self
    }

    pub fn organization(mut self, organization: Option<String>) -> Self {
        self.organization = organization;
        self
    }

    pub fn project(mut self, project: Option<String>) -> Self {
        self.project = project;
        self
    }

    pub fn custom_headers(mut self, custom_headers: Option<HashMap<String, String>>) -> Self {
        self.custom_headers = custom_headers;
        self
    }

    pub fn supports_streaming(mut self, supports_streaming: bool) -> Self {
        self.supports_streaming = supports_streaming;
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn custom_models(mut self, custom_models: Option<Vec<String>>) -> Self {
        self.custom_models = custom_models;
        self
    }

    pub fn dynamic_models(mut self, dynamic_models: Option<bool>) -> Self {
        self.dynamic_models = dynamic_models;
        self
    }

    pub fn skip_canonical_filtering(mut self, skip_canonical_filtering: bool) -> Self {
        self.skip_canonical_filtering = skip_canonical_filtering;
        self
    }

    pub fn preserve_thinking_context(mut self, preserve_thinking_context: bool) -> Self {
        self.preserve_thinking_context = preserve_thinking_context;
        self
    }

    pub fn build(self) -> OpenAiProvider {
        OpenAiProvider {
            api_client: self.api_client,
            base_path: self.base_path,
            organization: self.organization,
            project: self.project,
            custom_headers: self.custom_headers,
            supports_streaming: self.supports_streaming,
            name: self.name,
            custom_models: self.custom_models,
            dynamic_models: self.dynamic_models,
            skip_canonical_filtering: self.skip_canonical_filtering,
            preserve_thinking_context: self.preserve_thinking_context,
            n_ctx_cache: Arc::new(Mutex::new(HashMap::new())),
            transient_tail_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl OpenAiProvider {
    /// local-edition: optional HARD cap on the effective context window via the
    /// `GOOSE_LOCAL_CONTEXT_CAP` env var. Quality-first context for hybrid local models
    /// (Gated DeltaNet / Qwen3.6) keeps the working window lean — linear-attention recall
    /// degrades on bloated context and prefill is costly — so we cap below the model's
    /// native (often YaRN-extended ~1M) limit. No-op when the var is unset or 0.
    fn apply_local_context_cap(&self, limit: usize) -> usize {
        match std::env::var("GOOSE_LOCAL_CONTEXT_CAP")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
        {
            Some(cap) if cap > 0 => limit.min(cap),
            _ => limit,
        }
    }

    #[doc(hidden)]
    pub fn new(api_client: ApiClient) -> Self {
        Self {
            api_client,
            base_path: OPEN_AI_DEFAULT_BASE_PATH.to_string(),
            organization: None,
            project: None,
            custom_headers: None,
            supports_streaming: true,
            name: OPEN_AI_PROVIDER_NAME.to_string(),
            custom_models: None,
            dynamic_models: None,
            skip_canonical_filtering: false,
            preserve_thinking_context: false,
            n_ctx_cache: Arc::new(Mutex::new(HashMap::new())),
            transient_tail_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn normalize_base_path(base_path: &str) -> String {
        if let Some(path) = base_path.strip_prefix('/') {
            format!("/{}", path.trim_end_matches('/'))
        } else {
            base_path.trim_end_matches('/').to_string()
        }
    }

    fn is_chat_completions_path(base_path: &str) -> bool {
        let normalized = Self::normalize_base_path(base_path).to_ascii_lowercase();
        normalized.contains("chat/completions")
    }

    fn is_responses_path(base_path: &str) -> bool {
        let normalized = Self::normalize_base_path(base_path).to_ascii_lowercase();
        normalized.ends_with("responses") || normalized.contains("/responses")
    }

    fn is_responses_model(model_name: &str) -> bool {
        is_openai_responses_model(model_name)
    }

    fn should_use_responses_api(model_name: &str, base_path: &str) -> bool {
        let normalized_base_path = Self::normalize_base_path(base_path);
        // Only the standard "v1/chat/completions" is treated as a default
        // path that defers to model-based routing.  The versionless
        // "chat/completions" (derived from an OPENAI_BASE_URL without /v1)
        // is treated as custom because versionless gateways typically do not
        // support the Responses API.
        let has_custom_base_path = normalized_base_path != OPEN_AI_DEFAULT_BASE_PATH;

        if has_custom_base_path {
            if Self::is_responses_path(&normalized_base_path) {
                return true;
            }
            if Self::is_chat_completions_path(&normalized_base_path) {
                return false;
            }
        }

        Self::is_responses_model(model_name)
    }

    /// Providers known to reject `max_completion_tokens` and require
    /// the legacy `max_tokens` field instead.
    const PROVIDERS_NEEDING_MAX_TOKENS_REMAP: &[&str] = &[
        "cerebras",
        "custom_deepseek",
        "groq",
        "inception",
        "kimi",
        "lmstudio",
        "mistral",
        "moonshot",
        "nearai",
        "ovhcloud",
    ];

    const PROVIDERS_NEEDING_STANDARD_CHAT_PARAMS: &[&str] = &["nearai"];

    /// The provider goose routes its supervised MLX engine through (the swarm's `mlx-sidecar`
    /// device and the desktop's LeanZero MLX tile both resolve to it). Only these endpoints are
    /// asked whether they accept the transient-tail extension; no other provider is probed or
    /// sent the field.
    const PROVIDERS_FRONTING_RAPID_MLX: &[&str] = &["omlx"];

    /// Alibaba's Token Plan endpoint speaks a stricter dialect than the OpenAI shape it otherwise
    /// accepts: the cap must arrive as `max_completion_tokens`, `max_tokens` is rejected outright
    /// rather than ignored, and reasoning is opt-in per request through `enable_thinking` /
    /// `preserve_thinking`. Without all four the newer Qwen models answer without reasoning at all.
    const PROVIDERS_NEEDING_QWEN_TOKEN_PLAN: &[&str] = &["alibaba"];

    fn sanitize_request_for_compat(&self, mut payload: serde_json::Value) -> serde_json::Value {
        if let Some(obj) = payload.as_object_mut() {
            if Self::PROVIDERS_NEEDING_MAX_TOKENS_REMAP.contains(&self.name.as_str()) {
                if let Some(value) = obj.remove("max_completion_tokens") {
                    obj.entry("max_tokens").or_insert(value);
                }
            }

            if Self::PROVIDERS_NEEDING_QWEN_TOKEN_PLAN.contains(&self.name.as_str()) {
                if let Some(value) = obj.remove("max_tokens") {
                    obj.entry("max_completion_tokens").or_insert(value);
                }
                obj.insert("enable_thinking".to_string(), serde_json::Value::Bool(true));
                obj.insert(
                    "preserve_thinking".to_string(),
                    serde_json::Value::Bool(true),
                );
            }

            if Self::PROVIDERS_NEEDING_STANDARD_CHAT_PARAMS.contains(&self.name.as_str()) {
                let model_name = obj.get("model").and_then(|model| model.as_str());
                if !model_name.is_some_and(Self::is_responses_model) {
                    obj.remove("reasoning_effort");
                }

                if let Some(messages) = obj.get_mut("messages").and_then(|m| m.as_array_mut()) {
                    for message in messages {
                        if message
                            .get("role")
                            .and_then(|role| role.as_str())
                            .is_some_and(|role| role == "developer")
                        {
                            message["role"] = serde_json::Value::String("system".to_string());
                        }
                    }
                }
            }
        }

        payload
    }

    /// A request that asked for no reasoning (`ThinkingEffort::Off`: goose's helper calls — tool
    /// labels, titles, tool-pair digests, compaction, the end-of-turn reviewer — via goose's one
    /// helper path, `model_config::complete_helper`) reaches an MLX engine
    /// as `chat_template_kwargs.enable_thinking = false`. The chat format emits an effort only for
    /// OpenAI Responses models, so without this the Off was dropped on the way and the model's own
    /// template thought anyway — measured 2026-09-25 on the Qwen3.8-27B split: a 3–8 word tool label
    /// cost 51 completion tokens and 6.3 s with thinking, 6 tokens and 2.2 s with the switch off.
    /// Rapid-MLX and mlx_lm both pass these kwargs to the template renderer; a template that reads
    /// no `enable_thinking` ignores the extra variable. Off wins over the model profile's own
    /// switch (the swarm router fills it in only where the request set none), and the profile's
    /// other kwargs stay.
    fn carry_thinking_off_to_mlx(
        &self,
        model_config: &ModelConfig,
        payload: &mut serde_json::Value,
    ) -> Result<(), ProviderError> {
        if !Self::PROVIDERS_FRONTING_RAPID_MLX.contains(&self.name.as_str())
            || model_config.thinking_effort() != Some(ThinkingEffort::Off)
        {
            return Ok(());
        }
        let Some(obj) = payload.as_object_mut() else {
            return Err(ProviderError::ExecutionError(
                "the MLX request body is not a JSON object, so thinking cannot be switched off"
                    .to_string(),
            ));
        };
        match obj
            .entry("chat_template_kwargs")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
        {
            serde_json::Value::Object(kwargs) => {
                kwargs.insert(
                    "enable_thinking".to_string(),
                    serde_json::Value::Bool(false),
                );
                Ok(())
            }
            other => Err(ProviderError::ExecutionError(format!(
                "chat_template_kwargs is {other}, not an object, so the request's thinking-off cannot reach the MLX engine"
            ))),
        }
    }

    /// The benchmark's thinking pin reaches a LOCAL engine (2026-10-29, the T12 G1 gap). The bench
    /// pins `GOOSE_THINKING_EFFORT` (medium since b497d064bc), but the chat format emits an effort
    /// only for OpenAI Responses models, and Rapid-MLX switches thinking OFF on every tool-bearing
    /// request that pins no preference (`maybe_auto_disable_thinking_for_tools`). So a local
    /// entrant ran its whole Gauntlet without thinking while the cloud baseline it is compared
    /// with thought (cloud Qwen3.8 Flash 10-05: 167 of 174 assistant messages carry thinking).
    /// At a loopback fork engine (`lz_engine`, see [`Self::openai_at_lz_engine`]) an effort other
    /// than Off now rides as `chat_template_kwargs.enable_thinking = true` plus the OpenAI
    /// `reasoning_effort` (low / medium / high; Max sends high), which the engine maps onto the
    /// template's own vocabulary (#3043: Qwen3.8 low / medium / xhigh) or onto a reasoning cap.
    /// Off rides as `enable_thinking = false`, as on omlx. An explicit value already in the body
    /// wins; no effort configured = the body is unchanged.
    fn carry_effort_to_lz_engine(
        model_config: &ModelConfig,
        payload: &mut serde_json::Value,
        lz_engine: bool,
    ) -> Result<(), ProviderError> {
        let Some(effort) = model_config.thinking_effort().filter(|_| lz_engine) else {
            return Ok(());
        };
        let Some(obj) = payload.as_object_mut() else {
            return Err(ProviderError::ExecutionError(
                "the MLX request body is not a JSON object, so the thinking effort cannot reach it"
                    .to_string(),
            ));
        };
        let level = match effort {
            ThinkingEffort::Off => None,
            ThinkingEffort::Low => Some("low"),
            ThinkingEffort::Medium => Some("medium"),
            ThinkingEffort::High | ThinkingEffort::Max => Some("high"),
        };
        match obj
            .entry("chat_template_kwargs")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
        {
            serde_json::Value::Object(kwargs) => {
                kwargs
                    .entry("enable_thinking")
                    .or_insert(serde_json::Value::Bool(level.is_some()));
            }
            other => {
                return Err(ProviderError::ExecutionError(format!(
                    "chat_template_kwargs is {other}, not an object, so the thinking effort cannot reach the MLX engine"
                )))
            }
        }
        if let Some(level) = level {
            obj.entry("reasoning_effort")
                .or_insert_with(|| serde_json::Value::String(level.to_string()));
        }
        Ok(())
    }

    /// Whether this endpoint declared it honours `rapid_mlx_transient_tail` (the LeanZero Rapid-MLX
    /// fork, since v0.14.3-lz.2). The declaration is the engine identity: a stock Rapid-MLX or a
    /// real oMLX server behind `OMLX_HOST` never declares it and so never receives the field. Only a
    /// swarm worker's block is ever named by it: a chat keeps its blocks, so none of its is
    /// transient.
    async fn transient_tail(&self, model_name: &str) -> bool {
        if !Self::PROVIDERS_FRONTING_RAPID_MLX.contains(&self.name.as_str()) {
            return false;
        }
        self.declares_lz_engine(model_name).await
    }

    /// Whether this is the built-in `openai` provider at a LOOPBACK LeanZero Rapid-MLX engine: a
    /// benchmark's "Single model" entrant on a local model (the bench fence refuses swarm/omlx).
    /// The engine identity is the fork's `rapid_mlx_transient_tail` declaration (the same cached
    /// `/v1/models` probe as [`Self::transient_tail`]); a remote host or a stock server is never one.
    async fn openai_at_lz_engine(&self, model_name: &str) -> bool {
        self.name == "openai"
            && host_is_loopback(self.api_client.host())
            && self.declares_lz_engine(model_name).await
    }

    /// The cached `/v1/models` check that the endpoint serving `model_name` declares
    /// `rapid_mlx_transient_tail`, i.e. is the LeanZero Rapid-MLX fork. Only answered probes are cached.
    async fn declares_lz_engine(&self, model_name: &str) -> bool {
        if let Some(known) = self
            .transient_tail_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(model_name).copied())
        {
            return known;
        }
        let models_path =
            Self::map_base_path(&self.base_path, "models", OPEN_AI_DEFAULT_MODELS_PATH);
        let json = match self.fetch_models_json(&models_path).await {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!(
                    host = %sanitize_url(self.api_client.host()),
                    model = %model_name,
                    error = %e,
                    "transient_tail_probe_failed: the turn-context tail rides this request \
                     unmarked; the probe is retried on the next request"
                );
                return false;
            }
        };
        let accepted = declares_request_extension(&json, model_name, RAPID_MLX_TRANSIENT_TAIL);
        if !accepted {
            tracing::info!(
                host = %sanitize_url(self.api_client.host()),
                model = %model_name,
                "the engine does not declare {RAPID_MLX_TRANSIENT_TAIL}; its hybrid cache \
                 snapshots after the per-turn context block"
            );
        }
        if let Ok(mut cache) = self.transient_tail_cache.lock() {
            cache.insert(model_name.to_string(), accepted);
        }
        accepted
    }

    /// Whether the engine serving `model_name` reads images, as its `/v1/models` entry declares it
    /// in `capabilities` (the Rapid-MLX shape every goose MLX engine serves: the single engine,
    /// the pipeline split, the tensor split's wrapper). Asked fresh on every request that carries
    /// an image — never cached, because a remount can move the same id to another lane — and
    /// never asked when the request carries none, so a text request is byte-identical.
    async fn image_input(&self, model_name: &str) -> Result<ImageInput, ProviderError> {
        let models_path =
            Self::map_base_path(&self.base_path, "models", OPEN_AI_DEFAULT_MODELS_PATH);
        let json = self.fetch_models_json(&models_path).await?;
        Ok(declared_image_input(&json, model_name))
    }

    /// The format options with attached audio/video withheld behind their named placeholders when
    /// a goose MLX engine's `/v1/models` lists capabilities for the model without `audio` /
    /// `video`. A request without media, a provider that does not front a goose MLX engine and an
    /// engine that declares nothing (or cannot be asked) keep the options as given.
    async fn media_format_options(
        &self,
        model_name: &str,
        messages: &[Message],
        options: OpenAiFormatOptions,
    ) -> OpenAiFormatOptions {
        let carries_media = messages.iter().any(|m| {
            m.content
                .iter()
                .any(|c| matches!(c, MessageContent::Media(_)))
        });
        if !carries_media || !self.asks_engine_for_image_input() {
            return options;
        }
        let models_path =
            Self::map_base_path(&self.base_path, "models", OPEN_AI_DEFAULT_MODELS_PATH);
        match self.fetch_models_json(&models_path).await {
            Ok(json) => {
                let (withhold_audio, withhold_video) = withheld_media(&json, model_name);
                if withhold_audio || withhold_video {
                    tracing::info!(
                        host = %sanitize_url(self.api_client.host()),
                        model = %model_name,
                        withhold_audio,
                        withhold_video,
                        "media_withheld: the engine does not declare audio/video input for this \
                         model; those parts went as named placeholders"
                    );
                }
                OpenAiFormatOptions {
                    withhold_audio: options.withhold_audio || withhold_audio,
                    withhold_video: options.withhold_video || withhold_video,
                    ..options
                }
            }
            Err(e) => {
                tracing::warn!(
                    host = %sanitize_url(self.api_client.host()),
                    model = %model_name,
                    error = %e,
                    "media_input_probe_failed: the attached audio/video is sent as it is"
                );
                options
            }
        }
    }

    /// The media pre-pass (3.0.135). Rapid-MLX's MLLM lane caps a request that carries media at
    /// 8,192 prompt tokens (`mllm_batch_generator._prefill_cap_violation`), and a goose chat turn
    /// (system prompt + tools) is ~21k on its own, so every attached clip in the agent turn is
    /// refused 400. Raising the cap is not safe: a ~22k-token video prefill passed 33 GB (one
    /// forward, Gemma's bidirectional image attention). So each attached audio/video clip (and,
    /// on an engine that hears audio - the MLLM lane - each attached image) is sent to the same
    /// engine alone in a short side request, and the agent turn carries the returned transcript or
    /// description as a labelled text part. Text-only requests on that lane are chunked and exempt
    /// from the cap. `None` = nothing to do (no media, not a goose MLX engine, or the engine
    /// withholds every attached kind, which keeps the named placeholders).
    async fn media_prepass(
        &self,
        model_config: &ModelConfig,
        messages: &[Message],
    ) -> Option<Vec<Message>> {
        let carries = |m: &Message| {
            m.role == rmcp::model::Role::User
                && m.content
                    .iter()
                    .any(|c| matches!(c, MessageContent::Media(_) | MessageContent::Image(_)))
        };
        if !messages.iter().any(carries) || !self.asks_engine_for_image_input() {
            return None;
        }
        let model_name = model_config.model_name.as_str();
        let models_path =
            Self::map_base_path(&self.base_path, "models", OPEN_AI_DEFAULT_MODELS_PATH);
        let json = self.fetch_models_json(&models_path).await.ok()?;
        let caps = prepass_kinds(&json, model_name);
        if !caps.audio && !caps.video {
            return None;
        }
        let mut changed = false;
        let mut out = Vec::with_capacity(messages.len());
        for message in messages {
            if !carries(message) {
                out.push(message.clone());
                continue;
            }
            let ask = message.as_concat_text();
            let mut rewritten = message.clone();
            rewritten.content.clear();
            let mut image_no = 0usize;
            for content in &message.content {
                let attachment = match content {
                    MessageContent::Media(media)
                        if match media.kind {
                            MediaKind::Audio => caps.audio,
                            MediaKind::Video => caps.video,
                        } =>
                    {
                        Some(Attachment::Media(media))
                    }
                    MessageContent::Image(image) if caps.images => {
                        image_no += 1;
                        Some(Attachment::Image(image, image_no))
                    }
                    _ => None,
                };
                match attachment {
                    Some(attachment) => {
                        changed = true;
                        let text = self.attachment_text(model_name, &ask, &attachment).await;
                        rewritten.content.push(MessageContent::text(text));
                    }
                    None => rewritten.content.push(content.clone()),
                }
            }
            out.push(rewritten);
        }
        changed.then_some(out)
    }

    /// The labelled text the agent turn carries for one attachment: the engine's transcript or
    /// description from a side request (cached by content), or the reason it could not be made.
    async fn attachment_text(
        &self,
        model_name: &str,
        ask: &str,
        attachment: &Attachment<'_>,
    ) -> String {
        let key = format!(
            "{}|{}|{}",
            self.api_client.host(),
            model_name,
            attachment.digest()
        );
        let cached = MEDIA_PREPASS_CACHE
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned());
        if let Some(text) = cached {
            tracing::info!(model = %model_name, kind = attachment.kind(), "media_prepass: cached");
            return attachment.label(model_name, Ok(&text));
        }
        let started = std::time::Instant::now();
        let pieces = attachment.pieces();
        let whole = pieces.len() == 1;
        let mut texts = Vec::with_capacity(pieces.len());
        let mut failures = 0usize;
        let mut prompt_tokens = 0u64;
        for (span, part) in &pieces {
            let payload = prepass_payload(model_name, ask, attachment.instruction(), part.clone());
            let text = match self.prepass_request(&payload).await {
                Ok((text, tokens)) => {
                    prompt_tokens += tokens.unwrap_or(0);
                    text
                }
                Err(e) => {
                    failures += 1;
                    tracing::warn!(
                        host = %sanitize_url(self.api_client.host()),
                        model = %model_name,
                        kind = attachment.kind(),
                        error = %e,
                        "media_prepass_failed: the agent turn names the attachment and the reason"
                    );
                    if whole {
                        return attachment.label(model_name, Err(&e.to_string()));
                    }
                    format!("(this part could not be turned into text: {e})")
                }
            };
            texts.push(match span {
                Some((from, to)) => format!("[{} - {}] {text}", clock(*from), clock(*to)),
                None => text,
            });
        }
        let text = texts.join("\n");
        tracing::info!(
            host = %sanitize_url(self.api_client.host()),
            model = %model_name,
            kind = attachment.kind(),
            pieces = pieces.len(),
            failures,
            prompt_tokens,
            chars = text.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "media_prepass: the attachment went to the engine alone; the agent turn carries its text"
        );
        if failures == 0 {
            if let Ok(mut cache) = MEDIA_PREPASS_CACHE.lock() {
                if cache.len() >= MEDIA_PREPASS_CACHE_ENTRIES {
                    cache.clear();
                }
                cache.insert(key, text.clone());
            }
        }
        attachment.label(model_name, Ok(&text))
    }

    async fn prepass_request(
        &self,
        payload: &serde_json::Value,
    ) -> Result<(String, Option<u64>), ProviderError> {
        // One attempt: a clip the engine cannot decode fails the same way every time, and the
        // agent turn still goes with the reason named.
        let response = handle_status(
            self.api_client
                .response_post(&self.base_path, payload)
                .await?,
        )
        .await?;
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("Failed to parse JSON: {e}")))?;
        let text = json["choices"][0]["message"]["content"]
            .as_str()
            .map(str::trim)
            .unwrap_or_default();
        if text.is_empty() {
            return Err(ProviderError::RequestFailed(
                "the engine returned no text for the attachment".to_string(),
            ));
        }
        Ok((text.to_string(), json["usage"]["prompt_tokens"].as_u64()))
    }

    /// Whether this provider asks the endpoint's `/v1/models` if it reads images before sending
    /// one: the `omlx` lane, and the built-in `openai` provider pointed at a loopback host, which is
    /// how a benchmark's "Single model" entrant reaches the app's MLX engine (provider openai, host
    /// http://127.0.0.1:<engine port>). 2026-10-27: a local MiMo-V2.6-Flash Gauntlet sb-7.2 run on
    /// the pipeline split ended did_not_finish after 2 h when read_image's PNG went to :8191 as an
    /// image part (404 "Only 'text' content type is supported") — Q-260 had covered omlx only.
    /// A remote OpenAI-compatible host is not asked; its own answer decides (see
    /// `refuses_non_text_content`).
    fn asks_engine_for_image_input(&self) -> bool {
        Self::PROVIDERS_FRONTING_RAPID_MLX.contains(&self.name.as_str())
            || (self.name == "openai" && host_is_loopback(self.api_client.host()))
    }

    /// The request with every image part replaced by its named placeholder, and the chat's notice
    /// for the images it is the first to withhold.
    fn text_only_request(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
        format_options: OpenAiFormatOptions,
    ) -> Result<(serde_json::Value, Option<String>), ProviderError> {
        let (payload, images) = create_request_reporting_images(
            model_config,
            system,
            messages,
            tools,
            &ImageFormat::OpenAi,
            self.supports_streaming,
            OpenAiFormatOptions {
                text_only_engine: true,
                ..format_options
            },
        )?;
        Ok((
            payload,
            withheld_images_notice(&model_config.model_name, &images.withheld),
        ))
    }

    /// The finishing touches every chat-completions body gets before it is posted.
    fn finish_chat_payload(
        &self,
        model_config: &ModelConfig,
        messages: &[Message],
        payload: serde_json::Value,
        transient_tail: bool,
        lz_engine: bool,
    ) -> Result<serde_json::Value, ProviderError> {
        let mut payload = self.sanitize_request_for_compat(payload);
        self.carry_thinking_off_to_mlx(model_config, &mut payload)?;
        Self::carry_effort_to_lz_engine(model_config, &mut payload, lz_engine)?;
        if transient_tail {
            if let Some(tail) = turn_context_tail_suffix(messages, &payload) {
                payload[RAPID_MLX_TRANSIENT_TAIL] = serde_json::Value::String(tail);
            }
        }
        Ok(payload)
    }

    async fn post_chat(
        &self,
        payload: &serde_json::Value,
    ) -> Result<reqwest::Response, ProviderError> {
        self.with_retry(|| async {
            let resp = self
                .api_client
                .response_post(&self.base_path, payload)
                .await?;
            handle_status(resp).await
        })
        .await
    }

    /// Q-260: the request for this engine, with every image part withheld behind a named
    /// placeholder when the engine declares it reads text only, and the chat's notice for the
    /// images this request is the first to withhold. A request with no image, a provider that does
    /// not front a goose MLX engine, an engine that reads images, and one that declares nothing
    /// are all sent exactly as formatted.
    async fn chat_request(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
        format_options: OpenAiFormatOptions,
    ) -> Result<(serde_json::Value, Option<String>), ProviderError> {
        let build = |options: OpenAiFormatOptions| -> Result<_, ProviderError> {
            Ok(create_request_reporting_images(
                model_config,
                system,
                messages,
                tools,
                &ImageFormat::OpenAi,
                self.supports_streaming,
                options,
            )?)
        };
        let format_options = self
            .media_format_options(&model_config.model_name, messages, format_options)
            .await;
        let (payload, images) = build(format_options)?;
        if images.sent == 0 || !self.asks_engine_for_image_input() {
            return Ok((payload, None));
        }
        match self.image_input(&model_config.model_name).await {
            Ok(ImageInput::TextOnly) => {
                let (payload, images) = build(OpenAiFormatOptions {
                    text_only_engine: true,
                    ..format_options
                })?;
                tracing::info!(
                    host = %sanitize_url(self.api_client.host()),
                    model = %model_config.model_name,
                    withheld = images.withheld.len(),
                    "images_withheld: the engine declares it reads text only; every image part \
                     went as a named placeholder"
                );
                let notice = withheld_images_notice(&model_config.model_name, &images.withheld);
                Ok((payload, notice))
            }
            Ok(ImageInput::Reads) => Ok((payload, None)),
            Ok(ImageInput::Undeclared) => {
                tracing::warn!(
                    host = %sanitize_url(self.api_client.host()),
                    model = %model_config.model_name,
                    images = images.sent,
                    "image_input_undeclared: the engine's /v1/models names no capabilities, so \
                     whether it reads images is unknown; the images are sent as they are"
                );
                Ok((payload, None))
            }
            Err(e) => {
                tracing::warn!(
                    host = %sanitize_url(self.api_client.host()),
                    model = %model_config.model_name,
                    images = images.sent,
                    error = %e,
                    "image_input_probe_failed: whether the engine reads images is unknown; the \
                     images are sent as they are"
                );
                Ok((payload, None))
            }
        }
    }

    fn should_use_responses_api_for_provider(&self, model_name: &str) -> bool {
        if Self::PROVIDERS_NEEDING_STANDARD_CHAT_PARAMS.contains(&self.name.as_str()) {
            return false;
        }

        Self::should_use_responses_api(model_name, &self.base_path)
    }

    fn map_base_path(base_path: &str, target: &str, fallback: &str) -> String {
        let normalized = Self::normalize_base_path(base_path);
        if normalized.ends_with(target) || normalized.contains(&format!("/{target}")) {
            return normalized;
        }

        if Self::is_chat_completions_path(&normalized) {
            return normalized.replacen("chat/completions", target, 1);
        }

        if Self::is_responses_path(&normalized) {
            return normalized.replacen("responses", target, 1);
        }

        if normalized.starts_with('/') {
            format!("/{}", fallback.trim_start_matches('/'))
        } else {
            fallback.to_string()
        }
    }

    async fn fetch_models_from_api(&self) -> Result<Vec<String>, ProviderError> {
        let models_path =
            Self::map_base_path(&self.base_path, "models", OPEN_AI_DEFAULT_MODELS_PATH);
        let response = self.api_client.request(&models_path).response_get().await?;

        if response.status() == StatusCode::NOT_FOUND {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::EndpointNotFound(body));
        }

        let json = handle_response_openai_compat(response).await?;
        if let Some(err_obj) = json.get("error") {
            let msg = err_obj
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(ProviderError::Authentication(msg.to_string()));
        }

        let data = json.get("data").and_then(|v| v.as_array()).ok_or_else(|| {
            ProviderError::UsageError("Missing data field in JSON response".into())
        })?;
        let mut models: Vec<String> = data
            .iter()
            .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(str::to_string))
            .collect();
        models.sort();
        Ok(models)
    }

    /// Probe the server for the context window it actually has loaded for `model_name`.
    /// Two shapes, in this order: llama.cpp and Ollama report it as the non-standard
    /// `meta.n_ctx` on `/v1/models`; LM Studio reports it as `loaded_context_length` on
    /// `/api/v0/models` (its `/v1/models` carries no window at all). `Err` names what each
    /// endpoint said so the caller can log WHY it is falling to the default — r6f's opener was
    /// compacted at 107,199 tokens and r6h's data-stream lane compressed its code against
    /// "~11k remaining" because this probe missed LM Studio's shape and nothing said so.
    async fn probe_context_window(&self, model_name: &str) -> Result<usize, String> {
        let models_path =
            Self::map_base_path(&self.base_path, "models", OPEN_AI_DEFAULT_MODELS_PATH);
        let v1_miss = match self.fetch_models_json(&models_path).await {
            Ok(json) => match parse_n_ctx_from_models(&json, model_name) {
                Some(n_ctx) => {
                    tracing::info!(
                        model = %model_name,
                        context_window = n_ctx,
                        "context window from {} (meta.n_ctx, max_model_len or context_window)",
                        models_path
                    );
                    return Ok(n_ctx);
                }
                None => format!(
                    "{models_path}: no meta.n_ctx, max_model_len or context_window for {model_name} ({})",
                    describe_listing(&json)
                ),
            },
            Err(e) => format!("{models_path}: {e}"),
        };

        let native_miss = match self.fetch_models_json(LM_STUDIO_NATIVE_MODELS_PATH).await {
            Ok(json) => match parse_lm_studio_loaded_context_length(&json, model_name) {
                Ok(loaded) => {
                    tracing::info!(
                        model = %model_name,
                        context_window = loaded,
                        "context window from {} loaded_context_length",
                        LM_STUDIO_NATIVE_MODELS_PATH
                    );
                    return Ok(loaded);
                }
                Err(miss) => format!("{LM_STUDIO_NATIVE_MODELS_PATH}: {miss}"),
            },
            Err(e) => format!("{LM_STUDIO_NATIVE_MODELS_PATH}: {e}"),
        };

        Err(format!("{v1_miss}; {native_miss}"))
    }

    async fn fetch_models_json(&self, path: &str) -> Result<serde_json::Value, ProviderError> {
        let response = self.api_client.request(path).response_get().await?;
        handle_response_openai_compat(response).await
    }
}

/// The request field the LeanZero Rapid-MLX fork reads to snapshot a hybrid cache before the
/// request's volatile tail (`ChatCompletionRequest.rapid_mlx_transient_tail`).
const RAPID_MLX_TRANSIENT_TAIL: &str = "rapid_mlx_transient_tail";

/// Whether `/v1/models` lists `extension` in the `request_extensions` of `model_name`'s entry (or of
/// the sole entry, the single-model server serving under another id).
fn declares_request_extension(json: &serde_json::Value, model_name: &str, extension: &str) -> bool {
    served_entry(json, model_name)
        .and_then(|e| e.get("request_extensions"))
        .and_then(|v| v.as_array())
        .is_some_and(|list| list.iter().any(|v| v.as_str() == Some(extension)))
}

/// `model_name`'s entry in a `/v1/models` body, or the sole entry of a single-model server serving
/// under another id.
fn served_entry<'a>(
    json: &'a serde_json::Value,
    model_name: &str,
) -> Option<&'a serde_json::Value> {
    let data = json.get("data")?.as_array()?;
    data.iter()
        .find(|e| e.get("id").and_then(|v| v.as_str()) == Some(model_name))
        .or(match data.as_slice() {
            [only] => Some(only),
            _ => None,
        })
}

/// What a goose MLX engine's `/v1/models` entry declares about image input (Q-260): `capabilities`
/// listing `vision` reads images, `capabilities` without it reads text only, and no `capabilities`
/// (or no entry for the model) says nothing either way.
/// A host on this machine: `localhost`, 127.0.0.0/8 or `::1`.
fn host_is_loopback(host: &str) -> bool {
    match url::Url::parse(host).ok().as_ref().and_then(|u| u.host()) {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// An endpoint's refusal of a non-text content part. mlx_lm's server (the pipeline and tensor
/// splits) answers 404 "Only 'text' content type is supported."; other text-only servers word it
/// as "... does not support image ..." or "image input is not supported".
fn refuses_non_text_content(error: &ProviderError) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    text.contains("only 'text' content type")
        || text.contains("only text content")
        || (text.contains("image")
            && (text.contains("not supported") || text.contains("does not support")))
}

/// Rapid-MLX's MLLM lane refusing a request whose media prefill exceeds its per-batch cap
/// (`mllm_batch_generator._prefill_cap_violation`, HTTP 400 "... exceeds the per-batch cap ...").
/// An image left in the history (a tool's screenshot) is what still trips it after the pre-pass;
/// the turn is sent again with every image as its named placeholder.
fn refuses_media_prefill(error: &ProviderError) -> bool {
    error
        .to_string()
        .to_ascii_lowercase()
        .contains("exceeds the per-batch cap")
}

const MEDIA_PREPASS_CACHE_ENTRIES: usize = 64;
/// The longest audio piece one side request carries (Gemma 4 hears the first 30 s of a clip).
const MEDIA_PREPASS_AUDIO_PIECE_SECS: u32 = 30;

/// `m:ss` for a span label.
fn clock(secs: f64) -> String {
    let whole = secs.round() as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}
/// The side request's answer budget: a 2-minute transcript or a detailed description fits.
const MEDIA_PREPASS_MAX_TOKENS: u64 = 1024;
const MEDIA_PREPASS_SYSTEM: &str =
    "You turn one attached file into plain text for another assistant; answer with that text only.";

/// Transcripts and descriptions already made in this process, by host, model and content digest,
/// so a clip in the history is not sent to the engine again on every turn.
static MEDIA_PREPASS_CACHE: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Which attached parts the pre-pass sends alone, from the served entry's `/v1/models`
/// capabilities: audio and video when declared, images when the engine hears audio (audio is
/// declared only on the MLLM lane, where the media cap applies to images too).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct PrepassKinds {
    audio: bool,
    video: bool,
    images: bool,
}

fn prepass_kinds(json: &serde_json::Value, model_name: &str) -> PrepassKinds {
    let Some(caps) = served_entry(json, model_name)
        .and_then(|e| e.get("capabilities"))
        .and_then(|v| v.as_array())
    else {
        return PrepassKinds::default();
    };
    let has = |tag: &str| caps.iter().any(|c| c.as_str() == Some(tag));
    PrepassKinds {
        audio: has("audio"),
        video: has("video"),
        images: has("audio") && has("vision"),
    }
}

enum Attachment<'a> {
    Media(&'a MediaContent),
    Image(&'a rmcp::model::ImageContent, usize),
}

impl Attachment<'_> {
    fn kind(&self) -> &'static str {
        match self {
            Attachment::Media(m) => match m.kind {
                MediaKind::Audio => "audio",
                MediaKind::Video => "video",
            },
            Attachment::Image(..) => "image",
        }
    }

    fn digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let (mime, data) = match self {
            Attachment::Media(m) => (m.mime_type.as_str(), m.data.as_str()),
            Attachment::Image(i, _) => (i.mime_type.as_str(), i.data.as_str()),
        };
        let digest = Sha256::digest(data.as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        format!("{}|{mime}|{hex}", self.kind())
    }

    fn part(&self) -> serde_json::Value {
        match self {
            Attachment::Media(m) => convert_media(m),
            Attachment::Image(i, _) => convert_image(i, &ImageFormat::OpenAi),
        }
    }

    /// The parts the side requests carry, each with its span in the clip: a PCM WAV longer than
    /// [`MEDIA_PREPASS_AUDIO_PIECE_SECS`] goes piece by piece, everything else whole.
    fn pieces(&self) -> Vec<(Option<(f64, f64)>, serde_json::Value)> {
        if let Attachment::Media(m) = self {
            if m.kind == MediaKind::Audio {
                if let Some(pieces) = split_pcm_wav(&m.data, MEDIA_PREPASS_AUDIO_PIECE_SECS) {
                    return pieces
                        .into_iter()
                        .map(|p| {
                            let part = convert_media(&MediaContent {
                                data: p.data,
                                mime_type: "audio/wav".to_string(),
                                ..m.clone()
                            });
                            (Some((p.start_secs, p.end_secs)), part)
                        })
                        .collect();
                }
            }
        }
        vec![(None, self.part())]
    }

    fn instruction(&self) -> &'static str {
        match self {
            Attachment::Media(m) if m.kind == MediaKind::Audio => {
                "Transcribe the speech in this audio word for word, in its language. If there \
                 is no speech, describe the sound in one line."
            }
            Attachment::Media(_) => {
                "Describe this video in detail, in time order: what is shown, what moves or \
                 changes, and any text that appears on screen."
            }
            Attachment::Image(..) => {
                "Describe this image in detail: what it shows, its layout, and every piece of \
                 text in it, verbatim."
            }
        }
    }

    fn name(&self) -> String {
        match self {
            Attachment::Media(m) => match &m.name {
                Some(name) => format!("{name} ({})", m.mime_type),
                None => format!("({})", m.mime_type),
            },
            Attachment::Image(i, n) => format!("#{n} ({})", i.mime_type),
        }
    }

    /// The text part the agent turn carries, labelled as derived from the attachment.
    fn label(&self, model_name: &str, text: Result<&str, &str>) -> String {
        let model = model_name.rsplit('/').next().unwrap_or(model_name);
        let kind = self.kind();
        let made = match self {
            Attachment::Media(m) if m.kind == MediaKind::Audio => "transcript",
            _ => "description",
        };
        match text {
            Ok(text) => format!(
                "[Attached {kind} {} - {made} made from the attachment by {model}, not the \
                 file itself:]\n{text}\n[End of the {made} of the attached {kind}]",
                self.name()
            ),
            Err(reason) => format!(
                "[Attached {kind} {} - {model} could not turn it into text: {reason}]",
                self.name()
            ),
        }
    }
}

/// The side request for one attachment: a one-line system instruction, the person's own words
/// for context (bounded), the attachment, no tools, no history.
fn prepass_payload(
    model_name: &str,
    ask: &str,
    instruction: &str,
    part: serde_json::Value,
) -> serde_json::Value {
    let ask: String = ask.trim().chars().take(600).collect();
    let mut text = instruction.to_string();
    if !ask.is_empty() {
        text.push_str(&format!(
            " The person attached it with this message (for context only, do not answer it): \"{ask}\""
        ));
    }
    serde_json::json!({
        "model": model_name,
        "messages": [
            {"role": "system", "content": MEDIA_PREPASS_SYSTEM},
            {"role": "user", "content": [{"type": "text", "text": text}, part]},
        ],
        "max_tokens": MEDIA_PREPASS_MAX_TOKENS,
        "stream": false,
        "chat_template_kwargs": {"enable_thinking": false},
    })
}

/// Whether a chat-completions body carries an image part anywhere in its messages.
fn payload_carries_images(payload: &serde_json::Value) -> bool {
    payload["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|m| {
            m["content"].as_array().is_some_and(|parts| {
                parts
                    .iter()
                    .any(|p| p["type"] == "image_url" || p.get("image_url").is_some())
            })
        })
    })
}

fn declared_image_input(json: &serde_json::Value, model_name: &str) -> ImageInput {
    match served_entry(json, model_name)
        .and_then(|e| e.get("capabilities"))
        .and_then(|v| v.as_array())
    {
        Some(caps) if caps.iter().any(|c| c.as_str() == Some("vision")) => ImageInput::Reads,
        Some(_) => ImageInput::TextOnly,
        None => ImageInput::Undeclared,
    }
}

/// `(withhold_audio, withhold_video)` for `model_name` from a `/v1/models` body: a part is withheld
/// only when the served entry lists capabilities and that list lacks `audio` / `video`.
fn withheld_media(json: &serde_json::Value, model_name: &str) -> (bool, bool) {
    match served_entry(json, model_name)
        .and_then(|e| e.get("capabilities"))
        .and_then(|v| v.as_array())
    {
        Some(caps) => {
            let has = |tag: &str| caps.iter().any(|c| c.as_str() == Some(tag));
            (!has("audio"), !has("video"))
        }
        None => (false, false),
    }
}

fn describe_listing(json: &serde_json::Value) -> String {
    match json.get("data").and_then(|v| v.as_array()) {
        Some(data) => format!("among {} listed ids", data.len()),
        None => "no data array".to_string(),
    }
}

/// Extract the loaded context window for `model_name` from a `/v1/models` response body, from the
/// first field the server carries: llama.cpp/Ollama's `meta.n_ctx`; vLLM's and the LeanZero
/// Rapid-MLX engine's `max_model_len` (the window the engine SERVES — on a 64 GB Mac the RC13 line
/// lists 76,800 for Gemma 4 31B while the model's own window is 262,144); then the Rapid-MLX
/// `context_window` (the model's window). I-12 (2026-10-18): the Studio's app logged
/// `context_window_probe_missed` against an engine that listed both, so compaction ran on the
/// default limit.
fn parse_n_ctx_from_models(json: &serde_json::Value, model_name: &str) -> Option<usize> {
    let data = json.get("data")?.as_array()?;

    let n_ctx = |entry: &serde_json::Value| -> Option<usize> {
        let positive = |v: Option<&serde_json::Value>| {
            v.and_then(serde_json::Value::as_u64)
                .filter(|n| *n > 0)
                .map(|n| n as usize)
        };
        positive(entry.get("meta").and_then(|m| m.get("n_ctx")))
            .or_else(|| positive(entry.get("max_model_len")))
            .or_else(|| positive(entry.get("context_window")))
    };

    if let Some(entry) = data
        .iter()
        .find(|e| e.get("id").and_then(|v| v.as_str()) == Some(model_name))
    {
        return n_ctx(entry);
    }

    // For single-model servers without --alias, llama.cpp reports the loaded model
    // file path as id rather than the client's alias, so no entry matches above.
    // Fall back to the sole entry's n_ctx.
    match data.as_slice() {
        [only] => n_ctx(only),
        _ => None,
    }
}

/// Why LM Studio's `/api/v0/models` yielded no loaded window for the requested model. Rendered
/// into the probe's miss reason; never swallowed.
#[derive(Debug, PartialEq, Eq)]
enum LmStudioWindowMiss {
    NoDataArray,
    ModelNotListed {
        listed: usize,
    },
    /// The entry exists but carries no `loaded_context_length`: LM Studio omits the field while
    /// `state` is `not-loaded` and reports only the model's `max_context_length`.
    NotLoaded {
        state: String,
        max_context_length: Option<u64>,
    },
}

impl std::fmt::Display for LmStudioWindowMiss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDataArray => write!(f, "response carries no data array"),
            Self::ModelNotListed { listed } => {
                write!(f, "model not among the {listed} listed ids")
            }
            Self::NotLoaded {
                state,
                max_context_length: Some(max),
            } => write!(
                f,
                "no loaded_context_length (state {state}, max_context_length {max})"
            ),
            Self::NotLoaded {
                state,
                max_context_length: None,
            } => write!(f, "no loaded_context_length (state {state})"),
        }
    }
}

/// Extract `loaded_context_length` for `model_name` from an LM Studio `/api/v0/models` body.
/// Matches on `id` only — LM Studio lists every downloaded model, loaded or not, so the
/// sole-entry rule of the llama.cpp arm does not apply here.
fn parse_lm_studio_loaded_context_length(
    json: &serde_json::Value,
    model_name: &str,
) -> Result<usize, LmStudioWindowMiss> {
    let data = json
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or(LmStudioWindowMiss::NoDataArray)?;
    let entry = data
        .iter()
        .find(|e| e.get("id").and_then(|v| v.as_str()) == Some(model_name))
        .ok_or(LmStudioWindowMiss::ModelNotListed { listed: data.len() })?;
    match entry.get("loaded_context_length").and_then(|v| v.as_u64()) {
        Some(loaded) => Ok(loaded as usize),
        None => Err(LmStudioWindowMiss::NotLoaded {
            state: entry
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("<absent>")
                .to_string(),
            max_context_length: entry.get("max_context_length").and_then(|v| v.as_u64()),
        }),
    }
}

impl ProviderDescriptor for OpenAiProvider {
    fn metadata() -> ProviderMetadata {
        let models = OPEN_AI_KNOWN_MODELS
            .iter()
            .map(|(name, limit)| ModelInfo::new(*name, *limit))
            .collect();
        ProviderMetadata::with_models(
            OPEN_AI_PROVIDER_NAME,
            "OpenAI",
            "GPT-4 and other OpenAI models, including OpenAI compatible ones",
            OPEN_AI_DEFAULT_MODEL,
            models,
            OPEN_AI_DOC_URL,
            vec![
                ConfigKey::new("OPENAI_API_KEY", false, true, None, true),
                ConfigKey::new("OPENAI_BASE_URL", false, false, None, false),
                ConfigKey::new(
                    "OPENAI_HOST",
                    true,
                    false,
                    Some("https://api.openai.com"),
                    false,
                ),
                ConfigKey::new(
                    "OPENAI_BASE_PATH",
                    true,
                    false,
                    Some("v1/chat/completions"),
                    false,
                ),
                ConfigKey::new("OPENAI_ORGANIZATION", false, false, None, false),
                ConfigKey::new("OPENAI_PROJECT", false, false, None, false),
                ConfigKey::new("OPENAI_CUSTOM_HEADERS", false, true, None, false),
                ConfigKey::new("OPENAI_TIMEOUT", false, false, Some("600"), false),
            ],
        )
        .with_setup_steps(vec![
            "Go to https://platform.openai.com and sign up or log in",
            "Navigate to API Keys in the left sidebar",
            "Click 'Create new secret key'",
            "Copy the key and paste it above",
        ])
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn endpoint(&self) -> Option<&str> {
        Some(self.api_client.host())
    }

    fn skip_canonical_filtering(&self) -> bool {
        self.skip_canonical_filtering
    }

    /// Resolve the effective context limit. When the config carries an explicit
    /// limit (GOOSE_CONTEXT_LIMIT, a session override, or a known/canonical
    /// value) it is used as-is. Otherwise probe the server for the window it has
    /// loaded (`probe_context_window`: llama.cpp/Ollama `meta.n_ctx`, then LM Studio
    /// `loaded_context_length`) — the number the compaction guard and the per-turn
    /// context line run on. A miss falls to DEFAULT_CONTEXT_LIMIT LOUDLY: one warn per
    /// model naming the host, both endpoints and their reasons, never quietly. The probe
    /// is bounded by a short timeout so a hung endpoint can't stall the caller.
    async fn get_context_limit(&self, model_config: &ModelConfig) -> Result<usize, ProviderError> {
        if let Some(limit) = model_config.context_limit {
            return Ok(self.apply_local_context_cap(limit));
        }

        if let Some(cached) = self
            .n_ctx_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&model_config.model_name).copied())
        {
            return Ok(self
                .apply_local_context_cap(cached.unwrap_or_else(|| model_config.context_limit())));
        }

        const N_CTX_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
        let probed = match tokio::time::timeout(
            N_CTX_PROBE_TIMEOUT,
            self.probe_context_window(&model_config.model_name),
        )
        .await
        {
            Ok(Ok(window)) => Some(window),
            Ok(Err(reason)) => {
                tracing::warn!(
                    host = %sanitize_url(self.api_client.host()),
                    model = %model_config.model_name,
                    default_context_limit = model_config.context_limit(),
                    reason = %reason,
                    "context_window_probe_missed: no meta.n_ctx, max_model_len, context_window or loaded_context_length for this \
                     model; the compaction guard and the per-turn context line run on the default"
                );
                None
            }
            Err(_) => {
                tracing::warn!(
                    host = %sanitize_url(self.api_client.host()),
                    model = %model_config.model_name,
                    default_context_limit = model_config.context_limit(),
                    "context_window_probe_timed_out after {:?}; running on the default",
                    N_CTX_PROBE_TIMEOUT
                );
                None
            }
        };

        if let Ok(mut cache) = self.n_ctx_cache.lock() {
            cache.insert(model_config.model_name.clone(), probed);
        }

        Ok(self.apply_local_context_cap(probed.unwrap_or_else(|| model_config.context_limit())))
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        if let Some(custom_models) = &self.custom_models {
            if self.dynamic_models == Some(false) {
                return Ok(custom_models.clone());
            }
            match self.fetch_models_from_api().await {
                Ok(models) => return Ok(models),
                Err(e) if e.is_endpoint_not_found() => {
                    tracing::debug!(
                        "Models endpoint not implemented for provider '{}' ({}), using predefined list",
                        self.name,
                        e
                    );
                    return Ok(custom_models.clone());
                }
                Err(e) => return Err(e),
            }
        }

        self.fetch_models_from_api().await
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        if self.should_use_responses_api_for_provider(&model_config.model_name) {
            let mut payload = create_responses_request(model_config, system, messages, tools)?;
            payload["stream"] = serde_json::Value::Bool(self.supports_streaming);

            let mut log = start_log(model_config, &payload)?;

            let response = self
                .with_retry(|| async {
                    let payload_clone = payload.clone();
                    let resp = self
                        .api_client
                        .response_post(
                            &Self::map_base_path(
                                &self.base_path,
                                "responses",
                                OPEN_AI_DEFAULT_RESPONSES_PATH,
                            ),
                            &payload_clone,
                        )
                        .await?;
                    handle_status(resp).await
                })
                .await
                .inspect_err(|e| {
                    let _ = log.error(e);
                })?;

            if self.supports_streaming {
                stream_responses_compat(response, log)
            } else {
                let json: serde_json::Value = response.json().await.map_err(|e| {
                    ProviderError::RequestFailed(format!("Failed to parse JSON: {}", e))
                })?;

                let responses_api_response: ResponsesApiResponse =
                    serde_json::from_value(json.clone()).map_err(|e| {
                        ProviderError::ExecutionError(format!(
                            "Failed to parse responses API response: {}",
                            e
                        ))
                    })?;

                let message = responses_api_to_message(&responses_api_response)?;
                let usage_data = get_responses_usage(&responses_api_response);
                let usage = ProviderUsage::new(model_config.model_name.clone(), usage_data);

                log.write(
                    &serde_json::to_value(&message).unwrap_or_default(),
                    Some(&usage_data),
                )?;

                Ok(super::base::stream_from_single_message(message, usage))
            }
        } else {
            let transient_tail = self.transient_tail(&model_config.model_name).await;
            // Probed only when an effort is configured, so a request without one sends exactly
            // what it sent before (no extra /v1/models GET either).
            let lz_engine = model_config.thinking_effort().is_some()
                && self.openai_at_lz_engine(&model_config.model_name).await;
            let format_options = OpenAiFormatOptions {
                preserve_thinking_context: self.preserve_thinking_context,
                // Q-94: a chat's block rides the tool results it follows, never a user
                // turn of its own. It is kept in the history, so no engine needs it on a
                // user message to snapshot its cache before it.
                turn_context_joins_tool_results: true,
                text_only_engine: false,
                ..Default::default()
            };
            let prepared = self.media_prepass(model_config, messages).await;
            let messages: &[Message] = prepared.as_deref().unwrap_or(messages);
            let (payload, mut images_notice) = self
                .chat_request(model_config, system, messages, tools, format_options)
                .await?;
            let payload = self.finish_chat_payload(
                model_config,
                messages,
                payload,
                transient_tail,
                lz_engine,
            )?;
            let mut log = start_log(model_config, &payload)?;

            // This provider is the one the swarm engine actually calls (lmstudio's declarative
            // engine resolves here) — the adversarial review caught the first telemetry pass
            // instrumenting only OpenAiCompatibleProvider, which the fleet never routes through.
            let mut telemetry_t0 = std::time::Instant::now();
            let response = match self.post_chat(&payload).await {
                Ok(response) => response,
                // An endpoint that refuses image parts (an engine that declares nothing, a remote
                // text-only server) is sent the same turn once more with every image as its named
                // placeholder, so the agent reads that the image was not sent and carries on
                // instead of the session ending on the refusal (2026-10-27, MiMo Gauntlet).
                Err(e)
                    if (refuses_non_text_content(&e) || refuses_media_prefill(&e))
                        && payload_carries_images(&payload) =>
                {
                    let _ = log.error(&e);
                    tracing::warn!(
                        host = %sanitize_url(self.api_client.host()),
                        model = %model_config.model_name,
                        error = %e,
                        "images_refused: the endpoint reads text only; the turn is sent again with \
                         every image part as a named placeholder"
                    );
                    let (retry, notice) = self.text_only_request(
                        model_config,
                        system,
                        messages,
                        tools,
                        format_options,
                    )?;
                    images_notice = notice;
                    let retry = self.finish_chat_payload(
                        model_config,
                        messages,
                        retry,
                        transient_tail,
                        lz_engine,
                    )?;
                    log = start_log(model_config, &retry)?;
                    telemetry_t0 = std::time::Instant::now();
                    self.post_chat(&retry).await.inspect_err(|e| {
                        let _ = log.error(e);
                    })?
                }
                Err(e) => {
                    let _ = log.error(&e);
                    return Err(e);
                }
            };

            let stream = if self.supports_streaming {
                super::openai_compatible::stream_openai_compat_timed(
                    response,
                    log,
                    telemetry_t0,
                    model_config.model_name.clone(),
                )
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
                    Some(&usage_data),
                )?;

                let elapsed = telemetry_t0.elapsed();
                super::openai_compatible::telemetry_record(
                    &model_config.model_name,
                    elapsed,
                    elapsed,
                    Some(usage.usage),
                    0,
                    message.id.as_deref(),
                );
                Ok(super::base::stream_from_single_message(message, usage))
            }?;
            Ok(announced(stream, images_notice))
        }
    }
}

pub fn from_declarative_config(
    config: DeclarativeProviderConfig,
    tls_config: Option<TlsConfig>,
    key_resolver: impl KeyResolver,
) -> Result<OpenAiProviderBuilder> {
    let custom_models = if !config.models.is_empty() {
        Some(
            config
                .models
                .iter()
                .map(|m| m.name.clone())
                .collect::<Vec<String>>(),
        )
    } else {
        None
    };

    if config.dynamic_models == Some(false) && custom_models.is_none() {
        return Err(anyhow::anyhow!(
            "Provider '{}' has dynamic_models: false but no static models listed; \
             at least one entry in `models` is required.",
            config.name
        ));
    }

    let api_key = if config.api_key_env.is_empty() {
        None
    } else {
        match key_resolver.resolve_key(config.api_key_env.as_str()) {
            Ok(key) => Some(key),
            Err(err) => {
                if config.requires_auth {
                    anyhow::bail!("missing required key {}: {}", config.api_key_env, err);
                }
                None
            }
        }
    };

    let normalized_base_url = ensure_url_scheme(&config.base_url);
    let url = url::Url::parse(&normalized_base_url).map_err(|e| {
        anyhow::anyhow!(
            "Invalid base URL '{}': {}",
            sanitize_url(&config.base_url),
            e
        )
    })?;

    let host = url[..url::Position::BeforePath].to_string();
    let base_path = if let Some(ref explicit_path) = config.base_path {
        explicit_path.trim_start_matches('/').to_string()
    } else {
        derive_base_path(url.path())
    };

    let timeout_secs = config.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);

    let auth = match api_key {
        Some(key) if !key.is_empty() => AuthMethod::BearerToken(key),
        _ => AuthMethod::NoAuth,
    };
    let mut api_client = ApiClient::with_timeout_and_tls(
        host,
        auth,
        std::time::Duration::from_secs(timeout_secs),
        tls_config,
    )?;

    if let Some(query) = url.query() {
        let query_params = url::form_urlencoded::parse(query.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        api_client = api_client.with_query(query_params);
    }

    if let Some(headers) = &config.headers {
        let mut header_map = reqwest::header::HeaderMap::new();
        for (key, value) in headers {
            let header_name = reqwest::header::HeaderName::from_bytes(key.as_bytes())?;
            let header_value = reqwest::header::HeaderValue::from_str(value)?;
            header_map.insert(header_name, header_value);
        }
        api_client = api_client.with_headers(header_map)?;
    }

    Ok(OpenAiProviderBuilder::new(api_client)
        .base_path(base_path)
        .custom_headers(config.headers)
        .supports_streaming(config.supports_streaming.unwrap_or(true))
        .name(config.name.clone())
        .custom_models(custom_models)
        .dynamic_models(config.dynamic_models)
        .skip_canonical_filtering(config.skip_canonical_filtering)
        .preserve_thinking_context(config.preserves_thinking))
}

pub fn parse_custom_headers(s: String) -> HashMap<String, String> {
    s.split(',')
        .filter_map(|header| {
            let mut parts = header.splitn(2, '=');
            let key = parts.next().map(|s| s.trim().to_string())?;
            let value = parts.next().map(|s| s.trim().to_string())?;
            Some((key, value))
        })
        .collect()
}

pub fn derive_base_path(url_path: &str) -> String {
    let stripped = url_path.trim_start_matches('/');
    let normalized = stripped.trim_end_matches('/');
    if normalized.is_empty() {
        "v1/chat/completions".to_string()
    } else if normalized.ends_with("chat/completions") {
        stripped.to_string()
    } else if ends_with_version_segment(normalized) {
        format!("{}/chat/completions", normalized)
    } else {
        format!("{}/v1/chat/completions", normalized)
    }
}

fn ends_with_version_segment(path: &str) -> bool {
    let last = path.rsplit('/').next().unwrap_or(path);
    last.strip_prefix('v')
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::AuthMethod;
    use serde_json::json;

    pub(super) fn make_provider(name: &str) -> OpenAiProvider {
        OpenAiProvider {
            api_client: ApiClient::new_with_tls(
                "http://localhost".to_string(),
                AuthMethod::NoAuth,
                None,
            )
            .unwrap(),
            base_path: "v1/chat/completions".to_string(),
            organization: None,
            project: None,
            custom_headers: None,
            supports_streaming: true,
            name: name.to_string(),
            custom_models: None,
            dynamic_models: None,
            skip_canonical_filtering: false,
            preserve_thinking_context: false,
            n_ctx_cache: Arc::new(Mutex::new(HashMap::new())),
            transient_tail_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    const TEST_TURN_CONTEXT: &str =
        "<turn-context>\n<current-time>2026-09-23 14:07:00</current-time>\n\
                  <working-directory>/w</working-directory>\n</turn-context>";

    async fn post_bodies_through(
        name: &str,
        request_extensions: serde_json::Value,
    ) -> (Vec<serde_json::Value>, usize, String) {
        // A swarm worker's request: its block placed ahead of the task text, for this request
        // alone — the one shape whose block is transient.
        let messages = vec![Message::user()
            .with_text(TEST_TURN_CONTEXT)
            .with_text("find the handler")];
        let (bodies, probes) = post_bodies_for(name, request_extensions, &messages).await;
        (bodies, probes, format!("\n{TEST_TURN_CONTEXT}"))
    }

    async fn post_bodies_for(
        name: &str,
        request_extensions: serde_json::Value,
        messages: &[Message],
    ) -> (Vec<serde_json::Value>, usize) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "data": [{"id": "served", "request_extensions": request_extensions}],
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"},
                             "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            })))
            .mount(&server)
            .await;

        let mut provider = make_provider(name);
        provider.api_client =
            ApiClient::new_with_tls(server.uri(), AuthMethod::NoAuth, None).unwrap();
        provider.supports_streaming = false;
        for _ in 0..2 {
            let mut stream = provider
                .stream(&ModelConfig::new("served"), "system", messages, &[])
                .await
                .unwrap();
            use futures::StreamExt;
            while stream.next().await.is_some() {}
        }
        let requests = server.received_requests().await.unwrap();
        let probes = requests
            .iter()
            .filter(|r| r.method.as_str() == "GET")
            .count();
        let bodies = requests
            .iter()
            .filter(|r| r.method.as_str() == "POST")
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect();
        (bodies, probes)
    }

    /// What the engine answers and what goose sent it, per call: the POST bodies, how many
    /// `/v1/models` probes it made, and the messages each call streamed back to the chat.
    async fn image_calls_through(
        models_entry: serde_json::Value,
        conversations: &[Vec<Message>],
    ) -> (Vec<serde_json::Value>, usize, Vec<Vec<Message>>) {
        image_calls_via("omlx", models_entry, conversations, false).await
    }

    /// `provider_name` at a loopback engine whose `/v1/models` lists `models_entry`. With
    /// `refuse_images`, the engine answers a body carrying an image part as mlx_lm's server does
    /// (404 "Only 'text' content type is supported.") and every other body 200.
    async fn image_calls_via(
        provider_name: &str,
        models_entry: serde_json::Value,
        conversations: &[Vec<Message>],
        refuse_images: bool,
    ) -> (Vec<serde_json::Value>, usize, Vec<Vec<Message>>) {
        use futures::StreamExt;
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        if refuse_images {
            Mock::given(method("POST"))
                .and(path("/v1/chat/completions"))
                .and(body_string_contains("image_url"))
                .respond_with(
                    ResponseTemplate::new(404)
                        .set_body_json(json!({"error": "Only 'text' content type is supported."})),
                )
                .with_priority(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"object": "list", "data": [models_entry]})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"},
                             "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            })))
            .mount(&server)
            .await;

        let mut provider = make_provider(provider_name);
        provider.api_client =
            ApiClient::new_with_tls(server.uri(), AuthMethod::NoAuth, None).unwrap();
        provider.supports_streaming = false;
        let mut streamed = Vec::new();
        for messages in conversations {
            let stream = provider
                .stream(&ModelConfig::new("Org/Served-27B"), "system", messages, &[])
                .await
                .unwrap();
            let items: Vec<_> = stream.collect().await;
            streamed.push(
                items
                    .into_iter()
                    .filter_map(|item| item.unwrap().0)
                    .collect(),
            );
        }
        let requests = server.received_requests().await.unwrap();
        let probes = requests
            .iter()
            .filter(|r| r.method.as_str() == "GET")
            .count();
        let bodies = requests
            .iter()
            .filter(|r| r.method.as_str() == "POST")
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect();
        (bodies, probes, streamed)
    }

    /// 2026-10-27, local MiMo-V2.6-Flash Gauntlet sb-7.2 on the pipeline split (:8191): the
    /// benchmark's "Single model" entrant is the built-in `openai` provider at a loopback host,
    /// which Q-260 never asked, so read_image's PNG went as an image part, the engine answered 404
    /// "Only 'text' content type is supported" and the run ended did_not_finish. The same provider
    /// at a loopback engine that declares no vision now sends the placeholder instead.
    #[tokio::test]
    async fn the_benchmark_openai_entrant_at_a_text_only_local_engine_never_sends_an_image() {
        let (_dir, png) = q260_png();
        let (bodies, probes, streamed) = image_calls_via(
            "openai",
            json!({"id": "Org/Served-27B", "capabilities": ["text", "tools"]}),
            &q260_conversations(&png),
            true,
        )
        .await;
        assert_eq!(bodies.len(), 3, "one request per turn, none refused");
        for body in &bodies {
            assert_eq!(image_parts(body), 0, "{body}");
        }
        assert_eq!(probes, 3, "asked once per request that carries an image");
        let tool = bodies[2]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == json!("tool"))
            .unwrap()
            .clone();
        assert_eq!(
            tool["content"],
            json!(
                "Loaded image (image/png, 4112x2578). \
                 [image from read_image (image/png) not sent: this model reads text only]"
            ),
        );
        assert_eq!(
            notices(&streamed[2]),
            ["Served-27B reads text only — the image from read_image (image/png) was not sent"],
        );
    }

    /// An engine that declares nothing and refuses the image part is sent the turn once more with
    /// the placeholder, so the agent carries on; the chat is told the image was not sent.
    #[tokio::test]
    async fn an_endpoint_that_refuses_an_image_part_gets_the_turn_again_without_it() {
        let (_dir, png) = q260_png();
        let conversations = q260_conversations(&png);
        let (bodies, _, streamed) = image_calls_via(
            "openai",
            json!({"id": "Org/Served-27B"}),
            &conversations[2..],
            true,
        )
        .await;
        assert_eq!(
            bodies.len(),
            2,
            "the refused body, then the text-only retry"
        );
        assert_eq!(image_parts(&bodies[0]), 1);
        assert_eq!(image_parts(&bodies[1]), 0);
        assert_eq!(
            notices(&streamed[0]),
            ["Served-27B reads text only — the image from read_image (image/png) was not sent"],
        );
        assert!(streamed[0].iter().any(|m| m.as_concat_text() == "ok"));
    }

    /// Negative controls: the `openai` provider at a loopback engine that declares vision is sent
    /// every image as before with no notice; a remote host is never asked; an unrelated 4xx is
    /// still an error and is not retried; a body with no image is never re-sent.
    #[tokio::test]
    async fn a_vision_engine_behind_openai_still_gets_its_images_and_other_errors_stay_errors() {
        let (_dir, png) = q260_png();
        let (bodies, _, streamed) = image_calls_via(
            "openai",
            json!({"id": "Org/Served-27B", "capabilities": ["text", "vision", "tools"]}),
            &q260_conversations(&png),
            false,
        )
        .await;
        for body in &bodies {
            assert_eq!(image_parts(body), 1, "{body}");
        }
        assert!(streamed.iter().all(|call| notices(call).is_empty()));

        assert!(host_is_loopback("http://127.0.0.1:8191"));
        assert!(host_is_loopback("http://localhost:8090"));
        assert!(host_is_loopback("http://[::1]:8191"));
        assert!(!host_is_loopback("https://api.openai.com"));
        assert!(!host_is_loopback("http://192.168.0.2:8191"));
        let mut remote = make_provider("openai");
        remote.api_client =
            ApiClient::new_with_tls("https://api.example.com".into(), AuthMethod::NoAuth, None)
                .unwrap();
        assert!(!remote.asks_engine_for_image_input());
        assert!(make_provider("omlx").asks_engine_for_image_input());

        assert!(refuses_non_text_content(&ProviderError::RequestFailed(
            "Resource not found (404): Only 'text' content type is supported.".into()
        )));
        assert!(!refuses_non_text_content(&ProviderError::RequestFailed(
            "Resource not found (404): model 'x' not found".into()
        )));
        assert!(!payload_carries_images(&json!({"messages": [
            {"role": "user", "content": "hi"},
            {"role": "user", "content": [{"type": "text", "text": "hi"}]}
        ]})));
        assert!(payload_carries_images(&json!({"messages": [
            {"role": "user", "content": [{"type": "image_url", "image_url": {"url": "data:,"}}]}
        ]})));
    }

    fn notices(messages: &[Message]) -> Vec<String> {
        messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|c| match c {
                crate::conversation::message::MessageContent::SystemNotification(n) => {
                    Some(n.msg.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn image_parts(body: &serde_json::Value) -> usize {
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_array())
            .flatten()
            .filter(|part| part["type"] == json!("image_url"))
            .count()
    }

    /// Q-260's three conversations: a message naming a local .png (goose attaches the file), the
    /// next turn with that message in history, and read_image's result carrying an image.
    fn q260_conversations(png: &str) -> Vec<Vec<Message>> {
        use rmcp::model::{CallToolRequestParams, CallToolResult, Content};
        let asked = Message::user().with_text(format!("What heading is in {png} ?"));
        vec![
            vec![asked.clone()],
            vec![
                asked,
                Message::assistant().with_text("I could not read it."),
                Message::user().with_text("In one sentence: what is a permission scheme?"),
            ],
            vec![
                Message::user().with_text("Read the crop and tell me the heading."),
                Message::assistant()
                    .with_tool_request("call_img", Ok(CallToolRequestParams::new("read_image"))),
                Message::user().with_tool_response(
                    "call_img",
                    Ok(CallToolResult::success(vec![
                        Content::text("Loaded image (image/png, 4112x2578)."),
                        Content::image("iVBORw0KGgo=", "image/png"),
                    ])),
                ),
            ],
        ]
    }

    fn q260_png() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("q232-src.png");
        std::fs::write(&png, [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0]).unwrap();
        let png = png.to_string_lossy().into_owned();
        (dir, png)
    }

    /// Q-260, live on 3.0.65 (sessions 20260928_11/_12, llm_request.0.jsonl): the 27B tensor split
    /// answered 404 "Only 'text' content type is supported" to the attached .png and to
    /// read_image's image, and the image stayed in history so every later turn failed the same
    /// way. An engine that declares `capabilities` without `vision` is sent a named placeholder in
    /// every image's place, and the chat is told once — on the request that first withholds it.
    #[tokio::test]
    async fn a_text_only_engine_is_never_sent_an_image_and_the_chat_is_told_once() {
        let (_dir, png) = q260_png();
        let (bodies, _, streamed) = image_calls_through(
            json!({"id": "Org/Served-27B", "capabilities": ["text", "tools"]}),
            &q260_conversations(&png),
        )
        .await;

        for body in &bodies {
            assert_eq!(image_parts(body), 0, "{body}");
        }
        let asked = bodies[0]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        assert_eq!(
            asked["content"],
            json!(format!(
                "What heading is in {png} ?\n[image q232-src.png not sent: this model reads text only]"
            )),
        );
        assert_eq!(
            notices(&streamed[0]),
            ["Served-27B reads text only — the image q232-src.png was not sent"],
        );

        let history = bodies[1]["messages"].as_array().unwrap();
        assert!(history[1]["content"]
            .as_str()
            .unwrap()
            .ends_with("[image q232-src.png not sent: this model reads text only]"));
        assert!(
            notices(&streamed[1]).is_empty(),
            "a past image is withheld again, never announced again"
        );

        let tool = bodies[2]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == json!("tool"))
            .unwrap()
            .clone();
        assert_eq!(
            tool["content"],
            json!(
                "Loaded image (image/png, 4112x2578). \
                 [image from read_image (image/png) not sent: this model reads text only]"
            ),
        );
        assert_eq!(
            notices(&streamed[2]),
            ["Served-27B reads text only — the image from read_image (image/png) was not sent"],
        );
        assert!(streamed
            .iter()
            .all(|call| call.iter().any(|m| m.as_concat_text() == "ok")));
    }

    /// The negative control: an engine that declares `vision` gets every image exactly as before,
    /// and one that declares nothing is sent them unchanged (its own answer decides) — neither
    /// hears a notice. A request with no image never asks the engine anything new.
    #[tokio::test]
    async fn a_vision_engine_and_an_undeclared_one_are_sent_the_images_as_before() {
        let (_dir, png) = q260_png();
        let conversations = q260_conversations(&png);
        for entry in [
            json!({"id": "Org/Served-27B", "capabilities": ["text", "vision", "tools"]}),
            json!({"id": "Org/Served-27B"}),
        ] {
            let (bodies, _, streamed) = image_calls_through(entry.clone(), &conversations).await;
            assert_eq!(image_parts(&bodies[0]), 1, "{entry}");
            assert_eq!(image_parts(&bodies[1]), 1, "{entry}");
            assert_eq!(image_parts(&bodies[2]), 1, "{entry}");
            assert!(
                streamed.iter().all(|call| notices(call).is_empty()),
                "{entry}"
            );
        }

        let text_only = vec![vec![Message::user().with_text("no picture here")]; 2];
        let (bodies, probes, _) = image_calls_through(
            json!({"id": "Org/Served-27B", "capabilities": ["text"]}),
            &text_only,
        )
        .await;
        assert_eq!(bodies.len(), 2);
        assert_eq!(
            probes, 1,
            "the one cached transient-tail probe; a request without an image asks nothing more"
        );
    }

    /// Q-94, #1 turn 2 (sessions.db 764103/764105 → 764106): the request ends on two
    /// propose_knowledge results and the chat's block rides that tool-result message. Whatever the
    /// engine declares, the block is joined to the last `role: tool` message — no user turn holding
    /// only goose's context — and no transient tail names it: the chat keeps it, so the next
    /// request carries it too and a cache snapshot after it is the one that is read back.
    #[tokio::test]
    async fn a_chat_block_joins_the_tool_results_and_is_never_transient() {
        use rmcp::model::{CallToolRequestParams, CallToolResult, Content};
        let messages = vec![
            Message::user().with_text("How long can they stay on Data Center?"),
            Message::assistant().with_tool_request(
                "call_1",
                Ok(CallToolRequestParams::new("propose_knowledge")),
            ),
            Message::user()
                .with_tool_response(
                    "call_1",
                    Ok(CallToolResult::success(vec![Content::text(
                        "Proposed as knowledge in category \"atlassian-migration\"",
                    )])),
                )
                .with_text(TEST_TURN_CONTEXT),
        ];

        for declared in [
            json!([]),
            json!(["rapid_mlx_transient_tail"]),
            json!([
                "rapid_mlx_transient_tail",
                "rapid_mlx_transient_tail_on_tool"
            ]),
        ] {
            let (bodies, _) = post_bodies_for("omlx", declared.clone(), &messages).await;
            let sent = bodies[0]["messages"].as_array().unwrap();
            let last = sent.last().unwrap();
            assert_eq!(last["role"], json!("tool"), "{declared}: {sent:?}");
            assert!(last["content"]
                .as_str()
                .unwrap()
                .ends_with(&format!("\n{TEST_TURN_CONTEXT}")));
            assert_eq!(
                sent.iter().filter(|m| m["role"] == json!("user")).count(),
                1,
                "{declared}: no user turn besides the question: {sent:?}"
            );
            assert!(
                bodies[0].get(RAPID_MLX_TRANSIENT_TAIL).is_none(),
                "{declared}: a kept block is not transient"
            );
        }

        let (bodies, _) = post_bodies_for(
            "omlx",
            json!(["rapid_mlx_transient_tail"]),
            &[Message::user()
                .with_text("find the handler")
                .with_text(TEST_TURN_CONTEXT)],
        )
        .await;
        assert!(
            bodies[0].get(RAPID_MLX_TRANSIENT_TAIL).is_none(),
            "a chat's block on the person's words is kept too"
        );
    }

    /// Q-94 on a real engine: a chat turn's two tool steps, sent by this provider exactly as the
    /// agent loop sends them, against the engine at `OMLX_MEASURE_URL` (its `/v1/models` decides
    /// the shape). Step 2 keeps step 1's block, as the chat does. Prints what step 2 found cached
    /// of step 1's prefix.
    /// `OMLX_MEASURE_URL=http://127.0.0.1:<port> OMLX_MEASURE_MODEL=<served id> cargo test -p
    /// goose-providers --lib openai::tests::measure_the_tool_step_prefix -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn measure_the_tool_step_prefix() {
        use futures::StreamExt;
        use rmcp::model::{CallToolRequestParams, CallToolResult, Content};

        let url = std::env::var("OMLX_MEASURE_URL").expect("OMLX_MEASURE_URL");
        let model = std::env::var("OMLX_MEASURE_MODEL").expect("OMLX_MEASURE_MODEL");
        let mut provider = make_provider("omlx");
        provider.api_client = ApiClient::new_with_tls(url, AuthMethod::NoAuth, None).unwrap();
        provider.supports_streaming = false;
        let block = |time: &str| {
            format!(
                "<turn-context>\n<current-time>2026-09-26 {time}</current-time>\n\
                 <working-directory>/w</working-directory>\n</turn-context>"
            )
        };
        let listing: String = (0..400)
            .map(|i| format!("src/module_{i}.rs  {} lines\n", 40 + i))
            .collect();
        let call = |id: &str, command: &str| {
            Message::assistant().with_tool_request(
                id,
                Ok(CallToolRequestParams::new("shell").with_arguments(
                    serde_json::json!({ "command": command })
                        .as_object()
                        .unwrap()
                        .clone(),
                )),
            )
        };
        let result = |id: &str, text: &str| {
            Message::user().with_tool_response(
                id,
                Ok(CallToolResult::success(vec![Content::text(
                    text.to_string(),
                )])),
            )
        };
        let step1 = vec![
            Message::user()
                .with_text("Which module is the longest? Look, then answer in one line."),
            call("c1", "wc -l src/*.rs"),
            result("c1", &listing).with_text(block("10:00:00")),
        ];
        let step2 = vec![
            Message::user()
                .with_text("Which module is the longest? Look, then answer in one line."),
            call("c1", "wc -l src/*.rs"),
            result("c1", &listing).with_text(block("10:00:00")),
            call("c2", "head -3 src/module_399.rs"),
            result("c2", "// module 399\nfn main() {}\n").with_text(block("10:01:00")),
        ];
        let config = ModelConfig::new(&model).with_max_tokens(Some(8));
        let mut usages = Vec::new();
        for messages in [&step1, &step2] {
            let mut stream = provider
                .stream(&config, "You are a terse coding agent.", messages, &[])
                .await
                .unwrap();
            let mut last = None;
            while let Some(item) = stream.next().await {
                if let Ok((_, Some(usage))) = item {
                    last = Some(usage.usage);
                }
            }
            usages.push(last.expect("the engine reported usage"));
        }
        println!(
            "step 1 prompt {:?} (cached {:?}); step 2 prompt {:?}, cached {:?}",
            usages[0].input_tokens,
            usages[0].cache_read_input_tokens,
            usages[1].input_tokens,
            usages[1].cache_read_input_tokens
        );
    }

    /// The marker reaches ONLY an omlx endpoint that declared it, names exactly the appended
    /// block, and is probed once per model; a non-declaring engine and every other provider get a
    /// byte-identical request (negative controls), and a non-omlx provider is never probed.
    #[tokio::test]
    async fn the_transient_tail_rides_only_to_a_declaring_rapid_mlx_engine() {
        let (bodies, probes, tail) =
            post_bodies_through("omlx", json!(["rapid_mlx_transient_tail"])).await;
        assert_eq!(probes, 1, "one probe per model, then the cache answers");
        for body in &bodies {
            assert_eq!(body[RAPID_MLX_TRANSIENT_TAIL], json!(tail));
            let last = body["messages"].as_array().unwrap().last().unwrap();
            assert!(last["content"].as_str().unwrap().ends_with(&tail));
        }

        let (bodies, probes, _) = post_bodies_through("omlx", json!([])).await;
        assert_eq!(probes, 1);
        assert!(bodies
            .iter()
            .all(|b| b.get(RAPID_MLX_TRANSIENT_TAIL).is_none()));

        let (bodies, probes, _) =
            post_bodies_through("lmstudio", json!(["rapid_mlx_transient_tail"])).await;
        assert_eq!(
            probes, 0,
            "only the provider fronting the MLX engine is asked"
        );
        assert!(bodies
            .iter()
            .all(|b| b.get(RAPID_MLX_TRANSIENT_TAIL).is_none()));
    }

    async fn body_sent(name: &str, model_config: &ModelConfig) -> serde_json::Value {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"object": "list", "data": [{"id": "served"}]})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"},
                             "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            })))
            .mount(&server)
            .await;
        let mut provider = make_provider(name);
        provider.api_client =
            ApiClient::new_with_tls(server.uri(), AuthMethod::NoAuth, None).unwrap();
        provider.supports_streaming = false;
        let messages = vec![Message::user().with_text("Tool: shell\nArguments: {}")];
        let mut stream = provider
            .stream(model_config, "Summarize this tool call", &messages, &[])
            .await
            .unwrap();
        use futures::StreamExt;
        while stream.next().await.is_some() {}
        let requests = server.received_requests().await.unwrap();
        let post = requests
            .iter()
            .find(|r| r.method.as_str() == "POST")
            .expect("the chat request was posted");
        serde_json::from_slice(&post.body).unwrap()
    }

    /// `body_sent`, but the engine's `/v1/models` entry declares `request_extensions` (the fork
    /// identity) and the request carries one tool, like every agent turn of a benchmark.
    async fn body_sent_to_engine(
        name: &str,
        model_config: &ModelConfig,
        request_extensions: serde_json::Value,
    ) -> serde_json::Value {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "data": [{"id": "served", "request_extensions": request_extensions}],
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"},
                             "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            })))
            .mount(&server)
            .await;
        let mut provider = make_provider(name);
        provider.api_client =
            ApiClient::new_with_tls(server.uri(), AuthMethod::NoAuth, None).unwrap();
        provider.supports_streaming = false;
        let tool = Tool::new(
            "shell",
            "run a command",
            rmcp::object!({"type": "object", "properties": {"command": {"type": "string"}}}),
        );
        let messages = vec![Message::user().with_text("list the files")];
        let mut stream = provider
            .stream(model_config, "You are goose", &messages, &[tool])
            .await
            .unwrap();
        use futures::StreamExt;
        while stream.next().await.is_some() {}
        let requests = server.received_requests().await.unwrap();
        let post = requests
            .iter()
            .find(|r| r.method.as_str() == "POST")
            .expect("the chat request was posted");
        serde_json::from_slice(&post.body).unwrap()
    }

    /// T12 G1 (2026-10-29): the benchmark's effort pin reaches a loopback LeanZero engine on the
    /// built-in `openai` provider (the bench's Single-model entrant), so Rapid-MLX's tool-turn
    /// auto-disable no longer turns a local entrant's thinking off; Off still switches it off;
    /// no effort, a stock server, an explicit profile value and the omlx path are left as they were.
    #[tokio::test]
    async fn the_benchmark_effort_reaches_a_loopback_lz_engine_on_the_openai_provider() {
        let fork = json!(["rapid_mlx_transient_tail"]);
        let medium = ModelConfig::new("served").with_thinking_effort(ThinkingEffort::Medium);
        let body = body_sent_to_engine("openai", &medium, fork.clone()).await;
        assert_eq!(
            body["chat_template_kwargs"],
            json!({"enable_thinking": true})
        );
        assert_eq!(body["reasoning_effort"], json!("medium"));

        let max = ModelConfig::new("served").with_thinking_effort(ThinkingEffort::Max);
        let body = body_sent_to_engine("openai", &max, fork.clone()).await;
        assert_eq!(body["reasoning_effort"], json!("high"));

        let off = ModelConfig::new("served").with_thinking_effort(ThinkingEffort::Off);
        let body = body_sent_to_engine("openai", &off, fork.clone()).await;
        assert_eq!(
            body["chat_template_kwargs"],
            json!({"enable_thinking": false})
        );
        assert!(body.get("reasoning_effort").is_none());

        // negative controls
        let body = body_sent_to_engine("openai", &medium, json!([])).await;
        assert!(
            body.get("chat_template_kwargs").is_none(),
            "a stock server: {body}"
        );
        assert!(body.get("reasoning_effort").is_none());
        let body = body_sent_to_engine("omlx", &medium, fork.clone()).await;
        assert!(
            body.get("reasoning_effort").is_none(),
            "omlx keeps the swarm router's path"
        );
        let pinned = with_profile_kwargs(medium.clone());
        let body = body_sent_to_engine("openai", &pinned, fork.clone()).await;
        assert_eq!(
            body["chat_template_kwargs"],
            json!({"enable_thinking": true, "reasoning_effort": "low"}),
            "an explicit profile value wins"
        );
    }

    fn with_profile_kwargs(config: ModelConfig) -> ModelConfig {
        config.with_merged_request_params(HashMap::from([(
            "chat_template_kwargs".to_string(),
            json!({"enable_thinking": true, "reasoning_effort": "low"}),
        )]))
    }

    /// A helper call's ThinkingEffort::Off reaches the MLX engine as the template switch, over the
    /// profile's own; a request with no Off — the agent's own turn — and every other provider send
    /// exactly what they sent before (negative controls).
    #[tokio::test]
    async fn thinking_off_reaches_the_mlx_engine_as_the_template_switch() {
        let off = ModelConfig::new("served").with_thinking_effort(ThinkingEffort::Off);
        assert_eq!(
            body_sent("omlx", &off).await["chat_template_kwargs"],
            json!({"enable_thinking": false})
        );
        assert_eq!(
            body_sent("omlx", &with_profile_kwargs(off.clone())).await["chat_template_kwargs"],
            json!({"enable_thinking": false, "reasoning_effort": "low"})
        );

        let turn = ModelConfig::new("served");
        assert!(body_sent("omlx", &turn)
            .await
            .get("chat_template_kwargs")
            .is_none());
        assert_eq!(
            body_sent("omlx", &with_profile_kwargs(turn)).await["chat_template_kwargs"],
            json!({"enable_thinking": true, "reasoning_effort": "low"})
        );
        let high = ModelConfig::new("served").with_thinking_effort(ThinkingEffort::High);
        assert!(body_sent("omlx", &high)
            .await
            .get("chat_template_kwargs")
            .is_none());

        for other in ["lmstudio", "openai", "ollama"] {
            assert!(
                body_sent(other, &off)
                    .await
                    .get("chat_template_kwargs")
                    .is_none(),
                "{other} never receives the MLX template switch"
            );
        }
    }

    #[test]
    fn a_non_object_template_kwargs_refuses_the_thinking_off_loudly() {
        let provider = make_provider("omlx");
        let mut payload = json!({"model": "served", "messages": [], "chat_template_kwargs": "x"});
        let off = ModelConfig::new("served").with_thinking_effort(ThinkingEffort::Off);
        let err = provider
            .carry_thinking_off_to_mlx(&off, &mut payload)
            .unwrap_err();
        assert!(err.to_string().contains("not an object"), "{err}");
    }

    #[test]
    fn a_request_extension_is_read_from_the_served_entry_or_the_sole_one() {
        let listing = json!({"data": [
            {"id": "a", "request_extensions": ["rapid_mlx_transient_tail"]},
            {"id": "b", "request_extensions": []},
        ]});
        assert!(declares_request_extension(
            &listing,
            "a",
            RAPID_MLX_TRANSIENT_TAIL
        ));
        assert!(!declares_request_extension(
            &listing,
            "b",
            RAPID_MLX_TRANSIENT_TAIL
        ));
        assert!(!declares_request_extension(
            &listing,
            "c",
            RAPID_MLX_TRANSIENT_TAIL
        ));
        let sole =
            json!({"data": [{"id": "x", "request_extensions": ["rapid_mlx_transient_tail"]}]});
        assert!(declares_request_extension(
            &sole,
            "alias",
            RAPID_MLX_TRANSIENT_TAIL
        ));
        assert!(!declares_request_extension(
            &json!({"data": [{"id": "x"}]}),
            "x",
            RAPID_MLX_TRANSIENT_TAIL
        ));
    }

    /// Alibaba's Token Plan rejects a request outright when any of these four are wrong, and the
    /// rejection surfaces as a contract violation from the cloud harness rather than as a provider
    /// error, so it is worth failing the build instead of a benchmark episode.
    #[test]
    fn sanitize_applies_the_qwen_token_plan_contract_for_alibaba() {
        let provider = make_provider("alibaba");
        let payload = json!({
            "model": "qwen3.8-flash",
            "messages": [],
            "max_tokens": 131072
        });

        let result = provider.sanitize_request_for_compat(payload);
        let obj = result.as_object().unwrap();

        assert!(
            !obj.contains_key("max_tokens"),
            "max_tokens is rejected, not ignored"
        );
        assert_eq!(obj.get("max_completion_tokens").unwrap(), &json!(131072));
        assert_eq!(obj.get("enable_thinking").unwrap(), &json!(true));
        assert_eq!(obj.get("preserve_thinking").unwrap(), &json!(true));
    }

    #[test]
    fn sanitize_leaves_other_openai_providers_alone() {
        let provider = make_provider("openai");
        let payload = json!({"model": "gpt-5.6", "messages": [], "max_tokens": 4096});

        let obj = provider.sanitize_request_for_compat(payload);
        let obj = obj.as_object().unwrap();

        assert_eq!(obj.get("max_tokens").unwrap(), &json!(4096));
        assert!(!obj.contains_key("enable_thinking"));
    }

    #[test]
    fn sanitize_remaps_max_completion_tokens_for_compat_provider() {
        let provider = make_provider("mistral");
        let payload = json!({
            "model": "mistral-medium-latest",
            "messages": [],
            "max_completion_tokens": 16384
        });

        let result = provider.sanitize_request_for_compat(payload);
        let obj = result.as_object().unwrap();

        assert!(!obj.contains_key("max_completion_tokens"));
        assert_eq!(obj.get("max_tokens").unwrap(), &json!(16384));
    }

    #[test]
    fn sanitize_preserves_existing_max_tokens_for_compat_provider() {
        let provider = make_provider("mistral");
        let payload = json!({
            "model": "mistral-medium-latest",
            "messages": [],
            "max_tokens": 4096,
            "max_completion_tokens": 16384
        });

        let result = provider.sanitize_request_for_compat(payload);
        let obj = result.as_object().unwrap();

        assert!(!obj.contains_key("max_completion_tokens"));
        assert_eq!(obj.get("max_tokens").unwrap(), &json!(4096));
    }

    #[test]
    fn sanitize_noop_for_native_openai_provider() {
        let provider = make_provider("openai");
        let payload = json!({
            "model": "o3",
            "messages": [],
            "max_completion_tokens": 16384
        });

        let result = provider.sanitize_request_for_compat(payload);
        let obj = result.as_object().unwrap();

        assert!(obj.contains_key("max_completion_tokens"));
        assert!(!obj.contains_key("max_tokens"));
    }

    #[test]
    fn sanitize_noop_for_unknown_provider() {
        let provider = make_provider("some_future_provider");
        let payload = json!({
            "model": "future-model",
            "messages": [],
            "max_completion_tokens": 16384
        });

        let result = provider.sanitize_request_for_compat(payload);
        let obj = result.as_object().unwrap();

        assert!(obj.contains_key("max_completion_tokens"));
        assert!(!obj.contains_key("max_tokens"));
    }

    #[test]
    fn sanitize_no_token_params() {
        let provider = make_provider("groq");
        let payload = json!({
            "model": "llama-3.3-70b-versatile",
            "messages": []
        });

        let result = provider.sanitize_request_for_compat(payload.clone());
        assert_eq!(result, payload);
    }

    #[test]
    fn sanitize_nearai_reasoning_chat_params() {
        let provider = make_provider("nearai");
        let payload = json!({
            "model": "Qwen/Qwen3.6-35B-A3B-FP8",
            "messages": [
                {
                    "role": "developer",
                    "content": "system instructions"
                },
                {
                    "role": "user",
                    "content": "hello"
                }
            ],
            "reasoning_effort": "medium",
            "max_completion_tokens": 16384
        });

        let result = provider.sanitize_request_for_compat(payload);
        let obj = result.as_object().unwrap();

        assert!(!obj.contains_key("reasoning_effort"));
        assert!(!obj.contains_key("max_completion_tokens"));
        assert_eq!(obj.get("max_tokens").unwrap(), &json!(16384));
        assert_eq!(obj["messages"][0]["role"], "system");
        assert_eq!(obj["messages"][1]["role"], "user");
    }

    #[test]
    fn sanitize_nearai_preserves_openai_reasoning_effort() {
        let provider = make_provider("nearai");
        let payload = json!({
            "model": "openai/gpt-5",
            "messages": [],
            "reasoning_effort": "medium",
            "max_completion_tokens": 16384
        });

        let result = provider.sanitize_request_for_compat(payload);
        let obj = result.as_object().unwrap();

        assert_eq!(obj.get("reasoning_effort"), Some(&json!("medium")));
        assert!(!obj.contains_key("max_completion_tokens"));
        assert_eq!(obj.get("max_tokens").unwrap(), &json!(16384));
    }

    #[test]
    fn nearai_uses_chat_completions_for_openai_reasoning_models() {
        let provider = make_provider("nearai");

        assert!(!provider.should_use_responses_api_for_provider("openai/gpt-5"));
        assert!(!provider.should_use_responses_api_for_provider("openai/o3"));
    }

    #[test]
    fn responses_api_routing_uses_model_family_unless_path_forces_chat() {
        for (model_name, base_path, expected) in [
            ("gpt-5.4", "v1/chat/completions", true),
            ("gpt-5.4-xhigh", "v1/chat/completions", true),
            ("gpt-5.2-pro-2025-12-11", "v1/chat/completions", true),
            ("gpt-4o", "v1/chat/completions", false),
            ("gpt-5.2-codex", "openai/v1/chat/completions", false),
        ] {
            assert_eq!(
                OpenAiProvider::should_use_responses_api(model_name, base_path),
                expected,
                "unexpected routing for {model_name} via {base_path}"
            );
        }
    }

    #[test]
    fn custom_chat_path_maps_to_responses_path() {
        let responses_path = OpenAiProvider::map_base_path(
            "openai/v1/chat/completions",
            "responses",
            "v1/responses",
        );
        assert_eq!(responses_path, "openai/v1/responses");
    }

    #[test]
    fn responses_path_maps_to_models_path() {
        let models_path =
            OpenAiProvider::map_base_path("openai/v1/responses", "models", "v1/models");
        assert_eq!(models_path, "openai/v1/models");
    }

    #[test]
    fn unknown_path_falls_back_to_default_models_path() {
        let models_path = OpenAiProvider::map_base_path("custom/path", "models", "v1/models");
        assert_eq!(models_path, "v1/models");
    }

    #[test]
    fn absolute_chat_path_maps_to_absolute_responses_path() {
        let responses_path =
            OpenAiProvider::map_base_path("/v1/chat/completions", "responses", "v1/responses");
        assert_eq!(responses_path, "/v1/responses");
    }

    #[test]
    fn unknown_absolute_path_falls_back_to_absolute_models_path() {
        let models_path = OpenAiProvider::map_base_path("/custom/path", "models", "v1/models");
        assert_eq!(models_path, "/v1/models");
    }
    #[test]
    fn versionless_base_path_opts_out_of_responses_for_codex_models() {
        assert!(!OpenAiProvider::should_use_responses_api(
            "gpt-5-codex",
            "chat/completions"
        ));
    }

    #[test]
    fn ensure_url_scheme_adds_http_for_local_hosts() {
        assert_eq!(ensure_url_scheme("localhost:1234"), "http://localhost:1234");
        assert_eq!(
            ensure_url_scheme("127.0.0.1:8080/v1"),
            "http://127.0.0.1:8080/v1"
        );
        assert_eq!(ensure_url_scheme("0.0.0.0:3000"), "http://0.0.0.0:3000");
        assert_eq!(ensure_url_scheme("[::1]:1234"), "http://[::1]:1234");
    }

    #[test]
    fn ensure_url_scheme_adds_https_for_remote_hosts() {
        assert_eq!(
            ensure_url_scheme("api.example.com:8443/v1"),
            "https://api.example.com:8443/v1"
        );
        assert_eq!(ensure_url_scheme("example.com"), "https://example.com");
    }

    #[test]
    fn ensure_url_scheme_preserves_existing_scheme() {
        assert_eq!(
            ensure_url_scheme("http://localhost:1234"),
            "http://localhost:1234"
        );
        assert_eq!(
            ensure_url_scheme("https://api.openai.com/v1"),
            "https://api.openai.com/v1"
        );
    }

    fn custom_config(base_url: &str) -> DeclarativeProviderConfig {
        DeclarativeProviderConfig {
            name: "test-openai".to_string(),
            engine: crate::declarative::ProviderEngine::OpenAI,
            display_name: "Test OpenAI".to_string(),
            description: None,
            api_key_env: String::new(),
            base_url: base_url.to_string(),
            models: vec![crate::base::ModelInfo::new("test-model", 4096)],
            headers: None,
            timeout_seconds: None,
            supports_streaming: None,
            requires_auth: false,
            catalog_provider_id: None,
            base_path: None,
            env_vars: None,
            dynamic_models: Some(false),
            skip_canonical_filtering: false,
            model_doc_link: None,
            setup_steps: vec![],
            fast_model: None,
            preserves_thinking: false,
        }
    }

    #[test]
    fn from_custom_config_preserves_ipv6_authority() {
        let provider = from_declarative_config(
            custom_config("http://[::1]:1234/v1"),
            None,
            crate::declarative::EnvKeyResolver,
        )
        .unwrap()
        .build();

        assert_eq!(provider.api_client.host(), "http://[::1]:1234");
    }

    #[test]
    fn from_custom_config_preserves_userinfo_authority() {
        let provider = from_declarative_config(
            custom_config("https://user:pass@gateway.example/v1"),
            None,
            crate::declarative::EnvKeyResolver,
        )
        .unwrap()
        .build();

        assert_eq!(
            provider.api_client.host(),
            "https://user:pass@gateway.example"
        );
    }

    #[test]
    fn parse_n_ctx_falls_back_to_sole_entry_when_id_differs() {
        let body = json!({
            "data": [
                { "id": "/models/qwen3.gguf", "meta": { "n_ctx": 32768 } }
            ]
        });
        assert_eq!(parse_n_ctx_from_models(&body, "qwen3"), Some(32768));
    }

    /// I-12: the Rapid-MLX RC13-line engine's `/v1/models` entries as served on 2026-10-18 (Gabee's
    /// Gemma 4 31B under the memory guard, the Studio's Gemma 4 26B-A4B): no `meta`, the served
    /// window in `max_model_len`, the model's window in `context_window`.
    #[test]
    fn parse_n_ctx_reads_the_rapid_mlx_served_window() {
        let gabee = json!({"object": "list", "data": [{
            "id": "t12-candidates/gemma-4-31B-it-MLX-8bit", "object": "model", "owned_by": "rapid-mlx",
            "context_window": 262144, "max_model_len": 76800
        }]});
        assert_eq!(
            parse_n_ctx_from_models(&gabee, "t12-candidates/gemma-4-31B-it-MLX-8bit"),
            Some(76_800),
            "the served window wins over the model's own"
        );
        let studio = json!({"object": "list", "data": [{
            "id": "lmstudio-community/gemma-4-26B-A4B-it-MLX-8bit", "owned_by": "rapid-mlx",
            "context_window": 262144, "max_model_len": 262144
        }]});
        assert_eq!(
            parse_n_ctx_from_models(&studio, "lmstudio-community/gemma-4-26B-A4B-it-MLX-8bit"),
            Some(262_144)
        );
        let window_only = json!({"data": [{"id": "m", "context_window": 131072}]});
        assert_eq!(parse_n_ctx_from_models(&window_only, "m"), Some(131_072));
        let meta_first =
            json!({"data": [{"id": "m", "meta": {"n_ctx": 8192}, "max_model_len": 32768}]});
        assert_eq!(parse_n_ctx_from_models(&meta_first, "m"), Some(8192));
        let zero = json!({"data": [{"id": "m", "max_model_len": 0, "context_window": 0}]});
        assert_eq!(parse_n_ctx_from_models(&zero, "m"), None);
    }

    #[test]
    fn parse_n_ctx_no_fallback_with_multiple_unmatched_entries() {
        let body = json!({
            "data": [
                { "id": "model-a", "meta": { "n_ctx": 4096 } },
                { "id": "model-b", "meta": { "n_ctx": 8192 } }
            ]
        });
        assert_eq!(parse_n_ctx_from_models(&body, "model-c"), None);
    }

    /// The live LM Studio `/api/v0/models` body from the local node, 2026-09-02: three loaded
    /// entries (`gabee-qwen3.8-27b` at 180,224 — the r6f/r6h window) and seven `not-loaded`.
    const LM_STUDIO_API_V0_MODELS: &str = include_str!("testdata/lmstudio_api_v0_models.json");

    fn lm_studio_models() -> serde_json::Value {
        serde_json::from_str(LM_STUDIO_API_V0_MODELS).unwrap()
    }

    #[test]
    fn lm_studio_arm_reads_loaded_context_length_for_the_loaded_model() {
        let body = lm_studio_models();
        assert_eq!(
            parse_lm_studio_loaded_context_length(&body, "gabee-qwen3.8-27b"),
            Ok(180_224)
        );
        assert_eq!(
            parse_lm_studio_loaded_context_length(&body, "mihai-qwen3.8-27b"),
            Ok(262_144)
        );
    }

    #[test]
    fn lm_studio_arm_names_a_not_loaded_model_instead_of_guessing_its_window() {
        let miss =
            parse_lm_studio_loaded_context_length(&lm_studio_models(), "qwen3.8-27b").unwrap_err();
        assert_eq!(
            miss,
            LmStudioWindowMiss::NotLoaded {
                state: "not-loaded".to_string(),
                max_context_length: Some(262_144),
            }
        );
        assert_eq!(
            miss.to_string(),
            "no loaded_context_length (state not-loaded, max_context_length 262144)"
        );
    }

    #[test]
    fn lm_studio_arm_names_an_unlisted_model() {
        let miss = parse_lm_studio_loaded_context_length(&lm_studio_models(), "gabee").unwrap_err();
        assert_eq!(miss, LmStudioWindowMiss::ModelNotListed { listed: 10 });
        assert_eq!(miss.to_string(), "model not among the 10 listed ids");
    }

    #[test]
    fn lm_studio_arm_names_a_body_without_a_data_array() {
        assert_eq!(
            parse_lm_studio_loaded_context_length(
                &json!({ "error": "not found" }),
                "gabee-qwen3.8-27b"
            ),
            Err(LmStudioWindowMiss::NoDataArray)
        );
    }

    #[test]
    fn llama_cpp_arm_finds_nothing_in_the_lm_studio_body() {
        // The nine-week miss in one line: LM Studio's entries carry no `meta.n_ctx`, so the
        // first arm returns None and, before VA-112, the default was taken without a word.
        assert_eq!(
            parse_n_ctx_from_models(&lm_studio_models(), "gabee-qwen3.8-27b"),
            None
        );
        assert_eq!(describe_listing(&lm_studio_models()), "among 10 listed ids");
    }

    #[test]
    fn derive_base_path_not_removing_api_path() {
        let r = derive_base_path("https://opencode.ai/zen/go");
        assert_eq!(r, "https://opencode.ai/zen/go/v1/chat/completions");
    }

    #[test]
    fn derive_base_path_should_support_v1() {
        let r = derive_base_path("https://opencode.ai/zen/go/v1");
        assert_eq!(r, "https://opencode.ai/zen/go/v1/chat/completions");
    }

    #[test]
    fn derive_base_path_should_support_no_base_path() {
        let r = derive_base_path("https://opencode.ai/");
        assert_eq!(r, "https://opencode.ai/v1/chat/completions");
    }

    #[test]
    fn derive_base_path_preserves_non_v1_version_prefix() {
        // Zhipu's default base_url is https://open.bigmodel.cn/api/paas/v4 and
        // from_custom_config passes url.path() ("/api/paas/v4") here. The
        // existing /api/paas/v4 version must not gain an extra /v1 segment.
        let r = derive_base_path("/api/paas/v4");
        assert_eq!(r, "api/paas/v4/chat/completions");
    }

    #[test]
    fn derive_base_path_does_not_treat_v_word_as_version() {
        let r = derive_base_path("/api/voice");
        assert_eq!(r, "api/voice/v1/chat/completions");
    }
}

#[cfg(test)]
mod media_input_tests {
    use super::*;
    use crate::conversation::message::{MediaContent, MediaKind};
    use serde_json::json;

    fn clip(kind: MediaKind, data: &str, name: &str) -> MediaContent {
        MediaContent {
            kind,
            data: data.to_string(),
            mime_type: match kind {
                MediaKind::Audio => "audio/wav".to_string(),
                MediaKind::Video => "video/mp4".to_string(),
            },
            name: Some(name.to_string()),
        }
    }

    /// Every POST body goose sent, for each conversation streamed through an `omlx` provider at
    /// a mock engine whose `/v1/models` lists `caps`. A side request (the pre-pass) is answered
    /// `side_status` with "HEARD <n>" as its text; every other request is answered "ok".
    async fn prepass_bodies(
        caps: serde_json::Value,
        conversations: &[Vec<Message>],
        side_status: u16,
    ) -> Vec<serde_json::Value> {
        use futures::StreamExt;
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"object": "list", "data": [{"id": "served", "capabilities": caps}]}),
            ))
            .mount(&server)
            .await;
        let side = if side_status == 200 {
            ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"index": 0, "message": {"role": "assistant",
                             "content": "  HEARD Thursday at 4 pm  "}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 812, "completion_tokens": 9, "total_tokens": 821},
            }))
        } else {
            ResponseTemplate::new(side_status)
                .set_body_json(json!({"error": "audio decode failed"}))
        };
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains(MEDIA_PREPASS_SYSTEM))
            .respond_with(side)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"},
                             "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            })))
            .mount(&server)
            .await;

        let mut provider = super::tests::make_provider("omlx");
        provider.api_client =
            ApiClient::new_with_tls(server.uri(), AuthMethod::NoAuth, None).unwrap();
        provider.supports_streaming = false;
        for messages in conversations {
            let stream = provider
                .stream(&ModelConfig::new("served"), "system", messages, &[])
                .await
                .unwrap();
            let _: Vec<_> = stream.collect().await;
        }
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.method.as_str() == "POST")
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }

    fn is_side(body: &serde_json::Value) -> bool {
        body["messages"][0]["content"] == MEDIA_PREPASS_SYSTEM
    }

    const OMNI_CAPS: &[&str] = &["text", "vision", "video", "audio", "tools"];

    #[tokio::test]
    async fn each_attached_clip_goes_to_the_engine_alone_and_the_agent_turn_carries_its_text() {
        let turn = vec![Message::user()
            .with_text("what day is the meeting?")
            .with_media(clip(MediaKind::Audio, "UklGRprepass1", "call.wav"))
            .with_media(clip(MediaKind::Video, "AAAAprepass1", "clip.mp4"))];
        let mut second = turn.clone();
        second.push(Message::assistant().with_text("Thursday."));
        second.push(Message::user().with_text("and the time?"));
        let bodies = prepass_bodies(json!(OMNI_CAPS), &[turn, second], 200).await;

        let side: Vec<_> = bodies.iter().filter(|b| is_side(b)).collect();
        // One side request per clip; the second turn re-reads them from the cache.
        assert_eq!(side.len(), 2, "{bodies:#?}");
        assert_eq!(bodies.len(), 4);
        let audio_side = side
            .iter()
            .find(|b| b["messages"][1]["content"][1]["type"] == "input_audio")
            .expect("an audio side request");
        assert!(audio_side.get("tools").is_none());
        assert_eq!(audio_side["messages"].as_array().unwrap().len(), 2);
        assert_eq!(audio_side["max_tokens"], MEDIA_PREPASS_MAX_TOKENS);
        let ask = audio_side["messages"][1]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert!(ask.starts_with("Transcribe") && ask.contains("what day is the meeting?"));
        assert!(side
            .iter()
            .any(|b| b["messages"][1]["content"][1]["type"] == "video_url"));

        for main in bodies.iter().filter(|b| !is_side(b)) {
            let wire = serde_json::to_string(main).unwrap();
            assert!(
                !wire.contains("input_audio") && !wire.contains("video_url"),
                "{wire}"
            );
            assert!(wire.contains("[Attached audio call.wav (audio/wav) - transcript made from the attachment by served"));
            assert!(wire.contains("[Attached video clip.mp4 (video/mp4) - description made from the attachment by served"));
            assert!(wire.contains("HEARD Thursday at 4 pm"));
            assert!(wire.contains("what day is the meeting?"));
        }
    }

    #[tokio::test]
    async fn media_the_engine_does_not_declare_keeps_its_named_placeholder_and_no_side_request() {
        let turn = vec![Message::user().with_text("listen").with_media(clip(
            MediaKind::Audio,
            "UklGRprepass2",
            "call.wav",
        ))];
        let bodies = prepass_bodies(json!(["text", "tools"]), &[turn], 200).await;
        assert_eq!(bodies.len(), 1);
        assert!(!is_side(&bodies[0]));
        let wire = serde_json::to_string(&bodies[0]).unwrap();
        assert!(
            wire.contains("this model cannot take audio input"),
            "{wire}"
        );
        assert!(!wire.contains("UklGRprepass2"));
    }

    #[tokio::test]
    async fn a_failed_side_request_names_the_clip_and_the_reason_and_the_turn_still_goes() {
        let turn = vec![Message::user().with_text("listen").with_media(clip(
            MediaKind::Audio,
            "UklGRprepass3",
            "broken.wav",
        ))];
        let bodies = prepass_bodies(json!(OMNI_CAPS), &[turn], 400).await;
        assert_eq!(bodies.len(), 2, "{bodies:#?}");
        let main = bodies.iter().find(|b| !is_side(b)).unwrap();
        let wire = serde_json::to_string(main).unwrap();
        assert!(!wire.contains("UklGRprepass3"), "{wire}");
        assert!(wire.contains(
            "[Attached audio broken.wav (audio/wav) - served could not turn it into text:"
        ));
        assert!(wire.contains("audio decode failed"));
    }

    #[tokio::test]
    async fn an_image_is_described_first_only_on_an_engine_that_hears_audio() {
        let png = "iVBORw0KGgoprepass4";
        let turn = vec![Message::user()
            .with_text("what is this?")
            .with_image(png, "image/png")];
        let bodies = prepass_bodies(json!(OMNI_CAPS), std::slice::from_ref(&turn), 200).await;
        assert_eq!(bodies.iter().filter(|b| is_side(b)).count(), 1);
        let main = serde_json::to_string(bodies.iter().find(|b| !is_side(b)).unwrap()).unwrap();
        assert!(!main.contains(png) && main.contains("[Attached image #1 (image/png)"));

        // A vision engine without audio (the text lane, or a VLM) still gets the image itself.
        let bodies = prepass_bodies(json!(["text", "vision", "tools"]), &[turn], 200).await;
        assert_eq!(bodies.len(), 1);
        assert!(serde_json::to_string(&bodies[0]).unwrap().contains(png));
    }

    fn pcm_wav_b64(sample_rate: u32, secs: u32) -> String {
        use base64::Engine as _;
        let data_len = sample_rate * 2 * secs;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        for v in [16u32] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [1u16, 1u16] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&sample_rate.to_le_bytes());
        b.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        for v in [2u16, 16u16] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        b.extend(std::iter::repeat_n(7u8, data_len as usize));
        base64::engine::general_purpose::STANDARD.encode(b)
    }

    #[tokio::test]
    async fn a_long_wav_is_heard_in_30_second_pieces_and_the_transcript_names_each_span() {
        let turn = vec![Message::user()
            .with_text("summarise the call")
            .with_media(clip(MediaKind::Audio, &pcm_wav_b64(8_000, 70), "long.wav"))];
        let bodies = prepass_bodies(json!(OMNI_CAPS), &[turn], 200).await;
        let side: Vec<_> = bodies.iter().filter(|b| is_side(b)).collect();
        assert_eq!(side.len(), 3, "one side request per 30 s piece");
        let main = serde_json::to_string(bodies.iter().find(|b| !is_side(b)).unwrap()).unwrap();
        for span in [
            "[0:00 - 0:30] HEARD",
            "[0:30 - 1:00] HEARD",
            "[1:00 - 1:10] HEARD",
        ] {
            assert!(main.contains(span), "{span} in {main}");
        }
        assert!(!main.contains("input_audio"));
    }

    #[test]
    fn the_mllm_lane_media_cap_refusal_is_recognised() {
        let e = ProviderError::RequestFailed(
            "Request failed with status: 400 Bad Request. Vision-request prompt tokens (21193) \
             exceeds the per-batch cap (8192 ...)"
                .to_string(),
        );
        assert!(refuses_media_prefill(&e));
        assert!(!refuses_media_prefill(&ProviderError::RequestFailed(
            "boom".into()
        )));
    }

    #[test]
    fn media_is_withheld_only_when_the_engine_lists_capabilities_without_it() {
        let listing =
            |caps: serde_json::Value| json!({"data": [{"id": "m", "capabilities": caps}]});
        assert_eq!(
            withheld_media(&listing(json!(["text", "vision", "video", "audio"])), "m"),
            (false, false)
        );
        assert_eq!(
            withheld_media(&listing(json!(["text", "vision", "video"])), "m"),
            (true, false)
        );
        assert_eq!(withheld_media(&listing(json!(["text"])), "m"), (true, true));
        assert_eq!(
            withheld_media(&json!({"data": [{"id": "m"}]}), "m"),
            (false, false)
        );
    }
}
