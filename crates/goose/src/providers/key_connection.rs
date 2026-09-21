use crate::config::Config;
use crate::conversation::message::Message;
use crate::providers::base::ProviderMetadata;
use anyhow::{anyhow, Result};
use goose_providers::model::ModelConfig;

pub(crate) fn requires_connection_check(provider: &str) -> bool {
    matches!(
        provider,
        "aws_bedrock"
            | "azure_openai"
            | "openai"
            | "anthropic"
            | "google"
            | "alibaba"
            | "openrouter"
            | "ollama_cloud"
            | "minimax"
            | "mistral"
            | "zai"
            | "xai"
            | "moonshot"
            | "custom_deepseek"
    )
}

pub(crate) async fn check(
    provider_id: &str,
    metadata: &ProviderMetadata,
    test_model: Option<&str>,
) -> Result<()> {
    let config = Config::global();
    config.invalidate_secrets_cache();
    for key in metadata.config_keys.iter().filter(|key| {
        key.secret
            && !key.oauth_flow
            && (key.required
                || key.name == "AWS_BEARER_TOKEN_BEDROCK"
                || key.name == "AZURE_OPENAI_API_KEY")
    }) {
        if config.get_secret::<String>(&key.name)?.trim().is_empty() {
            return Err(anyhow!("{} requires {}", metadata.display_name, key.name));
        }
    }
    let model = if provider_id == "azure_openai" {
        config.get_param::<String>("AZURE_OPENAI_DEPLOYMENT_NAME")?
    } else if let Some(model) = test_model.filter(|model| !model.trim().is_empty()) {
        model.trim().to_owned()
    } else if let Some(model) = saved_test_model(provider_id) {
        model
    } else {
        crate::config::providers::get_provider_entry(config, provider_id)
            .map(|entry| entry.model)
            .filter(|model| !model.trim().is_empty())
            .unwrap_or_else(|| metadata.default_model.clone())
    };
    if model.trim().is_empty() {
        return Err(anyhow!(
            "No test model is configured for {}",
            metadata.display_name
        ));
    }
    let provider = crate::providers::create(provider_id, vec![]).await?;
    probe(provider.as_ref(), &model, &metadata.display_name).await?;
    save_test_model(provider_id, &model)
}

async fn probe(
    provider: &dyn crate::providers::base::Provider,
    model: &str,
    label: &str,
) -> Result<()> {
    let model_config = ModelConfig::new(model).with_max_tokens(Some(1024));
    let messages = [Message::user().with_text("Reply with OK.")];
    let (message, _) = crate::session_context::with_session_id(
        Some("provider-connection-check".into()),
        provider.complete(&model_config, "Respond briefly.", &messages, &[]),
    )
    .await
    .map_err(|error| anyhow!("{} could not run model '{}': {}", label, model, error))?;
    if message.as_concat_text().trim().is_empty() {
        return Err(anyhow!(
            "{} returned no visible response for model '{}'",
            label,
            model
        ));
    }
    Ok(())
}

static CHECKS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub(crate) fn status(provider: &str) -> Option<Option<String>> {
    CHECKS
        .lock()
        .expect("provider check lock poisoned")
        .get(provider)
        .cloned()
}

pub(crate) async fn check_and_record(provider: &str, metadata: &ProviderMetadata) -> Result<()> {
    let _guard = lock().await;
    let result = check(provider, metadata, None).await;
    CHECKS.lock().expect("provider check lock poisoned").insert(
        provider.to_owned(),
        result.as_ref().err().map(ToString::to_string),
    );
    result
}

pub(crate) fn mark_validated(provider: &str) {
    if requires_connection_check(provider) {
        CHECKS
            .lock()
            .expect("provider check lock poisoned")
            .insert(provider.to_owned(), None);
    }
}

static CONFIG_CHECK_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) async fn lock() -> tokio::sync::MutexGuard<'static, ()> {
    CONFIG_CHECK_LOCK.lock().await
}

pub(crate) fn saved_test_model(provider: &str) -> Option<String> {
    Config::global()
        .get_param::<std::collections::HashMap<String, String>>("provider_connection_models")
        .ok()
        .and_then(|models| models.get(provider).cloned())
}

pub(crate) fn save_test_model(provider: &str, model: &str) -> Result<()> {
    Config::global().update_param::<std::collections::HashMap<String, String>, _, _>(
        "provider_connection_models",
        |mut models| {
            models.insert(provider.to_owned(), model.trim().to_owned());
            models
        },
    )?;
    Ok(())
}

pub(crate) struct PendingConfig<'a> {
    config: &'a Config,
    previous: Vec<(String, bool, Option<serde_json::Value>)>,
    previous_environment: Vec<(String, Option<std::ffi::OsString>)>,
    committed: bool,
}

impl<'a> PendingConfig<'a> {
    pub(crate) fn capture(config: &'a Config, fields: &[(&str, bool)]) -> Result<Self> {
        let values = config.all_values()?;
        let secrets = config.all_secrets()?;
        Ok(Self {
            config,
            previous_environment: fields
                .iter()
                .map(|(key, _)| ((*key).to_owned(), std::env::var_os(key)))
                .collect(),
            previous: fields
                .iter()
                .map(|(key, secret)| {
                    (
                        (*key).to_owned(),
                        *secret,
                        if *secret {
                            secrets.get(*key)
                        } else {
                            values.get(*key)
                        }
                        .cloned(),
                    )
                })
                .collect(),
            committed: false,
        })
    }
    pub(crate) fn commit(&mut self) {
        self.committed = true;
    }
    pub(crate) fn restore(&mut self) -> Result<()> {
        for (key, secret, value) in &self.previous {
            match value {
                Some(value) => self.config.set(key, value, *secret)?,
                None => {
                    let result = if *secret {
                        self.config.delete_secret(key)
                    } else {
                        self.config.delete(key)
                    };
                    match result {
                        Ok(()) | Err(crate::config::ConfigError::NotFound(_)) => {}
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        }
        for (key, value) in &self.previous_environment {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        self.config.invalidate_secrets_cache();
        self.committed = true;
        Ok(())
    }
}
impl Drop for PendingConfig<'_> {
    fn drop(&mut self) {
        if !self.committed {
            if let Err(error) = self.restore() {
                tracing::error!(%error, "Could not restore provider configuration after cancelled validation");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn all_supported_cloud_providers_have_native_key_configuration() {
        for (provider, key) in [
            #[cfg(feature = "aws-providers")]
            ("aws_bedrock", "AWS_BEARER_TOKEN_BEDROCK"),
            ("azure_openai", "AZURE_OPENAI_API_KEY"),
            ("openai", "OPENAI_API_KEY"),
            ("anthropic", "ANTHROPIC_API_KEY"),
            ("google", "GOOGLE_API_KEY"),
            ("alibaba", "DASHSCOPE_API_KEY"),
            ("openrouter", "OPENROUTER_API_KEY"),
            ("ollama_cloud", "OLLAMA_CLOUD_API_KEY"),
            ("minimax", "MINIMAX_API_KEY"),
            ("mistral", "MISTRAL_API_KEY"),
            ("zai", "ZHIPU_API_KEY"),
            ("xai", "XAI_API_KEY"),
            ("moonshot", "MOONSHOT_API_KEY"),
            ("custom_deepseek", "DEEPSEEK_API_KEY"),
        ] {
            assert!(requires_connection_check(provider));
            let entry = crate::providers::get_from_registry(provider).await.unwrap();
            assert!(
                entry
                    .metadata()
                    .config_keys
                    .iter()
                    .any(|field| field.name == key && field.secret && !field.oauth_flow),
                "{provider} missing native API key field"
            );
            assert!(
                !entry
                    .metadata()
                    .config_keys
                    .iter()
                    .any(|field| field.oauth_flow),
                "{provider} still requires OAuth"
            );
        }
    }

    #[tokio::test]
    async fn connection_probe_checks_authenticated_inference_and_rejection() {
        use crate::providers::api_client::{ApiClient, AuthMethod};
        use crate::providers::openai_compatible::OpenAiCompatibleProvider;
        use wiremock::{
            matchers::{body_partial_json, header, method, path},
            Mock, MockServer, ResponseTemplate,
        };
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/chat/completions"))
            .and(header("Authorization", "Bearer valid-key"))
            .and(body_partial_json(serde_json::json!({"model":"test-model"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id":"test", "choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":5,"completion_tokens":1,"total_tokens":6}
            }))).expect(1).mount(&server).await;
        Mock::given(method("POST"))
            .and(header("Authorization", "Bearer expired-key"))
            .respond_with(ResponseTemplate::new(401).set_body_json(
                serde_json::json!({"error":{"message":"Expired API key","type":"invalid_api_key"}}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        for (key, succeeds) in [("valid-key", true), ("expired-key", false)] {
            let client = ApiClient::new_with_tls(
                format!("{}/v1", server.uri()),
                AuthMethod::BearerToken(key.into()),
                None,
            )
            .unwrap();
            let provider = OpenAiCompatibleProvider::new("openai".into(), client, String::new())
                .with_supports_streaming(false);
            let result = probe(&provider, "test-model", "OpenAI").await;
            assert_eq!(result.is_ok(), succeeds, "{result:?}");
        }
    }

    #[test]
    fn rejected_or_cancelled_save_restores_keys_and_endpoint() {
        let directory = tempfile::tempdir().unwrap();
        let config = Config::new_with_file_secrets(
            directory.path().join("config.yaml"),
            directory.path().join("secrets.json"),
        )
        .unwrap();
        config.set_param("TEST_ENDPOINT", "original").unwrap();
        config.set_secret("TEST_KEY", &"original-key").unwrap();
        {
            let _pending = PendingConfig::capture(
                &config,
                &[
                    ("TEST_ENDPOINT", false),
                    ("TEST_KEY", true),
                    ("TEST_NEW_KEY", true),
                ],
            )
            .unwrap();
            config.set_param("TEST_ENDPOINT", "candidate").unwrap();
            config.set_secret("TEST_KEY", &"rejected-key").unwrap();
            config.set_secret("TEST_NEW_KEY", &"new-key").unwrap();
        }
        assert_eq!(
            config.get_param::<String>("TEST_ENDPOINT").unwrap(),
            "original"
        );
        assert_eq!(
            config.get_secret::<String>("TEST_KEY").unwrap(),
            "original-key"
        );
        assert!(config.get_secret::<String>("TEST_NEW_KEY").is_err());
        let mut pending = PendingConfig::capture(&config, &[("TEST_KEY", true)]).unwrap();
        config.set_secret("TEST_KEY", &"validated-key").unwrap();
        pending.commit();
        drop(pending);
        assert_eq!(
            config.get_secret::<String>("TEST_KEY").unwrap(),
            "validated-key"
        );
    }
}
