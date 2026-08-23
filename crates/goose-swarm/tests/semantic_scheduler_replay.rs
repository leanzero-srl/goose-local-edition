use async_trait::async_trait;
use goose_provider_types::base::{ProviderHttpProtocol, expose_current_provider_http_request};
use goose_swarm::{
    AdmissionReceipt, AdmittedSemanticObservationRequest, AdmittedSemanticObservationReviewer,
    AdmittedSemanticReviewError, AuthorityScope, Dag, DeviceCfg, Difficulty, DispatchError,
    DispatchRequest, EventSink, GlobalProviderLeaseAuthority, HostCapacityEvidence,
    PhysicalAdmissionControl, PhysicalExecutionAuthority, PhysicalFleetSnapshot,
    ProviderLeaseWaitPolicy, ProviderLifecycle, ProviderLifecycleDispatcher,
    ProviderLifecycleJournal, ProviderRequestReceipt, ProviderTerminalKind,
    ProviderTerminalReceipt, RunScopedProviderLeaseAuthority, SEMANTIC_OBSERVATION_PROTOCOL,
    SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA, Scheduler, SealedProviderLeaseAuthority,
    SemanticObservationCapture, SemanticObservationCaptureRequest,
    SemanticObservationSnapshotDraft, SemanticObservationSnapshotProducer,
    SemanticObservationSummonsSignal, SemanticTraceSnapshot, SwarmEvent, TaskRunOutput, TaskSpec,
    TraceStateMeasurement, VerifiedPhysicalLane, VerifiedProviderProtocolRoute, WorkRole,
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
    eligible_routes: Option<Vec<String>>,
}

impl NudgeObserver {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            called: Notify::new(),
            eligible_routes: None,
        }
    }

    fn bound_to(eligible_routes: Vec<String>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            called: Notify::new(),
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
        let source_id = request.observation.snapshot.payload().neutral_signals[0]
            .source_id
            .clone();
        Ok(serde_json::json!({
            "protocol": SEMANTIC_OBSERVATION_PROTOCOL,
            "snapshot_hash": request.observation.snapshot.snapshot_hash(),
            "observation": {
                "action": "NUDGE",
                "summary": "the sealed trace could use a semantic correction",
                "evidence": [{
                    "source_id": source_id,
                    "observation": "the typed trace measurement is sealed"
                }],
                "guidance": "re-check the exact acceptance criterion"
            }
        })
        .to_string())
    }
}

struct GapDispatcher {
    release_long: Notify,
}

impl GapDispatcher {
    fn new() -> Self {
        Self {
            release_long: Notify::new(),
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
            .publish_for_scheduler()
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
        if request.task_id == "a-long" {
            self.release_long.notified().await;
        }
        started
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
async fn scheduler_uses_one_idle_route_for_one_trace_revision_and_never_delivers_the_nudge() {
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
