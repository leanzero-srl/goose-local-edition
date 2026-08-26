use super::*;
use crate as goose_swarm;
use crate::semantic_runtime::{
    BoundSemanticObservationCapture, EngineSemanticTaskAuthority, SemanticTaskEvidenceCapability,
    TraceStateMeasurement,
};
use crate::{
    parse_semantic_observation_reply, AcceptanceCriterionSnapshot,
    AdmittedSemanticObservationReceipt, AdmittedSemanticObservationRequest,
    AdmittedSemanticObservationReviewer, AdmittedSemanticReviewError, ArtifactExcerptSnapshot,
    AuthorityScope, BrokeredSemanticObservationPlane, EventSink, GlobalProviderLeaseAuthority,
    HostCapacityEvidence, NullSink, PhysicalAdmissionControl, PhysicalFleetSnapshot,
    ProviderLifecycleJournal, ProviderRequestReceipt, ProviderTerminalKind,
    ProviderTerminalReceipt, RunScopedProviderLeaseAuthority, SealedProviderLeaseAuthority,
    SemanticActivityPublisher, SemanticObservationAdmissionPolicy,
    SemanticObservationAdmissionSubmission, SemanticObservationCapture,
    SemanticObservationCaptureRequest, SemanticObservationSnapshotDraft,
    SemanticObservationSummonsSignal, SemanticTraceSnapshot, SourceRevisionKind, TaskVersion,
    VerifiedPhysicalLane, VerifiedProviderProtocolRoute, WorkOpportunity, WorkRole,
    SEMANTIC_OBSERVATION_PROTOCOL, SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
};
use async_trait::async_trait;
#[cfg(unix)]
use fs2::FileExt;
use goose_provider_types::base::{expose_current_provider_http_request, ProviderHttpProtocol};
use std::collections::BTreeMap;
#[cfg(unix)]
use std::fs::OpenOptions;
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::sync::Arc;
use tokio::sync::Notify;

const TRANSPORT_ID: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct EngineFixture {
    plane: BrokeredSemanticObservationPlane,
    request: SemanticObservationCaptureRequest,
    task_evidence: SemanticTaskEvidenceCapability,
    source_lifecycle: goose_swarm::ProviderLifecycle,
    source_request: goose_swarm::control_plane::StartedProviderRequest,
    _lease_root: tempfile::TempDir,
}

#[derive(Clone, Copy)]
enum ReplyKind {
    Continue,
    Nudge,
}

struct TypedReviewer(ReplyKind);

struct BlockingTypedReviewer {
    started: Notify,
    release: Notify,
}

struct TestJournal;

impl ProviderLifecycleJournal for TestJournal {
    fn provider_request_started(&self, _receipt: &ProviderRequestReceipt) -> Result<(), String> {
        Ok(())
    }

    fn provider_terminal(&self, _receipt: &ProviderTerminalReceipt) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait]
impl AdmittedSemanticObservationReviewer for TypedReviewer {
    async fn review(
        &self,
        request: AdmittedSemanticObservationRequest,
    ) -> Result<String, AdmittedSemanticReviewError> {
        expose_current_provider_http_request(
            ProviderHttpProtocol::OpenAiChatCompletions,
            TRANSPORT_ID,
        )
        .map_err(AdmittedSemanticReviewError::unresolved)?;
        Ok(raw_reply(self.0, &request.observation.snapshot))
    }
}

#[async_trait]
impl AdmittedSemanticObservationReviewer for BlockingTypedReviewer {
    async fn review(
        &self,
        request: AdmittedSemanticObservationRequest,
    ) -> Result<String, AdmittedSemanticReviewError> {
        expose_current_provider_http_request(
            ProviderHttpProtocol::OpenAiChatCompletions,
            TRANSPORT_ID,
        )
        .map_err(AdmittedSemanticReviewError::unresolved)?;
        self.started.notify_waiters();
        self.release.notified().await;
        Ok(raw_reply(
            ReplyKind::Continue,
            &request.observation.snapshot,
        ))
    }
}

fn raw_reply(kind: ReplyKind, snapshot: &goose_swarm::SealedSemanticObservationSnapshot) -> String {
    let trace_source = format!("trace:{}", snapshot.payload().trace.sequence);
    let observation = match kind {
        ReplyKind::Continue => serde_json::json!({
            "action": "CONTINUE",
            "summary": "the trace is still making grounded progress",
            "evidence": [{
                "source_id": trace_source,
                "observation": "the owned handler is advancing"
            }]
        }),
        ReplyKind::Nudge => serde_json::json!({
            "action": "NUDGE",
            "summary": "the trace is advancing against the wrong criterion",
            "evidence": [
                {
                    "source_id": "acceptance:criterion",
                    "observation": "the sealed handler response is required"
                },
                {
                    "source_id": trace_source,
                    "observation": "the worker is editing the unrelated parser"
                }
            ],
            "guidance": "return to the owned handler and prove the sealed response"
        }),
    };
    serde_json::json!({
        "protocol": SEMANTIC_OBSERVATION_PROTOCOL,
        "snapshot_hash": snapshot.snapshot_hash(),
        "observation": observation,
    })
    .to_string()
}

fn raw_reply_hash(raw: &str) -> String {
    let digest = <sha2::Sha256 as sha2::Digest>::digest(raw.as_bytes());
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    encoded
}

fn fleet() -> PhysicalFleetSnapshot {
    PhysicalFleetSnapshot::new(
        "semantic-authority-fleet",
        [
            ("worker-a", "model-a", "host-a", "instance-a"),
            ("worker-b", "model-b", "host-b", "instance-b"),
            ("observer", "judge-model", "judge-host", "judge-instance"),
        ]
        .into_iter()
        .map(|(logical, model, host, instance)| VerifiedPhysicalLane {
            logical_device_id: logical.to_string(),
            model_id: model.to_string(),
            host_id: host.to_string(),
            model_instance_id: instance.to_string(),
            provider_transport_id: TRANSPORT_ID.to_string(),
            advertised_instance_capacity: 1,
            routing_weight: 1,
            capacity_evidence: HostCapacityEvidence::ProbeSingleStream {
                probe_epoch: format!("probe-{logical}"),
            },
            route_evidence_id: format!("route-{logical}"),
        })
        .collect(),
    )
    .unwrap()
}

fn task_source() -> TaskVersion {
    TaskVersion {
        authority_scope: AuthorityScope::new("run-a", "build"),
        phase_epoch: 3,
        task_id: "task-a".to_string(),
        attempt: 0,
        revision: 1,
        kind: SourceRevisionKind::TaskAttempt,
    }
}

async fn start_unexposed_source_request(
    control: &PhysicalAdmissionControl,
    work_id: &str,
    logical_device_id: &str,
) -> (
    goose_swarm::AdmissionReceipt,
    goose_swarm::ProviderLifecycle,
    goose_swarm::control_plane::StartedProviderRequest,
) {
    let source = task_source();
    control.set_source_revision(source.clone()).await.unwrap();
    let admitted = control
        .admit(WorkOpportunity {
            work_id: work_id.to_string(),
            role: WorkRole::Build,
            priority: WorkRole::Build.priority(),
            task_rank: 11,
            source,
            eligible_logical_device_ids: vec![logical_device_id.to_string()],
            preferred_model_id: None,
            excluded_logical_device_id: None,
        })
        .await
        .unwrap();
    let admission = admitted.receipt().clone();
    let lifecycle = admitted.lifecycle();
    let started = lifecycle.start_provider_request().await.unwrap();
    (admission, lifecycle, started)
}

async fn expose_source_request(started: &goose_swarm::control_plane::StartedProviderRequest) {
    started
        .scope_http(async {
            expose_current_provider_http_request(
                ProviderHttpProtocol::OpenAiChatCompletions,
                TRANSPORT_ID,
            )
        })
        .await
        .unwrap();
}

fn capture_request(admission: &goose_swarm::AdmissionReceipt) -> SemanticObservationCaptureRequest {
    let publisher = SemanticActivityPublisher::from_admission(admission);
    SemanticObservationCaptureRequest {
        task_id: "task-a".to_string(),
        attempt: 0,
        task_rank: 11,
        goal: "Implement the sealed handler".to_string(),
        task_contract: "Change only the owned handler and prove its response".to_string(),
        owned_files: vec!["src/task.rs".to_string()],
        contract_version: "contract-v1".to_string(),
        acceptance_oracle: vec![AcceptanceCriterionSnapshot {
            id: "criterion".to_string(),
            text: "The owned handler returns the sealed response".to_string(),
        }],
        dependency_contract_versions: BTreeMap::new(),
        sibling_contract_versions: BTreeMap::new(),
        allowed_finding_routes: vec!["integrate-verify".to_string()],
        running_logical_device_id: publisher.logical_device_id.clone(),
        running_model_id: publisher.model_id.clone(),
        activity_publisher: publisher,
    }
}

fn observation_capture(
    request: &SemanticObservationCaptureRequest,
    trace_revision: u64,
) -> SemanticObservationCapture {
    let artifact_version = format!("artifact-{trace_revision}");
    let summons = SemanticObservationSummonsSignal::TraceStateAdvanced {
        source_id: "signal:progress".to_string(),
        measurement: TraceStateMeasurement {
            measurement_hash: format!("measurement-{trace_revision}"),
            tool_calls: 1,
            failed_tool_calls: 0,
            malformed_tool_calls: 0,
            pending_tool_calls: 0,
            thinking_chars: 500,
            recurrence_window_chars: 48,
            recurrence_observed_windows: 100,
            recurrence_repeated_windows: 10,
            recurrence_repeat_share: 0.1,
            provider_stream_revision: 0,
            provider_stream_chunks: 0,
            provider_stream_bytes: 0,
            provider_structured_output_chunks: 0,
            provider_structured_output_bytes: 0,
            provider_last_progress_elapsed_ms: 0,
            provider_structured_output_active: false,
            artifact_version: artifact_version.clone(),
        },
        provenance: "typed provider activity".to_string(),
    };
    let snapshot = SemanticObservationSnapshotDraft {
        schema_version: SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
        authority_scope: request.activity_publisher.source.authority_scope.clone(),
        phase_epoch: request.activity_publisher.source.phase_epoch,
        task_id: request.task_id.clone(),
        attempt: request.attempt,
        source_revision: trace_revision,
        contract_version: request.contract_version.clone(),
        artifact_version,
        goal: request.goal.clone(),
        task_contract: request.task_contract.clone(),
        acceptance_oracle: request.acceptance_oracle.clone(),
        dependency_contract_versions: request.dependency_contract_versions.clone(),
        sibling_contract_versions: request.sibling_contract_versions.clone(),
        allowed_finding_routes: request.allowed_finding_routes.clone(),
        artifacts: vec![ArtifactExcerptSnapshot {
            source_id: "artifact:handler-snapshot".to_string(),
            path: "src/task.rs".to_string(),
            excerpt: "pub fn handler() -> &'static str { \"wrong\" }".to_string(),
            complete: false,
        }],
        trace: SemanticTraceSnapshot {
            sequence: trace_revision,
            recent_reasoning: "I keep editing the unrelated parser".to_string(),
            recent_actions: vec!["edited src/task.rs".to_string()],
            prior_intervention: None,
            response_to_prior_intervention: None,
        },
        neutral_signals: vec![summons.neutral_signal()],
    }
    .seal()
    .unwrap();
    SemanticObservationCapture::new(snapshot, summons).unwrap()
}

async fn fixture_with_source_exposure(expose_source: bool) -> EngineFixture {
    fixture_with_source_exposure_in_scope(expose_source, "semantic-authority").await
}

async fn fixture_with_source_exposure_in_scope(
    expose_source: bool,
    authority_scope: &str,
) -> EngineFixture {
    let sink: Arc<dyn EventSink> = Arc::new(NullSink);
    let lease_root = tempfile::tempdir().unwrap();
    let fleet = fleet();
    let sealed = SealedProviderLeaseAuthority::from_fleet_snapshot(
        &fleet,
        [VerifiedProviderProtocolRoute::new(
            TRANSPORT_ID,
            ProviderHttpProtocol::OpenAiChatCompletions,
        )
        .unwrap()],
    )
    .unwrap();
    let physical = Arc::new(
        GlobalProviderLeaseAuthority::open_test_root(lease_root.path().join("provider-leases"))
            .unwrap(),
    );
    let provider_leases = RunScopedProviderLeaseAuthority::new(physical, sealed);
    let control = PhysicalAdmissionControl::new_with_journal_and_provider_leases(
        authority_scope,
        fleet,
        sink.clone(),
        Arc::new(TestJournal),
        Some(provider_leases),
    )
    .unwrap();
    let plane = BrokeredSemanticObservationPlane::new(control.clone(), sink).unwrap();
    let (admission, source_lifecycle, source_request) =
        start_unexposed_source_request(&control, "build:task-a:first", "worker-a").await;
    if expose_source {
        expose_source_request(&source_request).await;
    }
    let request = capture_request(&admission);
    let engine_authority =
        EngineSemanticTaskAuthority::mint_from_scheduler_state(&request).unwrap();
    let task_evidence = plane
        .register_scheduler_task_evidence(engine_authority, &request)
        .unwrap();
    EngineFixture {
        plane,
        request,
        task_evidence,
        source_lifecycle,
        source_request,
        _lease_root: lease_root,
    }
}

async fn fixture() -> EngineFixture {
    fixture_with_source_exposure(true).await
}

fn seal_trace(fixture: &EngineFixture, trace_revision: u64) -> BoundSemanticObservationCapture {
    let capture = observation_capture(&fixture.request, trace_revision);
    fixture
        .plane
        .publish_scheduler_activity(&fixture.task_evidence, &fixture.request, &capture)
        .unwrap();
    let permit = fixture
        .plane
        .mint_semantic_nudge_capture_permit(
            &fixture.task_evidence,
            &fixture.request,
            &fixture.source_request,
        )
        .unwrap();
    fixture
        .plane
        .seal_semantic_nudge_capture(capture, &fixture.request, &fixture.task_evidence, permit)
        .unwrap()
}

async fn review(
    plane: &BrokeredSemanticObservationPlane,
    bound: &BoundSemanticObservationCapture,
    kind: ReplyKind,
) -> AdmittedSemanticObservationReceipt {
    let submission = plane
        .submit_if_idle(
            bound.snapshot().clone(),
            SemanticObservationAdmissionPolicy {
                task_rank: 11,
                eligible_logical_device_ids: vec!["observer".to_string()],
                preferred_model_id: None,
                excluded_logical_device_id: None,
            },
            Arc::new(TypedReviewer(kind)),
        )
        .await
        .unwrap()
        .expect("observer lane is idle");
    match submission {
        SemanticObservationAdmissionSubmission::Started(handle) => handle.wait().await.unwrap(),
        SemanticObservationAdmissionSubmission::Rejected(rejection) => {
            panic!("semantic review was rejected: {:?}", rejection.rejection)
        }
    }
}

#[tokio::test]
async fn forged_matching_acceptance_pair_cannot_use_registered_task_capability() {
    let fixture = fixture().await;
    let mut forged = fixture.request.clone();
    forged.acceptance_oracle[0].text = "A substituted matching-looking criterion".to_string();
    let authentic_authority =
        EngineSemanticTaskAuthority::mint_from_scheduler_state(&fixture.request).unwrap();
    assert_eq!(
        fixture
            .plane
            .register_scheduler_task_evidence(authentic_authority, &forged)
            .unwrap_err(),
        SemanticNudgeAuthorityError::TaskEvidenceNotRegistered.to_string()
    );
    assert!(matches!(
        fixture.plane.mint_semantic_nudge_capture_permit(
            &fixture.task_evidence,
            &forged,
            &fixture.source_request,
        ),
        Err(SemanticNudgeAuthorityError::TaskEvidenceNotRegistered)
    ));
}

#[tokio::test]
async fn capture_permit_requires_verified_exposure_before_witness_mint() {
    let fixture = fixture_with_source_exposure(false).await;
    assert!(matches!(
        fixture.plane.mint_semantic_nudge_capture_permit(
            &fixture.task_evidence,
            &fixture.request,
            &fixture.source_request,
        ),
        Err(SemanticNudgeAuthorityError::InvalidCapture(detail))
            if detail.contains("not exposed at witness mint")
    ));
    expose_source_request(&fixture.source_request).await;
    fixture
        .plane
        .mint_semantic_nudge_capture_permit(
            &fixture.task_evidence,
            &fixture.request,
            &fixture.source_request,
        )
        .expect("the exact request may mint after verified exposure");
}

#[tokio::test]
async fn identical_capture_bytes_cannot_bind_to_two_genuine_provider_requests() {
    let fixture = fixture().await;
    let EngineFixture {
        plane,
        request,
        task_evidence,
        source_lifecycle,
        source_request,
        _lease_root,
    } = fixture;
    let first_capture = observation_capture(&request, 7);
    let second_capture = observation_capture(&request, 7);
    assert_eq!(
        first_capture.snapshot().snapshot_hash(),
        second_capture.snapshot().snapshot_hash()
    );
    plane
        .publish_scheduler_activity(&task_evidence, &request, &first_capture)
        .unwrap();
    let first_permit = plane
        .mint_semantic_nudge_capture_permit(&task_evidence, &request, &source_request)
        .unwrap();
    let first_provider_key = source_request.receipt().key.clone();
    let _first_bound = plane
        .seal_semantic_nudge_capture(first_capture, &request, &task_evidence, first_permit)
        .unwrap();
    source_request
        .provider_terminal(ProviderTerminalKind::Finished)
        .await
        .unwrap();
    let second_source_request = source_lifecycle.start_provider_request().await.unwrap();
    expose_source_request(&second_source_request).await;
    assert_ne!(first_provider_key, second_source_request.receipt().key);
    let second_permit = plane
        .mint_semantic_nudge_capture_permit(&task_evidence, &request, &second_source_request)
        .unwrap();
    assert!(matches!(
        plane.seal_semantic_nudge_capture(second_capture, &request, &task_evidence, second_permit,),
        Err(SemanticNudgeAuthorityError::SnapshotAlreadyBound)
    ));
}

#[tokio::test]
async fn authentic_continue_cannot_be_mutated_into_nudge() {
    let fixture = fixture().await;
    let bound = seal_trace(&fixture, 7);
    let mut receipt = review(&fixture.plane, &bound, ReplyKind::Continue).await;
    let raw_nudge = raw_reply(ReplyKind::Nudge, bound.snapshot());
    receipt.observation.decision = parse_semantic_observation_reply(bound.snapshot(), &raw_nudge);
    assert!(matches!(
        fixture
            .plane
            .issue_semantic_nudge_eligibility(bound, receipt),
        Err(SemanticNudgeAuthorityError::InvalidEvidence(
            SemanticNudgeEligibilityError::InvalidAdmittedReceipt
        ))
    ));
}

#[tokio::test]
async fn authentic_continue_from_real_admitted_path_remains_observation_only() {
    let fixture = fixture().await;
    let bound = seal_trace(&fixture, 7);
    let receipt = review(&fixture.plane, &bound, ReplyKind::Continue).await;
    assert_eq!(
        receipt.observation().action(),
        goose_swarm::SemanticJudgeAction::Continue
    );
    assert_eq!(
        receipt.reviewer_provider_terminal(),
        Some(ProviderTerminalKind::Finished)
    );
    assert!(fixture
        .plane
        .issue_semantic_nudge_eligibility(bound, receipt)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn authentic_nudge_observation_cannot_replace_an_authentic_continue() {
    let first_fixture = fixture().await;
    let second_fixture = fixture().await;
    let first_bound = seal_trace(&first_fixture, 7);
    let second_bound = seal_trace(&second_fixture, 7);
    assert_eq!(
        first_bound.snapshot().canonical_json(),
        second_bound.snapshot().canonical_json()
    );
    let mut first = review(&first_fixture.plane, &first_bound, ReplyKind::Continue).await;
    let second = review(&second_fixture.plane, &second_bound, ReplyKind::Nudge).await;
    first.observation = second.observation.clone();
    first.reviewer_raw_reply_hash = second.reviewer_raw_reply_hash.clone();
    let caller_digest = admitted_semantic_receipt_hash(
        &first.completed_admission,
        &first.observation,
        first.reviewer_completion.as_ref(),
        first.reviewer_raw_reply_hash.as_deref(),
    );
    first.authority_seal.review_id = format!("caller-reseal:{caller_digest}");
    assert!(matches!(
        first_fixture
            .plane
            .issue_semantic_nudge_eligibility(first_bound, first),
        Err(SemanticNudgeAuthorityError::InvalidEvidence(
            SemanticNudgeEligibilityError::InvalidAdmittedReceipt
        ))
    ));
}

#[tokio::test]
async fn intact_authentic_receipt_cannot_cross_semantic_authorities() {
    let first_fixture = fixture().await;
    let second_fixture = fixture().await;
    let first_bound = seal_trace(&first_fixture, 7);
    let second_bound = seal_trace(&second_fixture, 7);
    assert_eq!(
        first_bound.snapshot().snapshot_hash(),
        second_bound.snapshot().snapshot_hash()
    );
    let second = review(&second_fixture.plane, &second_bound, ReplyKind::Nudge).await;
    second_fixture
        .plane
        .admitted_receipt_authority
        .verify(&second)
        .unwrap();
    assert_eq!(
        second.admission().fleet_snapshot_id,
        "semantic-authority-fleet"
    );
    assert!(matches!(
        first_fixture
            .plane
            .issue_semantic_nudge_eligibility(first_bound, second),
        Err(SemanticNudgeAuthorityError::InvalidEvidence(
            SemanticNudgeEligibilityError::InvalidAdmittedReceipt
        ))
    ));
}

#[tokio::test]
async fn forged_reseal_of_mutated_continue_is_rejected() {
    let fixture = fixture().await;
    let bound = seal_trace(&fixture, 7);
    let mut receipt = review(&fixture.plane, &bound, ReplyKind::Continue).await;
    let raw_nudge = raw_reply(ReplyKind::Nudge, bound.snapshot());
    receipt.observation.decision = parse_semantic_observation_reply(bound.snapshot(), &raw_nudge);
    receipt.observation.reviewer_reply_hash = Some(raw_reply_hash(&raw_nudge));
    receipt.reviewer_raw_reply_hash = Some(raw_reply_hash(&raw_nudge));
    let caller_digest = admitted_semantic_receipt_hash(
        &receipt.completed_admission,
        &receipt.observation,
        receipt.reviewer_completion.as_ref(),
        receipt.reviewer_raw_reply_hash.as_deref(),
    );
    receipt.authority_seal.review_id = format!("caller-reseal:{caller_digest}");
    assert!(matches!(
        fixture
            .plane
            .issue_semantic_nudge_eligibility(bound, receipt),
        Err(SemanticNudgeAuthorityError::InvalidEvidence(
            SemanticNudgeEligibilityError::InvalidAdmittedReceipt
        ))
    ));
}

#[tokio::test]
async fn cross_admission_splice_fails_the_engine_seal() {
    let first_fixture = fixture().await;
    let second_fixture =
        fixture_with_source_exposure_in_scope(true, "semantic-authority-splice").await;
    let first_bound = seal_trace(&first_fixture, 7);
    let second_bound = seal_trace(&second_fixture, 7);
    let mut first = review(&first_fixture.plane, &first_bound, ReplyKind::Nudge).await;
    let mut second = review(&second_fixture.plane, &second_bound, ReplyKind::Nudge).await;
    assert_ne!(
        first.admission().admission_id,
        second.admission().admission_id
    );
    assert_eq!(
        first.admission().physical_host_id,
        second.admission().physical_host_id
    );
    assert_eq!(first.admission().model_id, second.admission().model_id);
    assert_eq!(
        first.admission().provider_transport_id,
        second.admission().provider_transport_id
    );
    std::mem::swap(
        &mut first.completed_admission,
        &mut second.completed_admission,
    );
    assert!(matches!(
        first_fixture
            .plane
            .admitted_receipt_authority
            .verify(&first),
        Err(SemanticNudgeEligibilityError::InvalidAdmittedReceipt)
    ));
}

#[tokio::test]
async fn cross_request_terminal_splice_fails_the_engine_seal() {
    let first_fixture = fixture().await;
    let second_fixture = fixture().await;
    let first_bound = seal_trace(&first_fixture, 7);
    let second_bound = seal_trace(&second_fixture, 7);
    let mut first = review(&first_fixture.plane, &first_bound, ReplyKind::Nudge).await;
    let second = review(&second_fixture.plane, &second_bound, ReplyKind::Nudge).await;
    let first_completion = first.reviewer_completion.as_ref().unwrap();
    let second_completion = second.reviewer_completion.as_ref().unwrap();
    assert_ne!(
        first_completion.request().key,
        second_completion.request().key
    );
    assert_eq!(
        first_completion.request().physical_host_id,
        second_completion.request().physical_host_id
    );
    assert_eq!(
        first_completion.request().model_instance_id,
        second_completion.request().model_instance_id
    );
    first.reviewer_completion = Some(CompletedProviderRequest::forge_spliced_for_replay(
        first_completion,
        second_completion,
    ));
    assert!(matches!(
        first_fixture
            .plane
            .admitted_receipt_authority
            .verify(&first),
        Err(SemanticNudgeEligibilityError::InvalidAdmittedReceipt)
    ));
}

#[tokio::test]
async fn source_terminal_after_review_rejects_redemption_without_semantic_callback() {
    let fixture = fixture().await;
    let bound = seal_trace(&fixture, 7);
    let receipt = review(&fixture.plane, &bound, ReplyKind::Nudge).await;
    let eligibility = fixture
        .plane
        .issue_semantic_nudge_eligibility(bound, receipt)
        .unwrap()
        .expect("authentic grounded NUDGE is eligible");
    let EngineFixture {
        plane,
        source_request,
        _lease_root,
        ..
    } = fixture;
    source_request
        .provider_terminal(ProviderTerminalKind::Finished)
        .await
        .unwrap();
    assert!(matches!(
        plane.redeem_existing_judge_nudge(&eligibility),
        Err(SemanticNudgeAuthorityError::SourceProviderNotLive),
    ));
    let ledger = lock_nudge_ledger(&plane.nudge_authority.inner);
    assert!(ledger.source_provider_sessions.is_empty());
    assert!(ledger.unused_capture_permits.is_empty());
    assert!(ledger.captures.is_empty());
    assert!(ledger.current_capture_by_task.is_empty());
    assert!(ledger.capabilities.is_empty());
}

#[tokio::test]
async fn source_drop_after_review_rejects_redemption_without_semantic_callback() {
    let fixture = fixture().await;
    let bound = seal_trace(&fixture, 7);
    let receipt = review(&fixture.plane, &bound, ReplyKind::Nudge).await;
    let eligibility = fixture
        .plane
        .issue_semantic_nudge_eligibility(bound, receipt)
        .unwrap()
        .expect("authentic grounded NUDGE is eligible");
    let EngineFixture {
        plane,
        source_request,
        _lease_root,
        ..
    } = fixture;
    drop(source_request);
    assert!(matches!(
        plane.redeem_existing_judge_nudge(&eligibility),
        Err(SemanticNudgeAuthorityError::SourceProviderNotLive),
    ));
}

#[tokio::test]
async fn newer_trace_after_review_invalidates_older_issue() {
    let fixture = fixture().await;
    let old = seal_trace(&fixture, 7);
    let old_receipt = review(&fixture.plane, &old, ReplyKind::Nudge).await;
    let newer_activity = observation_capture(&fixture.request, 8);
    fixture
        .plane
        .publish_scheduler_activity(&fixture.task_evidence, &fixture.request, &newer_activity)
        .unwrap();
    assert!(matches!(
        fixture
            .plane
            .issue_semantic_nudge_eligibility(old, old_receipt),
        Err(SemanticNudgeAuthorityError::CaptureNotCurrent)
    ));
}

#[tokio::test]
async fn newer_activity_without_a_second_capture_invalidates_issued_eligibility() {
    let fixture = fixture().await;
    let bound = seal_trace(&fixture, 7);
    let receipt = review(&fixture.plane, &bound, ReplyKind::Nudge).await;
    let eligibility = fixture
        .plane
        .issue_semantic_nudge_eligibility(bound, receipt)
        .unwrap()
        .unwrap();
    let newer_activity = observation_capture(&fixture.request, 8);
    fixture
        .plane
        .publish_scheduler_activity(&fixture.task_evidence, &fixture.request, &newer_activity)
        .unwrap();
    assert!(matches!(
        fixture.plane.redeem_existing_judge_nudge(&eligibility),
        Err(SemanticNudgeAuthorityError::CaptureNotCurrent)
    ));
    let ledger = lock_nudge_ledger(&fixture.plane.nudge_authority.inner);
    assert!(ledger.captures.is_empty());
    assert!(ledger.current_capture_by_task.is_empty());
    assert!(ledger.capabilities.is_empty());
}

#[tokio::test]
async fn stale_capture_cannot_be_sealed_after_newer_activity_publication() {
    let fixture = fixture().await;
    let stale = observation_capture(&fixture.request, 7);
    fixture
        .plane
        .publish_scheduler_activity(&fixture.task_evidence, &fixture.request, &stale)
        .unwrap();
    let permit = fixture
        .plane
        .mint_semantic_nudge_capture_permit(
            &fixture.task_evidence,
            &fixture.request,
            &fixture.source_request,
        )
        .unwrap();
    let newer = observation_capture(&fixture.request, 8);
    fixture
        .plane
        .publish_scheduler_activity(&fixture.task_evidence, &fixture.request, &newer)
        .unwrap();
    assert!(matches!(
        fixture.plane.seal_semantic_nudge_capture(
            stale,
            &fixture.request,
            &fixture.task_evidence,
            permit,
        ),
        Err(SemanticNudgeAuthorityError::CaptureNotCurrent)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_failed_delivery_does_not_spend_the_capability() {
    let fixture = fixture().await;
    let bound = seal_trace(&fixture, 7);
    let receipt = review(&fixture.plane, &bound, ReplyKind::Nudge).await;
    let eligibility = Arc::new(
        fixture
            .plane
            .issue_semantic_nudge_eligibility(bound, receipt)
            .unwrap()
            .unwrap(),
    );
    let first_plane = fixture.plane.clone();
    let first_eligibility = eligibility.clone();
    let first = tokio::task::spawn_blocking(move || {
        first_plane.redeem_existing_judge_nudge(&first_eligibility)
    });
    let second_plane = fixture.plane.clone();
    let second =
        tokio::task::spawn_blocking(move || second_plane.redeem_existing_judge_nudge(&eligibility));
    let results = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(SemanticNudgeAuthorityError::DeliveryUnavailableAfterSpend)
            ))
            .count(),
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_terminal_during_redemption_cannot_open_a_retry_path() {
    let fixture = fixture().await;
    let bound = seal_trace(&fixture, 7);
    let receipt = review(&fixture.plane, &bound, ReplyKind::Nudge).await;
    let eligibility = fixture
        .plane
        .issue_semantic_nudge_eligibility(bound, receipt)
        .unwrap()
        .unwrap();
    let result = fixture
        .plane
        .nudge_authority
        .redeem_record_with_pin_hook(&eligibility, || {});
    assert!(matches!(
        result,
        Err(SemanticNudgeAuthorityError::DeliveryUnavailableAfterSpend)
    ));
    fixture
        .source_request
        .provider_terminal(ProviderTerminalKind::Finished)
        .await
        .unwrap();
    assert!(matches!(
        fixture.plane.redeem_existing_judge_nudge(&eligibility),
        Err(SemanticNudgeAuthorityError::SourceProviderNotLive)
    ));
}

#[tokio::test]
async fn duplicate_redemption_is_atomically_rejected() {
    let fixture = fixture().await;
    let bound = seal_trace(&fixture, 7);
    let receipt = review(&fixture.plane, &bound, ReplyKind::Nudge).await;
    let eligibility = fixture
        .plane
        .issue_semantic_nudge_eligibility(bound, receipt)
        .unwrap()
        .expect("authentic grounded NUDGE is eligible");
    assert!(matches!(
        fixture.plane.redeem_existing_judge_nudge(&eligibility),
        Err(SemanticNudgeAuthorityError::DeliveryUnavailableAfterSpend)
    ));
    assert!(matches!(
        fixture.plane.redeem_existing_judge_nudge(&eligibility),
        Err(SemanticNudgeAuthorityError::DeliveryUnavailableAfterSpend)
    ));
}

#[cfg(unix)]
const TERMINAL_CONTENTION_CHILD_MODE: &str = "GOOSE_SEMANTIC_TERMINAL_CONTENTION_CHILD";
#[cfg(unix)]
const TERMINAL_CONTENTION_ROOT: &str = "GOOSE_SEMANTIC_TERMINAL_CONTENTION_ROOT";
#[cfg(unix)]
const TERMINAL_CONTENTION_READY: &str = "GOOSE_SEMANTIC_TERMINAL_CONTENTION_READY";
#[cfg(unix)]
const TERMINAL_CONTENTION_RELEASE: &str = "GOOSE_SEMANTIC_TERMINAL_CONTENTION_RELEASE";

#[cfg(unix)]
#[test]
fn semantic_terminal_contention_lock_holder_child() {
    if std::env::var_os(TERMINAL_CONTENTION_CHILD_MODE).is_none() {
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os(TERMINAL_CONTENTION_ROOT).unwrap());
    let ready = std::path::PathBuf::from(std::env::var_os(TERMINAL_CONTENTION_READY).unwrap());
    let release = std::path::PathBuf::from(std::env::var_os(TERMINAL_CONTENTION_RELEASE).unwrap());
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("control.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    std::fs::write(&ready, b"locked").unwrap();
    while !release.exists() {
        std::thread::yield_now();
    }
    lock.unlock().unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_terminal_authority_contention_returns_typed_failure_without_spinning() {
    let fixture = fixture().await;
    let capture = observation_capture(&fixture.request, 7);
    let reviewer = Arc::new(BlockingTypedReviewer {
        started: Notify::new(),
        release: Notify::new(),
    });
    let submission = fixture
        .plane
        .submit_if_idle(
            capture.snapshot().clone(),
            SemanticObservationAdmissionPolicy {
                task_rank: 11,
                eligible_logical_device_ids: vec!["observer".to_string()],
                preferred_model_id: None,
                excluded_logical_device_id: None,
            },
            reviewer.clone(),
        )
        .await
        .unwrap()
        .unwrap();
    let handle = match submission {
        SemanticObservationAdmissionSubmission::Started(handle) => handle,
        SemanticObservationAdmissionSubmission::Rejected(rejected) => {
            panic!("semantic review was rejected: {:?}", rejected.rejection)
        }
    };
    reviewer.started.notified().await;

    let ready = fixture._lease_root.path().join("terminal-contention-ready");
    let release = fixture
        ._lease_root
        .path()
        .join("terminal-contention-release");
    let lease_authority = fixture._lease_root.path().join("provider-leases");
    let child = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg(
            "semantic_control::authority_replay_tests::semantic_terminal_contention_lock_holder_child",
        )
        .arg("--nocapture")
        .env(TERMINAL_CONTENTION_CHILD_MODE, "1")
        .env(TERMINAL_CONTENTION_ROOT, &lease_authority)
        .env(TERMINAL_CONTENTION_READY, &ready)
        .env(TERMINAL_CONTENTION_RELEASE, &release)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !ready.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("lease authority lock holder did not start");

    reviewer.release.notify_one();
    let error = tokio::time::timeout(std::time::Duration::from_secs(2), handle.wait())
        .await
        .expect("authority contention entered an unbounded retry loop")
        .unwrap_err();
    assert!(matches!(
        error,
        SemanticObservationAdmissionError::ProviderLifecycleUnresolved { .. }
    ));
    assert!(error.to_string().contains("contended"));

    std::fs::write(&release, b"release").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "lock-holder child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
