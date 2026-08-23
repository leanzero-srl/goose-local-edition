//! goose-swarm: a weighted work-queue scheduler for the local LM Studio / LM Link swarm.
//!
//! Fork-only crate (goose-local-edition). The 27B planner emits a typed DAG; this crate owns the
//! locking DAG queue, per-device weights, pull-based work-stealing, and the shared-context store.
//! It is model-agnostic: tasks run through the [`dispatch::TaskDispatcher`] trait, so the
//! concurrency core is unit-testable against a mock with no model involved.

pub mod broker;
pub mod coherence;
pub mod context;
pub mod control_plane;
pub mod dag;
pub mod dispatch;
pub mod event;
pub mod judge;
pub mod memory_classify;
pub mod provider_lease;
pub mod repair;
pub mod replan;
pub mod scheduler;
pub mod semantic_control;
pub mod semantic_observation;
pub mod semantic_runtime;

pub use broker::{
    AdmissionReceipt, AuthorityScope, BrokerError, BrokerGrant, CapacityUpdateReceipt,
    HostCapacityEvidence, LocalCompletionKind, LocalCompletionReceipt, PhysicalBroker,
    PhysicalExecutionAuthority, PhysicalFleetSnapshot, PhysicalHostOccupancy,
    ProviderNotStartedReceipt, ProviderRequestDisposition, ProviderRequestKey,
    ProviderRequestQueueReceipt, ProviderRequestReceipt, ProviderStartsClosure,
    ProviderTerminalKind, ProviderTerminalReceipt, QuarantinedAdmissionReceipt, QueueReceipt,
    ReleasedAdmissionReceipt, RevokedAdmissionReceipt, SourceRevisionKind, StaleWorkReceipt,
    TaskVersion, UnresolvedAdmissionReceipt, VerifiedPhysicalIdentity, VerifiedPhysicalLane,
    WithdrawnWorkReceipt, WorkOpportunity, WorkPriority, WorkRole,
};
pub use coherence::{extract_signatures, scope_contract_bundle, SigLang};
pub use context::SharedContext;
pub use control_plane::{
    AdmittedWork, CapturedProviderRequest, CompletedAdmission, CompletedProviderRequest,
    PhysicalAdmissionControl, ProviderLifecycle, ProviderLifecycleDispatcher,
    ProviderLifecycleJournal, ProviderLifecycleOperationError, ProviderLifecycleStartError,
    ProviderLifecycleTransitionError, ProviderNudgeDelivery, ProviderNudgeDeliveryReceipt,
    ProviderNudgeSafetyGate, ProviderStartKey, ProviderStartLookupError, ProviderStartRegistry,
    ProviderStartSession, StartedProviderRequest,
};
pub use dag::{
    expand_subsplits, extract_subsplit, fill_fan_enabled, specs_from_plan_json, Dag, Difficulty,
    Node, ReplanAuthorityFact, ReplanAuthorityReceipt, TaskId, TaskSpec, TaskState,
};
pub use dispatch::{
    DispatchError, DispatchRequest, ProviderDispatchClass, TaskDispatcher, TaskRunOutput,
    ToolCallRecord,
};
pub use event::{EventSink, NullSink, SwarmEvent};
pub use judge::{
    deterministic_verdict, is_split_candidate, ChildSpec, Judge, JudgeConfig, JudgeInput,
    JudgeOutcome, JudgeRequest, PreReviewOutput, PreReviewRequest, PreReviewer, Verdict,
};
pub use provider_lease::{
    ActiveProviderLeaseSnapshot, ExposedProviderLease, GlobalProviderLeaseAuthority,
    LeaseHostCapacityEvidence, LeaseWorkPriority, LeaseWorkRole, PhysicalProviderLeaseAuthority,
    ProviderLeaseAuthoritySnapshot, ProviderLeaseBoundaryStatus, ProviderLeaseBusy,
    ProviderLeaseBusyKind, ProviderLeaseClaim, ProviderLeaseError, ProviderLeaseHttpBoundary,
    ProviderLeaseReleaseReceipt, ProviderLeaseStatus, ProviderLeaseTransitionError,
    ProviderLeaseTry, ProviderLeaseWaitPolicy, ReservedProviderLease,
    RunScopedProviderLeaseAuthority, SealedProviderLeaseAuthority, VerifiedProviderProtocolRoute,
};
pub use repair::{
    repair_tree_snapshot, ArtifactEvidence, CandidateDelta, DefectId, DefectKind, DefectLedger,
    DefectObservation, EvidenceRef, FileMutation, FindingProvenance, GateId, ImpactEvidence,
    ImpactFact, MechanicalSeverity, PromotionDecision, ProvisionalTaskReceipt,
    RepairCandidatePatch, RepairEpoch, RepairTransaction, RepairTreeSnapshot, RequiredVerification,
    RequirementId, RulerIdentity, RulerLegId, SalvageReason, SemanticAcceptanceReceipt,
    SemanticReviewRequest, SubjectRef, TaskCompletionDisposition,
};
pub use replan::{ReplanContext, Replanner};
pub use scheduler::{
    qa_enabled, salvage_require_critical, salvage_spin_enabled, sink_review_enabled,
    tail_review_enabled, testgen_enabled, AttemptRecord, DeviceCfg, DeviceSummary, RunReport,
    Scheduler, TaskOutcome,
};
pub use semantic_control::{
    semantic_observation_task_version, AdmittedSemanticObservationHandle,
    AdmittedSemanticObservationReceipt, AdmittedSemanticObservationRequest,
    AdmittedSemanticObservationReviewer, AdmittedSemanticReviewError,
    BrokeredSemanticObservationPlane, RejectedSemanticObservationAdmission,
    SemanticObservationAdmissionError, SemanticObservationAdmissionPolicy,
    SemanticObservationAdmissionStage, SemanticObservationAdmissionSubmission,
};
pub use semantic_observation::{
    parse_semantic_observation_reply, semantic_observation_response_schema,
    AcceptanceCriterionSnapshot, ArtifactExcerptSnapshot, NeutralJudgeSignal,
    ParsedSemanticObservation, SealedSemanticObservationSnapshot, SemanticEvidenceCitation,
    SemanticJudgeAction, SemanticObservationBody, SemanticObservationReceipt,
    SemanticObservationRejection, SemanticObservationReply, SemanticObservationRequest,
    SemanticObservationSnapshotDraft, SemanticProtocolFailure, SemanticProtocolFailureKind,
    SemanticSplitBoundary, SemanticTraceSnapshot, SEMANTIC_OBSERVATION_PROTOCOL,
    SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
};
pub use semantic_runtime::{
    SemanticActivityPublisher, SemanticObservationCapture, SemanticObservationCaptureRequest,
    SemanticObservationSnapshotProducer, SemanticObservationSummonsSignal, SemanticTraceRevision,
    TraceStateMeasurement,
};
