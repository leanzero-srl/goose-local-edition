use async_trait::async_trait;
use goose_swarm::{
    AcceptanceCriterionSnapshot, AdmittedSemanticObservationRequest,
    AdmittedSemanticObservationReviewer, BrokerError, BrokeredSemanticObservationPlane, EventSink,
    HostCapacityEvidence, LocalCompletionKind, NeutralJudgeSignal, PhysicalAdmissionControl,
    PhysicalFleetSnapshot, ProviderTerminalKind, SemanticJudgeAction,
    SemanticObservationAdmissionError, SemanticObservationAdmissionPolicy,
    SemanticObservationAdmissionStage, SemanticObservationAdmissionSubmission,
    SemanticObservationSnapshotDraft, SemanticProtocolFailureKind, SemanticTraceSnapshot,
    SourceRevisionKind, SwarmEvent, TaskVersion, VerifiedPhysicalLane, WorkOpportunity, WorkRole,
    SEMANTIC_OBSERVATION_PROTOCOL, SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<serde_json::Value>>,
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

fn control(scope: &str, sink: Arc<dyn EventSink>) -> PhysicalAdmissionControl {
    let snapshot = PhysicalFleetSnapshot::new(
        format!("snapshot:{scope}"),
        vec![VerifiedPhysicalLane {
            logical_device_id: "lane-a".into(),
            model_id: "model-a".into(),
            host_id: "host-a".into(),
            model_instance_id: "instance-a".into(),
            advertised_instance_capacity: 1,
            routing_weight: 1,
            capacity_evidence: HostCapacityEvidence::MeasuredProfile {
                profile_hash: format!("profile:{scope}"),
                profile_key: "test-runtime:model:context:semantic-observation".into(),
                max_concurrent: 1,
            },
            route_evidence_id: format!("route:{scope}"),
        }],
    )
    .unwrap();
    PhysicalAdmissionControl::new(scope, snapshot, sink).unwrap()
}

fn snapshot(
    task_id: &str,
    revision: u64,
    reasoning: &str,
) -> goose_swarm::SealedSemanticObservationSnapshot {
    SemanticObservationSnapshotDraft {
        schema_version: SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
        task_id: task_id.into(),
        attempt: 0,
        source_revision: revision,
        contract_version: "contract-v1".into(),
        artifact_version: format!("artifact-v{revision}"),
        goal: "Verify the sealed task contract".into(),
        task_contract: "Advance the owned artifact and prove the exact acceptance criterion".into(),
        acceptance_oracle: vec![AcceptanceCriterionSnapshot {
            id: "criterion".into(),
            text: "The exact criterion has direct evidence".into(),
        }],
        dependency_contract_versions: BTreeMap::new(),
        sibling_contract_versions: BTreeMap::new(),
        allowed_finding_routes: vec!["integrate-verify".into()],
        artifacts: Vec::new(),
        trace: SemanticTraceSnapshot {
            sequence: revision,
            recent_reasoning: reasoning.into(),
            recent_actions: vec!["advanced one check".into()],
            prior_intervention: None,
            response_to_prior_intervention: None,
        },
        neutral_signals: vec![NeutralJudgeSignal {
            source_id: "signal:progress".into(),
            kind: "stream_progress".into(),
            value: serde_json::json!({"events_advanced": 1}),
            provenance: "correlated replay fixture".into(),
        }],
    }
    .seal()
    .unwrap()
}

fn continue_reply(request: &AdmittedSemanticObservationRequest) -> String {
    serde_json::json!({
        "protocol": SEMANTIC_OBSERVATION_PROTOCOL,
        "snapshot_hash": request.observation.snapshot.snapshot_hash(),
        "observation": {
            "action": "CONTINUE",
            "summary": "the sealed trace advances one criterion",
            "evidence": [{
                "source_id": "signal:progress",
                "observation": "the correlated event count advanced"
            }]
        }
    })
    .to_string()
}

#[derive(Default)]
struct ContinueReviewer {
    calls: AtomicUsize,
}

#[async_trait]
impl AdmittedSemanticObservationReviewer for ContinueReviewer {
    async fn review(&self, request: AdmittedSemanticObservationRequest) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.admission.role, WorkRole::SemanticJudgeObservation);
        assert!(matches!(
            request.admission.source.kind,
            SourceRevisionKind::Trace { .. }
        ));
        assert!(request.provider_request_id.ends_with(":provider:0"));
        Ok(continue_reply(&request))
    }
}

struct BlockingFirstReviewer {
    calls: AtomicUsize,
    first_started: Notify,
    release_first: Notify,
}

#[async_trait]
impl AdmittedSemanticObservationReviewer for BlockingFirstReviewer {
    async fn review(&self, request: AdmittedSemanticObservationRequest) -> Result<String, String> {
        let ordinal = self.calls.fetch_add(1, Ordering::SeqCst);
        if ordinal == 0 {
            self.first_started.notify_one();
            self.release_first.notified().await;
        }
        Ok(continue_reply(&request))
    }
}

struct FailingReviewer {
    calls: AtomicUsize,
    panic: bool,
}

#[async_trait]
impl AdmittedSemanticObservationReviewer for FailingReviewer {
    async fn review(&self, _request: AdmittedSemanticObservationRequest) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.panic {
            panic!("adversarial semantic provider panic");
        }
        Err("adversarial semantic provider failure".into())
    }
}

fn event_count(sink: &RecordingSink, name: &str) -> usize {
    sink.events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| event["event"] == name)
        .count()
}

async fn wait_for_occupancy(control: &PhysicalAdmissionControl, expected: (usize, usize)) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if control.occupancy().await == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("control never reached occupancy {expected:?}"));
}

async fn wait_for_published_revision(sink: &RecordingSink, revision: u64) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if sink.events.lock().unwrap().iter().any(|event| {
                event["event"] == "semantic_observation_source_published"
                    && event["source"]["revision"] == revision
            }) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("semantic source revision {revision} was not published"));
}

async fn finish_blocker(admitted: &goose_swarm::AdmittedWork) {
    let lifecycle = admitted.lifecycle();
    let key = lifecycle
        .provider_request_started("provider:blocker")
        .await
        .unwrap();
    lifecycle
        .provider_terminal(key, ProviderTerminalKind::Finished)
        .await
        .unwrap();
    admitted
        .complete_local(LocalCompletionKind::Success)
        .await
        .unwrap();
}

#[tokio::test]
async fn one_trace_revision_calls_the_provider_once_and_rejects_replays_before_the_call() {
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control("semantic-dedup", event_sink.clone());
    let plane = BrokeredSemanticObservationPlane::new(control.clone(), event_sink).unwrap();
    let duplicate_sink: Arc<dyn EventSink> = sink.clone();
    assert!(matches!(
        BrokeredSemanticObservationPlane::new(control.clone(), duplicate_sink),
        Err(SemanticObservationAdmissionError::ObservationPlaneAlreadyBound)
    ));
    let reviewer = Arc::new(ContinueReviewer::default());
    let sealed = snapshot("detail-api", 7, "advance");

    let first = match plane
        .submit(
            sealed.clone(),
            SemanticObservationAdmissionPolicy::default(),
            reviewer.clone(),
        )
        .await
        .unwrap()
    {
        SemanticObservationAdmissionSubmission::Started(handle) => handle,
        SemanticObservationAdmissionSubmission::Rejected(_) => panic!("first review rejected"),
    };
    let replay = tokio::spawn({
        let plane = plane.clone();
        let sealed = sealed.clone();
        let reviewer = reviewer.clone();
        async move {
            plane
                .submit(
                    sealed,
                    SemanticObservationAdmissionPolicy::default(),
                    reviewer,
                )
                .await
        }
    });
    let receipt = first.wait().await.unwrap();
    assert_eq!(receipt.observation.action(), SemanticJudgeAction::Continue);
    assert!(!receipt.has_intervention_authority());
    let rejected = replay.await.unwrap().unwrap();
    match rejected {
        SemanticObservationAdmissionSubmission::Rejected(rejected) => {
            assert!(rejected.admission.is_some());
        }
        SemanticObservationAdmissionSubmission::Started(_) => panic!("replay called provider"),
    }

    assert_eq!(reviewer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(event_count(&sink, "broker_admission_granted"), 2);
    assert_eq!(event_count(&sink, "broker_provider_request_permitted"), 1);
    assert_eq!(event_count(&sink, "broker_provider_terminal_observed"), 1);
    assert_eq!(event_count(&sink, "broker_provider_not_started"), 1);
    assert_eq!(event_count(&sink, "broker_admission_released"), 2);
    assert_eq!(control.occupancy().await, (0, 0));

    let conflicting = snapshot("detail-api", 7, "same revision, different immutable bytes");
    assert!(matches!(
        plane
            .submit(
                conflicting,
                SemanticObservationAdmissionPolicy::default(),
                reviewer.clone(),
            )
            .await,
        Err(SemanticObservationAdmissionError::Broker {
            stage: SemanticObservationAdmissionStage::PublishSource,
            error: BrokerError::ConflictingSourceRevision { .. }
        })
    ));
    assert_eq!(reviewer.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn newer_trace_supersedes_an_in_flight_result_without_overlapping_calls() {
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control("semantic-supersession", event_sink.clone());
    let plane = BrokeredSemanticObservationPlane::new(control.clone(), event_sink).unwrap();
    let reviewer = Arc::new(BlockingFirstReviewer {
        calls: AtomicUsize::new(0),
        first_started: Notify::new(),
        release_first: Notify::new(),
    });
    let old = snapshot("detail-store", 10, "old trace");
    let old_handle = match plane
        .submit(
            old,
            SemanticObservationAdmissionPolicy::default(),
            reviewer.clone(),
        )
        .await
        .unwrap()
    {
        SemanticObservationAdmissionSubmission::Started(handle) => handle,
        SemanticObservationAdmissionSubmission::Rejected(_) => panic!("old review rejected"),
    };
    reviewer.first_started.notified().await;

    let newer = tokio::spawn({
        let plane = plane.clone();
        let reviewer = reviewer.clone();
        async move {
            plane
                .submit(
                    snapshot("detail-store", 11, "new trace"),
                    SemanticObservationAdmissionPolicy::default(),
                    reviewer,
                )
                .await
        }
    });
    wait_for_published_revision(&sink, 11).await;
    assert!(!newer.is_finished());
    reviewer.release_first.notify_one();

    let old_receipt = old_handle.wait().await.unwrap();
    assert!(old_receipt.observation.stale);
    assert_eq!(
        old_receipt.observation.action(),
        SemanticJudgeAction::Abstain
    );
    let new_handle = match newer.await.unwrap().unwrap() {
        SemanticObservationAdmissionSubmission::Started(handle) => handle,
        SemanticObservationAdmissionSubmission::Rejected(_) => panic!("new review rejected"),
    };
    let new_receipt = new_handle.wait().await.unwrap();
    assert!(!new_receipt.observation.stale);
    assert_eq!(
        new_receipt.observation.action(),
        SemanticJudgeAction::Continue
    );
    assert_eq!(reviewer.calls.load(Ordering::SeqCst), 2);
    assert_eq!(event_count(&sink, "broker_provider_request_permitted"), 2);
    assert_eq!(event_count(&sink, "broker_provider_terminal_observed"), 2);
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test]
async fn queued_old_trace_is_pruned_before_admission_and_never_calls_the_provider() {
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control("semantic-queued-stale", event_sink.clone());
    let plane = BrokeredSemanticObservationPlane::new(control.clone(), event_sink).unwrap();
    let reviewer = Arc::new(ContinueReviewer::default());

    let blocker_source = TaskVersion {
        task_id: "blocker".into(),
        attempt: 0,
        revision: 1,
        kind: SourceRevisionKind::TaskAttempt,
    };
    control
        .set_source_revision(blocker_source.clone())
        .await
        .unwrap();
    let blocker = control
        .admit(WorkOpportunity {
            work_id: "build:blocker".into(),
            role: WorkRole::Build,
            priority: WorkRole::Build.priority(),
            task_rank: 0,
            source: blocker_source,
            eligible_logical_device_ids: Vec::new(),
            preferred_model_id: None,
            excluded_logical_device_id: None,
        })
        .await
        .unwrap();

    let old = tokio::spawn({
        let plane = plane.clone();
        let reviewer = reviewer.clone();
        async move {
            plane
                .submit(
                    snapshot("detail-web", 20, "queued old trace"),
                    SemanticObservationAdmissionPolicy::default(),
                    reviewer,
                )
                .await
        }
    });
    wait_for_occupancy(&control, (1, 1)).await;
    let newer = tokio::spawn({
        let plane = plane.clone();
        let reviewer = reviewer.clone();
        async move {
            plane
                .submit(
                    snapshot("detail-web", 21, "authoritative new trace"),
                    SemanticObservationAdmissionPolicy::default(),
                    reviewer,
                )
                .await
        }
    });
    assert!(matches!(
        old.await.unwrap(),
        Err(SemanticObservationAdmissionError::Broker {
            stage: SemanticObservationAdmissionStage::PhysicalAdmission,
            error: BrokerError::StaleOpportunity { .. }
        })
    ));
    wait_for_occupancy(&control, (1, 1)).await;
    finish_blocker(&blocker).await;
    let handle = match newer.await.unwrap().unwrap() {
        SemanticObservationAdmissionSubmission::Started(handle) => handle,
        SemanticObservationAdmissionSubmission::Rejected(_) => panic!("new review rejected"),
    };
    handle.wait().await.unwrap();

    assert_eq!(reviewer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(event_count(&sink, "broker_work_stale"), 1);
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test]
async fn cancelling_a_queued_submission_withdraws_it_and_allows_one_exact_retry() {
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control("semantic-queued-cancel", event_sink.clone());
    let plane = BrokeredSemanticObservationPlane::new(control.clone(), event_sink).unwrap();
    let reviewer = Arc::new(ContinueReviewer::default());

    let blocker_source = TaskVersion {
        task_id: "blocker".into(),
        attempt: 0,
        revision: 1,
        kind: SourceRevisionKind::TaskAttempt,
    };
    control
        .set_source_revision(blocker_source.clone())
        .await
        .unwrap();
    let blocker = control
        .admit(WorkOpportunity {
            work_id: "build:blocker".into(),
            role: WorkRole::Build,
            priority: WorkRole::Build.priority(),
            task_rank: 0,
            source: blocker_source,
            eligible_logical_device_ids: Vec::new(),
            preferred_model_id: None,
            excluded_logical_device_id: None,
        })
        .await
        .unwrap();

    let sealed = snapshot("detail-cancel", 25, "queued cancellation");
    let queued = tokio::spawn({
        let plane = plane.clone();
        let reviewer = reviewer.clone();
        let sealed = sealed.clone();
        async move {
            plane
                .submit(
                    sealed,
                    SemanticObservationAdmissionPolicy::default(),
                    reviewer,
                )
                .await
        }
    });
    wait_for_occupancy(&control, (1, 1)).await;
    queued.abort();
    let _ = queued.await;
    wait_for_occupancy(&control, (0, 1)).await;
    assert_eq!(reviewer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(event_count(&sink, "broker_work_withdrawn"), 1);

    finish_blocker(&blocker).await;
    let handle = match plane
        .submit(
            sealed,
            SemanticObservationAdmissionPolicy::default(),
            reviewer.clone(),
        )
        .await
        .unwrap()
    {
        SemanticObservationAdmissionSubmission::Started(handle) => handle,
        SemanticObservationAdmissionSubmission::Rejected(_) => panic!("retry rejected"),
    };
    handle.wait().await.unwrap();
    assert_eq!(reviewer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test]
async fn provider_failures_and_panics_produce_failed_terminals_and_release_admission() {
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control("semantic-provider-failure", event_sink.clone());
    let plane = BrokeredSemanticObservationPlane::new(control.clone(), event_sink).unwrap();

    for (revision, panic) in [(30, false), (31, true)] {
        let reviewer = Arc::new(FailingReviewer {
            calls: AtomicUsize::new(0),
            panic,
        });
        let handle = match plane
            .submit(
                snapshot("detail-failure", revision, "provider failure trace"),
                SemanticObservationAdmissionPolicy::default(),
                reviewer.clone(),
            )
            .await
            .unwrap()
        {
            SemanticObservationAdmissionSubmission::Started(handle) => handle,
            SemanticObservationAdmissionSubmission::Rejected(_) => {
                panic!("failure review rejected")
            }
        };
        let receipt = handle.wait().await.unwrap();
        assert_eq!(receipt.local_completion, LocalCompletionKind::Error);
        assert_eq!(receipt.observation.action(), SemanticJudgeAction::Abstain);
        assert_eq!(
            receipt
                .observation
                .decision
                .failure()
                .map(|failure| &failure.kind),
            Some(&SemanticProtocolFailureKind::ReviewerFailed)
        );
        assert_eq!(reviewer.calls.load(Ordering::SeqCst), 1);
    }

    {
        let events = sink.events.lock().unwrap();
        let terminal_kinds: Vec<String> = events
            .iter()
            .filter(|event| event["event"] == "broker_provider_terminal_observed")
            .map(|event| event["receipt"]["kind"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(terminal_kinds, vec!["failed", "failed"]);
        assert_eq!(
            events
                .iter()
                .filter(|event| event["event"] == "broker_admission_released")
                .count(),
            2
        );
    }
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test]
async fn dropping_the_wait_handle_does_not_cancel_an_admitted_provider_lifecycle() {
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn EventSink> = sink.clone();
    let control = control("semantic-dropped-handle", event_sink.clone());
    let plane = BrokeredSemanticObservationPlane::new(control.clone(), event_sink).unwrap();
    let reviewer = Arc::new(BlockingFirstReviewer {
        calls: AtomicUsize::new(0),
        first_started: Notify::new(),
        release_first: Notify::new(),
    });
    let handle = match plane
        .submit(
            snapshot("detail-drop", 40, "detached lifecycle"),
            SemanticObservationAdmissionPolicy::default(),
            reviewer.clone(),
        )
        .await
        .unwrap()
    {
        SemanticObservationAdmissionSubmission::Started(handle) => handle,
        SemanticObservationAdmissionSubmission::Rejected(_) => panic!("review rejected"),
    };
    reviewer.first_started.notified().await;
    drop(handle);
    reviewer.release_first.notify_one();
    tokio::time::timeout(Duration::from_secs(2), control.wait_until_drained())
        .await
        .expect("detached semantic lifecycle must drain");

    assert_eq!(reviewer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(event_count(&sink, "broker_provider_terminal_observed"), 1);
    assert_eq!(event_count(&sink, "broker_admission_released"), 1);
}
