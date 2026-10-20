use anyhow::{bail, Result};
use async_trait::async_trait;
use futures::future::BoxFuture;
use goose_providers::images::{announced, withheld_images_notice, ImageFormat, ImageInput};
use serde_json::{json, Value};
use std::collections::HashMap;

use super::api_client::{ApiClient, AuthMethod};
use super::base::{
    model_info_for_provider_model, ConfigKey, DeclaredModelParameters, MessageStream, ModelInfo,
    Provider, ProviderDef, ProviderMetadata,
};
use super::openai_compatible::{handle_status, stream_openai_compat_timed};
use super::retry::ProviderRetry;
use crate::conversation::message::Message;
use crate::providers::formats::openrouter as openrouter_format;
use goose_providers::errors::ProviderError;
use goose_providers::formats::anthropic_cache::apply_anthropic_cache_breakpoints;
use goose_providers::formats::openai::{create_request_reporting_images, OpenAiFormatOptions};
use goose_providers::model::ModelConfig;
use goose_providers::request_log::{start_log, LoggerHandleExt};
use rmcp::model::Tool;

pub const OPENROUTER_PROVIDER_NAME: &str = "openrouter";
pub const OPENROUTER_PARAMETERS_CONFIG_KEY: &str = "OPENROUTER_PARAMETERS";
/// The owner's opt-in to OpenRouter's middle-out compression; off unless set to true.
pub const OPENROUTER_MIDDLE_OUT_CONFIG_KEY: &str = "OPENROUTER_MIDDLE_OUT";
/// OpenRouter's plugin id for middle-out (openrouter.ai/docs/guides/features/message-transforms).
pub const CONTEXT_COMPRESSION_PLUGIN: &str = "context-compression";
pub const OPENROUTER_DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4";
pub const OPENROUTER_DEFAULT_FAST_MODEL: &str = "google/gemini-2.5-flash";
pub const OPENROUTER_MODEL_PREFIX_ANTHROPIC: &str = "anthropic";

// OpenRouter can run many models, we suggest the default
pub const OPENROUTER_KNOWN_MODELS: &[&str] = &[
    "x-ai/grok-code-fast-1",
    "anthropic/claude-sonnet-4.5",
    "anthropic/claude-sonnet-4",
    "anthropic/claude-opus-4.1",
    "anthropic/claude-opus-4",
    "google/gemini-2.5-pro",
    "google/gemini-2.5-flash",
    "deepseek/deepseek-r1-0528",
    "qwen/qwen3-coder",
    "moonshotai/kimi-k2",
];
pub const OPENROUTER_DOC_URL: &str = "https://openrouter.ai/models";

/// App attribution (openrouter.ai/docs/app-attribution): HTTP-Referer is the app's identity on
/// OpenRouter's rankings, X-OpenRouter-Title its display name (X-Title is the legacy spelling,
/// still read). Usage from this fork is credited to LeanZero, not to upstream goose.
pub const OPENROUTER_ATTRIBUTION_HEADERS: &[(&str, &str)] = &[
    ("HTTP-Referer", "https://leanzero.net"),
    ("X-OpenRouter-Title", "LeanZero Goose"),
    ("X-Title", "LeanZero Goose"),
    ("X-OpenRouter-Categories", "cli-agent,programming-app"),
];

#[derive(serde::Serialize)]
pub struct OpenRouterProvider {
    #[serde(skip)]
    api_client: ApiClient,
    supports_streaming: bool,
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    configured_parameters: Option<HashMap<String, Value>>,
    #[serde(skip)]
    listing: tokio::sync::OnceCell<ModelsListing>,
    #[serde(skip)]
    resolved_windows: std::sync::Mutex<HashMap<String, Option<usize>>>,
    /// Models this provider already told the chat that Off cannot switch their reasoning off.
    #[serde(skip)]
    effort_notices: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl OpenRouterProvider {
    pub async fn from_env(
        tls_config: Option<crate::providers::api_client::TlsConfig>,
    ) -> Result<Self> {
        Self::with_parameters(tls_config, configured_openrouter_parameters()?)
    }

    /// The provider a key check runs. OPENROUTER_PARAMETERS steers WHERE a chosen model runs (a
    /// `provider.order` pin with `allow_fallbacks: false` removes every other upstream); it is no
    /// part of the key. Under a pin the saved default model can have no endpoint at all: on
    /// 2026-10-03 an `["xiaomi"]` pin turned the check of a working key into OpenRouter's 404 "No
    /// endpoints found for deepseek/deepseek-v4.1-flash", recorded as the connection error, and
    /// OpenRouter vanished from every configured-provider picker for as long as the pin stood.
    pub(crate) fn for_key_check(
        tls_config: Option<crate::providers::api_client::TlsConfig>,
    ) -> Result<Self> {
        Self::with_parameters(tls_config, None)
    }

    fn with_parameters(
        tls_config: Option<crate::providers::api_client::TlsConfig>,
        configured_parameters: Option<HashMap<String, Value>>,
    ) -> Result<Self> {
        let api_client = openrouter_api_client(crate::config::Config::global(), tls_config)?
            .with_request_builder(crate::session_context::session_id_request_builder());

        Ok(Self {
            api_client,
            supports_streaming: true,
            name: OPENROUTER_PROVIDER_NAME.to_string(),
            configured_parameters,
            listing: tokio::sync::OnceCell::new(),
            resolved_windows: std::sync::Mutex::new(HashMap::new()),
            effort_notices: Default::default(),
        })
    }

    async fn fetch_models_listing(&self) -> Result<Vec<ListedModel>, ProviderError> {
        let response = self
            .api_client
            .request("api/v1/models")
            .response_get()
            .await
            .map_err(|e| {
                ProviderError::RequestFailed(format!(
                    "Failed to fetch models from OpenRouter API: {}",
                    e
                ))
            })?;

        let json: Value = response.json().await.map_err(|e| {
            ProviderError::RequestFailed(format!(
                "Failed to parse OpenRouter API response as JSON: {}",
                e
            ))
        })?;

        parse_models_listing(&json)
    }

    /// The live models listing, fetched once; a failed fetch is not remembered, so the next lookup
    /// tries again.
    async fn listing(&self) -> Result<&ModelsListing, ProviderError> {
        self.listing
            .get_or_try_init(|| async {
                let listing = self.fetch_models_listing().await?;
                Ok(ModelsListing::of(&listing))
            })
            .await
    }

    async fn context_windows(&self) -> Result<&HashMap<String, usize>, ProviderError> {
        Ok(&self.listing().await?.windows)
    }

    /// Whether `model_name` takes image input, as OpenRouter's models listing declares it in
    /// `architecture.input_modalities`.
    async fn listed_image_input(&self, model_name: &str) -> Result<ImageInput, ProviderError> {
        Ok(self
            .listing()
            .await?
            .image_inputs
            .get(model_name)
            .copied()
            .unwrap_or(ImageInput::Undeclared))
    }

    /// Fits the request's `reasoning.effort` to what the listing declares for the model
    /// (`fit_reasoning_effort`): an "Off" turn on a model that cannot switch reasoning off was a
    /// 400 on every call. A listing that cannot be read (or does not answer within 10 s) leaves
    /// the request as it was. Returns the chat notice, once per model, when Off could not be
    /// honoured: the picker still reads "Off" while the model reasons at its lowest level.
    async fn fit_reasoning_effort(
        &self,
        payload: &mut Value,
        model_config: &ModelConfig,
        users_reasoning: bool,
    ) -> Option<String> {
        payload.get("reasoning")?;
        let model = &model_config.model_name;
        let listing =
            match tokio::time::timeout(std::time::Duration::from_secs(10), self.listing()).await {
                Ok(Ok(listing)) => listing,
                failed => {
                    let why = match failed {
                        Ok(Err(err)) => err.to_string(),
                        _ => "no answer within 10 s".to_string(),
                    };
                    tracing::warn!(
                        model = %model,
                        "reasoning_effort_unfitted: OpenRouter's models listing could not be read \
                         ({why}); the effort is sent as configured"
                    );
                    return None;
                }
            };
        // A routing variant (`:nitro`, `:floor`, `:online`) is not a listing id of its own.
        let declared = listing.parameters.get(model).or_else(|| {
            model
                .rsplit_once(':')
                .and_then(|(base, _)| listing.parameters.get(base))
        })?;
        let goose_effort = if users_reasoning {
            None
        } else {
            model_config.thinking_effort()
        };
        let (sent, fitted) =
            openrouter_format::fit_reasoning_effort(payload, declared, goose_effort)?;
        tracing::info!(
            model = %model,
            "reasoning_effort_fitted: {sent} -> {fitted} (the model declares mandatory={}, levels {:?})",
            declared.reasoning_mandatory,
            declared.effort_levels
        );
        if sent != "none" {
            return None;
        }
        let first = self
            .effort_notices
            .lock()
            .map(|mut told| told.insert(model.clone()))
            .unwrap_or(false);
        first.then(|| {
            let level = if fitted == "model default" {
                "its default level".to_string()
            } else {
                format!("its lowest level ({fitted})")
            };
            format!(
                "{model} cannot switch reasoning off, so Thinking Off runs it at {level}. \
                 OpenRouter refuses a request with reasoning off for this model."
            )
        })
    }

    /// The request for `model_config`, with every image part withheld behind a named placeholder
    /// when the listing declares the model text-only, and the chat's notice for the images this
    /// request is the first to withhold. OpenRouter answers an image part sent to a text-only model
    /// with 404 "No endpoints found that support image input" — a turn lost, and a session lost
    /// once the image sits in history. A request with no image never consults the listing.
    async fn chat_request(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<(Value, Option<String>), ProviderError> {
        let options = OpenAiFormatOptions {
            preserve_thinking_context: true,
            ..Default::default()
        };
        let build = |options: OpenAiFormatOptions| -> Result<_, ProviderError> {
            Ok(create_request_reporting_images(
                model_config,
                system,
                messages,
                tools,
                &ImageFormat::OpenAi,
                true,
                options,
            )?)
        };
        let (payload, images) = build(options)?;
        if images.sent == 0 {
            return Ok((payload, None));
        }
        match self.listed_image_input(&model_config.model_name).await {
            Ok(ImageInput::TextOnly) => {
                let (payload, images) = build(OpenAiFormatOptions {
                    text_only_engine: true,
                    ..options
                })?;
                tracing::info!(
                    model = %model_config.model_name,
                    withheld = images.withheld.len(),
                    "images_withheld: OpenRouter's models listing declares this model's \
                     input_modalities without image; every image part went as a named placeholder"
                );
                let notice = withheld_images_notice(&model_config.model_name, &images.withheld);
                Ok((payload, notice))
            }
            Ok(ImageInput::Reads) => Ok((payload, None)),
            Ok(ImageInput::Undeclared) => {
                tracing::warn!(
                    model = %model_config.model_name,
                    images = images.sent,
                    "image_input_undeclared: OpenRouter's models listing names no \
                     input_modalities for this model, so whether it takes images is unknown; \
                     the images are sent as they are"
                );
                Ok((payload, None))
            }
            Err(err) => {
                tracing::warn!(
                    model = %model_config.model_name,
                    images = images.sent,
                    reason = %err,
                    "image_input_probe_failed: OpenRouter's models listing could not be read, \
                     so whether this model takes images is unknown; the images are sent as they \
                     are"
                );
                Ok((payload, None))
            }
        }
    }

    /// The window OpenRouter's live models listing declares for `model_name` — `None`, with a
    /// warning, when the listing cannot be fetched (retried on the next lookup) or does not carry
    /// the model (remembered, warned once).
    async fn listed_context_window(&self, model_name: &str) -> Option<usize> {
        if let Some(resolved) = self
            .resolved_windows
            .lock()
            .ok()
            .and_then(|cache| cache.get(model_name).copied())
        {
            return resolved;
        }

        let windows = match self.context_windows().await {
            Ok(windows) => windows,
            Err(err) => {
                // Not remembered: a transient failure must not pin the default for the life of a
                // long-running process — the next lookup tries the listing again.
                tracing::warn!(
                    model = %model_name,
                    reason = %err,
                    "context_window_listing_failed: OpenRouter's models listing could not be \
                     read; the compaction guard and the per-turn context line run on the default"
                );
                return None;
            }
        };
        let window = windows.get(model_name).copied();
        if window.is_none() {
            tracing::warn!(
                model = %model_name,
                listed_models = windows.len(),
                "context_window_unlisted: OpenRouter's models listing carries no \
                 context_length for this model; the compaction guard and the per-turn \
                 context line run on the default"
            );
        }

        if let Ok(mut cache) = self.resolved_windows.lock() {
            cache.insert(model_name.to_string(), window);
        }
        window
    }
}

struct ListedModel {
    id: String,
    context_length: Option<usize>,
    image_input: ImageInput,
    parameters: DeclaredModelParameters,
}

/// What the models listing declares per model id, kept for the life of the provider.
struct ModelsListing {
    windows: HashMap<String, usize>,
    image_inputs: HashMap<String, ImageInput>,
    parameters: HashMap<String, DeclaredModelParameters>,
}

impl ModelsListing {
    fn of(listing: &[ListedModel]) -> Self {
        Self {
            windows: context_windows_of(listing),
            image_inputs: listing
                .iter()
                .map(|model| (model.id.clone(), model.image_input))
                .collect(),
            parameters: listing
                .iter()
                .map(|model| (model.id.clone(), model.parameters.clone()))
                .collect(),
        }
    }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// A listing entry's `supported_parameters`, its `reasoning` block (`supported_efforts`,
/// `default_effort`, `mandatory`) and the non-null values of its `default_parameters`.
fn listed_parameters(model: &Value) -> DeclaredModelParameters {
    let reasoning = model.get("reasoning");
    DeclaredModelParameters {
        supported_parameters: string_list(model.get("supported_parameters")),
        effort_levels: string_list(reasoning.and_then(|r| r.get("supported_efforts"))),
        default_effort: reasoning
            .and_then(|r| r.get("default_effort"))
            .and_then(Value::as_str)
            .map(str::to_string),
        reasoning_mandatory: reasoning
            .and_then(|r| r.get("mandatory"))
            .and_then(Value::as_bool)
            == Some(true),
        default_parameters: model
            .get("default_parameters")
            .and_then(Value::as_object)
            .map(|defaults| {
                defaults
                    .iter()
                    .filter(|(_, value)| !value.is_null())
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// A listing entry's `architecture.input_modalities` (e.g. `["text"]`, `["text", "image"]`): a
/// list naming `image` takes images, a list without it is text-only, no list declares nothing.
fn listed_image_input(model: &Value) -> ImageInput {
    match model
        .get("architecture")
        .and_then(|architecture| architecture.get("input_modalities"))
        .and_then(Value::as_array)
    {
        Some(modalities) if modalities.iter().any(|m| m.as_str() == Some("image")) => {
            ImageInput::Reads
        }
        Some(_) => ImageInput::TextOnly,
        None => ImageInput::Undeclared,
    }
}

fn parse_models_listing(json: &Value) -> Result<Vec<ListedModel>, ProviderError> {
    if let Some(err_obj) = json.get("error") {
        let msg = err_obj
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(ProviderError::RequestFailed(format!(
            "OpenRouter API returned an error: {}",
            msg
        )));
    }

    let data = json
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ProviderError::UsageError("Missing data field in JSON response".into()))?;

    Ok(data
        .iter()
        .filter_map(|model| {
            let id = model.get("id").and_then(|v| v.as_str())?;
            let context_length = model
                .get("context_length")
                .and_then(Value::as_u64)
                .filter(|&length| length > 0)
                .map(|length| length as usize);
            Some(ListedModel {
                id: id.to_string(),
                context_length,
                image_input: listed_image_input(model),
                parameters: listed_parameters(model),
            })
        })
        .collect())
}

fn context_windows_of(listing: &[ListedModel]) -> HashMap<String, usize> {
    listing
        .iter()
        .filter_map(|model| Some((model.id.clone(), model.context_length?)))
        .collect()
}

fn is_gemini_model(model_name: &str) -> bool {
    model_name.starts_with("google/")
}

/// The client every OpenRouter call goes through — chat, the models listing, the host probe: the
/// saved OPENROUTER_API_KEY as the bearer, OPENROUTER_HOST (default openrouter.ai) and the
/// attribution headers.
pub(crate) fn openrouter_api_client(
    config: &crate::config::Config,
    tls_config: Option<crate::providers::api_client::TlsConfig>,
) -> Result<ApiClient> {
    let api_key: String = config.get_secret("OPENROUTER_API_KEY")?;
    let host: String = config
        .get_param("OPENROUTER_HOST")
        .unwrap_or_else(|_| "https://openrouter.ai".to_string());
    OPENROUTER_ATTRIBUTION_HEADERS.iter().try_fold(
        ApiClient::new_with_tls(host, AuthMethod::BearerToken(api_key), tls_config)?,
        |client, (name, value)| client.with_header(name, value),
    )
}

pub(crate) fn parse_openrouter_parameters(raw: Value) -> Result<HashMap<String, Value>> {
    match raw {
        Value::Object(params) => Ok(params.into_iter().collect()),
        Value::String(raw_json) => match serde_json::from_str::<Value>(&raw_json)? {
            Value::Object(params) => Ok(params.into_iter().collect()),
            _ => bail!("{OPENROUTER_PARAMETERS_CONFIG_KEY} must be a JSON object"),
        },
        _ => bail!("{OPENROUTER_PARAMETERS_CONFIG_KEY} must be a JSON object"),
    }
}

fn configured_openrouter_parameters() -> Result<Option<HashMap<String, Value>>> {
    let config = crate::config::Config::global();
    match config.get_param::<Value>(OPENROUTER_PARAMETERS_CONFIG_KEY) {
        Ok(raw) => parse_openrouter_parameters(raw).map(Some),
        Err(crate::config::ConfigError::NotFound(_)) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn merge_request_params(
    request_params: &mut Option<HashMap<String, Value>>,
    params: HashMap<String, Value>,
) {
    request_params
        .get_or_insert_with(HashMap::new)
        .extend(params);
}

/// The configured OPENROUTER_PARAMETERS one request carries. A `provider` routing block (a host
/// pin such as `{"order": ["mistral"], "allow_fallbacks": false}`) is set for the ENTRANT model;
/// background work on the provider's default fast model drops it (I-11, 2026-10-18): under a
/// `mistral` pin every title/summary helper call to google/gemini-2.5-flash came back 404 "No
/// endpoints found ... Every candidate endpoint was removed during routing" (12-16 per Mistral
/// Large 4 Forge build), then fell back to the entrant. Every other request keeps every parameter.
fn parameters_for_request(
    params: &HashMap<String, Value>,
    model_name: &str,
    background: bool,
) -> HashMap<String, Value> {
    let mut params = params.clone();
    if background && model_name == OPENROUTER_DEFAULT_FAST_MODEL {
        params.remove("provider");
    }
    params
}

fn merge_openrouter_parameters(model: &mut ModelConfig, params: HashMap<String, Value>) {
    merge_request_params(&mut model.request_params, params);
}

/// Whether the owner opted into middle-out (OPENROUTER_MIDDLE_OUT: true). Absent or unreadable is
/// off: compression drops the middle of the conversation without telling goose or the model.
pub(crate) fn middle_out_enabled(config: &crate::config::Config) -> bool {
    match config.get_param::<bool>(OPENROUTER_MIDDLE_OUT_CONFIG_KEY) {
        Ok(on) => on,
        Err(crate::config::ConfigError::NotFound(_)) => false,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "{OPENROUTER_MIDDLE_OUT_CONFIG_KEY} is unreadable; OpenRouter requests go without \
                 context compression"
            );
            false
        }
    }
}

/// Sets OpenRouter's context compression on `payload` and says whether the request allows it.
///
/// Off (the default), compression is disabled explicitly: OpenRouter compresses on its own for
/// every endpoint of 8,192 tokens or less, and a compressing request may be routed to any endpoint
/// whose window holds half the request ("When context compression is enabled, OpenRouter will
/// first try to find models whose context length is at least half of your total required tokens";
/// without it "the request will fail with an error message", which goose's context-length path
/// compacts and retries — openrouter.ai/docs/guides/features/message-transforms). Measured on the
/// Forge 2.0 builds of 2026-10: with `transforms: ["middle-out"]` on every request, Grok 4.7's
/// billed prompt fell 207,145 -> 76,843 at call #182 while goose kept sending the whole history,
/// and Tencent Hy4's low build was served 5-9k of a 95k-905k history on 268 of 301 calls.
///
/// On, the request carries `transforms: ["middle-out"]`. A request whose OPENROUTER_PARAMETERS
/// already name `transforms` or the context-compression plugin is left as the owner wrote it.
fn apply_context_compression(payload: &mut Value, middle_out: bool) -> bool {
    let Some(obj) = payload.as_object_mut() else {
        return false;
    };
    let plugin = obj
        .get("plugins")
        .and_then(Value::as_array)
        .and_then(|plugins| {
            plugins
                .iter()
                .find(|p| p.get("id").and_then(Value::as_str) == Some(CONTEXT_COMPRESSION_PLUGIN))
        });
    if let Some(plugin) = plugin {
        return plugin.get("enabled").and_then(Value::as_bool) != Some(false);
    }
    if let Some(transforms) = obj.get("transforms") {
        return transforms
            .as_array()
            .is_some_and(|t| t.iter().any(|t| t.as_str() == Some("middle-out")));
    }
    if middle_out {
        obj.insert("transforms".to_string(), json!(["middle-out"]));
        return true;
    }
    let off = json!({"id": CONTEXT_COMPRESSION_PLUGIN, "enabled": false});
    match obj.get_mut("plugins") {
        Some(Value::Array(plugins)) => plugins.push(off),
        Some(_) => {}
        None => {
            obj.insert("plugins".to_string(), json!([off]));
        }
    }
    false
}

/// goose's own count of a request: the whole of it, and the system prompt and tools alone.
struct SentPrompt {
    total: usize,
    fixed: usize,
}

async fn count_sent_prompt(
    system: &str,
    messages: &[Message],
    tools: &[Tool],
) -> Option<SentPrompt> {
    match crate::token_counter::create_token_counter().await {
        Ok(counter) => Some(SentPrompt {
            total: counter.count_chat_tokens(system, messages, tools),
            fixed: counter.count_chat_tokens(system, &[], tools),
        }),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "compression_unwatched: goose could not count this OpenRouter request, so a \
                 compressed prompt goes unnoticed on this call"
            );
            None
        }
    }
}

/// With compression allowed, compares each reported prompt with what goose sent
/// (`context_mgmt::prompt_was_compressed`) and tells the compaction guard either way.
fn watch_for_compression(
    stream: MessageStream,
    session_id: String,
    model: String,
    sent: SentPrompt,
) -> MessageStream {
    use futures::TryStreamExt;
    Box::pin(stream.inspect_ok(move |(_, usage)| {
        let Some(reported) = usage.as_ref().and_then(|usage| usage.usage.input_tokens) else {
            return;
        };
        let reported = reported.max(0) as usize;
        if crate::context_mgmt::prompt_was_compressed(sent.total, reported) {
            crate::context_mgmt::note_compressed_prompt(
                &session_id,
                &model,
                sent.total,
                reported,
                sent.fixed,
            );
        } else {
            crate::context_mgmt::forget_compressed_prompt(&session_id);
        }
    }))
}

impl goose_providers::base::ProviderDescriptor for OpenRouterProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            OPENROUTER_PROVIDER_NAME,
            "OpenRouter",
            "Router for many model providers",
            OPENROUTER_DEFAULT_MODEL,
            OPENROUTER_KNOWN_MODELS.to_vec(),
            OPENROUTER_DOC_URL,
            vec![
                ConfigKey::new("OPENROUTER_API_KEY", true, true, None, true),
                ConfigKey::new(
                    "OPENROUTER_HOST",
                    false,
                    false,
                    Some("https://openrouter.ai"),
                    false,
                ),
                ConfigKey::new(OPENROUTER_PARAMETERS_CONFIG_KEY, false, false, None, false),
                ConfigKey::new(OPENROUTER_MIDDLE_OUT_CONFIG_KEY, false, false, None, false),
            ],
        )
        .with_setup_steps(vec![
            "Go to https://openrouter.ai/settings/keys",
            "Click 'Create' or use an existing API key",
            "Copy the key and paste it above",
        ])
        .with_fast_model(OPENROUTER_DEFAULT_FAST_MODEL)
    }
}

impl ProviderDef for OpenRouterProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<crate::providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(Self::from_env(tls_config))
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        let listing = self.fetch_models_listing().await?;
        let _ = self.listing.set(ModelsListing::of(&listing));
        let mut models: Vec<String> = listing.into_iter().map(|model| model.id).collect();
        models.sort();
        Ok(models)
    }

    /// An explicit or catalogued window (GOOSE_CONTEXT_LIMIT, a session override, the canonical
    /// registry) is used as-is; otherwise the `context_length` OpenRouter's live models listing
    /// declares for the model, so a model newer than the bundled catalog does not compact at the
    /// 128,000 default's threshold.
    async fn get_context_limit(&self, model_config: &ModelConfig) -> Result<usize, ProviderError> {
        if let Some(limit) = model_config.context_limit {
            return Ok(limit);
        }
        Ok(self
            .listed_context_window(&model_config.model_name)
            .await
            .unwrap_or_else(|| model_config.context_limit()))
    }

    async fn fetch_model_info(&self, model_name: &str) -> Result<ModelInfo, ProviderError> {
        let mut info = model_info_for_provider_model(self.get_name(), model_name);
        if info.context_limit == 0 {
            if let Some(window) = self.listed_context_window(model_name).await {
                info.context_limit = window;
            }
        }
        Ok(info)
    }

    async fn declared_model_parameters(
        &self,
        model_name: &str,
    ) -> Result<Option<DeclaredModelParameters>, ProviderError> {
        Ok(self.listing().await?.parameters.get(model_name).cloned())
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let session_id = crate::session_context::current_session_id().unwrap_or_default();

        let mut merged_model;
        let model_config = if let Some(params) = &self.configured_parameters {
            merged_model = model_config.clone();
            merge_openrouter_parameters(
                &mut merged_model,
                parameters_for_request(
                    params,
                    &model_config.model_name,
                    crate::background_work::current_kind().is_some(),
                ),
            );
            &merged_model
        } else {
            model_config
        };

        let (mut payload, images_notice) = self
            .chat_request(model_config, system, messages, tools)
            .await?;

        // Add user field for OpenRouter attribution/rate-limiting
        if !session_id.is_empty() {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("user".to_string(), Value::String(session_id.to_string()));
            }
        }

        if supports_cache_control(model_config) {
            apply_anthropic_cache_breakpoints(&mut payload);
        }

        if is_gemini_model(&model_config.model_name) {
            openrouter_format::add_reasoning_details_to_request(&mut payload, messages);
        }
        // The user's own reasoning setting (OPENROUTER_PARAMETERS or a saved per-model field)
        // arrives as `reasoning` or `reasoning_effort`; only goose's own effort is raised by Max.
        let users_reasoning = payload.get("reasoning").is_some()
            || model_config.request_params.as_ref().is_some_and(|params| {
                params.contains_key("reasoning") || params.contains_key("reasoning_effort")
            });
        openrouter_format::apply_reasoning_config(&mut payload, model_config);
        let effort_notice = self
            .fit_reasoning_effort(&mut payload, model_config, users_reasoning)
            .await;
        let notice = match (images_notice, effort_notice) {
            (Some(images), Some(effort)) => Some(format!("{images}\n\n{effort}")),
            (images, effort) => images.or(effort),
        };

        // Read per request, so the owner's switch applies to the chat already open.
        let compresses = apply_context_compression(
            &mut payload,
            middle_out_enabled(crate::config::Config::global()),
        );
        // A background call (the compaction's own summary, a title) is sized by its own request,
        // not by the chat's window: what it is billed says nothing about the conversation.
        let watched = compresses
            && !session_id.is_empty()
            && crate::background_work::current_kind().is_none();
        let sent = match watched {
            true => count_sent_prompt(system, messages, tools).await,
            false => None,
        };

        let mut log = start_log(model_config, &payload)?;

        let telemetry_t0 = std::time::Instant::now();
        let response = self
            .with_retry(|| async {
                let resp = self
                    .api_client
                    .response_post("api/v1/chat/completions", &payload)
                    .await?;
                handle_status(resp).await
            })
            .await
            .inspect_err(|e| {
                let _ = log.error(e);
            })?;

        // The telemetry line carries the generation id OpenRouter bills under, so a benchmark harness
        // can read the provider's own bill per call (evals/swarm-bench/bench/bench_cost.py).
        let stream = stream_openai_compat_timed(
            response,
            log,
            telemetry_t0,
            model_config.model_name.clone(),
        )?;
        let stream = match sent {
            Some(sent) => watch_for_compression(
                stream,
                session_id.to_string(),
                model_config.model_name.clone(),
                sent,
            ),
            None => stream,
        };
        Ok(announced(stream, notice))
    }
}

fn supports_cache_control(model: &ModelConfig) -> bool {
    model
        .model_name
        .starts_with(OPENROUTER_MODEL_PREFIX_ANTHROPIC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_providers::base::ProviderDescriptor;
    use goose_providers::formats::openai::create_request;

    fn model_config(model_name: &str) -> ModelConfig {
        ModelConfig {
            model_name: model_name.to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
        }
    }

    #[tokio::test]
    async fn every_openrouter_request_credits_leanzero() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"data\":[]}"))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");
        let secrets_path = dir.path().join("secrets.yaml");
        std::fs::write(&config_path, format!("OPENROUTER_HOST: {}\n", server.uri())).unwrap();
        std::fs::write(&secrets_path, "OPENROUTER_API_KEY: or-test\n").unwrap();
        let config =
            crate::config::Config::new_with_file_secrets(&config_path, secrets_path).unwrap();
        let _env = env_lock::lock_env([
            ("OPENROUTER_HOST", None::<&str>),
            ("OPENROUTER_API_KEY", None::<&str>),
        ]);

        openrouter_api_client(&config, None)
            .unwrap()
            .response_get("api/v1/models")
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        let sent = |name: &str| {
            requests[0]
                .headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        assert_eq!(
            sent("HTTP-Referer").as_deref(),
            Some("https://leanzero.net")
        );
        assert_eq!(
            sent("X-OpenRouter-Title").as_deref(),
            Some("LeanZero Goose")
        );
        assert_eq!(sent("X-Title").as_deref(), Some("LeanZero Goose"));
        assert_eq!(
            sent("X-OpenRouter-Categories").as_deref(),
            Some("cli-agent,programming-app")
        );
    }

    #[test]
    fn metadata_includes_openrouter_parameters_config_key() {
        let metadata = OpenRouterProvider::metadata();

        assert!(metadata
            .config_keys
            .iter()
            .any(|key| key.name == OPENROUTER_PARAMETERS_CONFIG_KEY));
    }

    #[test]
    fn parse_openrouter_parameters_accepts_object_value() {
        let params = parse_openrouter_parameters(json!({
            "verbosity": "xhigh",
            "reasoning": { "effort": "high" }
        }))
        .unwrap();

        assert_eq!(params["verbosity"], json!("xhigh"));
        assert_eq!(params["reasoning"], json!({ "effort": "high" }));
    }

    #[test]
    fn parse_openrouter_parameters_accepts_json_string_value() {
        let params = parse_openrouter_parameters(json!(
            r#"{"plugins":[{"id":"web"}],"reasoning":{"max_tokens":2000}}"#
        ))
        .unwrap();

        assert_eq!(params["plugins"], json!([{ "id": "web" }]));
        assert_eq!(params["reasoning"], json!({ "max_tokens": 2000 }));
    }

    #[test]
    fn parse_openrouter_parameters_rejects_non_object_json_string() {
        let err = parse_openrouter_parameters(json!(r#"["web"]"#)).unwrap_err();

        assert!(err
            .to_string()
            .contains("OPENROUTER_PARAMETERS must be a JSON object"));
    }

    #[test]
    fn merge_openrouter_parameters_updates_model_request_params() {
        let mut model = model_config("anthropic/claude-sonnet-4");
        model.request_params = Some(HashMap::from([("verbosity".to_string(), json!("low"))]));

        let params = parse_openrouter_parameters(json!({
            "plugins": [{ "id": "web" }],
            "verbosity": "xhigh"
        }))
        .unwrap();

        merge_openrouter_parameters(&mut model, params);

        let request_params = model.request_params.as_ref().unwrap();
        assert_eq!(request_params["plugins"], json!([{ "id": "web" }]));
        assert_eq!(request_params["verbosity"], json!("xhigh"));
    }

    #[test]
    fn a_host_pin_stays_off_the_fast_model_helper_calls_only() {
        // I-11: the laptop chain pinned Mistral Large 4 to `mistral`; helper calls inherited it.
        let params = parse_openrouter_parameters(json!({
            "provider": {"order": ["mistral"], "allow_fallbacks": false},
            "verbosity": "low"
        }))
        .unwrap();

        let helper = parameters_for_request(&params, OPENROUTER_DEFAULT_FAST_MODEL, true);
        assert!(!helper.contains_key("provider"), "{helper:?}");
        assert_eq!(helper["verbosity"], json!("low"));

        let entrant_turn = parameters_for_request(&params, "mistralai/mistral-large-4-0", false);
        assert_eq!(entrant_turn["provider"]["order"], json!(["mistral"]));
        // Background work on the ENTRANT model (compaction extending the chat) keeps the pin.
        let entrant_compaction =
            parameters_for_request(&params, "mistralai/mistral-large-4-0", true);
        assert_eq!(entrant_compaction["provider"]["order"], json!(["mistral"]));
        // The fast model as the session's own turn (someone benchmarking it) keeps the pin.
        let fast_turn = parameters_for_request(&params, OPENROUTER_DEFAULT_FAST_MODEL, false);
        assert!(fast_turn.contains_key("provider"));
    }

    fn models_listing() -> Value {
        json!({"data": [
            {"id": "vendor/new-model-1m", "context_length": 1048576},
            {"id": "vendor/small-model", "context_length": 32768},
            {"id": "vendor/no-window"},
            {"id": "vendor/zero-window", "context_length": 0},
        ]})
    }

    fn provider_with_listing(listing: &Value) -> OpenRouterProvider {
        let listing = ModelsListing::of(&parse_models_listing(listing).unwrap());
        OpenRouterProvider {
            api_client: ApiClient::new_with_tls(
                "http://127.0.0.1:9".to_string(),
                AuthMethod::BearerToken("test".to_string()),
                None,
            )
            .unwrap(),
            supports_streaming: true,
            name: OPENROUTER_PROVIDER_NAME.to_string(),
            configured_parameters: None,
            listing: tokio::sync::OnceCell::new_with(Some(listing)),
            resolved_windows: std::sync::Mutex::new(HashMap::new()),
            effort_notices: Default::default(),
        }
    }

    #[test]
    fn the_models_listing_keeps_only_declared_windows() {
        let windows = context_windows_of(&parse_models_listing(&models_listing()).unwrap());

        assert_eq!(
            windows,
            HashMap::from([
                ("vendor/new-model-1m".to_string(), 1_048_576),
                ("vendor/small-model".to_string(), 32_768),
            ])
        );
    }

    #[test]
    fn a_models_listing_error_is_an_error() {
        let err = parse_models_listing(&json!({"error": {"message": "bad key"}}))
            .err()
            .unwrap();
        assert!(err.to_string().contains("bad key"));
    }

    #[tokio::test]
    async fn an_unset_context_limit_comes_from_the_live_listing() {
        let provider = provider_with_listing(&models_listing());

        assert_eq!(
            provider
                .get_context_limit(&model_config("vendor/new-model-1m"))
                .await
                .unwrap(),
            1_048_576
        );
        assert_eq!(
            provider
                .fetch_model_info("vendor/small-model")
                .await
                .unwrap()
                .context_limit,
            32_768
        );
    }

    #[tokio::test]
    async fn an_explicit_context_limit_wins_over_the_listing() {
        let provider = provider_with_listing(&models_listing());
        let mut model = model_config("vendor/new-model-1m");
        model.context_limit = Some(200_000);

        assert_eq!(provider.get_context_limit(&model).await.unwrap(), 200_000);
    }

    #[tokio::test]
    async fn a_model_the_listing_does_not_size_runs_on_the_default() {
        let provider = provider_with_listing(&models_listing());

        for model in ["vendor/no-window", "vendor/zero-window", "vendor/absent"] {
            assert_eq!(
                provider
                    .get_context_limit(&model_config(model))
                    .await
                    .unwrap(),
                goose_providers::model::DEFAULT_CONTEXT_LIMIT,
                "{model}"
            );
        }
    }

    /// The request goose actually sends in a tool loop: the turn-context block rides a trailing
    /// user message, and the moving breakpoint lands on the tool result before it.
    #[test]
    fn a_tool_loop_request_breaks_on_the_tool_result_not_the_turn_context() {
        let turn_context = "<turn-context>\n<current-time>2026-10-01 20:49</current-time>\n\
                            <working-directory>/tmp</working-directory>\n</turn-context>";
        let tool_result = |id: &str, text: &str| {
            Message::user().with_tool_response(
                id,
                Ok(rmcp::model::CallToolResult::success(vec![
                    rmcp::model::Content::text(text),
                ])),
            )
        };
        let messages = vec![
            Message::user().with_text("cat the files"),
            Message::assistant()
                .with_tool_request("c1", Ok(rmcp::model::CallToolRequestParams::new("shell"))),
            tool_result("c1", "part one"),
            Message::assistant()
                .with_tool_request("c2", Ok(rmcp::model::CallToolRequestParams::new("shell"))),
            tool_result("c2", "part two").with_text(turn_context),
        ];
        let mut payload = create_request(
            &model_config("anthropic/claude-haiku-4.5"),
            "system prompt",
            &messages,
            &[],
            &ImageFormat::OpenAi,
            true,
        )
        .unwrap();
        apply_anthropic_cache_breakpoints(&mut payload);

        let marked: Vec<(String, String)> = payload["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| {
                let part = m["content"]
                    .as_array()?
                    .iter()
                    .find(|p| p.get("cache_control").is_some())?;
                Some((
                    m["role"].as_str()?.to_string(),
                    part["text"].as_str()?.to_string(),
                ))
            })
            .collect();
        assert_eq!(
            marked,
            vec![
                ("system".to_string(), "system prompt".to_string()),
                ("tool".to_string(), "part one".to_string()),
                ("tool".to_string(), "part two".to_string()),
            ]
        );
        let last = payload["messages"].as_array().unwrap().last().unwrap();
        assert_eq!(last["role"], json!("user"));
        assert_eq!(last["content"], json!(turn_context));
    }

    fn block_at(minute: u32) -> String {
        format!(
            "<turn-context>\n<current-time>2026-10-02 14:{minute:02}:00</current-time>\n\
             <working-directory>/w</working-directory>\n\
             context: {} of 1,050,000 tokens used (0%)\n</turn-context>",
            1_000 + 100 * minute
        )
    }

    fn shell_step(id: &str, output: &str) -> [Message; 2] {
        [
            Message::assistant().with_tool_request(
                id,
                Ok(rmcp::model::CallToolRequestParams::new("developer__shell")),
            ),
            Message::user().with_tool_response(
                id,
                Ok(rmcp::model::CallToolResult::success(vec![
                    rmcp::model::Content::text(output),
                ])),
            ),
        ]
    }

    /// The bodies OpenRouter is sent for a question and six shell steps — the measured probe —
    /// with each request's block kept in the history the way the chat keeps it (`keep`), or only
    /// on the newest request (the request shape before the blocks were kept).
    fn tool_loop_requests(model: &str, keep: bool) -> Vec<Value> {
        let mut stored = vec![Message::user().with_text("Run six shell commands, then say DONE.")];
        let mut requests = Vec::new();
        for step in 0..=6u32 {
            if step > 0 {
                stored.extend(shell_step(&format!("c{step}"), &format!("output {step}")));
            }
            let block = Message::user().with_text(block_at(step)).agent_only();
            let sent = if keep {
                stored.push(block);
                stored.clone()
            } else {
                let mut sent = stored.clone();
                sent.push(block);
                sent
            };
            let fixed = crate::agents::moim::fixed_for_request(
                crate::conversation::Conversation::new_unvalidated(sent),
            )
            .expect("a tool loop needs no unexpected fixes");
            let messages =
                crate::agents::reply_parts::messages_for_provider(fixed.messages(), false);
            let mut payload = create_request(
                &model_config(model),
                "system prompt",
                messages.messages(),
                &[],
                &ImageFormat::OpenAi,
                true,
            )
            .unwrap();
            if supports_cache_control(&model_config(model)) {
                apply_anthropic_cache_breakpoints(&mut payload);
            }
            requests.push(payload);
        }
        requests
    }

    /// The messages as the upstream caches them: a string content is one text part, and the
    /// breakpoints are request markers, not content.
    fn cached_form(payload: &Value) -> Vec<Value> {
        payload["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                let mut m = m.clone();
                let parts = match m["content"].take() {
                    Value::String(text) => vec![json!({"type": "text", "text": text})],
                    Value::Array(parts) => parts
                        .into_iter()
                        .map(|mut p| {
                            if let Some(p) = p.as_object_mut() {
                                p.remove("cache_control");
                            }
                            p
                        })
                        .collect(),
                    other => vec![other],
                };
                m["content"] = json!(parts);
                m
            })
            .collect()
    }

    fn extends_previous(requests: &[Value], form: impl Fn(&Value) -> Vec<Value>) -> Vec<bool> {
        requests
            .windows(2)
            .map(|pair| {
                let (before, after) = (form(&pair[0]), form(&pair[1]));
                after.len() > before.len() && after[..before.len()] == before[..]
            })
            .collect()
    }

    /// The defect measured 2026-10-02 (openai/gpt-6-luna through OpenRouter, 0 cached tokens on
    /// every call): each request must contain the previous one unchanged as its prefix, because
    /// OpenAI's cache reads only an exact earlier prompt. NEGATIVE CONTROL: with the block on the
    /// newest request only, no request extends the one before it.
    #[test]
    fn every_tool_loop_request_extends_the_previous_one_byte_for_byte() {
        let exact = |p: &Value| p["messages"].as_array().unwrap().clone();
        let kept = tool_loop_requests("openai/gpt-6-luna", true);
        assert_eq!(extends_previous(&kept, exact), vec![true; 6]);
        assert_eq!(kept[0]["messages"][1]["role"], json!("user"));
        assert_eq!(
            kept[0]["messages"][1]["content"],
            json!(format!(
                "Run six shell commands, then say DONE.\n{}",
                block_at(0)
            ))
        );
        let last = kept[6]["messages"].as_array().unwrap();
        assert_eq!(
            last.iter()
                .filter(|m| m["content"]
                    .as_str()
                    .is_some_and(|c| c.contains("<turn-context>")))
                .count(),
            7,
            "every block the loop was sent is still in the newest request"
        );

        let dropped = tool_loop_requests("openai/gpt-6-luna", false);
        assert_eq!(extends_previous(&dropped, exact), vec![false; 6]);
    }

    /// The Anthropic breakpoints keep working over the kept blocks: every request extends the
    /// previous one as the upstream caches it, and its second breakpoint sits where the previous
    /// request put its moving one, so the history is read back from that entry.
    #[test]
    fn claude_requests_extend_the_previous_one_and_read_back_its_breakpoint() {
        let requests = tool_loop_requests("anthropic/claude-haiku-4.5", true);
        assert_eq!(extends_previous(&requests, cached_form), vec![true; 6]);

        let marked = |payload: &Value| -> Vec<usize> {
            payload["messages"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    m["role"] != json!("system")
                        && m["content"].as_array().is_some_and(|parts| {
                            parts.iter().any(|p| p.get("cache_control").is_some())
                        })
                })
                .map(|(i, _)| i)
                .collect()
        };
        for pair in requests.windows(2) {
            let moving = *marked(&pair[0]).last().unwrap();
            assert!(
                marked(&pair[1]).contains(&moving),
                "{:?} vs {:?}",
                marked(&pair[0]),
                marked(&pair[1])
            );
        }
        for payload in &requests {
            for m in payload["messages"].as_array().unwrap() {
                for part in m["content"].as_array().into_iter().flatten() {
                    let text = part["text"].as_str().unwrap_or_default();
                    assert!(
                        part.get("cache_control").is_none() || !text.contains("<turn-context>"),
                        "a breakpoint never sits on a block: {part}"
                    );
                }
            }
        }
    }

    /// The bench harness bills an OpenRouter run per generation id (bench_cost.py); that id reaches
    /// it only through this line, for completed and dropped streams alike.
    #[tokio::test]
    async fn every_streamed_call_writes_its_generation_id_to_the_telemetry_file() {
        use futures::StreamExt;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let gen_id = "gen-1790911486-OyqP2p7UE9dNJc1weFAo";
        let chunk = |delta: Value, finish: Value, usage: Option<Value>| {
            let mut c = json!({"id": gen_id, "object": "chat.completion.chunk", "created": 1,
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
            .and(path("/api/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;
        let mut provider = provider_with_listing(&models_listing());
        provider.api_client = ApiClient::new_with_tls(
            server.uri(),
            AuthMethod::BearerToken("test".to_string()),
            None,
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let telemetry = dir.path().join("telemetry.jsonl");
        let _guard = env_lock::lock_env([(
            "GOOSE_SWARM_TELEMETRY_FILE",
            Some(telemetry.to_str().unwrap()),
        )]);
        let model = model_config("deepseek/deepseek-v4.1-flash");
        let messages = [Message::user().with_text("hi")];

        let mut stream = provider
            .stream(&model, "system", &messages, &[])
            .await
            .unwrap();
        while let Some(item) = stream.next().await {
            item.unwrap();
        }
        drop(stream);
        let mut stream = provider
            .stream(&model, "system", &messages, &[])
            .await
            .unwrap();
        stream.next().await.unwrap().unwrap();
        drop(stream);

        let lines: Vec<Value> = std::fs::read_to_string(&telemetry)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0]["response_id"], json!(gen_id));
        assert_eq!(lines[0]["ended"], json!("completed"));
        assert_eq!(lines[0]["prompt_tokens"], json!(2527));
        assert_eq!(lines[1]["response_id"], json!(gen_id));
        assert_eq!(lines[1]["ended"], json!("incomplete"));
        assert_eq!(lines[1]["usage"], json!(false));
    }

    /// Entries shaped as OpenRouter's live `/api/v1/models` serves them (2026-10-03).
    fn modality_listing() -> Value {
        json!({"data": [
            {"id": "inclusionai/ling-3.1-flash", "context_length": 262144,
             "architecture": {"modality": "text->text", "input_modalities": ["text"],
                              "output_modalities": ["text"]}},
            {"id": "anthropic/claude-sonnet-4", "context_length": 1000000,
             "architecture": {"modality": "text+image+file->text",
                              "input_modalities": ["image", "text", "file"],
                              "output_modalities": ["text"]}},
            {"id": "vendor/no-architecture", "context_length": 32768},
        ]})
    }

    /// The run that lost its session: `read_image` returned st3.png, and the request carried it.
    fn screenshot_turn() -> Vec<Message> {
        vec![
            Message::user().with_text("Check the page renders"),
            Message::assistant().with_tool_request(
                "c1",
                Ok(rmcp::model::CallToolRequestParams::new("read_image")),
            ),
            Message::user().with_tool_response(
                "c1",
                Ok(rmcp::model::CallToolResult::success(vec![
                    rmcp::model::Content::text(
                        "Loaded image from /w/st3.png (475916 bytes, image/png, 1440x3233).",
                    ),
                    rmcp::model::Content::image("aW1hZ2VkYXRh", "image/png"),
                ])),
            ),
        ]
    }

    fn image_parts(payload: &Value) -> usize {
        payload["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_array())
            .flatten()
            .filter(|part| part["type"] == json!("image_url"))
            .count()
    }

    #[test]
    fn the_listing_declares_each_models_image_input() {
        let listing = ModelsListing::of(&parse_models_listing(&modality_listing()).unwrap());

        assert_eq!(
            listing.image_inputs,
            HashMap::from([
                (
                    "inclusionai/ling-3.1-flash".to_string(),
                    ImageInput::TextOnly
                ),
                ("anthropic/claude-sonnet-4".to_string(), ImageInput::Reads),
                ("vendor/no-architecture".to_string(), ImageInput::Undeclared),
            ])
        );
        assert_eq!(
            listing.windows.get("inclusionai/ling-3.1-flash"),
            Some(&262_144)
        );
    }

    #[tokio::test]
    async fn a_text_only_model_is_told_the_image_was_omitted_and_never_sent_it() {
        let provider = provider_with_listing(&modality_listing());

        let (payload, notice) = provider
            .chat_request(
                &model_config("inclusionai/ling-3.1-flash"),
                "system prompt",
                &screenshot_turn(),
                &[],
            )
            .await
            .unwrap();

        assert_eq!(image_parts(&payload), 0, "{payload}");
        let tool = payload["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == json!("tool"))
            .unwrap();
        assert_eq!(
            tool["content"],
            json!(
                "Loaded image from /w/st3.png (475916 bytes, image/png, 1440x3233). \
                 [image from read_image (image/png) not sent: this model reads text only]"
            )
        );
        assert_eq!(
            notice.as_deref(),
            Some("ling-3.1-flash reads text only — the image from read_image (image/png) was not sent")
        );
    }

    #[tokio::test]
    async fn a_vision_model_and_an_undeclared_one_are_sent_the_image_unchanged() {
        let provider = provider_with_listing(&modality_listing());

        for model in ["anthropic/claude-sonnet-4", "vendor/no-architecture"] {
            let (payload, notice) = provider
                .chat_request(
                    &model_config(model),
                    "system prompt",
                    &screenshot_turn(),
                    &[],
                )
                .await
                .unwrap();
            let formatted = create_request(
                &model_config(model),
                "system prompt",
                &screenshot_turn(),
                &[],
                &ImageFormat::OpenAi,
                true,
            )
            .unwrap();

            assert_eq!(payload, formatted, "{model}");
            assert_eq!(image_parts(&payload), 1, "{model}");
            assert_eq!(notice, None, "{model}");
        }
    }
}

#[cfg(test)]
mod model_field_tests {
    use super::*;
    use crate::model_fields::{self, FieldValues};
    use goose_providers::thinking::ThinkingEffort;

    /// Entries shaped as OpenRouter's live `/api/v1/models` served them on 2026-10-04.
    fn listing() -> Value {
        json!({"data": [
            {"id": "deepseek/deepseek-v4.1-flash", "context_length": 1048576,
             "supported_parameters": ["frequency_penalty", "include_reasoning", "max_tokens",
                                      "min_p", "reasoning", "reasoning_effort", "temperature",
                                      "tools", "top_k", "top_p"],
             "default_parameters": {},
             "reasoning": {"mandatory": false, "default_enabled": true,
                           "supported_efforts": ["max", "high", "low"], "default_effort": "high"}},
            {"id": "anthropic/claude-sonnet-4.5", "context_length": 1000000,
             "supported_parameters": ["include_reasoning", "max_tokens", "reasoning",
                                      "temperature", "tools", "top_k", "top_p"],
             "default_parameters": {"temperature": 1, "top_p": 1, "top_k": null},
             "reasoning": {"mandatory": false}},
            {"id": "vendor/plain", "context_length": 32768,
             "supported_parameters": ["tools"]},
            // as served on 2026-10-20
            {"id": "minimax/minimax-m3.1-flash:free", "context_length": 1000000,
             "supported_parameters": ["include_reasoning", "max_tokens", "reasoning", "tools"],
             "reasoning": {"mandatory": true, "default_enabled": true,
                           "supported_efforts": ["max", "xhigh", "high", "medium", "low"],
                           "default_effort": "max"}}
        ]})
    }

    fn provider(host: &str) -> OpenRouterProvider {
        let listing = ModelsListing::of(&parse_models_listing(&listing()).unwrap());
        OpenRouterProvider {
            api_client: ApiClient::new_with_tls(
                host.to_string(),
                AuthMethod::BearerToken("test".to_string()),
                None,
            )
            .unwrap(),
            supports_streaming: true,
            name: OPENROUTER_PROVIDER_NAME.to_string(),
            configured_parameters: None,
            listing: tokio::sync::OnceCell::new_with(Some(listing)),
            resolved_windows: std::sync::Mutex::new(HashMap::new()),
            effort_notices: Default::default(),
        }
    }

    #[tokio::test]
    async fn the_listing_declares_each_models_parameters_and_reasoning_block() {
        let provider = provider("http://127.0.0.1:9");

        let deepseek = provider
            .declared_model_parameters("deepseek/deepseek-v4.1-flash")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(deepseek.effort_levels, vec!["max", "high", "low"]);
        assert_eq!(deepseek.default_effort.as_deref(), Some("high"));
        assert!(!deepseek.reasoning_mandatory);
        assert!(deepseek.supported_parameters.contains(&"min_p".to_string()));

        let claude = provider
            .declared_model_parameters("anthropic/claude-sonnet-4.5")
            .await
            .unwrap()
            .unwrap();
        assert!(claude.effort_levels.is_empty());
        assert_eq!(
            claude.default_parameters,
            std::collections::BTreeMap::from([
                ("temperature".to_string(), json!(1)),
                ("top_p".to_string(), json!(1)),
            ])
        );

        assert_eq!(
            provider
                .declared_model_parameters("vendor/absent")
                .await
                .unwrap(),
            None
        );
    }

    /// The whole path a saved effort takes: the values become request params, and the body
    /// OpenRouter receives carries `reasoning: {effort}` — over a global thinking-effort default.
    #[tokio::test]
    async fn a_saved_effort_reaches_the_wire_as_reasoning_effort() {
        use futures::StreamExt;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = [
            format!(
                "data: {}\n\n",
                json!({"id": "gen-1", "object": "chat.completion.chunk", "created": 1,
                       "model": "deepseek/deepseek-v4.1-flash",
                       "choices": [{"index": 0, "delta": {"role": "assistant", "content": "ok"},
                                    "finish_reason": "stop"}]})
            ),
            "data: [DONE]\n\n".to_string(),
        ]
        .concat();
        Mock::given(method("POST"))
            .and(path("/api/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;
        let provider = provider(&server.uri());
        let dir = tempfile::tempdir().unwrap();
        let telemetry = dir.path().join("telemetry.jsonl");
        let _guard = env_lock::lock_env([(
            "GOOSE_SWARM_TELEMETRY_FILE",
            Some(telemetry.to_str().unwrap()),
        )]);

        let saved = FieldValues::from([
            ("effort".to_string(), json!("low")),
            ("top_k".to_string(), json!(40)),
        ]);
        let mut model = ModelConfig::new("deepseek/deepseek-v4.1-flash")
            .with_merged_request_params(model_fields::request_params(
                OPENROUTER_PROVIDER_NAME,
                &saved,
            ))
            .with_default_thinking_effort(Some(ThinkingEffort::High));
        model.reasoning = Some(true);

        let mut stream = provider
            .stream(&model, "system", &[Message::user().with_text("hi")], &[])
            .await
            .unwrap();
        while let Some(item) = stream.next().await {
            item.unwrap();
        }

        let requests = server.received_requests().await.unwrap();
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(sent["reasoning"], json!({"effort": "low"}));
        assert_eq!(sent["top_k"], json!(40));
        assert!(sent.get("reasoning_effort").is_none(), "{sent}");
        assert!(sent.get("thinking_effort").is_none(), "{sent}");
    }

    /// One streamed turn of `config` through `provider` against a mock OpenRouter (the models
    /// listing answers 500 unless the provider already holds one): the body's `reasoning` and the
    /// chat notices the turn carried.
    async fn turn_on_the_wire(
        provider: impl FnOnce(&str) -> OpenRouterProvider,
        config: &ModelConfig,
    ) -> (Value, Vec<String>) {
        use futures::StreamExt;
        use goose_providers::conversation::message::{MessageContent, SystemNotificationType};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = [
            format!(
                "data: {}\n\n",
                json!({"id": "gen-1", "object": "chat.completion.chunk", "created": 1,
                       "model": config.model_name,
                       "choices": [{"index": 0, "delta": {"role": "assistant", "content": "ok"},
                                    "finish_reason": "stop"}]})
            ),
            "data: [DONE]\n\n".to_string(),
        ]
        .concat();
        Mock::given(method("POST"))
            .and(path("/api/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        // Every streamed call appends to the telemetry file: hold the env lock with a file of our
        // own, or these turns land in a parallel test's file (every_streamed_call_writes_...).
        let dir = tempfile::tempdir().unwrap();
        let telemetry = dir.path().join("telemetry.jsonl");
        let _guard = env_lock::lock_env([(
            "GOOSE_SWARM_TELEMETRY_FILE",
            Some(telemetry.to_str().unwrap()),
        )]);
        let provider = provider(&server.uri());
        let mut notices = Vec::new();
        for _ in 0..2 {
            let mut stream = provider
                .stream(config, "system", &[Message::user().with_text("hi")], &[])
                .await
                .unwrap();
            while let Some(item) = stream.next().await {
                if let (Some(message), _) = item.unwrap() {
                    for content in &message.content {
                        if let MessageContent::SystemNotification(n) = content {
                            if n.notification_type == SystemNotificationType::InlineMessage {
                                notices.push(n.msg.clone());
                            }
                        }
                    }
                }
            }
        }
        let requests = server.received_requests().await.unwrap();
        let chats: Vec<_> = requests
            .iter()
            .filter(|r| r.url.path() == "/api/v1/chat/completions")
            .collect();
        assert_eq!(chats.len(), 2, "both turns reached OpenRouter");
        let sent: Value = serde_json::from_slice(&chats[1].body).unwrap();
        (
            sent.get("reasoning").cloned().unwrap_or(Value::Null),
            notices,
        )
    }

    fn reasoning_config(model: &str, effort: ThinkingEffort) -> ModelConfig {
        let mut config = ModelConfig::new(model).with_thinking_effort(effort);
        config.reasoning = Some(true);
        config
    }

    /// What a turn on `model` at the global `effort` puts in the body's `reasoning`.
    async fn reasoning_on_the_wire(model: &str, effort: ThinkingEffort) -> Value {
        turn_on_the_wire(provider, &reasoning_config(model, effort))
            .await
            .0
    }

    /// MiniMax M3.1 Flash (2026-10-20) answers `effort: none` with 400 "Reasoning is mandatory for
    /// this endpoint and cannot be disabled": an Off session there failed every turn.
    #[tokio::test]
    async fn off_on_a_mandatory_reasoning_model_reaches_the_wire_as_its_cheapest_level() {
        let minimax = "minimax/minimax-m3.1-flash:free";
        assert_eq!(
            reasoning_on_the_wire(minimax, ThinkingEffort::Off).await,
            json!({"effort": "low"})
        );
        assert_eq!(
            reasoning_on_the_wire(minimax, ThinkingEffort::Max).await,
            json!({"effort": "max"})
        );
        assert_eq!(
            reasoning_on_the_wire(minimax, ThinkingEffort::Medium).await,
            json!({"effort": "medium"})
        );
        // a routing variant is looked up by its listed id
        assert_eq!(
            reasoning_on_the_wire("minimax/minimax-m3.1-flash:free:nitro", ThinkingEffort::Off)
                .await,
            json!({"effort": "low"})
        );
        // a model that can switch reasoning off still gets none, and an unlisted model is unchanged
        let deepseek = "deepseek/deepseek-v4.1-flash";
        assert_eq!(
            reasoning_on_the_wire(deepseek, ThinkingEffort::Off).await,
            json!({"effort": "none"})
        );
        assert_eq!(
            reasoning_on_the_wire(deepseek, ThinkingEffort::Max).await,
            json!({"effort": "max"})
        );
        assert_eq!(
            reasoning_on_the_wire("qwen/unlisted", ThinkingEffort::Off).await,
            json!({"effort": "none"})
        );
    }

    #[tokio::test]
    async fn the_chat_is_told_once_that_off_cannot_switch_reasoning_off() {
        let (_, notices) = turn_on_the_wire(
            provider,
            &reasoning_config("minimax/minimax-m3.1-flash:free", ThinkingEffort::Off),
        )
        .await;
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(
            notices[0].contains("cannot switch reasoning off"),
            "{}",
            notices[0]
        );
        assert!(notices[0].contains("lowest level (low)"), "{}", notices[0]);
        // a model that honours Off, and a Max upgrade, say nothing
        for (model, effort) in [
            ("deepseek/deepseek-v4.1-flash", ThinkingEffort::Off),
            ("minimax/minimax-m3.1-flash:free", ThinkingEffort::Max),
        ] {
            let (_, notices) = turn_on_the_wire(provider, &reasoning_config(model, effort)).await;
            assert!(notices.is_empty(), "{model}: {notices:?}");
        }
    }

    #[tokio::test]
    async fn the_global_max_never_raises_the_users_own_effort() {
        // OPENROUTER_PARAMETERS {"reasoning_effort": "high"} lands in the request params
        let mut config = reasoning_config("deepseek/deepseek-v4.1-flash", ThinkingEffort::Max);
        config
            .request_params
            .get_or_insert_with(HashMap::new)
            .insert("reasoning_effort".to_string(), json!("high"));
        assert_eq!(
            turn_on_the_wire(provider, &config).await.0,
            json!({"effort": "high"})
        );
        // ...but the user's own none on a mandatory model is still fixed
        let mut config = reasoning_config("minimax/minimax-m3.1-flash:free", ThinkingEffort::Max);
        config
            .request_params
            .get_or_insert_with(HashMap::new)
            .insert("reasoning".to_string(), json!({"effort": "none"}));
        assert_eq!(
            turn_on_the_wire(provider, &config).await.0,
            json!({"effort": "low"})
        );
    }

    #[tokio::test]
    async fn an_unreadable_listing_sends_the_turn_as_configured() {
        let unread = |host: &str| {
            let mut provider = provider(host);
            provider.listing = tokio::sync::OnceCell::new();
            provider
        };
        let (reasoning, notices) = turn_on_the_wire(
            unread,
            &reasoning_config("minimax/minimax-m3.1-flash:free", ThinkingEffort::Off),
        )
        .await;
        assert_eq!(reasoning, json!({"effort": "none"}));
        assert!(notices.is_empty(), "{notices:?}");
    }
}

#[cfg(test)]
mod context_compression_tests {
    use super::*;

    fn off() -> Value {
        json!({"id": CONTEXT_COMPRESSION_PLUGIN, "enabled": false})
    }

    #[test]
    fn compression_is_off_by_default_and_the_request_says_so() {
        let mut payload = json!({"model": "x-ai/grok-4.7", "messages": []});
        assert!(!apply_context_compression(&mut payload, false));
        assert_eq!(payload.get("transforms"), None, "{payload}");
        assert_eq!(payload["plugins"], json!([off()]));
    }

    #[test]
    fn the_owners_opt_in_sends_middle_out() {
        let mut payload = json!({"model": "x-ai/grok-4.7", "messages": []});
        assert!(apply_context_compression(&mut payload, true));
        assert_eq!(payload["transforms"], json!(["middle-out"]));
        assert_eq!(payload.get("plugins"), None, "{payload}");
    }

    #[test]
    fn the_owners_other_plugins_ride_beside_the_off_switch() {
        let mut payload = json!({"plugins": [{"id": "web"}]});
        assert!(!apply_context_compression(&mut payload, false));
        assert_eq!(payload["plugins"], json!([{"id": "web"}, off()]));
    }

    #[test]
    fn compression_named_in_the_parameters_is_left_as_written() {
        let mut on = json!({"plugins": [{"id": CONTEXT_COMPRESSION_PLUGIN}]});
        let before = on.clone();
        assert!(apply_context_compression(&mut on, false));
        assert_eq!(on, before);

        let mut none = json!({"transforms": []});
        assert!(!apply_context_compression(&mut none, true));
        assert_eq!(none, json!({"transforms": []}));

        let mut legacy = json!({"transforms": ["middle-out"]});
        assert!(apply_context_compression(&mut legacy, false));
        assert_eq!(legacy, json!({"transforms": ["middle-out"]}));
    }

    #[test]
    fn middle_out_is_on_only_when_the_owner_says_true() {
        let _env = env_lock::lock_env([(OPENROUTER_MIDDLE_OUT_CONFIG_KEY, None::<&str>)]);
        let dir = tempfile::tempdir().unwrap();
        let read = |yaml: &str| {
            let config_path = dir.path().join("config.yaml");
            std::fs::write(&config_path, yaml).unwrap();
            let config = crate::config::Config::new_with_file_secrets(
                &config_path,
                dir.path().join("secrets.yaml"),
            )
            .unwrap();
            middle_out_enabled(&config)
        };
        assert!(!read("OPENROUTER_HOST: https://openrouter.ai\n"));
        assert!(read("OPENROUTER_MIDDLE_OUT: true\n"));
        assert!(!read("OPENROUTER_MIDDLE_OUT: false\n"));
        assert!(!read("OPENROUTER_MIDDLE_OUT: sometimes\n"));
    }

    /// The body OpenRouter receives on a default turn: no `transforms`, compression switched off.
    #[tokio::test]
    async fn a_default_turn_reaches_the_wire_without_middle_out() {
        use futures::StreamExt;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = [
            format!(
                "data: {}\n\n",
                json!({"id": "gen-1", "object": "chat.completion.chunk", "created": 1,
                       "model": "vendor/plain",
                       "choices": [{"index": 0, "delta": {"role": "assistant", "content": "ok"},
                                    "finish_reason": "stop"}]})
            ),
            "data: [DONE]\n\n".to_string(),
        ]
        .concat();
        Mock::given(method("POST"))
            .and(path("/api/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let telemetry = dir.path().join("telemetry.jsonl");
        let _guard = env_lock::lock_env([
            (
                "GOOSE_SWARM_TELEMETRY_FILE",
                Some(telemetry.to_str().unwrap()),
            ),
            (OPENROUTER_MIDDLE_OUT_CONFIG_KEY, Some("false")),
        ]);
        let provider = OpenRouterProvider {
            api_client: ApiClient::new_with_tls(
                server.uri(),
                AuthMethod::BearerToken("test".to_string()),
                None,
            )
            .unwrap(),
            supports_streaming: true,
            name: OPENROUTER_PROVIDER_NAME.to_string(),
            configured_parameters: Some(HashMap::from([(
                "plugins".to_string(),
                json!([{"id": "web"}]),
            )])),
            listing: tokio::sync::OnceCell::new(),
            resolved_windows: std::sync::Mutex::new(HashMap::new()),
            effort_notices: Default::default(),
        };
        let mut stream = provider
            .stream(
                &ModelConfig::new("vendor/plain"),
                "system",
                &[Message::user().with_text("hi")],
                &[],
            )
            .await
            .unwrap();
        while let Some(item) = stream.next().await {
            item.unwrap();
        }
        let requests = server.received_requests().await.unwrap();
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(sent.get("transforms"), None, "{sent}");
        assert_eq!(sent["plugins"], json!([{"id": "web"}, off()]));
    }
}

#[cfg(test)]
mod compression_watch_tests {
    use super::*;
    use crate::background_work::{self, BackgroundWorkKind};
    use futures::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A turn billed 40 prompt tokens, whatever was sent.
    async fn billed_40() -> MockServer {
        let server = MockServer::start().await;
        let chunk = json!({"id": "gen-1", "object": "chat.completion.chunk", "created": 1,
                           "model": "vendor/plain",
                           "choices": [{"index": 0, "delta": {"role": "assistant", "content": "ok"},
                                        "finish_reason": "stop"}],
                           "usage": {"prompt_tokens": 40, "completion_tokens": 1,
                                     "total_tokens": 41}});
        Mock::given(method("POST"))
            .and(path("/api/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!("data: {chunk}\n\ndata: [DONE]\n\n"),
                "text/event-stream",
            ))
            .mount(&server)
            .await;
        server
    }

    fn provider(host: &str) -> OpenRouterProvider {
        OpenRouterProvider {
            api_client: ApiClient::new_with_tls(
                host.to_string(),
                AuthMethod::BearerToken("test".to_string()),
                None,
            )
            .unwrap(),
            supports_streaming: true,
            name: OPENROUTER_PROVIDER_NAME.to_string(),
            configured_parameters: None,
            listing: tokio::sync::OnceCell::new(),
            resolved_windows: std::sync::Mutex::new(HashMap::new()),
            effort_notices: Default::default(),
        }
    }

    async fn drain(stream: MessageStream) {
        let mut stream = stream;
        while let Some(item) = stream.next().await {
            item.unwrap();
        }
    }

    /// The compaction's own summary call is sized by the compaction, not by the chat's window: a
    /// small reported prompt there says nothing about the chat, so it is never watched.
    #[tokio::test]
    async fn a_background_call_is_never_read_as_a_compressed_chat() {
        let server = billed_40().await;
        let dir = tempfile::tempdir().unwrap();
        let telemetry = dir.path().join("telemetry.jsonl");
        let _guard = env_lock::lock_env([
            (
                "GOOSE_SWARM_TELEMETRY_FILE",
                Some(telemetry.to_str().unwrap()),
            ),
            (OPENROUTER_MIDDLE_OUT_CONFIG_KEY, Some("true")),
        ]);
        let provider = provider(&server.uri());
        let model = ModelConfig::new("vendor/plain");
        let history = [Message::user().with_text("the batch moved forward. ".repeat(2_000))];

        let foreground = crate::session_context::with_session_id(
            Some("watch-foreground".to_string()),
            provider.stream(&model, "system", &history, &[]),
        )
        .await
        .unwrap();
        drain(foreground).await;
        assert!(
            crate::context_mgmt::compressed_prompt("watch-foreground").is_some(),
            "the chat's own call billed 40 of ~10k is compressed"
        );

        let background = background_work::run(
            BackgroundWorkKind::Compaction,
            "watch-background",
            provider.stream(&model, "system", &history, &[]),
        )
        .await
        .unwrap();
        drain(background).await;
        assert_eq!(
            crate::context_mgmt::compressed_prompt("watch-background"),
            None
        );
    }
}
