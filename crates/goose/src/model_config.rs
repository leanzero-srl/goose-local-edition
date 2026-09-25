use crate::config::{Config, ConfigError};
use crate::conversation::message::Message;
use crate::providers::base::Provider;
use anyhow::{anyhow, Result};
use goose_providers::conversation::token_usage::ProviderUsage;
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use goose_providers::thinking::ThinkingEffort;
use rmcp::model::Tool;
use serde_json::Value;
use std::collections::HashMap;

pub fn model_config_from_user_config(
    provider_name: &str,
    model_name: impl AsRef<str>,
) -> Result<ModelConfig> {
    let model = base_model_config_from_user_config(model_name.as_ref())?;
    materialize_model_config(provider_name, model)
}

pub fn model_config_from_user_config_with_session_settings(
    provider_name: &str,
    model_name: impl AsRef<str>,
    previous: Option<&ModelConfig>,
    request_params: Option<HashMap<String, Value>>,
    context_limit: Option<usize>,
) -> Result<ModelConfig> {
    let config = Config::global();
    let model = base_model_config_from_user_config(model_name.as_ref())?;
    let model = materialize_model_config_inner(model, provider_name, false)?
        .with_context_limit(context_limit)
        .with_inherited_session_settings_from(previous, request_params)
        .with_default_thinking_effort(config.get_goose_thinking_effort());

    Ok(model.with_canonical_limits(provider_name))
}

pub fn materialize_model_config(provider_name: &str, model: ModelConfig) -> Result<ModelConfig> {
    let model = materialize_model_config_inner(model, provider_name, true)?;
    Ok(model.with_canonical_limits(provider_name))
}

fn materialize_model_config_inner(
    mut model: ModelConfig,
    provider_name: &str,
    include_default_thinking_effort: bool,
) -> Result<ModelConfig> {
    let config = Config::global();

    if model.temperature.is_none() {
        model = model.with_temperature(get_goose_temperature(config)?);
    }

    if model.toolshim && model.toolshim_model.is_none() {
        model = model.with_toolshim_model(get_goose_toolshim_model(config)?);
    }

    model = model
        .with_default_context_limit(config.get_goose_context_limit()?)
        .with_default_max_tokens(config.get_goose_max_tokens()?);

    if include_default_thinking_effort {
        model = model.with_default_thinking_effort(config.get_goose_thinking_effort());
    }

    if provider_name == goose_providers::openai::OPEN_AI_PROVIDER_NAME {
        model = apply_openai_request_params(model);
    }

    Ok(model)
}

fn configured_fast_model_name() -> Option<String> {
    Config::global()
        .get_param::<String>("GOOSE_FAST_MODEL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Resolve the model config to use for lightweight "fast" tasks (session
/// naming, compaction, summarization). Resolution order:
///   1. `GOOSE_FAST_MODEL` (user override)
///   2. the provider's declared default fast model
///   3. the supplied `model_config` (i.e. the main model)
///
/// The resulting config is materialized against the same provider so it picks
/// up context limits, temperature, and other provider defaults.
pub async fn get_fast_model(
    provider_name: &str,
    model_config: &ModelConfig,
) -> Result<ModelConfig> {
    let fast_model_name = match configured_fast_model_name() {
        Some(name) => Some(name),
        None => provider_default_fast_model(provider_name).await,
    };

    match fast_model_name {
        Some(name) if name != model_config.model_name => {
            let cfg = model_config_from_user_config(provider_name, name)?;
            // K1 prep: a qwen-class fast model on LM Studio starts every generation inside an
            // open <think> block, and `ThinkingEffort::Off` provably never reaches the server
            // (reasoning_effort is emitted only for OpenAI-Responses models) — so an auxiliary
            // summarizer reasons at full effort, which defeats the whole point of routing
            // compaction to a small model. The assistant PREFILL that pre-closes the block is
            // the one mechanism verified to work; scoped to qwen-family ids so a non-thinking
            // fast model never gets stray think-tags in its output. Dormant until the operator
            // sets GOOSE_FAST_MODEL.
            let cfg = if cfg.model_name.to_lowercase().contains("qwen") {
                let mut params = std::collections::HashMap::new();
                params.insert(
                    goose_providers::formats::openai::PREFILL_ASSISTANT_KEY.to_string(),
                    serde_json::Value::String("<think>\n\n</think>\n\n".to_string()),
                );
                cfg.with_merged_request_params(params)
            } else {
                cfg
            };
            Ok(cfg)
        }
        _ => Ok(model_config.clone()),
    }
}

/// The config every helper call runs with — the fast model, reasoning off. A helper's answer is
/// a label, a title or a digest whose own prompt already says what to write; reasoning first buys
/// nothing and, on a local engine, competes with the agent's turn for the same decode.
pub async fn get_fast_model_without_reasoning(
    provider_name: &str,
    model_config: &ModelConfig,
) -> Result<ModelConfig> {
    Ok(get_fast_model(provider_name, model_config)
        .await?
        .with_thinking_effort(ThinkingEffort::Off))
}

/// Run a completion for a lightweight "fast" task (session naming, compaction,
/// summarization) using the provider's fast model, falling back to the supplied
/// main `model_config` if the fast model errors.
pub async fn complete_fast(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
) -> Result<(Message, ProviderUsage), ProviderError> {
    let fast_model_config = get_fast_model_without_reasoning(provider.get_name(), model_config)
        .await
        .map_err(|e| ProviderError::ExecutionError(e.to_string()))?;

    match crate::session_context::with_session_id(
        Some(session_id.to_string()),
        provider.complete(&fast_model_config, system, messages, tools),
    )
    .await
    {
        Ok(response) => Ok(response),
        Err(e) if fast_model_config.model_name != model_config.model_name => {
            tracing::warn!(
                "Fast model {} failed with error: {}. Falling back to main model {}",
                fast_model_config.model_name,
                e,
                model_config.model_name
            );
            let fallback_config = model_config
                .clone()
                .with_thinking_effort(ThinkingEffort::Off);
            crate::session_context::with_session_id(
                Some(session_id.to_string()),
                provider.complete(&fallback_config, system, messages, tools),
            )
            .await
        }
        Err(e) => Err(e),
    }
}

async fn provider_default_fast_model(provider_name: &str) -> Option<String> {
    if provider_name == goose_providers::openai::OPEN_AI_PROVIDER_NAME {
        return crate::providers::openai_def::live_fast_model();
    }

    crate::providers::get_from_registry(provider_name)
        .await
        .ok()
        .and_then(|entry| entry.metadata().fast_model.clone())
}

fn apply_openai_request_params(mut model: ModelConfig) -> ModelConfig {
    let config = Config::global();
    if let Some(store) = config.get_openai_store() {
        model = model.with_merged_request_params(HashMap::from([(
            "store".to_string(),
            serde_json::json!(store),
        )]));
    }
    model
}

fn base_model_config_from_user_config(model_name: &str) -> Result<ModelConfig> {
    let config = Config::global();
    let mut model = ModelConfig {
        model_name: model_name.to_string(),
        context_limit: None,
        temperature: get_goose_temperature(config)?,
        max_tokens: None,
        toolshim: get_goose_toolshim(config)?.unwrap_or(false),
        toolshim_model: get_goose_toolshim_model(config)?,
        request_params: None,
        reasoning: None,
    };
    model.normalize_effort_suffix();
    Ok(model)
}

fn get_goose_temperature(config: &Config) -> Result<Option<f32>> {
    match config.get_param::<f32>("GOOSE_TEMPERATURE") {
        Ok(temp) if temp < 0.0 => Err(anyhow!(
            "Value for 'GOOSE_TEMPERATURE' is out of valid range: {temp}"
        )),
        Ok(temp) => Ok(Some(temp)),
        Err(ConfigError::NotFound(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn get_goose_toolshim(config: &Config) -> Result<Option<bool>> {
    match config.get_param::<serde_yaml::Value>("GOOSE_TOOLSHIM") {
        Ok(value) => parse_yaml_bool_config("GOOSE_TOOLSHIM", value).map(Some),
        Err(ConfigError::NotFound(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Resolve the global toolshim setting, defaulting to false when unset.
pub fn global_toolshim() -> bool {
    get_goose_toolshim(Config::global())
        .ok()
        .flatten()
        .unwrap_or(false)
}

fn get_goose_toolshim_model(config: &Config) -> Result<Option<String>> {
    match config.get_param::<String>("GOOSE_TOOLSHIM_OLLAMA_MODEL") {
        Ok(value) if value.trim().is_empty() => Err(anyhow!(
            "Invalid value for 'GOOSE_TOOLSHIM_OLLAMA_MODEL': '{value}' - cannot be empty if set"
        )),
        Ok(value) => Ok(Some(value)),
        Err(ConfigError::NotFound(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn parse_bool_config(key: &str, value: &str) -> Result<bool> {
    match value.to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(anyhow!(
            "Invalid value for '{key}': '{value}' - must be one of: 1, true, yes, on, 0, false, no, off"
        )),
    }
}

fn parse_yaml_bool_config(key: &str, value: serde_yaml::Value) -> Result<bool> {
    match value {
        serde_yaml::Value::Bool(value) => Ok(value),
        serde_yaml::Value::Number(value) => parse_bool_config(key, &value.to_string()),
        serde_yaml::Value::String(value) => parse_bool_config(key, &value),
        other => {
            Err(anyhow!(
            "Invalid value for '{key}': '{}' - must be one of: 1, true, yes, on, 0, false, no, off",
            serde_yaml::to_string(&other).unwrap_or_else(|_| "<unprintable>".to_string()).trim()
        ))
        }
    }
}

/// An MLX engine endpoint for request-body tests: the `omlx` definition (the provider every MLX
/// node resolves to — local sidecar, distributed split, remote peer) aimed at a mock that answers
/// every chat request with one short streamed label and records what it was sent.
#[cfg(test)]
pub(crate) mod mlx_endpoint {
    use crate::providers::base::Provider;
    use serde_json::{json, Value};
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub(crate) const SERVED: &str = "mihai-qwen3.8-27b-atlassian-q8-mlx";

    pub(crate) struct MlxEndpoint {
        server: MockServer,
        pub(crate) provider: Arc<dyn Provider>,
    }

    fn chunk(delta: Value, finish: Value) -> String {
        let chunk = json!({"id": "c1", "object": "chat.completion.chunk", "model": SERVED,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}});
        format!("data: {chunk}\n\n")
    }

    impl MlxEndpoint {
        pub(crate) async fn start() -> Self {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/v1/models"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"object": "list", "data": [{"id": SERVED}]})),
                )
                .mount(&server)
                .await;
            let body = format!(
                "{}{}data: [DONE]\n\n",
                chunk(
                    json!({"role": "assistant", "content": "reading project configuration"}),
                    Value::Null
                ),
                chunk(json!({}), json!("stop")),
            );
            Mock::given(method("POST"))
                .and(path("/v1/chat/completions"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(body),
                )
                .mount(&server)
                .await;
            let mut config = crate::config::declarative_providers::load_provider("omlx")
                .expect("the omlx definition loads")
                .config;
            config.base_url = format!("{}/v1/chat/completions", server.uri());
            config.env_vars = None;
            let provider = crate::providers::openai_def::from_custom_config(config, None)
                .expect("the omlx provider builds");
            Self {
                server,
                provider: Arc::new(provider),
            }
        }

        /// Every chat request body the engine received, in order.
        pub(crate) async fn bodies(&self) -> Vec<Value> {
            self.server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|r| r.method == wiremock::http::Method::POST)
                .map(|r| serde_json::from_slice(&r.body).unwrap())
                .collect()
        }
    }

    pub(crate) fn thinking_off() -> Value {
        json!({"enable_thinking": false})
    }

    /// The agent's own turn is the negative control: its config carries no reasoning choice, so
    /// the engine receives no template switch and the model's template decides as before.
    #[tokio::test]
    async fn the_agent_turn_carries_no_template_switch() {
        use futures::StreamExt;
        let engine = MlxEndpoint::start().await;
        let turn = goose_providers::model::ModelConfig::new(SERVED);
        let messages = vec![crate::conversation::message::Message::user().with_text("hi")];
        let mut stream = engine
            .provider
            .stream(&turn, "You are goose", &messages, &[])
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        let bodies = engine.bodies().await;
        assert_eq!(bodies.len(), 1);
        assert!(
            bodies[0].get("chat_template_kwargs").is_none(),
            "{}",
            bodies[0]
        );
    }
}
