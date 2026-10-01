use anyhow::{bail, Result};
use async_trait::async_trait;
use futures::future::BoxFuture;
use goose_providers::images::ImageFormat;
use serde_json::{json, Value};
use std::collections::HashMap;

use super::api_client::{ApiClient, AuthMethod};
use super::base::{
    model_info_for_provider_model, ConfigKey, MessageStream, ModelInfo, Provider, ProviderDef,
    ProviderMetadata,
};
use super::openai_compatible::{handle_status, stream_openai_compat};
use super::retry::ProviderRetry;
use crate::conversation::message::Message;
use crate::providers::formats::openrouter as openrouter_format;
use goose_providers::errors::ProviderError;
use goose_providers::formats::anthropic_cache::apply_anthropic_cache_breakpoints;
use goose_providers::formats::openai::create_request;
use goose_providers::model::ModelConfig;
use goose_providers::request_log::{start_log, LoggerHandleExt};
use rmcp::model::Tool;

pub const OPENROUTER_PROVIDER_NAME: &str = "openrouter";
const OPENROUTER_PARAMETERS_CONFIG_KEY: &str = "OPENROUTER_PARAMETERS";
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
    context_windows: tokio::sync::OnceCell<HashMap<String, usize>>,
    #[serde(skip)]
    resolved_windows: std::sync::Mutex<HashMap<String, Option<usize>>>,
}

impl OpenRouterProvider {
    pub async fn from_env(
        tls_config: Option<crate::providers::api_client::TlsConfig>,
    ) -> Result<Self> {
        let config = crate::config::Config::global();
        let api_key: String = config.get_secret("OPENROUTER_API_KEY")?;
        let host: String = config
            .get_param("OPENROUTER_HOST")
            .unwrap_or_else(|_| "https://openrouter.ai".to_string());

        let configured_parameters = configured_openrouter_parameters()?;

        let auth = AuthMethod::BearerToken(api_key);
        let api_client = ApiClient::new_with_tls(host, auth, tls_config)?
            .with_request_builder(crate::session_context::session_id_request_builder())
            .with_header("HTTP-Referer", "https://goose-docs.ai")?
            .with_header("X-Title", "goose")?;

        Ok(Self {
            api_client,
            supports_streaming: true,
            name: OPENROUTER_PROVIDER_NAME.to_string(),
            configured_parameters,
            context_windows: tokio::sync::OnceCell::new(),
            resolved_windows: std::sync::Mutex::new(HashMap::new()),
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

    async fn context_windows(&self) -> Result<&HashMap<String, usize>, ProviderError> {
        self.context_windows
            .get_or_try_init(|| async {
                let listing = self.fetch_models_listing().await?;
                Ok(context_windows_of(&listing))
            })
            .await
    }

    /// The window OpenRouter's live models listing declares for `model_name` — `None`, after one
    /// warning per model, when the listing cannot be fetched or does not carry the model.
    async fn listed_context_window(&self, model_name: &str) -> Option<usize> {
        if let Some(resolved) = self
            .resolved_windows
            .lock()
            .ok()
            .and_then(|cache| cache.get(model_name).copied())
        {
            return resolved;
        }

        let window = match self.context_windows().await {
            Ok(windows) => {
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
                window
            }
            Err(err) => {
                tracing::warn!(
                    model = %model_name,
                    reason = %err,
                    "context_window_listing_failed: OpenRouter's models listing could not be \
                     read; the compaction guard and the per-turn context line run on the default"
                );
                None
            }
        };

        if let Ok(mut cache) = self.resolved_windows.lock() {
            cache.insert(model_name.to_string(), window);
        }
        window
    }
}

struct ListedModel {
    id: String,
    context_length: Option<usize>,
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

fn parse_openrouter_parameters(raw: Value) -> Result<HashMap<String, Value>> {
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

fn merge_openrouter_parameters(model: &mut ModelConfig, params: HashMap<String, Value>) {
    merge_request_params(&mut model.request_params, params);
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
        let _ = self.context_windows.set(context_windows_of(&listing));
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
            merge_openrouter_parameters(&mut merged_model, params.clone());
            &merged_model
        } else {
            model_config
        };

        let mut payload = create_request(
            model_config,
            system,
            messages,
            tools,
            &ImageFormat::OpenAi,
            true,
        )?;

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
        openrouter_format::apply_reasoning_config(&mut payload, model_config);

        if let Some(obj) = payload.as_object_mut() {
            obj.insert("transforms".to_string(), json!(["middle-out"]));
        }

        let mut log = start_log(model_config, &payload)?;

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

        stream_openai_compat(response, log)
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

    fn models_listing() -> Value {
        json!({"data": [
            {"id": "vendor/new-model-1m", "context_length": 1048576},
            {"id": "vendor/small-model", "context_length": 32768},
            {"id": "vendor/no-window"},
            {"id": "vendor/zero-window", "context_length": 0},
        ]})
    }

    fn provider_with_listing(listing: &Value) -> OpenRouterProvider {
        let windows = context_windows_of(&parse_models_listing(listing).unwrap());
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
            context_windows: tokio::sync::OnceCell::new_with(Some(windows)),
            resolved_windows: std::sync::Mutex::new(HashMap::new()),
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
}
