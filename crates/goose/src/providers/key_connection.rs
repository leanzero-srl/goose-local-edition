use crate::config::Config;
use crate::conversation::message::Message;
use crate::providers::base::ProviderMetadata;
use anyhow::{anyhow, Result};
use goose_providers::model::ModelConfig;

/// The providers whose saved settings must be proven before they stick: the supported cloud
/// families by name, and every endpoint the user added themselves (`ProviderType::Custom` — an
/// OpenAI-compatible server from the Cloud Providers tab), whose chosen default model is run once
/// and saved exactly like a cloud family's.
pub(crate) fn requires_connection_check(
    provider: &str,
    provider_type: crate::providers::base::ProviderType,
) -> bool {
    provider_type == crate::providers::base::ProviderType::Custom
        || matches!(
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

/// What a check established. A model listing is NOT proof of a key — OpenRouter's `/models` is public
/// and answered 444 models to a made-up key (2026-09-22) — so a key is `Proven` only when a model ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verification {
    /// A model answered with the key: the key works.
    Proven,
    /// Only the provider's model listing answered; no default model is chosen yet, so nothing ran.
    ListedOnly,
}

/// The message a `ListedOnly` state shows until a default model is chosen and run.
pub(crate) const NO_DEFAULT_MODEL_YET: &str =
    "No default model chosen yet — pick one to prove the key";

/// Verify a provider's credentials. With an explicit `model` (the user's chosen default) the model
/// is run once and saved as the provider's default; otherwise the saved default is run. When no
/// default exists yet the provider is asked for its model list — the list the setup dialog offers
/// the default from — but that is `ListedOnly`, never proof. A provider with no listing at all runs
/// its registry default model, unsaved: the default stays the user's choice.
pub(crate) async fn check(
    provider_id: &str,
    metadata: &ProviderMetadata,
    model: Option<&str>,
) -> Result<Verification> {
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
    let provider = crate::providers::create(provider_id, vec![]).await?;
    if provider_id == "azure_openai" {
        let deployment = config.get_param::<String>("AZURE_OPENAI_DEPLOYMENT_NAME")?;
        probe(provider.as_ref(), &deployment, &metadata.display_name).await?;
        save_default_model(provider_id, &deployment)?;
        return Ok(Verification::Proven);
    }
    let chosen = model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
        .or(saved_default_model(provider_id)?);
    if let Some(model) = chosen {
        probe(provider.as_ref(), &model, &metadata.display_name).await?;
        save_default_model(provider_id, &model)?;
        return Ok(Verification::Proven);
    }
    let listed = list_models(provider.as_ref(), &metadata.display_name).await?;
    if !listed.is_empty() {
        return Ok(Verification::ListedOnly);
    }
    let registry_default = metadata.default_model.trim();
    if registry_default.is_empty() {
        return Err(anyhow!(
            "{} lists no models and has no registry default to run",
            metadata.display_name
        ));
    }
    probe(provider.as_ref(), registry_default, &metadata.display_name).await?;
    Ok(Verification::Proven)
}

/// The models this key can see, as the provider reports them. An empty answer means the provider
/// has no listing endpoint (the trait default), never that the key is bad — a rejected key is an
/// error from the provider.
pub async fn list_models(
    provider: &dyn crate::providers::base::Provider,
    label: &str,
) -> Result<Vec<String>> {
    provider
        .fetch_supported_models()
        .await
        .map_err(|error| anyhow!("{} could not list models: {}", label, error))
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
    record(provider, &result);
    result.map(|_| ())
}

/// The status a check leaves behind: `Proven` clears it, `ListedOnly` states what is still missing,
/// an error carries the provider's own words. Callers record only what `requires_connection_check`
/// admitted.
pub(crate) fn record(provider: &str, result: &Result<Verification>) {
    let status = match result {
        Ok(Verification::Proven) => None,
        Ok(Verification::ListedOnly) => Some(NO_DEFAULT_MODEL_YET.to_owned()),
        Err(error) => Some(error.to_string()),
    };
    CHECKS
        .lock()
        .expect("provider check lock poisoned")
        .insert(provider.to_owned(), status);
}

static CONFIG_CHECK_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) async fn lock() -> tokio::sync::MutexGuard<'static, ()> {
    CONFIG_CHECK_LOCK.lock().await
}

const DEFAULT_MODELS_KEY: &str = "provider_default_models";
/// The key installs before 3.0.7 wrote the checked model under; read once, never written again.
const LEGACY_CONNECTION_MODELS_KEY: &str = "provider_connection_models";

fn default_models(config: &Config) -> Result<std::collections::HashMap<String, String>> {
    for key in [DEFAULT_MODELS_KEY, LEGACY_CONNECTION_MODELS_KEY] {
        match config.get_param::<std::collections::HashMap<String, String>>(key) {
            Ok(models) => return Ok(models),
            Err(crate::config::ConfigError::NotFound(_)) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(std::collections::HashMap::new())
}

/// The default model the user chose for a provider — the first pick in node selection. `None`
/// means no choice was saved under either key; a corrupt map is an error, never an absence.
pub fn saved_default_model(provider: &str) -> Result<Option<String>> {
    Ok(default_models(Config::global())?.get(provider).cloned())
}

pub(crate) fn save_default_model(provider: &str, model: &str) -> Result<()> {
    let config = Config::global();
    let mut models = default_models(config)?;
    models.insert(provider.to_owned(), model.trim().to_owned());
    config.set_param(DEFAULT_MODELS_KEY, serde_json::to_value(models)?)?;
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
            let entry = crate::providers::get_from_registry(provider).await.unwrap();
            assert!(requires_connection_check(provider, entry.provider_type()));
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
    // The keys are this test's own: `config::base`'s tests set process env vars such as
    // TEST_KEY, and an env var overrides the file, so a shared name reads another test's value.
    fn rejected_or_cancelled_save_restores_keys_and_endpoint() {
        let directory = tempfile::tempdir().unwrap();
        let config = Config::new_with_file_secrets(
            directory.path().join("config.yaml"),
            directory.path().join("secrets.json"),
        )
        .unwrap();
        config
            .set_param("KEY_CONNECTION_TEST_ENDPOINT", "original")
            .unwrap();
        config
            .set_secret("KEY_CONNECTION_TEST_KEY", &"original-key")
            .unwrap();
        {
            let _pending = PendingConfig::capture(
                &config,
                &[
                    ("KEY_CONNECTION_TEST_ENDPOINT", false),
                    ("KEY_CONNECTION_TEST_KEY", true),
                    ("KEY_CONNECTION_TEST_NEW_KEY", true),
                ],
            )
            .unwrap();
            config
                .set_param("KEY_CONNECTION_TEST_ENDPOINT", "candidate")
                .unwrap();
            config
                .set_secret("KEY_CONNECTION_TEST_KEY", &"rejected-key")
                .unwrap();
            config
                .set_secret("KEY_CONNECTION_TEST_NEW_KEY", &"new-key")
                .unwrap();
        }
        assert_eq!(
            config
                .get_param::<String>("KEY_CONNECTION_TEST_ENDPOINT")
                .unwrap(),
            "original"
        );
        assert_eq!(
            config
                .get_secret::<String>("KEY_CONNECTION_TEST_KEY")
                .unwrap(),
            "original-key"
        );
        assert!(config
            .get_secret::<String>("KEY_CONNECTION_TEST_NEW_KEY")
            .is_err());
        let mut pending =
            PendingConfig::capture(&config, &[("KEY_CONNECTION_TEST_KEY", true)]).unwrap();
        config
            .set_secret("KEY_CONNECTION_TEST_KEY", &"validated-key")
            .unwrap();
        pending.commit();
        drop(pending);
        assert_eq!(
            config
                .get_secret::<String>("KEY_CONNECTION_TEST_KEY")
                .unwrap(),
            "validated-key"
        );
    }
}
