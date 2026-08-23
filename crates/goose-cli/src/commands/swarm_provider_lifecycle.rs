use async_trait::async_trait;
use futures::StreamExt;
use goose::config::GooseMode;
use goose::conversation::message::Message;
use goose::providers::base::{
    MessageStream, ModelInfo, PermissionRouting, Provider, ProviderHttpProtocol,
    SingleAttemptFailureProvenance, SingleAttemptStream, SingleAttemptStreamOutcome,
};
use goose_provider_types::base::{
    ProviderStreamChunkKind, ProviderStreamProgressSink, scope_provider_stream_progress,
};
use goose_provider_types::errors::ProviderError;
use goose_provider_types::model::ModelConfig;
use goose_provider_types::permission::PermissionConfirmation;
use goose_provider_types::retry::RetryConfig;
use goose_swarm::{
    ProviderLifecycle, ProviderNudgeDelivery, ProviderTerminalKind, StartedProviderRequest,
};
use rmcp::model::Tool;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use tokio::sync::Notify;

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

pub(crate) trait ProviderNudgeDeliveryFactory: Send + Sync {
    fn open(&self) -> Arc<dyn ProviderNudgeDelivery>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreSchedulerProviderTerminalKind {
    Finished,
    Failed,
    Cancelled,
    Unproven,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreSchedulerProviderLifecyclePhase {
    Started,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PreSchedulerProviderLifecycleEvent {
    pub(crate) generation: u64,
    pub(crate) phase: PreSchedulerProviderLifecyclePhase,
    pub(crate) terminal: Option<PreSchedulerProviderTerminalKind>,
    pub(crate) successful: bool,
    pub(crate) physical_broker_accounting: &'static str,
    pub(crate) payload_logged: bool,
}

struct PreSchedulerActiveProviderRequest {
    generation: u64,
    delivery: Arc<dyn ProviderNudgeDelivery>,
}

#[derive(Default)]
struct PreSchedulerProviderControlState {
    next_generation: u64,
    active: Option<PreSchedulerActiveProviderRequest>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PreSchedulerNudgeCapture {
    pub(crate) generation: u64,
    structured_output_bytes: u64,
}

pub(crate) struct PreSchedulerProviderControl {
    state: Mutex<PreSchedulerProviderControlState>,
    observer: Arc<dyn Fn(PreSchedulerProviderLifecycleEvent) + Send + Sync>,
}

impl PreSchedulerProviderControl {
    pub(crate) fn new(
        observer: Arc<dyn Fn(PreSchedulerProviderLifecycleEvent) + Send + Sync>,
    ) -> Self {
        Self {
            state: Mutex::new(PreSchedulerProviderControlState::default()),
            observer,
        }
    }

    fn state(&self) -> MutexGuard<'_, PreSchedulerProviderControlState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn begin(&self, delivery: Arc<dyn ProviderNudgeDelivery>) -> u64 {
        let generation = {
            let mut state = self.state();
            state.next_generation = state.next_generation.saturating_add(1);
            let generation = state.next_generation;
            state.active = Some(PreSchedulerActiveProviderRequest {
                generation,
                delivery,
            });
            generation
        };
        (self.observer)(PreSchedulerProviderLifecycleEvent {
            generation,
            phase: PreSchedulerProviderLifecyclePhase::Started,
            terminal: None,
            successful: false,
            physical_broker_accounting: "unavailable_pre_scheduler",
            payload_logged: false,
        });
        generation
    }

    fn finish(&self, generation: u64, terminal: PreSchedulerProviderTerminalKind) {
        let admitted = {
            let mut state = self.state();
            if state.active.as_ref().map(|active| active.generation) != Some(generation) {
                false
            } else {
                state.active = None;
                true
            }
        };
        if admitted {
            (self.observer)(PreSchedulerProviderLifecycleEvent {
                generation,
                phase: PreSchedulerProviderLifecyclePhase::Terminal,
                terminal: Some(terminal),
                successful: terminal == PreSchedulerProviderTerminalKind::Finished,
                physical_broker_accounting: "unavailable_pre_scheduler",
                payload_logged: false,
            });
        }
    }

    pub(crate) fn capture(
        &self,
        progress: ProviderStreamProgressSnapshot,
    ) -> Option<PreSchedulerNudgeCapture> {
        self.state()
            .active
            .as_ref()
            .map(|active| PreSchedulerNudgeCapture {
                generation: active.generation,
                structured_output_bytes: progress.structured_output_bytes,
            })
    }

    pub(crate) fn try_enqueue_nudge(
        &self,
        capture: PreSchedulerNudgeCapture,
        progress: ProviderStreamProgressSnapshot,
        guidance: String,
    ) -> std::result::Result<(), String> {
        if progress.structured_output_active
            || progress.structured_output_bytes > capture.structured_output_bytes
        {
            return Err(
                "provider is decoding structured output; supervision remains observational"
                    .to_string(),
            );
        }
        let delivery = self
            .state()
            .active
            .as_ref()
            .filter(|active| active.generation == capture.generation)
            .map(|active| active.delivery.clone())
            .ok_or_else(|| "captured provider request is no longer active".to_string())?;
        delivery.try_enqueue(guidance)
    }
}

pub(crate) fn bind_pre_scheduler_provider_lifecycle(
    provider: Arc<dyn Provider>,
    nudge_factory: Arc<dyn ProviderNudgeDeliveryFactory>,
    control: Arc<PreSchedulerProviderControl>,
) -> Arc<dyn Provider> {
    Arc::new(PreSchedulerLifecycleProvider {
        inner: provider,
        nudge_factory,
        control,
    })
}

pub(crate) fn bind_current_provider_lifecycle(
    provider: Arc<dyn Provider>,
    nudge_factory: Option<Arc<dyn ProviderNudgeDeliveryFactory>>,
    stream_progress: Option<Arc<ProviderStreamProgressMeter>>,
) -> Arc<dyn Provider> {
    let lifecycle_bound = ACTIVE_PROVIDER_LIFECYCLE
        .try_with(|lifecycle| {
            Arc::new(LifecycleProvider {
                inner: provider.clone(),
                lifecycle: lifecycle.clone(),
                nudge_factory: nudge_factory.clone(),
            }) as Arc<dyn Provider>
        })
        .unwrap_or(provider);
    match stream_progress {
        Some(progress) => Arc::new(StreamProgressProvider {
            inner: lifecycle_bound,
            progress,
        }),
        None => lifecycle_bound,
    }
}

struct LifecycleProvider {
    inner: Arc<dyn Provider>,
    lifecycle: ProviderLifecycle,
    nudge_factory: Option<Arc<dyn ProviderNudgeDeliveryFactory>>,
}

struct PreSchedulerLifecycleProvider {
    inner: Arc<dyn Provider>,
    nudge_factory: Arc<dyn ProviderNudgeDeliveryFactory>,
    control: Arc<PreSchedulerProviderControl>,
}

struct StreamProgressProvider {
    inner: Arc<dyn Provider>,
    progress: Arc<ProviderStreamProgressMeter>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderStreamProgressSnapshot {
    pub(crate) revision: u64,
    pub(crate) chunks: u64,
    pub(crate) bytes: u64,
    pub(crate) structured_output_chunks: u64,
    pub(crate) structured_output_bytes: u64,
    pub(crate) last_progress_elapsed_ms: u64,
    pub(crate) structured_output_active: bool,
}

pub(crate) struct ProviderStreamProgressMeter {
    started: Instant,
    snapshot: Mutex<ProviderStreamProgressSnapshot>,
    changed: Notify,
}

impl ProviderStreamProgressMeter {
    pub(crate) fn new() -> Self {
        Self {
            started: Instant::now(),
            snapshot: Mutex::new(ProviderStreamProgressSnapshot::default()),
            changed: Notify::new(),
        }
    }

    fn state(&self) -> MutexGuard<'_, ProviderStreamProgressSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub(crate) fn snapshot(&self) -> ProviderStreamProgressSnapshot {
        *self.state()
    }

    pub(crate) async fn changed_since(&self, revision: u64) -> ProviderStreamProgressSnapshot {
        loop {
            let notified = self.changed.notified();
            let snapshot = self.snapshot();
            if snapshot.revision > revision {
                return snapshot;
            }
            notified.await;
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}

impl ProviderStreamProgressSink for ProviderStreamProgressMeter {
    fn record_decoded_chunk(&self, decoded_bytes: usize, kind: ProviderStreamChunkKind) {
        let mut snapshot = self.state();
        snapshot.revision = snapshot.revision.saturating_add(1);
        snapshot.chunks = snapshot.chunks.saturating_add(1);
        snapshot.bytes = snapshot.bytes.saturating_add(decoded_bytes as u64);
        if kind == ProviderStreamChunkKind::StructuredOutput {
            snapshot.structured_output_chunks = snapshot.structured_output_chunks.saturating_add(1);
            snapshot.structured_output_bytes = snapshot
                .structured_output_bytes
                .saturating_add(decoded_bytes as u64);
            snapshot.structured_output_active = true;
        }
        snapshot.last_progress_elapsed_ms = self.elapsed_ms();
        drop(snapshot);
        self.changed.notify_waiters();
    }

    fn structured_output_completed(&self) {
        let mut snapshot = self.state();
        if !snapshot.structured_output_active {
            return;
        }
        snapshot.revision = snapshot.revision.saturating_add(1);
        snapshot.structured_output_active = false;
        snapshot.last_progress_elapsed_ms = self.elapsed_ms();
        drop(snapshot);
        self.changed.notify_waiters();
    }
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

    async fn finish_cooperatively(
        &mut self,
        requested: ProviderTerminalKind,
        delivery: Option<&Arc<dyn ProviderNudgeDelivery>>,
    ) -> Result<(), ProviderError> {
        let kind = match delivery {
            Some(delivery) if !delivery.natural_terminal_allowed() => {
                delivery.cancelled().await;
                ProviderTerminalKind::Cancelled
            }
            _ => requested,
        };
        self.finish(kind).await
    }
}

fn lifecycle_error(action: &str, error: impl std::fmt::Display) -> ProviderError {
    ProviderError::ExecutionError(format!(
        "physical provider lifecycle {action} failed: {error}"
    ))
}

fn observe_provider_stream(
    mut stream: MessageStream,
    progress: Arc<ProviderStreamProgressMeter>,
) -> MessageStream {
    Box::pin(async_stream::stream! {
        loop {
            let item = scope_provider_stream_progress(progress.clone(), stream.next()).await;
            let Some(item) = item else { break; };
            yield item;
        }
    })
}

#[async_trait]
impl Provider for StreamProgressProvider {
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

    fn single_attempt_failure_provenance(
        &self,
        error: &ProviderError,
    ) -> SingleAttemptFailureProvenance {
        self.inner.single_attempt_failure_provenance(error)
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let stream = self
            .inner
            .stream(model_config, system, messages, tools)
            .await?;
        Ok(observe_provider_stream(stream, self.progress.clone()))
    }

    async fn stream_once(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let stream = self
            .inner
            .stream_once(model_config, system, messages, tools)
            .await?;
        Ok(observe_provider_stream(stream, self.progress.clone()))
    }

    async fn stream_once_with_terminal_proof(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<SingleAttemptStream, ProviderError> {
        let attempt = self
            .inner
            .stream_once_with_terminal_proof(model_config, system, messages, tools)
            .await?;
        Ok(SingleAttemptStream::new(
            observe_provider_stream(attempt.stream, self.progress.clone()),
            attempt.terminal,
        ))
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

async fn finish_pre_scheduler_naturally(
    control: &PreSchedulerProviderControl,
    generation: u64,
    requested: PreSchedulerProviderTerminalKind,
    delivery: &Arc<dyn ProviderNudgeDelivery>,
) -> PreSchedulerProviderTerminalKind {
    let terminal = if delivery.natural_terminal_allowed() {
        requested
    } else {
        delivery.cancelled().await;
        PreSchedulerProviderTerminalKind::Cancelled
    };
    control.finish(generation, terminal);
    terminal
}

#[async_trait]
impl Provider for PreSchedulerLifecycleProvider {
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

    fn single_attempt_failure_provenance(
        &self,
        error: &ProviderError,
    ) -> SingleAttemptFailureProvenance {
        self.inner.single_attempt_failure_provenance(error)
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        if !self
            .inner
            .supports_terminal_proven_single_attempt_streaming()
        {
            return Err(ProviderError::NotImplemented(format!(
                "provider `{}` has no terminal-proven single-attempt stream boundary for pre-scheduler supervision",
                self.inner.get_name()
            )));
        }

        let delivery = self.nudge_factory.open();
        let generation = self.control.begin(delivery.clone());
        let attempt = tokio::select! {
            biased;
            _ = delivery.cancelled() => None,
            result = self.inner.stream_once_with_terminal_proof(
                model_config,
                system,
                messages,
                tools,
            ) => Some(result),
        };
        let Some(attempt) = attempt else {
            self.control
                .finish(generation, PreSchedulerProviderTerminalKind::Cancelled);
            return Ok(Box::pin(futures::stream::empty()));
        };
        let attempt = match attempt {
            Ok(attempt) => attempt,
            Err(error) => {
                let requested = if self.inner.single_attempt_failure_provenance(&error)
                    == SingleAttemptFailureProvenance::TerminalResponse
                {
                    PreSchedulerProviderTerminalKind::Failed
                } else {
                    PreSchedulerProviderTerminalKind::Unproven
                };
                let terminal =
                    finish_pre_scheduler_naturally(&self.control, generation, requested, &delivery)
                        .await;
                if terminal == PreSchedulerProviderTerminalKind::Cancelled {
                    return Ok(Box::pin(futures::stream::empty()));
                }
                return Err(error);
            }
        };
        let control = self.control.clone();
        Ok(Box::pin(async_stream::stream! {
            let mut stream = attempt.stream;
            let terminal_proof = attempt.terminal;
            loop {
                let next = tokio::select! {
                    biased;
                    _ = delivery.cancelled() => {
                        control.finish(generation, PreSchedulerProviderTerminalKind::Cancelled);
                        return;
                    }
                    item = stream.next() => item,
                };
                let Some(item) = next else { break; };
                let proven = match terminal_proof.outcome() {
                    SingleAttemptStreamOutcome::Finished => {
                        Some(PreSchedulerProviderTerminalKind::Finished)
                    }
                    SingleAttemptStreamOutcome::Failed => {
                        Some(PreSchedulerProviderTerminalKind::Failed)
                    }
                    SingleAttemptStreamOutcome::Pending => None,
                };
                if let Some(requested) = proven {
                    let terminal = finish_pre_scheduler_naturally(
                        &control,
                        generation,
                        requested,
                        &delivery,
                    )
                    .await;
                    if terminal == PreSchedulerProviderTerminalKind::Cancelled {
                        return;
                    }
                }
                yield item;
                if proven.is_some() {
                    return;
                }
            }

            let requested = match terminal_proof.outcome() {
                SingleAttemptStreamOutcome::Finished => {
                    PreSchedulerProviderTerminalKind::Finished
                }
                SingleAttemptStreamOutcome::Failed => PreSchedulerProviderTerminalKind::Failed,
                SingleAttemptStreamOutcome::Pending => {
                    control.finish(generation, PreSchedulerProviderTerminalKind::Unproven);
                    yield Err(ProviderError::ExecutionError(
                        "pre-scheduler single-attempt stream ended without explicit provider terminal proof"
                            .to_string(),
                    ));
                    return;
                }
            };
            let terminal = finish_pre_scheduler_naturally(
                &control,
                generation,
                requested,
                &delivery,
            )
            .await;
            if terminal == PreSchedulerProviderTerminalKind::Cancelled {
                return;
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

    async fn stream_once_with_terminal_proof(
        &self,
        _model_config: &ModelConfig,
        _system: &str,
        _messages: &[Message],
        _tools: &[Tool],
    ) -> Result<SingleAttemptStream, ProviderError> {
        Err(ProviderError::NotImplemented(
            "nested pre-scheduler lifecycle wrapping is forbidden".to_string(),
        ))
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
        let nudge_delivery = self.nudge_factory.as_ref().map(|factory| factory.open());
        let publication = match &nudge_delivery {
            Some(delivery) => started.publish_for_scheduler_with_nudge_delivery(delivery.clone()),
            None => started.publish_for_scheduler(),
        };
        if let Err(error) = publication {
            let detail = format!("provider start publication failed: {error}");
            return match started.abandon_before_exposure(&detail).await {
                Ok(()) => Err(lifecycle_error("provider start publication", error)),
                Err(abandon_error) => Err(ProviderError::ExecutionError(format!(
                    "{detail}; physical provider lifecycle abandon failed: {abandon_error}"
                ))),
            };
        }
        let single_attempt_result = if let Some(delivery) = &nudge_delivery {
            tokio::select! {
                biased;
                _ = delivery.cancelled() => None,
                result = started.scope_http(self.inner.stream_once_with_terminal_proof(
                    model_config,
                    system,
                    messages,
                    tools,
                )) => Some(result),
            }
        } else {
            Some(
                started
                    .scope_http(self.inner.stream_once_with_terminal_proof(
                        model_config,
                        system,
                        messages,
                        tools,
                    ))
                    .await,
            )
        };
        let mut terminal = ProviderTerminalGuard::new(started);
        let Some(single_attempt_result) = single_attempt_result else {
            terminal.finish(ProviderTerminalKind::Cancelled).await?;
            return Ok(Box::pin(futures::stream::empty()));
        };
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
            loop {
                let next = match &nudge_delivery {
                    Some(delivery) => tokio::select! {
                        biased;
                        _ = delivery.cancelled() => {
                            if let Err(receipt_error) = terminal.finish(ProviderTerminalKind::Cancelled).await {
                                yield Err(receipt_error);
                            }
                            return;
                        }
                        item = stream.next() => item,
                    },
                    None => stream.next().await,
                };
                let Some(item) = next else { break; };
                match item {
                    Ok(value) => {
                        let terminal_kind = match terminal_proof.outcome() {
                            SingleAttemptStreamOutcome::Finished => Some(ProviderTerminalKind::Finished),
                            SingleAttemptStreamOutcome::Failed => Some(ProviderTerminalKind::Failed),
                            SingleAttemptStreamOutcome::Pending => None,
                        };
                        if let Some(kind) = terminal_kind {
                            if let Err(receipt_error) = terminal
                                .finish_cooperatively(kind, nudge_delivery.as_ref())
                                .await
                            {
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
                            if let Err(receipt_error) = terminal
                                .finish_cooperatively(kind, nudge_delivery.as_ref())
                                .await
                            {
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
                    if let Err(receipt_error) = terminal
                        .finish_cooperatively(kind, nudge_delivery.as_ref())
                        .await
                    {
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
        PhysicalAdmissionControl, PhysicalFleetSnapshot, ProviderStartKey,
        ProviderStartLookupError, SourceRevisionKind, TaskVersion, VerifiedPhysicalIdentity,
        WorkOpportunity, WorkRole,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    struct ProgressOnlyProvider;

    #[derive(Default)]
    struct TestNudgeDelivery {
        guidance: Mutex<Vec<String>>,
        cancelled: tokio::sync::Notify,
        is_cancelled: std::sync::atomic::AtomicBool,
    }

    #[async_trait]
    impl ProviderNudgeDelivery for TestNudgeDelivery {
        fn try_enqueue(&self, guidance: String) -> std::result::Result<(), String> {
            self.guidance.lock().unwrap().push(guidance);
            self.is_cancelled.store(true, Ordering::Release);
            self.cancelled.notify_waiters();
            Ok(())
        }

        fn natural_terminal_allowed(&self) -> bool {
            !self.is_cancelled.load(Ordering::Acquire)
        }

        async fn cancelled(&self) {
            while !self.is_cancelled.load(Ordering::Acquire) {
                self.cancelled.notified().await;
            }
        }
    }

    #[derive(Default)]
    struct TestNudgeFactory {
        deliveries: Mutex<Vec<Arc<TestNudgeDelivery>>>,
    }

    impl TestNudgeFactory {
        fn latest(&self) -> Arc<TestNudgeDelivery> {
            self.deliveries
                .lock()
                .unwrap()
                .last()
                .cloned()
                .expect("provider request opened a nudge delivery")
        }
    }

    impl ProviderNudgeDeliveryFactory for TestNudgeFactory {
        fn open(&self) -> Arc<dyn ProviderNudgeDelivery> {
            let delivery = Arc::new(TestNudgeDelivery::default());
            self.deliveries.lock().unwrap().push(delivery.clone());
            delivery
        }
    }

    struct PendingThenFinishedProvider {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl Provider for PendingThenFinishedProvider {
        fn get_name(&self) -> &str {
            "pending-then-finished"
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
                "supervised provider must use one-attempt streaming".to_string(),
            ))
        }

        async fn stream_once_with_terminal_proof(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<SingleAttemptStream, ProviderError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(SingleAttemptStream::new(
                    Box::pin(stream::pending()),
                    goose::providers::base::SingleAttemptTerminalProof::default(),
                ))
            } else {
                Ok(SingleAttemptStream::finished(Box::pin(stream::once(
                    async {
                        Ok((
                            Some(Message::assistant().with_text("restarted")),
                            Some(ProviderUsage::new("mock".to_string(), Usage::default())),
                        ))
                    },
                ))))
            }
        }
    }

    #[async_trait]
    impl Provider for ProgressOnlyProvider {
        fn get_name(&self) -> &str {
            "progress-only"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            Ok(Box::pin(stream::once(async {
                goose_provider_types::base::record_current_provider_stream_chunk(
                    4096,
                    ProviderStreamChunkKind::StructuredOutput,
                );
                Ok((None, None))
            })))
        }
    }

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
            bind_current_provider_lifecycle(Arc::new(MockProvider { behavior }), None, None)
        })
        .await
    }

    #[tokio::test]
    async fn progress_observer_wraps_planning_calls_without_provider_lifecycle() {
        assert!(!provider_lifecycle_active());
        let progress = Arc::new(ProviderStreamProgressMeter::new());
        let provider = bind_current_provider_lifecycle(
            Arc::new(ProgressOnlyProvider),
            None,
            Some(progress.clone()),
        );
        let mut stream = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        stream.next().await.unwrap().unwrap();

        let snapshot = progress.snapshot();
        assert_eq!(snapshot.revision, 1);
        assert_eq!(snapshot.chunks, 1);
        assert_eq!(snapshot.bytes, 4096);
        assert_eq!(snapshot.structured_output_chunks, 1);
        assert_eq!(snapshot.structured_output_bytes, 4096);
        assert!(snapshot.structured_output_active);
    }

    #[tokio::test]
    async fn zero_byte_dead_call_is_nudged_then_restarted_after_cancel_terminal_proof() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = events.clone();
        let control = Arc::new(PreSchedulerProviderControl::new(Arc::new(move |event| {
            recorded.lock().unwrap().push(event);
        })));
        let factory = Arc::new(TestNudgeFactory::default());
        let provider = bind_pre_scheduler_provider_lifecycle(
            Arc::new(PendingThenFinishedProvider {
                calls: AtomicUsize::new(0),
            }),
            factory.clone(),
            control.clone(),
        );
        let progress = ProviderStreamProgressSnapshot::default();
        let mut first = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        let capture = control
            .capture(progress)
            .expect("the zero-byte request is active");
        control
            .try_enqueue_nudge(
                capture,
                progress,
                "continue from the same session".to_string(),
            )
            .unwrap();
        assert!(first.next().await.is_none());

        let cancellation_index = events
            .lock()
            .unwrap()
            .iter()
            .position(|event| event.terminal == Some(PreSchedulerProviderTerminalKind::Cancelled))
            .expect("cancellation terminal proof precedes continuation");
        let mut restarted = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        let item = restarted.next().await.unwrap().unwrap();
        assert_eq!(item.0.unwrap().as_concat_text(), "restarted");
        let second_start_index = events
            .lock()
            .unwrap()
            .iter()
            .rposition(|event| event.phase == PreSchedulerProviderLifecyclePhase::Started)
            .unwrap();
        assert!(cancellation_index < second_start_index);
        assert_eq!(factory.latest().guidance.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn growing_structured_output_cannot_be_interrupted_by_a_stale_judge_capture() {
        let control = Arc::new(PreSchedulerProviderControl::new(Arc::new(|_| {})));
        let factory = Arc::new(TestNudgeFactory::default());
        let provider = bind_pre_scheduler_provider_lifecycle(
            Arc::new(MockProvider {
                behavior: Behavior::Pending,
            }),
            factory.clone(),
            control.clone(),
        );
        let _stream = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        let before = ProviderStreamProgressSnapshot::default();
        let capture = control.capture(before).unwrap();
        let growing = ProviderStreamProgressSnapshot {
            revision: 1,
            chunks: 1,
            bytes: 4096,
            structured_output_chunks: 1,
            structured_output_bytes: 4096,
            last_progress_elapsed_ms: 1,
            structured_output_active: true,
        };
        let error = control
            .try_enqueue_nudge(capture, growing, "must not interrupt".to_string())
            .unwrap_err();
        assert!(error.contains("structured output"));
        assert!(factory.latest().guidance.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancelled_pre_scheduler_request_never_emits_false_success() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = events.clone();
        let control = Arc::new(PreSchedulerProviderControl::new(Arc::new(move |event| {
            recorded.lock().unwrap().push(event);
        })));
        let factory = Arc::new(TestNudgeFactory::default());
        let provider = bind_pre_scheduler_provider_lifecycle(
            Arc::new(MockProvider {
                behavior: Behavior::Pending,
            }),
            factory,
            control.clone(),
        );
        let progress = ProviderStreamProgressSnapshot::default();
        let mut stream = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        control
            .try_enqueue_nudge(
                control.capture(progress).unwrap(),
                progress,
                "retry".to_string(),
            )
            .unwrap();
        assert!(stream.next().await.is_none());
        let terminal = events
            .lock()
            .unwrap()
            .iter()
            .find(|event| event.phase == PreSchedulerProviderLifecyclePhase::Terminal)
            .cloned()
            .unwrap();
        assert_eq!(
            terminal.terminal,
            Some(PreSchedulerProviderTerminalKind::Cancelled)
        );
        assert!(!terminal.successful);
        assert_eq!(
            terminal.physical_broker_accounting,
            "unavailable_pre_scheduler"
        );
        assert!(!terminal.payload_logged);
    }

    #[tokio::test]
    async fn lifecycle_scope_is_visible_only_while_admitted() {
        let (_, work) = admitted().await;
        assert!(!provider_lifecycle_active());
        scope_provider_lifecycle(work.lifecycle(), async {
            assert!(provider_lifecycle_active());
            let provider = bind_current_provider_lifecycle(
                Arc::new(MockProvider {
                    behavior: Behavior::Finished,
                }),
                None,
                None,
            );
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
            bind_current_provider_lifecycle(Arc::new(RetryOnlyProvider), None, None)
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
            bind_current_provider_lifecycle(Arc::new(WrongTransportProvider), None, None)
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
    async fn explicit_stream_terminal_closes_published_provider_start() {
        let (control, work) = admitted().await;
        let provider_start = ProviderStartKey::from_admission(work.receipt());
        let provider = wrapped_for(Behavior::Finished, work.lifecycle()).await;
        let mut output = provider
            .stream(&ModelConfig::new("model-a"), "", &[], &[])
            .await
            .unwrap();
        control
            .provider_start_registry()
            .query(&provider_start)
            .unwrap();
        while let Some(item) = output.next().await {
            item.unwrap();
        }
        assert!(matches!(
            control.provider_start_registry().query(&provider_start),
            Err(ProviderStartLookupError::NotLive { .. })
        ));
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
        assert!(
            error
                .to_string()
                .contains("without explicit provider terminal")
        );
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                work.complete_local(LocalCompletionKind::Error),
            )
            .await
            .is_err()
        );
        assert_eq!(control.occupancy().await, (0, 1));
    }

    #[tokio::test]
    async fn provider_error_records_failed_terminal() {
        let (control, work) = admitted().await;
        let provider = wrapped_for(Behavior::Failed, work.lifecycle()).await;
        assert!(
            provider
                .stream(&ModelConfig::new("model-a"), "", &[], &[])
                .await
                .is_err()
        );
        work.complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
        assert_eq!(control.occupancy().await, (0, 0));
    }

    #[tokio::test]
    async fn network_error_before_stream_keeps_provider_claim_unresolved() {
        let (control, work) = admitted().await;
        let provider = wrapped_for(Behavior::NetworkFailed, work.lifecycle()).await;
        assert!(
            provider
                .stream(&ModelConfig::new("model-a"), "", &[], &[])
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                work.complete_local(LocalCompletionKind::Error),
            )
            .await
            .is_err()
        );
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
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                work.complete_local(LocalCompletionKind::StreamDropped),
            )
            .await
            .is_err()
        );
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
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                work.complete_local(LocalCompletionKind::StreamDropped),
            )
            .await
            .is_err()
        );
        assert_eq!(control.occupancy().await, (0, 1));
    }

    #[tokio::test]
    async fn cancelled_stream_creation_closes_published_provider_start() {
        let (control, work) = admitted().await;
        let provider_start = ProviderStartKey::from_admission(work.receipt());
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
        control
            .provider_start_registry()
            .query(&provider_start)
            .expect("LifecycleProvider must publish before entering provider HTTP");
        task.abort();
        let _ = task.await;
        assert!(matches!(
            control.provider_start_registry().query(&provider_start),
            Err(ProviderStartLookupError::NotLive { .. })
        ));
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                work.complete_local(LocalCompletionKind::CancellationRequested),
            )
            .await
            .is_err()
        );
        assert_eq!(control.occupancy().await, (0, 1));
    }
}
