use crate::agents::provider_lifecycle::{LifecycleSettings, ProviderRequestLifecycle};
use crate::benchmark_budget;
use crate::providers::base::{MessageStream, ModelInfo, PermissionRouting, Provider};
use async_trait::async_trait;
use goose_providers::conversation::message::Message;
use goose_providers::errors::ProviderError;
use goose_providers::goose_mode::GooseMode;
use goose_providers::model::ModelConfig;
use goose_providers::permission::PermissionConfirmation;
use goose_providers::retry::RetryConfig;
use rmcp::model::Tool;
use std::sync::Arc;

enum GuardSource {
    Environment,
    #[cfg(test)]
    Test {
        config: std::path::PathBuf,
        ledger: std::path::PathBuf,
        lifecycle: std::path::PathBuf,
    },
}

pub(crate) struct BenchmarkGuardProvider {
    inner: Arc<dyn Provider>,
    source: GuardSource,
}

impl BenchmarkGuardProvider {
    pub(crate) fn wrap_if_requested(inner: Arc<dyn Provider>) -> Arc<dyn Provider> {
        if benchmark_budget::guard_requested() {
            Arc::new(Self {
                inner,
                source: GuardSource::Environment,
            })
        } else {
            inner
        }
    }

    fn reservation(
        &self,
        model_config: &ModelConfig,
    ) -> Result<Option<benchmark_budget::BenchmarkBudgetReservation>, ProviderError> {
        match &self.source {
            GuardSource::Environment => {
                benchmark_budget::reserve_request(self.inner.get_name(), model_config)
            }
            #[cfg(test)]
            GuardSource::Test { config, ledger, .. } => {
                benchmark_budget::reserve_from_paths_for_test(
                    config,
                    ledger,
                    self.inner.get_name(),
                    model_config,
                )
                .map(Some)
            }
        }
    }

    fn lifecycle_settings(&self) -> LifecycleSettings {
        match &self.source {
            GuardSource::Environment => LifecycleSettings::from_env(),
            #[cfg(test)]
            GuardSource::Test { lifecycle, .. } => {
                LifecycleSettings::for_test(lifecycle.clone(), true)
            }
        }
    }

    #[cfg(test)]
    fn for_test(
        inner: Arc<dyn Provider>,
        config: std::path::PathBuf,
        ledger: std::path::PathBuf,
        lifecycle: std::path::PathBuf,
    ) -> Self {
        Self {
            inner,
            source: GuardSource::Test {
                config,
                ledger,
                lifecycle,
            },
        }
    }
}

#[async_trait]
impl Provider for BenchmarkGuardProvider {
    fn get_name(&self) -> &str {
        self.inner.get_name()
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let reservation = self.reservation(model_config)?;
        let session = crate::session_context::current_session_id()
            .unwrap_or_else(|| "unscoped-provider-call".to_string());
        let mut lifecycle = ProviderRequestLifecycle::begin(
            self.lifecycle_settings(),
            self.inner.get_name().to_string(),
            model_config.model_name.clone(),
            session,
            reservation,
        )?;

        match self
            .inner
            .stream(model_config, system, messages, tools)
            .await
        {
            Ok(stream) => {
                lifecycle.admitted()?;
                Ok(lifecycle.wrap(stream))
            }
            Err(error) => {
                if let Err(evidence_error) = lifecycle.pre_admission_error(&error) {
                    Err(evidence_error)
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn get_context_limit(&self, model_config: &ModelConfig) -> Result<usize, ProviderError> {
        self.inner.get_context_limit(model_config).await
    }

    fn retry_config(&self) -> RetryConfig {
        self.inner.retry_config()
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        self.inner.fetch_supported_models().await
    }

    async fn fetch_supported_model_info(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.inner.fetch_supported_model_info().await
    }

    async fn fetch_model_info(&self, model_name: &str) -> Result<ModelInfo, ProviderError> {
        self.inner.fetch_model_info(model_name).await
    }

    fn skip_canonical_filtering(&self) -> bool {
        self.inner.skip_canonical_filtering()
    }

    async fn fetch_recommended_models(&self, toolshim: bool) -> Result<Vec<String>, ProviderError> {
        self.inner.fetch_recommended_models(toolshim).await
    }

    async fn fetch_recommended_model_info(
        &self,
        toolshim: bool,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        self.inner.fetch_recommended_model_info(toolshim).await
    }

    async fn map_to_canonical_model(
        &self,
        provider_model: &str,
    ) -> Result<Option<String>, ProviderError> {
        self.inner.map_to_canonical_model(provider_model).await
    }

    fn manages_own_context(&self) -> bool {
        self.inner.manages_own_context()
    }

    async fn configure_oauth(&self) -> Result<(), ProviderError> {
        self.inner.configure_oauth().await
    }

    async fn refresh_credentials(&self) -> Result<(), ProviderError> {
        self.inner.refresh_credentials().await
    }

    async fn update_mode(&self, session_id: &str, mode: GooseMode) -> Result<(), ProviderError> {
        self.inner.update_mode(session_id, mode).await
    }

    fn permission_routing(&self) -> PermissionRouting {
        self.inner.permission_routing()
    }

    async fn handle_permission_confirmation(
        &self,
        request_id: &str,
        confirmation: &PermissionConfirmation,
    ) -> bool {
        self.inner
            .handle_permission_confirmation(request_id, confirmation)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::base::ProviderUsage;
    use goose_providers::conversation::token_usage::Usage;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Provider for CountingProvider {
        fn get_name(&self) -> &str {
            "provider"
        }

        async fn stream(
            &self,
            model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let usage = ProviderUsage::new(
                model_config.model_name.clone(),
                Usage::new(Some(100), Some(200), Some(300)),
            );
            Ok(crate::providers::base::stream_from_single_message(
                Message::assistant().with_text("done"),
                usage,
            ))
        }
    }

    fn fixture(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let config = root.join("budget-config.json");
        let ledger = root.join("budget-ledger.json");
        std::fs::write(
            &config,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "currency": "USD",
                "total_cap": 1.0,
                "provider_caps": {"provider": 1.0},
                "models": {
                    "provider/model": {
                        "provider": "provider",
                        "model": "model",
                        "context_limit": 1000,
                        "max_output_tokens": 1000,
                        "pricing": {
                            "input_per_million": 100.0,
                            "output_per_million": 100.0,
                            "source": "https://example.test/pricing",
                            "verified_at": "2026-08-23"
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            &ledger,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "currency": "USD",
                "total_cap": 1.0,
                "provider_caps": {"provider": 1.0},
                "spent_upper_bound": 0.0,
                "provider_spent_upper_bound": {"provider": 0.0},
                "outstanding": {},
                "settled": [],
                "updated_at": "now"
            }))
            .unwrap(),
        )
        .unwrap();
        (config, ledger)
    }

    #[tokio::test]
    async fn direct_complete_is_reserved_recorded_and_settled_once() {
        let temp = tempfile::tempdir().unwrap();
        let (config, ledger) = fixture(temp.path());
        let lifecycle = temp.path().join("lifecycle.jsonl");
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = BenchmarkGuardProvider::for_test(
            Arc::new(CountingProvider {
                calls: calls.clone(),
            }),
            config,
            ledger.clone(),
            lifecycle.clone(),
        );
        let model = ModelConfig::new("model")
            .with_context_limit(Some(1000))
            .with_max_tokens(Some(1000));

        let (_, usage) = provider.complete(&model, "", &[], &[]).await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(usage.usage.total_tokens, Some(300));
        let ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(ledger).unwrap()).unwrap();
        assert!(ledger["outstanding"].as_object().unwrap().is_empty());
        assert_eq!(ledger["settled"].as_array().unwrap().len(), 1);
        let events: Vec<serde_json::Value> = std::fs::read_to_string(lifecycle)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.last().unwrap()["state"], "provider_terminal");
        assert!(events
            .iter()
            .all(|event| event["request_id"] == events[0]["request_id"]));
    }

    #[tokio::test]
    async fn normal_agent_stream_has_one_reservation_and_one_terminal_fsm() {
        use futures::StreamExt;

        let temp = tempfile::tempdir().unwrap();
        let (config, ledger) = fixture(temp.path());
        let lifecycle = temp.path().join("lifecycle.jsonl");
        let calls = Arc::new(AtomicUsize::new(0));
        let provider: Arc<dyn Provider> = Arc::new(BenchmarkGuardProvider::for_test(
            Arc::new(CountingProvider {
                calls: calls.clone(),
            }),
            config,
            ledger.clone(),
            lifecycle.clone(),
        ));
        let model = ModelConfig::new("model")
            .with_context_limit(Some(1000))
            .with_max_tokens(Some(1000));
        let mut stream = crate::agents::Agent::stream_response_from_provider(
            provider,
            model,
            "agent-session",
            "system",
            &[Message::user().with_text("test")],
            &[],
            &[],
        )
        .await
        .unwrap();
        while let Some(item) = stream.next().await {
            item.unwrap();
        }

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(ledger).unwrap()).unwrap();
        assert_eq!(ledger["settled"].as_array().unwrap().len(), 1);
        assert!(ledger["outstanding"].as_object().unwrap().is_empty());
        let events: Vec<serde_json::Value> = std::fs::read_to_string(lifecycle)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            events
                .iter()
                .filter(|event| event["state"] == "queued")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event["state"] == "provider_terminal")
                .count(),
            1
        );
    }
}
