use async_trait::async_trait;
use futures::StreamExt;
use goose::config::GooseMode;
use goose::conversation::message::Message;
use goose::providers::base::{MessageStream, ModelInfo, PermissionRouting, Provider};
use goose_provider_types::errors::ProviderError;
use goose_provider_types::model::ModelConfig;
use goose_provider_types::permission::PermissionConfirmation;
use goose_provider_types::retry::RetryConfig;
use goose_swarm::{ProviderLifecycle, ProviderRequestKey, ProviderTerminalKind};
use rmcp::model::Tool;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

tokio::task_local! {
    static ACTIVE_PROVIDER_LIFECYCLE: ProviderLifecycle;
}

static PROVIDER_REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) async fn scope_provider_lifecycle<F>(
    lifecycle: ProviderLifecycle,
    future: F,
) -> F::Output
where
    F: Future,
{
    ACTIVE_PROVIDER_LIFECYCLE.scope(lifecycle, future).await
}

pub(crate) fn bind_current_provider_lifecycle(provider: Arc<dyn Provider>) -> Arc<dyn Provider> {
    ACTIVE_PROVIDER_LIFECYCLE
        .try_with(|lifecycle| {
            Arc::new(LifecycleProvider {
                inner: provider.clone(),
                lifecycle: lifecycle.clone(),
            }) as Arc<dyn Provider>
        })
        .unwrap_or(provider)
}

struct LifecycleProvider {
    inner: Arc<dyn Provider>,
    lifecycle: ProviderLifecycle,
}

impl LifecycleProvider {
    fn next_request_id(&self) -> String {
        let sequence = PROVIDER_REQUEST_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        format!(
            "{}:provider-turn:{sequence}",
            self.lifecycle.admission().admission_id
        )
    }
}

struct ProviderTerminalGuard {
    lifecycle: ProviderLifecycle,
    key: Option<ProviderRequestKey>,
}

impl ProviderTerminalGuard {
    fn new(lifecycle: ProviderLifecycle, key: ProviderRequestKey) -> Self {
        Self {
            lifecycle,
            key: Some(key),
        }
    }

    async fn finish(&mut self, kind: ProviderTerminalKind) -> Result<(), ProviderError> {
        let Some(key) = self.key.take() else {
            return Ok(());
        };
        if let Err(error) = self.lifecycle.provider_terminal(key.clone(), kind).await {
            self.key = Some(key);
            return Err(lifecycle_error("terminal receipt", error));
        }
        Ok(())
    }
}

impl Drop for ProviderTerminalGuard {
    fn drop(&mut self) {
        let Some(key) = self.key.take() else {
            return;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let lifecycle = self.lifecycle.clone();
        handle.spawn(async move {
            let _ = lifecycle
                .provider_terminal(key, ProviderTerminalKind::Cancelled)
                .await;
        });
    }
}

fn lifecycle_error(action: &str, error: impl std::fmt::Display) -> ProviderError {
    ProviderError::ExecutionError(format!(
        "physical provider lifecycle {action} failed: {error}"
    ))
}

#[async_trait]
impl Provider for LifecycleProvider {
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
        let key = self
            .lifecycle
            .provider_request_started(self.next_request_id())
            .await
            .map_err(|error| lifecycle_error("start receipt", error))?;
        let mut terminal = ProviderTerminalGuard::new(self.lifecycle.clone(), key);
        let stream = match self
            .inner
            .stream(model_config, system, messages, tools)
            .await
        {
            Ok(stream) => stream,
            Err(provider_error) => {
                terminal
                    .finish(ProviderTerminalKind::Failed)
                    .await
                    .map_err(|receipt_error| {
                        ProviderError::ExecutionError(format!(
                            "provider failed ({provider_error}); physical provider lifecycle terminal receipt failed: {receipt_error}"
                        ))
                    })?;
                return Err(provider_error);
            }
        };
        Ok(Box::pin(async_stream::stream! {
            let mut stream = stream;
            let mut terminal = terminal;
            while let Some(item) = stream.next().await {
                match item {
                    Ok(value) => yield Ok(value),
                    Err(provider_error) => {
                        match terminal.finish(ProviderTerminalKind::Failed).await {
                            Ok(()) => yield Err(provider_error),
                            Err(receipt_error) => yield Err(ProviderError::ExecutionError(format!(
                                "provider stream failed ({provider_error}); {receipt_error}"
                            ))),
                        }
                        return;
                    }
                }
            }
            if let Err(receipt_error) = terminal.finish(ProviderTerminalKind::Finished).await {
                yield Err(receipt_error);
            }
        }))
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
    use futures::stream;
    use goose::conversation::message::Message;
    use goose::providers::base::{ProviderUsage, Usage};
    use goose_swarm::{
        HostCapacityEvidence, LocalCompletionKind, NullSink, PhysicalAdmissionControl,
        PhysicalFleetSnapshot, SourceRevisionKind, TaskVersion, VerifiedPhysicalIdentity,
        WorkOpportunity, WorkRole,
    };
    use std::time::Duration;

    #[derive(Clone)]
    enum Behavior {
        Finished,
        Failed,
        Pending,
        StartPending(Arc<tokio::sync::Notify>),
    }

    struct MockProvider {
        behavior: Behavior,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn get_name(&self) -> &str {
            "mock"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            match &self.behavior {
                Behavior::Finished => Ok(Box::pin(stream::once(async {
                    Ok((
                        Some(Message::assistant().with_text("ok")),
                        Some(ProviderUsage::new("mock".to_string(), Usage::default())),
                    ))
                }))),
                Behavior::Failed => Err(ProviderError::ExecutionError("mock failure".to_string())),
                Behavior::Pending => Ok(Box::pin(stream::pending())),
                Behavior::StartPending(entered) => {
                    entered.notify_one();
                    futures::future::pending().await
                }
            }
        }
    }

    async fn admitted() -> (PhysicalAdmissionControl, goose_swarm::AdmittedWork) {
        let identity = VerifiedPhysicalIdentity {
            host_id: "host-a".to_string(),
            model_instance_id: "instance-a".to_string(),
            advertised_instance_capacity: 1,
            capacity_evidence: HostCapacityEvidence::ProbeSingleStream {
                probe_epoch: "probe-a".to_string(),
            },
            route_evidence_id: "route-a".to_string(),
        };
        let snapshot = PhysicalFleetSnapshot::new(
            "snapshot-a",
            vec![identity.into_lane("device-a".to_string(), "model-a".to_string(), 1)],
        )
        .unwrap();
        let control = PhysicalAdmissionControl::new("test", snapshot, Arc::new(NullSink)).unwrap();
        let source = TaskVersion {
            task_id: "task-a".to_string(),
            attempt: 0,
            revision: 1,
            kind: SourceRevisionKind::TaskAttempt,
        };
        control.set_source_revision(source.clone()).await.unwrap();
        let work = control
            .admit(WorkOpportunity {
                work_id: "work-a".to_string(),
                role: WorkRole::Build,
                priority: WorkRole::Build.priority(),
                task_rank: 0,
                source,
                eligible_logical_device_ids: vec!["device-a".to_string()],
                preferred_model_id: None,
                excluded_logical_device_id: None,
            })
            .await
            .unwrap();
        (control, work)
    }

    async fn wrapped_for(behavior: Behavior, lifecycle: ProviderLifecycle) -> Arc<dyn Provider> {
        scope_provider_lifecycle(lifecycle, async move {
            bind_current_provider_lifecycle(Arc::new(MockProvider { behavior }))
        })
        .await
    }

    #[tokio::test]
    async fn natural_stream_end_records_finished_before_local_success() {
        let (control, work) = admitted().await;
        let provider = wrapped_for(Behavior::Finished, work.lifecycle()).await;
        let mut output = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        while let Some(item) = output.next().await {
            item.unwrap();
        }
        work.complete_local(LocalCompletionKind::Success)
            .await
            .unwrap();
        assert_eq!(control.occupancy().await, (0, 0));
    }

    #[tokio::test]
    async fn provider_error_records_failed_terminal() {
        let (control, work) = admitted().await;
        let provider = wrapped_for(Behavior::Failed, work.lifecycle()).await;
        assert!(provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .is_err());
        work.complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
        assert_eq!(control.occupancy().await, (0, 0));
    }

    #[tokio::test]
    async fn dropped_stream_records_cancelled_terminal() {
        let (control, work) = admitted().await;
        let provider = wrapped_for(Behavior::Pending, work.lifecycle()).await;
        let output = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        drop(output);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if work
                    .complete_local(LocalCompletionKind::Error)
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(control.occupancy().await, (0, 0));
    }

    #[tokio::test]
    async fn cancelled_stream_creation_records_cancelled_terminal() {
        let (control, work) = admitted().await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let waiting = entered.notified();
        let provider = wrapped_for(Behavior::StartPending(entered.clone()), work.lifecycle()).await;
        let task = tokio::spawn(async move {
            provider
                .stream(&ModelConfig::new("model-a"), "", &[], &[])
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap();
        task.abort();
        let _ = task.await;
        tokio::time::timeout(
            Duration::from_secs(2),
            work.complete_local(LocalCompletionKind::Error),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(control.occupancy().await, (0, 0));
    }
}
