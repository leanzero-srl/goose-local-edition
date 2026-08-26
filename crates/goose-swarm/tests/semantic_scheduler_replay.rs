use async_trait::async_trait;
use goose_provider_types::base::{expose_current_provider_http_request, ProviderHttpProtocol};
use goose_swarm::{
    AdmissionReceipt, AdmittedSemanticObservationRequest, AdmittedSemanticObservationReviewer,
    AdmittedSemanticReviewError, AuthorityScope, CompletedProviderRequest, Dag, DeviceCfg,
    Difficulty, DispatchError, DispatchRequest, EventSink, GlobalProviderLeaseAuthority,
    HostCapacityEvidence, PhysicalAdmissionControl, PhysicalExecutionAuthority,
    PhysicalFleetSnapshot, ProviderLeaseWaitPolicy, ProviderLifecycle, ProviderLifecycleDispatcher,
    ProviderLifecycleJournal, ProviderNudgeDelivery, ProviderNudgeSafetySnapshot,
    ProviderRequestReceipt, ProviderTerminalKind, ProviderTerminalReceipt,
    RunScopedProviderLeaseAuthority, Scheduler, SealedProviderLeaseAuthority,
    SemanticObservationCapture, SemanticObservationCaptureRequest,
    SemanticObservationSnapshotDraft, SemanticObservationSnapshotProducer,
    SemanticObservationSummonsSignal, SemanticTraceSnapshot, SwarmEvent, TaskRunOutput, TaskSpec,
    TraceStateMeasurement, VerifiedPhysicalLane, VerifiedProviderProtocolRoute, WorkRole,
    SEMANTIC_OBSERVATION_PROTOCOL, SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

const VERIFIED_TRANSPORT: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<serde_json::Value>>,
}

impl RecordingSink {
    fn count(&self, event_name: &str) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event["event"] == event_name)
            .count()
    }

    fn semantic_admissions(&self) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                event["event"] == "broker_admission_granted"
                    && event["receipt"]["role"] == "semantic_judge_observation"
            })
            .count()
    }

    fn worker_terminal_kinds(&self, task_id: &str) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                event["event"] == "broker_provider_terminal_observed"
                    && event["admission"]["source"]["task_id"] == task_id
                    && event["admission"]["role"] == "build"
            })
            .filter_map(|event| event["receipt"]["kind"].as_str().map(str::to_string))
            .collect()
    }

    fn capture_failure_reasons(&self, task_id: &str) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                event["event"] == "semantic_observation_capture_failed"
                    && event["task_id"] == task_id
            })
            .filter_map(|event| event["reason"].as_str().map(str::to_string))
            .collect()
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, event: &SwarmEvent) {
        self.events
            .lock()
            .unwrap()
            .push(serde_json::to_value(event).unwrap());
    }

    fn write_value(&self, value: serde_json::Value) {
        self.events.lock().unwrap().push(value);
    }
}

fn lane(device: &str, model: &str, host: &str, capacity: u32) -> VerifiedPhysicalLane {
    VerifiedPhysicalLane {
        logical_device_id: device.to_string(),
        model_id: model.to_string(),
        host_id: host.to_string(),
        model_instance_id: format!("instance:{device}"),
        provider_transport_id: VERIFIED_TRANSPORT.to_string(),
        advertised_instance_capacity: capacity,
        routing_weight: 1,
        capacity_evidence: HostCapacityEvidence::MeasuredProfile {
            profile_hash: format!("profile:{host}:{capacity}"),
            profile_key: "test-runtime:model:semantic-observer".to_string(),
            max_concurrent: capacity,
        },
        route_evidence_id: format!("route:{host}:{device}"),
    }
}

fn device(id: &str, model_id: &str) -> DeviceCfg {
    DeviceCfg {
        id: id.to_string(),
        model_id: model_id.to_string(),
        weight: 1,
        enabled: true,
        speed_weight: 1,
        supervision: false,
    }
}

fn task(id: &str) -> TaskSpec {
    TaskSpec {
        id: id.to_string(),
        description: format!("Implement the exact contract for {id} and prove its output"),
        difficulty: Difficulty::Easy,
        preferred_model: None,
        owned_files: vec![format!("{id}.txt")],
        deps: Vec::new(),
        subsplit: Vec::new(),
        replan_authority: None,
    }
}

fn control(
    scope: &str,
    lanes: Vec<VerifiedPhysicalLane>,
    sink: Arc<dyn EventSink>,
    authority_root: &Path,
) -> PhysicalAdmissionControl {
    let snapshot = PhysicalFleetSnapshot::new(format!("snapshot:{scope}"), lanes).unwrap();
    let sealed = SealedProviderLeaseAuthority::from_fleet_snapshot(
        &snapshot,
        [VerifiedProviderProtocolRoute::new(
            VERIFIED_TRANSPORT,
            ProviderHttpProtocol::OpenAiChatCompletions,
        )
        .unwrap()],
    )
    .unwrap();
    let global = Arc::new(
        GlobalProviderLeaseAuthority::open_test_root(authority_root.join("authority")).unwrap(),
    );
    let leases = RunScopedProviderLeaseAuthority::new_with_wait_policy(
        global,
        sealed,
        ProviderLeaseWaitPolicy::new(Duration::from_millis(1)),
    );
    PhysicalAdmissionControl::new_with_journal_and_provider_leases(
        scope,
        snapshot,
        sink,
        Arc::new(NullJournal),
        Some(leases),
    )
    .unwrap()
}

struct NullJournal;

impl ProviderLifecycleJournal for NullJournal {
    fn provider_request_started(&self, _receipt: &ProviderRequestReceipt) -> Result<(), String> {
        Ok(())
    }

    fn provider_terminal(&self, _receipt: &ProviderTerminalReceipt) -> Result<(), String> {
        Ok(())
    }
}

fn execution() -> PhysicalExecutionAuthority {
    PhysicalExecutionAuthority::new(
        AuthorityScope::new("semantic-scheduler-replay", "execute"),
        0,
        WorkRole::Build,
    )
}

struct FixedSnapshotProducer {
    calls: AtomicUsize,
    changed: Notify,
}

impl FixedSnapshotProducer {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            changed: Notify::new(),
        }
    }

    async fn wait_for_calls(&self, expected: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if self.calls.load(Ordering::SeqCst) >= expected {
                    return;
                }
                self.changed.notified().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("snapshot producer did not reach {expected} calls"));
    }
}

#[async_trait]
impl SemanticObservationSnapshotProducer for FixedSnapshotProducer {
    async fn capture(
        &self,
        request: SemanticObservationCaptureRequest,
    ) -> Result<Option<SemanticObservationCapture>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_waiters();
        let measurement = TraceStateMeasurement {
            measurement_hash: format!("measurement:{}:{}", request.task_id(), request.attempt()),
            tool_calls: 1,
            failed_tool_calls: 0,
            malformed_tool_calls: 0,
            pending_tool_calls: 0,
            thinking_chars: 4096,
            recurrence_window_chars: 48,
            recurrence_observed_windows: 300,
            recurrence_repeated_windows: 12,
            recurrence_repeat_share: 0.04,
            provider_stream_revision: 0,
            provider_stream_chunks: 0,
            provider_stream_bytes: 0,
            provider_structured_output_chunks: 0,
            provider_structured_output_bytes: 0,
            provider_last_progress_elapsed_ms: 0,
            provider_structured_output_active: false,
            artifact_version: "artifact-v1".to_string(),
        };
        let summons = SemanticObservationSummonsSignal::TraceStateAdvanced {
            source_id: format!("measurement:{}:{}", request.task_id(), request.attempt()),
            measurement,
            provenance: "correlated test activity digest".to_string(),
        };
        let snapshot = SemanticObservationSnapshotDraft {
            schema_version: SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
            authority_scope: request
                .activity_publisher()
                .source()
                .authority_scope
                .clone(),
            phase_epoch: request.activity_publisher().source().phase_epoch,
            task_id: request.task_id().to_string(),
            attempt: request.attempt(),
            source_revision: 1,
            contract_version: request.contract_version().to_string(),
            artifact_version: "artifact-v1".to_string(),
            goal: request.goal().to_string(),
            task_contract: request.task_contract().to_string(),
            acceptance_oracle: request.acceptance_oracle().to_vec(),
            dependency_contract_versions: request.dependency_contract_versions().clone(),
            sibling_contract_versions: request.sibling_contract_versions().clone(),
            allowed_finding_routes: request.allowed_finding_routes().to_vec(),
            artifacts: Vec::new(),
            trace: SemanticTraceSnapshot {
                sequence: 1,
                recent_reasoning: "The worker advanced an exact contract check".to_string(),
                recent_actions: vec!["read owned artifact".to_string()],
                prior_intervention: None,
                response_to_prior_intervention: None,
            },
            neutral_signals: vec![summons.neutral_signal()],
        }
        .seal()
        .map_err(|error| error.to_string())?;
        Ok(Some(SemanticObservationCapture::new(snapshot, summons)?))
    }
}

struct NudgeObserver {
    calls: AtomicUsize,
    called: Notify,
    release: Notify,
    hold_review: bool,
    eligible_routes: Option<Vec<String>>,
}

impl NudgeObserver {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            called: Notify::new(),
            release: Notify::new(),
            hold_review: false,
            eligible_routes: None,
        }
    }

    fn held() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            called: Notify::new(),
            release: Notify::new(),
            hold_review: true,
            eligible_routes: None,
        }
    }

    fn release_review(&self) {
        self.release.notify_one();
    }

    fn bound_to(eligible_routes: Vec<String>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            called: Notify::new(),
            release: Notify::new(),
            hold_review: false,
            eligible_routes: Some(eligible_routes),
        }
    }
}

#[async_trait]
impl AdmittedSemanticObservationReviewer for NudgeObserver {
    fn eligible_logical_device_ids(&self) -> Option<Vec<String>> {
        self.eligible_routes.clone()
    }

    async fn review(
        &self,
        request: AdmittedSemanticObservationRequest,
    ) -> Result<String, AdmittedSemanticReviewError> {
        expose_current_provider_http_request(
            ProviderHttpProtocol::OpenAiChatCompletions,
            VERIFIED_TRANSPORT,
        )
        .map_err(AdmittedSemanticReviewError::unresolved)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.called.notify_waiters();
        if self.hold_review {
            self.release.notified().await;
        }
        Ok(serde_json::json!({
            "protocol": SEMANTIC_OBSERVATION_PROTOCOL,
            "snapshot_hash": request.observation.snapshot.snapshot_hash(),
            "observation": {
                "action": "NUDGE",
                "summary": "the sealed trace could use a semantic correction",
                "evidence": [
                    {
                        "source_id": "acceptance:task-contract",
                        "observation": "the task contract is the exact sealed acceptance criterion"
                    },
                    {
                        "source_id": "trace:1",
                        "observation": "the current sealed trace needs a semantic correction"
                    }
                ],
                "guidance": "re-check the exact acceptance criterion"
            }
        })
        .to_string())
    }
}

#[derive(Default)]
struct ReplayNudgeState {
    bound: Option<ProviderRequestReceipt>,
    reserved: bool,
    closed: bool,
    guidance: Vec<String>,
    order: Vec<String>,
}

struct ReplayNudgeDelivery {
    state: Mutex<ReplayNudgeState>,
    progress: Mutex<ProviderNudgeSafetySnapshot>,
    cancelled: tokio::sync::watch::Sender<bool>,
    confirmed: tokio::sync::watch::Sender<Option<Result<ProviderTerminalReceipt, String>>>,
}

impl ReplayNudgeDelivery {
    fn new() -> Self {
        let (cancelled, _) = tokio::sync::watch::channel(false);
        let (confirmed, _) = tokio::sync::watch::channel(None);
        Self {
            state: Mutex::new(ReplayNudgeState::default()),
            progress: Mutex::new(ProviderNudgeSafetySnapshot::default()),
            cancelled,
            confirmed,
        }
    }

    fn guidance(&self) -> Vec<String> {
        self.state.lock().unwrap().guidance.clone()
    }

    fn order(&self) -> Vec<String> {
        self.state.lock().unwrap().order.clone()
    }

    fn record_cancel(&self) {
        self.state.lock().unwrap().order.push("cancel".to_string());
    }

    fn record_resume(&self) {
        self.state.lock().unwrap().order.push("resume".to_string());
    }

    fn advance_structured_output(&self) {
        let mut progress = self.progress.lock().unwrap();
        progress.provider_stream_revision += 1;
        progress.provider_stream_chunks += 1;
        progress.provider_stream_bytes += 64;
        progress.provider_structured_output_chunks += 1;
        progress.provider_structured_output_bytes += 64;
        progress.provider_structured_output_active = true;
    }

    async fn wait_for_guidance(&self) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.state.lock().unwrap().guidance.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("semantic nudge was not enqueued");
    }

    async fn wait_for_bound_request(&self) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.state.lock().unwrap().bound.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("provider nudge delivery was not bound to a request");
    }

    fn release_cancel(&self) {
        let _ = self.cancelled.send(true);
    }
}

#[async_trait]
impl ProviderNudgeDelivery for ReplayNudgeDelivery {
    fn bind_request(&self, request: &ProviderRequestReceipt) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        match state.bound.as_ref() {
            Some(existing) if existing != request => {
                Err("delivery already bound to another request".to_string())
            }
            Some(_) => Ok(()),
            None => {
                state.bound = Some(request.clone());
                Ok(())
            }
        }
    }

    fn reserve_at_capture(
        &self,
        capture: ProviderNudgeSafetySnapshot,
        reserve: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), String> {
        let progress = self.progress.lock().unwrap();
        if progress.provider_structured_output_chunks != capture.provider_structured_output_chunks
            || progress.provider_structured_output_bytes != capture.provider_structured_output_bytes
            || progress.provider_structured_output_active
                != capture.provider_structured_output_active
        {
            return Err("provider structured output changed after semantic capture".to_string());
        }
        reserve()
    }

    fn try_enqueue(&self, guidance: String) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if state.closed || state.reserved {
            return Err("delivery is closed or already reserved".to_string());
        }
        state.reserved = true;
        state.guidance.push(guidance);
        state.order.push("steer".to_string());
        Ok(())
    }

    fn natural_terminal_allowed(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.reserved {
            return false;
        }
        state.closed = true;
        true
    }

    fn cancellation_terminal_confirmation_required(&self) -> bool {
        self.state.lock().unwrap().reserved
    }

    async fn cancelled(&self) {
        let mut receiver = self.cancelled.subscribe();
        while !*receiver.borrow_and_update() {
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }

    fn confirm_cancelled_terminal(
        &self,
        completed: CompletedProviderRequest,
    ) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        let result = match state.bound.as_ref() {
            Some(request)
                if completed.request() == request
                    && completed.terminal().kind == ProviderTerminalKind::Cancelled =>
            {
                state.order.push("terminal".to_string());
                Ok(completed.terminal().clone())
            }
            _ => Err("cancel terminal does not match bound request".to_string()),
        };
        drop(state);
        self.confirmed.send_replace(Some(result.clone()));
        result.map(drop)
    }

    async fn confirmed_cancelled_terminal(&self) -> Result<ProviderTerminalReceipt, String> {
        let mut receiver = self.confirmed.subscribe();
        loop {
            if let Some(result) = receiver.borrow_and_update().clone() {
                return result;
            }
            receiver
                .changed()
                .await
                .map_err(|_| "cancel confirmation channel closed".to_string())?;
        }
    }
}

struct GapDispatcher {
    release_long: Notify,
    nudge_delivery: Arc<ReplayNudgeDelivery>,
    resumed_delivery: Arc<ReplayNudgeDelivery>,
}

impl GapDispatcher {
    fn new() -> Self {
        Self {
            release_long: Notify::new(),
            nudge_delivery: Arc::new(ReplayNudgeDelivery::new()),
            resumed_delivery: Arc::new(ReplayNudgeDelivery::new()),
        }
    }
}

#[async_trait]
impl ProviderLifecycleDispatcher for GapDispatcher {
    async fn run_admitted(
        &self,
        request: DispatchRequest,
        _admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        let started = lifecycle
            .start_provider_request()
            .await
            .map_err(|error| DispatchError::Terminal(error.to_string()))?;
        started
            .publish_for_scheduler_with_nudge_delivery(self.nudge_delivery.clone())
            .map_err(|error| DispatchError::Terminal(error.to_string()))?;
        started
            .scope_http(async {
                expose_current_provider_http_request(
                    ProviderHttpProtocol::OpenAiChatCompletions,
                    VERIFIED_TRANSPORT,
                )
            })
            .await
            .map_err(DispatchError::Terminal)?;
        let terminal_kind = if request.task_id == "a-long" {
            tokio::select! {
                biased;
                _ = self.nudge_delivery.cancelled() => ProviderTerminalKind::Cancelled,
                _ = self.release_long.notified() => {
                    if self.nudge_delivery.natural_terminal_allowed() {
                        ProviderTerminalKind::Finished
                    } else {
                        self.nudge_delivery.cancelled().await;
                        ProviderTerminalKind::Cancelled
                    }
                }
            }
        } else {
            ProviderTerminalKind::Finished
        };
        if terminal_kind == ProviderTerminalKind::Cancelled {
            self.nudge_delivery.record_cancel();
            let completed = started
                .provider_terminal_with_completion(ProviderTerminalKind::Cancelled)
                .await
                .map_err(|error| DispatchError::Terminal(error.to_string()))?;
            self.nudge_delivery
                .confirm_cancelled_terminal(completed)
                .map_err(DispatchError::Terminal)?;
            self.nudge_delivery.record_resume();
            let resumed = lifecycle
                .start_provider_request()
                .await
                .map_err(|error| DispatchError::Terminal(error.to_string()))?;
            resumed
                .publish_for_scheduler_with_nudge_delivery(self.resumed_delivery.clone())
                .map_err(|error| DispatchError::Terminal(error.to_string()))?;
            resumed
                .scope_http(async {
                    expose_current_provider_http_request(
                        ProviderHttpProtocol::OpenAiChatCompletions,
                        VERIFIED_TRANSPORT,
                    )
                })
                .await
                .map_err(DispatchError::Terminal)?;
            self.release_long.notified().await;
            assert!(self.resumed_delivery.natural_terminal_allowed());
            resumed
                .provider_terminal(ProviderTerminalKind::Finished)
                .await
                .map_err(|error| DispatchError::Terminal(error.to_string()))?;
            return Ok(format!("completed:{}", request.task_id).into());
        }
        started
            .provider_terminal(terminal_kind)
            .await
            .map_err(|error| DispatchError::Terminal(error.to_string()))?;
        Ok(format!("completed:{}", request.task_id).into())
    }
}

struct TerminalGapDispatcher {
    first_terminal: AtomicUsize,
    second_started: AtomicUsize,
    continue_session: Notify,
    release_second: Notify,
}

impl TerminalGapDispatcher {
    fn new() -> Self {
        Self {
            first_terminal: AtomicUsize::new(0),
            second_started: AtomicUsize::new(0),
            continue_session: Notify::new(),
            release_second: Notify::new(),
        }
    }

    async fn wait_for(counter: &AtomicUsize, label: &str) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while counter.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{label} was not observed"));
    }
}

#[async_trait]
impl ProviderLifecycleDispatcher for TerminalGapDispatcher {
    async fn run_admitted(
        &self,
        request: DispatchRequest,
        _admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        let first = lifecycle
            .start_provider_request()
            .await
            .map_err(|error| DispatchError::Terminal(error.to_string()))?;
        first
            .publish_for_scheduler()
            .map_err(|error| DispatchError::Terminal(error.to_string()))?;
        first
            .scope_http(async {
                expose_current_provider_http_request(
                    ProviderHttpProtocol::OpenAiChatCompletions,
                    VERIFIED_TRANSPORT,
                )
            })
            .await
            .map_err(DispatchError::Terminal)?;
        first
            .provider_terminal(ProviderTerminalKind::Finished)
            .await
            .map_err(|error| DispatchError::Terminal(error.to_string()))?;
        self.first_terminal.store(1, Ordering::SeqCst);

        self.continue_session.notified().await;
        let second = lifecycle
            .start_provider_request()
            .await
            .map_err(|error| DispatchError::Terminal(error.to_string()))?;
        second
            .publish_for_scheduler()
            .map_err(|error| DispatchError::Terminal(error.to_string()))?;
        second
            .scope_http(async {
                expose_current_provider_http_request(
                    ProviderHttpProtocol::OpenAiChatCompletions,
                    VERIFIED_TRANSPORT,
                )
            })
            .await
            .map_err(DispatchError::Terminal)?;
        self.second_started.store(1, Ordering::SeqCst);
        self.release_second.notified().await;
        second
            .provider_terminal(ProviderTerminalKind::Finished)
            .await
            .map_err(|error| DispatchError::Terminal(error.to_string()))?;
        Ok(format!("completed:{}", request.task_id).into())
    }
}

async fn wait_for_semantic_release(sink: &RecordingSink) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if sink.events.lock().unwrap().iter().any(|event| {
                event["event"] == "broker_admission_released"
                    && event["receipt"]["admission"]["role"]
                        == serde_json::json!("semantic_judge_observation")
            }) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("semantic admission did not release");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_delivers_one_nudge_before_cancel_and_rejects_replayed_trace_revision() {
    let authority = tempfile::tempdir().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control(
        "semantic-scheduler-replay",
        vec![
            lane("lane-a", "model-a", "host-a", 1),
            lane("lane-b", "model-b", "host-b", 1),
        ],
        event_sink,
        authority.path(),
    );
    let producer = Arc::new(FixedSnapshotProducer::new());
    let observer = Arc::new(NudgeObserver::new());
    let dispatcher = Arc::new(GapDispatcher::new());
    let run = tokio::spawn({
        let sink = sink.clone();
        let control = control.clone();
        let producer = producer.clone();
        let observer = observer.clone();
        let dispatcher = dispatcher.clone();
        async move {
            Scheduler::new(
                vec![device("lane-a", "model-a"), device("lane-b", "model-b")],
                2,
            )
            .with_sink(sink)
            .with_semantic_observation(producer, observer)
            .run_with_physical_admission(
                Dag::from_specs(vec![task("a-long")]).unwrap(),
                dispatcher,
                control,
                execution(),
                "Build the exact replay fixture".to_string(),
                String::new(),
            )
            .await
        }
    });

    tokio::time::timeout(Duration::from_secs(2), observer.called.notified())
        .await
        .expect("idle semantic observer did not run");
    wait_for_semantic_release(&sink).await;
    dispatcher.nudge_delivery.wait_for_guidance().await;
    dispatcher.nudge_delivery.release_cancel();
    dispatcher.resumed_delivery.wait_for_bound_request().await;
    producer.wait_for_calls(2).await;
    dispatcher.release_long.notify_one();

    let report = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("physical run did not finish")
        .unwrap()
        .unwrap();
    assert_eq!(report.done, vec!["a-long".to_string()]);
    assert!(report.failed.is_empty());
    assert_eq!(producer.calls.load(Ordering::SeqCst), 2);
    assert_eq!(observer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(sink.count("semantic_observation_summoned"), 1);
    assert_eq!(sink.semantic_admissions(), 1);
    assert_eq!(
        dispatcher.nudge_delivery.guidance(),
        vec!["re-check the exact acceptance criterion".to_string()]
    );
    assert_eq!(
        dispatcher.nudge_delivery.order(),
        vec!["steer", "cancel", "terminal", "resume"]
    );
    assert!(dispatcher
        .nudge_delivery
        .try_enqueue("second nudge".to_string())
        .is_err());
    assert!(dispatcher.resumed_delivery.guidance().is_empty());
    assert_eq!(
        sink.worker_terminal_kinds("a-long"),
        vec!["cancelled", "finished"]
    );
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_vetoes_nudge_when_structured_output_advances_after_capture() {
    let authority = tempfile::tempdir().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control(
        "semantic-scheduler-structured-veto",
        vec![
            lane("lane-a", "model-a", "host-a", 1),
            lane("lane-b", "model-b", "host-b", 1),
        ],
        event_sink,
        authority.path(),
    );
    let producer = Arc::new(FixedSnapshotProducer::new());
    let observer = Arc::new(NudgeObserver::held());
    let dispatcher = Arc::new(GapDispatcher::new());
    let run = tokio::spawn({
        let sink = sink.clone();
        let control = control.clone();
        let producer = producer.clone();
        let observer = observer.clone();
        let dispatcher = dispatcher.clone();
        async move {
            Scheduler::new(
                vec![device("lane-a", "model-a"), device("lane-b", "model-b")],
                2,
            )
            .with_sink(sink)
            .with_semantic_observation(producer, observer)
            .run_with_physical_admission(
                Dag::from_specs(vec![task("a-long")]).unwrap(),
                dispatcher,
                control,
                execution(),
                "Build the structured-output safety fixture".to_string(),
                String::new(),
            )
            .await
        }
    });

    tokio::time::timeout(Duration::from_secs(2), observer.called.notified())
        .await
        .expect("idle semantic observer did not run");
    dispatcher.nudge_delivery.advance_structured_output();
    observer.release_review();
    tokio::time::timeout(Duration::from_secs(2), async {
        while sink.count("semantic_observation_capture_failed") == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("structured-output safety rejection was not emitted");
    dispatcher.release_long.notify_one();

    let report = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("physical run did not finish")
        .unwrap()
        .unwrap();
    assert_eq!(report.done, vec!["a-long".to_string()]);
    assert!(report.failed.is_empty());
    assert!(dispatcher.nudge_delivery.guidance().is_empty());
    assert!(dispatcher.nudge_delivery.order().is_empty());
    assert_eq!(
        sink.worker_terminal_kinds("a-long"),
        vec!["finished".to_string()]
    );
    assert_eq!(observer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn terminal_source_start_is_retired_once_and_next_turn_rebinds_without_blocking_task() {
    let authority = tempfile::tempdir().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control(
        "semantic-scheduler-source-rebind",
        vec![
            lane("lane-a", "model-a", "host-a", 1),
            lane("lane-b", "model-b", "host-b", 1),
        ],
        event_sink,
        authority.path(),
    );
    let producer = Arc::new(FixedSnapshotProducer::new());
    let observer = Arc::new(NudgeObserver::bound_to(Vec::new()));
    let dispatcher = Arc::new(TerminalGapDispatcher::new());
    let run = tokio::spawn({
        let sink = sink.clone();
        let control = control.clone();
        let producer = producer.clone();
        let observer = observer.clone();
        let dispatcher = dispatcher.clone();
        async move {
            Scheduler::new(
                vec![device("lane-a", "model-a"), device("lane-b", "model-b")],
                2,
            )
            .with_sink(sink)
            .with_semantic_observation(producer, observer)
            .run_with_physical_admission(
                Dag::from_specs(vec![task("a-long")]).unwrap(),
                dispatcher,
                control,
                execution(),
                "Build the provider-start rebind fixture".to_string(),
                String::new(),
            )
            .await
        }
    });

    TerminalGapDispatcher::wait_for(&dispatcher.first_terminal, "first provider terminal").await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if sink
                .capture_failure_reasons("a-long")
                .iter()
                .any(|reason| reason.contains("no longer live"))
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("terminal source start was not retired");
    let failures_after_retirement = sink.capture_failure_reasons("a-long").len();
    let captures_before_rebind = producer.calls.load(Ordering::SeqCst);

    dispatcher.continue_session.notify_one();
    TerminalGapDispatcher::wait_for(&dispatcher.second_started, "second provider start").await;
    producer.wait_for_calls(captures_before_rebind + 1).await;
    assert_eq!(
        sink.capture_failure_reasons("a-long").len(),
        failures_after_retirement,
        "the retired provider request must not be polled again before a new request binds"
    );
    dispatcher.release_second.notify_one();

    let report = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("task did not complete independently of observation retirement")
        .unwrap()
        .unwrap();
    assert_eq!(report.done, vec!["a-long".to_string()]);
    assert!(report.failed.is_empty());
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_fails_closed_when_the_only_verified_route_is_the_observed_worker() {
    let authority = tempfile::tempdir().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control(
        "semantic-scheduler-no-route",
        vec![lane("lane-a", "model-a", "host-a", 2)],
        event_sink,
        authority.path(),
    );
    let producer = Arc::new(FixedSnapshotProducer::new());
    let observer = Arc::new(NudgeObserver::new());
    let dispatcher = Arc::new(GapDispatcher::new());
    let run = tokio::spawn({
        let sink = sink.clone();
        let control = control.clone();
        let producer = producer.clone();
        let observer = observer.clone();
        let dispatcher = dispatcher.clone();
        async move {
            Scheduler::new(vec![device("lane-a", "model-a")], 2)
                .with_sink(sink)
                .with_semantic_observation(producer, observer)
                .run_with_physical_admission(
                    Dag::from_specs(vec![task("a-long")]).unwrap(),
                    dispatcher,
                    control,
                    execution(),
                    "Build the exact no-route fixture".to_string(),
                    String::new(),
                )
                .await
        }
    });

    producer.wait_for_calls(1).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if sink.count("semantic_observation_deferred") == 1 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("no-route deferral was not emitted");
    dispatcher.release_long.notify_one();
    let report = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("physical run did not finish")
        .unwrap()
        .unwrap();

    assert_eq!(report.done, vec!["a-long".to_string()]);
    assert_eq!(observer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(sink.count("semantic_observation_summoned"), 1);
    assert_eq!(sink.semantic_admissions(), 0);
    let provider_events_for_semantic = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| {
            matches!(
                event["event"].as_str(),
                Some("broker_provider_request_permitted")
                    | Some("broker_provider_terminal_observed")
                    | Some("broker_provider_not_started")
            ) && event["admission"]["role"] == "semantic_judge_observation"
        })
        .count();
    assert_eq!(provider_events_for_semantic, 0);
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_never_admits_an_idle_lane_without_a_verified_reviewer_provider() {
    let authority = tempfile::tempdir().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control(
        "semantic-scheduler-no-provider-binding",
        vec![
            lane("lane-a", "model-a", "host-a", 1),
            lane("lane-b", "model-b", "host-b", 1),
        ],
        event_sink,
        authority.path(),
    );
    let producer = Arc::new(FixedSnapshotProducer::new());
    let observer = Arc::new(NudgeObserver::bound_to(vec!["lane-a".to_string()]));
    let dispatcher = Arc::new(GapDispatcher::new());
    let run = tokio::spawn({
        let sink = sink.clone();
        let control = control.clone();
        let producer = producer.clone();
        let observer = observer.clone();
        let dispatcher = dispatcher.clone();
        async move {
            Scheduler::new(
                vec![device("lane-a", "model-a"), device("lane-b", "model-b")],
                2,
            )
            .with_sink(sink)
            .with_semantic_observation(producer, observer)
            .run_with_physical_admission(
                Dag::from_specs(vec![task("a-long")]).unwrap(),
                dispatcher,
                control,
                execution(),
                "Build the exact provider-binding fixture".to_string(),
                String::new(),
            )
            .await
        }
    });

    producer.wait_for_calls(1).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if sink.count("semantic_observation_deferred") == 1 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("missing-provider deferral was not emitted");
    dispatcher.release_long.notify_one();
    let report = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("physical run did not finish")
        .unwrap()
        .unwrap();

    assert_eq!(report.done, vec!["a-long".to_string()]);
    assert_eq!(observer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(sink.count("semantic_observation_summoned"), 1);
    assert_eq!(sink.semantic_admissions(), 0);
    assert_eq!(control.occupancy().await, (0, 0));
}
