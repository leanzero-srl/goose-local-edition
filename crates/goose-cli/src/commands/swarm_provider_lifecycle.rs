use async_trait::async_trait;
use futures::StreamExt;
use goose::config::GooseMode;
use goose::conversation::message::Message;
use goose::providers::base::{
    MessageStream, ModelInfo, PermissionRouting, Provider, ProviderHttpProtocol,
    SingleAttemptFailureProvenance, SingleAttemptStreamOutcome,
};
use goose_provider_types::errors::ProviderError;
use goose_provider_types::model::ModelConfig;
use goose_provider_types::permission::PermissionConfirmation;
use goose_provider_types::retry::RetryConfig;
use goose_swarm::{ProviderLifecycle, ProviderTerminalKind, StartedProviderRequest};
use rmcp::model::Tool;
use std::future::Future;
use std::sync::Arc;

tokio::task_local! {
    static ACTIVE_PROVIDER_LIFECYCLE: ProviderLifecycle;
}

pub(crate) async fn scope_provider_lifecycle<F>(
    lifecycle: ProviderLifecycle,
    future: F,
) -> F::Output
where
    F: Future,
{
    ACTIVE_PROVIDER_LIFECYCLE.scope(lifecycle, future).await
}

pub(crate) fn provider_lifecycle_active() -> bool {
    ACTIVE_PROVIDER_LIFECYCLE.try_with(|_| ()).is_ok()
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

struct ProviderTerminalGuard {
    request: Option<StartedProviderRequest>,
}

impl ProviderTerminalGuard {
    fn new(request: StartedProviderRequest) -> Self {
        Self {
            request: Some(request),
        }
    }

    async fn finish(&mut self, kind: ProviderTerminalKind) -> Result<(), ProviderError> {
        let Some(request) = self.request.take() else {
            return Ok(());
        };
        if let Err(error) = request.provider_terminal(kind).await {
            let detail = error.to_string();
            if let Some(request) = error.into_retryable_request() {
                self.request = Some(request);
            }
            return Err(lifecycle_error("terminal receipt", detail));
        }
        Ok(())
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

    fn transport_identity(&self, model_name: &str) -> Option<String> {
        self.inner.transport_identity(model_name)
    }

    fn provider_http_protocol(&self, model_name: &str) -> Option<ProviderHttpProtocol> {
        self.inner.provider_http_protocol(model_name)
    }

    fn supports_single_attempt_streaming(&self) -> bool {
        self.inner.supports_single_attempt_streaming()
    }

    fn supports_terminal_proven_single_attempt_streaming(&self) -> bool {
        self.inner
            .supports_terminal_proven_single_attempt_streaming()
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        if model_config.model_name != self.lifecycle.admission().model_id {
            let detail = format!(
                "model {:?} does not match sealed physical admission model {:?}",
                model_config.model_name,
                self.lifecycle.admission().model_id
            );
            self.lifecycle
                .provider_not_started(detail.clone())
                .await
                .map_err(|error| lifecycle_error("provider-not-started receipt", error))?;
            return Err(ProviderError::ExecutionError(detail));
        }
        if self
            .inner
            .transport_identity(&model_config.model_name)
            .as_deref()
            != Some(self.lifecycle.admission().provider_transport_id.as_str())
        {
            let detail = format!(
                "provider `{}` transport does not match its sealed physical admission",
                self.inner.get_name()
            );
            self.lifecycle
                .provider_not_started(detail.clone())
                .await
                .map_err(|error| lifecycle_error("provider-not-started receipt", error))?;
            return Err(ProviderError::ExecutionError(detail));
        }
        if !self
            .inner
            .supports_terminal_proven_single_attempt_streaming()
        {
            let detail = format!(
                "provider `{}` has no terminal-proven single-attempt stream boundary",
                self.inner.get_name()
            );
            self.lifecycle
                .provider_not_started(detail.clone())
                .await
                .map_err(|error| lifecycle_error("provider-not-started receipt", error))?;
            return Err(ProviderError::NotImplemented(detail));
        }
        let started = self
            .lifecycle
            .start_provider_request()
            .await
            .map_err(|error| lifecycle_error("start receipt", error))?;
        if let Some(expected_protocol) = started.http_protocol() {
            if self.inner.provider_http_protocol(&model_config.model_name)
                != Some(expected_protocol)
            {
                let detail = format!(
                    "provider `{}` HTTP protocol does not match its sealed physical route",
                    self.inner.get_name()
                );
                return match started.abandon_before_exposure(&detail).await {
                    Ok(()) => Err(ProviderError::ExecutionError(detail)),
                    Err(error) => Err(ProviderError::ExecutionError(format!(
                        "{detail}; physical provider lifecycle abandon failed: {error}"
                    ))),
                };
            }
        }
        let single_attempt_result = started
            .scope_http(self.inner.stream_once_with_terminal_proof(
                model_config,
                system,
                messages,
                tools,
            ))
            .await;
        let mut terminal = ProviderTerminalGuard::new(started);
        let single_attempt = match single_attempt_result {
            Ok(stream) => stream,
            Err(provider_error) => {
                if self
                    .inner
                    .single_attempt_failure_provenance(&provider_error)
                    == SingleAttemptFailureProvenance::TerminalResponse
                {
                    terminal
                        .finish(ProviderTerminalKind::Failed)
                        .await
                        .map_err(|receipt_error| {
                            ProviderError::ExecutionError(format!(
                                "provider failed ({provider_error}); physical provider lifecycle terminal receipt failed: {receipt_error}"
                            ))
                        })?;
                }
                return Err(provider_error);
            }
        };
        Ok(Box::pin(async_stream::stream! {
            let mut stream = single_attempt.stream;
            let terminal_proof = single_attempt.terminal;
            let mut terminal = terminal;
            while let Some(item) = stream.next().await {
                match item {
                    Ok(value) => {
                        let terminal_kind = match terminal_proof.outcome() {
                            SingleAttemptStreamOutcome::Finished => Some(ProviderTerminalKind::Finished),
                            SingleAttemptStreamOutcome::Failed => Some(ProviderTerminalKind::Failed),
                            SingleAttemptStreamOutcome::Pending => None,
                        };
                        if let Some(kind) = terminal_kind {
                            if let Err(receipt_error) = terminal.finish(kind).await {
                                yield Err(receipt_error);
                                return;
                            }
                        }
                        yield Ok(value);
                    }
                    Err(provider_error) => {
                        let terminal_kind = match terminal_proof.outcome() {
                            SingleAttemptStreamOutcome::Finished => Some(ProviderTerminalKind::Finished),
                            SingleAttemptStreamOutcome::Failed => Some(ProviderTerminalKind::Failed),
                            SingleAttemptStreamOutcome::Pending => None,
                        };
                        if let Some(kind) = terminal_kind {
                            if let Err(receipt_error) = terminal.finish(kind).await {
                                yield Err(ProviderError::ExecutionError(format!(
                                    "provider stream failed ({provider_error}); physical provider lifecycle terminal receipt failed: {receipt_error}"
                                )));
                                return;
                            }
                        }
                        yield Err(provider_error);
                        return;
                    }
                }
            }
            let terminal_kind = match terminal_proof.outcome() {
                SingleAttemptStreamOutcome::Finished => Some(ProviderTerminalKind::Finished),
                SingleAttemptStreamOutcome::Failed => Some(ProviderTerminalKind::Failed),
                SingleAttemptStreamOutcome::Pending => None,
            };
            match terminal_kind {
                Some(kind) => {
                    if let Err(receipt_error) = terminal.finish(kind).await {
                        yield Err(receipt_error);
                    }
                }
                None => {
                    yield Err(ProviderError::ExecutionError(
                        "single-attempt stream ended without explicit provider terminal proof"
                            .to_string(),
                    ));
                }
            }
        }))
    }

    async fn stream_once(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        self.stream(model_config, system, messages, tools).await
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
        AuthorityScope, HostCapacityEvidence, LocalCompletionKind, NullSink,
        PhysicalAdmissionControl, PhysicalFleetSnapshot, SourceRevisionKind, TaskVersion,
        VerifiedPhysicalIdentity, WorkOpportunity, WorkRole,
    };
    use std::time::Duration;

    const TRANSPORT_A: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TRANSPORT_B: &str =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[derive(Clone)]
    enum Behavior {
        Finished,
        BareEof,
        Failed,
        NetworkFailed,
        StreamFailed,
        Pending,
        StartPending(Arc<tokio::sync::Notify>),
    }

    struct MockProvider {
        behavior: Behavior,
    }

    struct RetryOnlyProvider;

    struct WrongTransportProvider;

    #[async_trait]
    impl Provider for MockProvider {
        fn get_name(&self) -> &str {
            "mock"
        }

        fn transport_identity(&self, _model_name: &str) -> Option<String> {
            Some(TRANSPORT_A.to_string())
        }

        fn supports_single_attempt_streaming(&self) -> bool {
            true
        }

        fn supports_terminal_proven_single_attempt_streaming(&self) -> bool {
            true
        }

        fn single_attempt_failure_provenance(
            &self,
            error: &ProviderError,
        ) -> SingleAttemptFailureProvenance {
            if matches!(error, ProviderError::ServerError(_)) {
                SingleAttemptFailureProvenance::TerminalResponse
            } else {
                SingleAttemptFailureProvenance::Unresolved
            }
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            Err(ProviderError::ExecutionError(
                "retry-capable stream path must not be called".to_string(),
            ))
        }

        async fn stream_once(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            Ok(self
                .stream_once_with_terminal_proof(_model_config, _system, _messages, _tools)
                .await?
                .stream)
        }

        async fn stream_once_with_terminal_proof(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<goose::providers::base::SingleAttemptStream, ProviderError> {
            match &self.behavior {
                Behavior::Finished => Ok(goose::providers::base::SingleAttemptStream::finished(
                    Box::pin(stream::once(async {
                        Ok((
                            Some(Message::assistant().with_text("ok")),
                            Some(ProviderUsage::new("mock".to_string(), Usage::default())),
                        ))
                    })),
                )),
                Behavior::BareEof => Ok(goose::providers::base::SingleAttemptStream::new(
                    Box::pin(stream::empty()),
                    goose::providers::base::SingleAttemptTerminalProof::default(),
                )),
                Behavior::Failed => Err(ProviderError::ServerError("mock failure".to_string())),
                Behavior::NetworkFailed => {
                    Err(ProviderError::NetworkError("mock network loss".to_string()))
                }
                Behavior::StreamFailed => Ok(goose::providers::base::SingleAttemptStream::new(
                    Box::pin(stream::once(async {
                        Err(ProviderError::NetworkError(
                            "mock mid-stream loss".to_string(),
                        ))
                    })),
                    goose::providers::base::SingleAttemptTerminalProof::default(),
                )),
                Behavior::Pending => Ok(goose::providers::base::SingleAttemptStream::new(
                    Box::pin(stream::pending()),
                    goose::providers::base::SingleAttemptTerminalProof::default(),
                )),
                Behavior::StartPending(entered) => {
                    entered.notify_one();
                    futures::future::pending().await
                }
            }
        }
    }

    #[async_trait]
    impl Provider for RetryOnlyProvider {
        fn get_name(&self) -> &str {
            "retry-only"
        }

        fn transport_identity(&self, _model_name: &str) -> Option<String> {
            Some(TRANSPORT_A.to_string())
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            Err(ProviderError::ExecutionError(
                "ordinary stream must not be reached".to_string(),
            ))
        }
    }

    #[async_trait]
    impl Provider for WrongTransportProvider {
        fn get_name(&self) -> &str {
            "wrong-transport"
        }

        fn transport_identity(&self, _model_name: &str) -> Option<String> {
            Some(TRANSPORT_B.to_string())
        }

        fn supports_single_attempt_streaming(&self) -> bool {
            true
        }

        fn supports_terminal_proven_single_attempt_streaming(&self) -> bool {
            true
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            Err(ProviderError::ExecutionError(
                "transport-drift provider must not be called".to_string(),
            ))
        }

        async fn stream_once(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            Err(ProviderError::ExecutionError(
                "transport-drift provider must not be called".to_string(),
            ))
        }
    }

    async fn admitted() -> (PhysicalAdmissionControl, goose_swarm::AdmittedWork) {
        let identity = VerifiedPhysicalIdentity {
            host_id: "host-a".to_string(),
            model_instance_id: "instance-a".to_string(),
            provider_transport_id: TRANSPORT_A.to_string(),
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
            authority_scope: AuthorityScope::new("provider-lifecycle-replay", "build"),
            phase_epoch: 0,
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
    async fn lifecycle_scope_is_visible_only_while_admitted() {
        let (_, work) = admitted().await;
        assert!(!provider_lifecycle_active());
        scope_provider_lifecycle(work.lifecycle(), async {
            assert!(provider_lifecycle_active());
            let provider = bind_current_provider_lifecycle(Arc::new(MockProvider {
                behavior: Behavior::Finished,
            }));
            assert_eq!(
                provider.transport_identity("model-a").as_deref(),
                Some(TRANSPORT_A)
            );
        })
        .await;
        assert!(!provider_lifecycle_active());
        work.complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn retry_only_provider_is_rejected_before_a_request_starts() {
        let (control, work) = admitted().await;
        let provider = scope_provider_lifecycle(work.lifecycle(), async {
            bind_current_provider_lifecycle(Arc::new(RetryOnlyProvider))
        })
        .await;
        let error = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .err()
            .expect("provider without stream_once must fail closed");
        assert!(matches!(error, ProviderError::NotImplemented(_)));
        work.complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
        assert_eq!(control.occupancy().await, (0, 0));
    }

    #[tokio::test]
    async fn transport_drift_is_rejected_before_a_request_starts() {
        let (control, work) = admitted().await;
        let provider = scope_provider_lifecycle(work.lifecycle(), async {
            bind_current_provider_lifecycle(Arc::new(WrongTransportProvider))
        })
        .await;
        let error = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .err()
            .expect("provider transport drift must fail closed");
        assert!(error.to_string().contains("transport"));
        work.complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
        assert_eq!(control.occupancy().await, (0, 0));
    }

    #[tokio::test]
    async fn explicit_stream_terminal_records_finished_before_local_success() {
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
    async fn bare_stream_eof_keeps_provider_claim_unresolved() {
        let (control, work) = admitted().await;
        let provider = wrapped_for(Behavior::BareEof, work.lifecycle()).await;
        let mut output = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        let error = output.next().await.unwrap().unwrap_err();
        assert!(error
            .to_string()
            .contains("without explicit provider terminal"));
        assert!(tokio::time::timeout(
            Duration::from_millis(50),
            work.complete_local(LocalCompletionKind::Error),
        )
        .await
        .is_err());
        assert_eq!(control.occupancy().await, (0, 1));
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
    async fn network_error_before_stream_keeps_provider_claim_unresolved() {
        let (control, work) = admitted().await;
        let provider = wrapped_for(Behavior::NetworkFailed, work.lifecycle()).await;
        assert!(provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .is_err());
        assert!(tokio::time::timeout(
            Duration::from_millis(50),
            work.complete_local(LocalCompletionKind::Error),
        )
        .await
        .is_err());
        assert_eq!(control.occupancy().await, (0, 1));
    }

    #[tokio::test]
    async fn mid_stream_error_keeps_provider_claim_unresolved() {
        let (control, work) = admitted().await;
        let provider = wrapped_for(Behavior::StreamFailed, work.lifecycle()).await;
        let mut output = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        assert!(output.next().await.unwrap().is_err());
        assert!(tokio::time::timeout(
            Duration::from_millis(50),
            work.complete_local(LocalCompletionKind::StreamDropped),
        )
        .await
        .is_err());
        assert_eq!(control.occupancy().await, (0, 1));
    }

    #[tokio::test]
    async fn dropped_stream_keeps_provider_claim_unresolved() {
        let (control, work) = admitted().await;
        let provider = wrapped_for(Behavior::Pending, work.lifecycle()).await;
        let output = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        drop(output);
        assert!(tokio::time::timeout(
            Duration::from_millis(50),
            work.complete_local(LocalCompletionKind::StreamDropped),
        )
        .await
        .is_err());
        assert_eq!(control.occupancy().await, (0, 1));
    }

    #[tokio::test]
    async fn cancelled_stream_creation_keeps_provider_claim_unresolved() {
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
        assert!(tokio::time::timeout(
            Duration::from_millis(50),
            work.complete_local(LocalCompletionKind::CancellationRequested),
        )
        .await
        .is_err());
        assert_eq!(control.occupancy().await, (0, 1));
    }
}
