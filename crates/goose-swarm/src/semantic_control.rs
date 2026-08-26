//! Physical admission and provider-lifecycle binding for semantic observations.
//!
//! This adapter admits typed observations and lets engine-held, provider-bound authority redeem a
//! grounded NUDGE into the exact live worker session.

use crate::broker::{
    AdmissionReceipt, BrokerError, LocalCompletionKind, ProviderRequestKey, ProviderTerminalKind,
    SourceRevisionKind, TaskVersion, WorkOpportunity, WorkRole,
};
use crate::control_plane::{
    AdmittedWork, CompletedAdmission, CompletedProviderRequest, ExposedProviderRequestWitness,
    LiveProviderRequestSession, PhysicalAdmissionControl, ProviderLifecycle,
    ProviderLifecycleTransitionError, ProviderNudgeDelivery, ProviderNudgeSafetySnapshot,
    ProviderStartSession, StartedProviderRequest,
};
use crate::event::EventSink;
use crate::semantic_observation::{
    validate_reply, ParsedSemanticObservation, SealedSemanticObservationSnapshot,
    SemanticEvidenceCitation, SemanticObservationBody, SemanticObservationHandle,
    SemanticObservationPlane, SemanticObservationReceipt, SemanticObservationRejection,
    SemanticObservationRequest, SemanticObservationReviewer, SemanticObservationSubmission,
    SemanticProtocolFailureKind, SEMANTIC_OBSERVATION_PROTOCOL,
};
use crate::semantic_runtime::{
    BoundSemanticObservationCapture, EngineSemanticTaskAuthority, SemanticNudgeBoundary,
    SemanticObservationCapture, SemanticObservationCaptureRequest,
    SemanticSourceProviderSessionBoundary, SemanticTaskAcceptanceSlice,
    SemanticTaskEvidenceCapability, SemanticTraceRevision,
};
use async_trait::async_trait;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::{oneshot, Mutex as AsyncMutex, OwnedMutexGuard};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SemanticObservationAdmissionPolicy {
    pub task_rank: u64,
    pub eligible_logical_device_ids: Vec<String>,
    pub preferred_model_id: Option<String>,
    pub excluded_logical_device_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AdmittedSemanticObservationRequest {
    pub observation: SemanticObservationRequest,
    pub admission: AdmissionReceipt,
    /// `None` during route-only preflight; `Some` only after the engine has minted the exact
    /// non-reconstructible provider request.
    pub provider_request_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdmittedSemanticReviewError {
    TerminalFailure(String),
    LocalFailureAfterTerminal {
        detail: String,
        provider_terminal: ProviderTerminalKind,
    },
    ProviderLifecycleUnresolved(String),
}

impl AdmittedSemanticReviewError {
    pub fn terminal_failure(detail: impl Into<String>) -> Self {
        Self::TerminalFailure(detail.into())
    }

    pub fn unresolved(detail: impl Into<String>) -> Self {
        Self::ProviderLifecycleUnresolved(detail.into())
    }

    pub fn local_failure_after_terminal(
        detail: impl Into<String>,
        provider_terminal: ProviderTerminalKind,
    ) -> Self {
        Self::LocalFailureAfterTerminal {
            detail: detail.into(),
            provider_terminal,
        }
    }

    fn detail(&self) -> &str {
        match self {
            Self::TerminalFailure(detail) | Self::ProviderLifecycleUnresolved(detail) => detail,
            Self::LocalFailureAfterTerminal { detail, .. } => detail,
        }
    }
}

#[async_trait]
pub trait AdmittedSemanticObservationReviewer: Send + Sync {
    /// Exact logical lanes for which this reviewer has a provider binding. `None` preserves the
    /// generic adapter contract and leaves rejection to `verify_admission`; production route-bound
    /// adapters return `Some` so the scheduler never queues work for a provider it cannot serve.
    fn eligible_logical_device_ids(&self) -> Option<Vec<String>> {
        None
    }

    /// Validate that this exact admission can be served by a verified provider without starting a
    /// request. Production adapters use this to fail closed on route/model drift.
    fn verify_admission(
        &self,
        _request: &AdmittedSemanticObservationRequest,
    ) -> std::result::Result<(), String> {
        Ok(())
    }

    async fn review(
        &self,
        request: AdmittedSemanticObservationRequest,
    ) -> std::result::Result<String, AdmittedSemanticReviewError>;
}

#[derive(Debug)]
struct SemanticAdmittedReceiptSeal {
    authority_id: String,
    review_id: String,
}

#[derive(Clone)]
struct SemanticAdmittedReceiptAuthority {
    authority_id: Arc<str>,
    sealed_reviews: Arc<Mutex<HashMap<String, String>>>,
}

impl SemanticAdmittedReceiptAuthority {
    fn new() -> Self {
        Self {
            authority_id: Arc::from(format!(
                "semantic-admitted-receipt-authority:{:032x}{:032x}",
                rand::random::<u128>(),
                rand::random::<u128>()
            )),
            sealed_reviews: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn seal(
        &self,
        completed_admission: CompletedAdmission,
        observation: SemanticObservationReceipt,
        reviewer_completion: Option<CompletedProviderRequest>,
        reviewer_raw_reply_hash: Option<String>,
    ) -> Result<AdmittedSemanticObservationReceipt, SemanticNudgeEligibilityError> {
        validate_admitted_semantic_receipt_material(
            &completed_admission,
            &observation,
            reviewer_completion.as_ref(),
            reviewer_raw_reply_hash.as_deref(),
        )?;
        let review_id = format!("semantic-review:{:032x}", rand::random::<u128>());
        let canonical_hash = admitted_semantic_receipt_hash(
            &completed_admission,
            &observation,
            reviewer_completion.as_ref(),
            reviewer_raw_reply_hash.as_deref(),
        );
        let mut sealed_reviews = lock_sealed_reviews(&self.sealed_reviews);
        if sealed_reviews
            .insert(review_id.clone(), canonical_hash)
            .is_some()
        {
            return Err(SemanticNudgeEligibilityError::InvalidAdmittedReceipt);
        }
        drop(sealed_reviews);
        Ok(AdmittedSemanticObservationReceipt {
            completed_admission,
            observation,
            reviewer_completion,
            reviewer_raw_reply_hash,
            authority_seal: SemanticAdmittedReceiptSeal {
                authority_id: self.authority_id.to_string(),
                review_id,
            },
        })
    }

    fn verify<'a>(
        &self,
        receipt: &'a AdmittedSemanticObservationReceipt,
    ) -> Result<&'a CompletedProviderRequest, SemanticNudgeEligibilityError> {
        if receipt.authority_seal.authority_id != self.authority_id.as_ref() {
            return Err(SemanticNudgeEligibilityError::InvalidAdmittedReceipt);
        }
        validate_admitted_semantic_receipt_material(
            &receipt.completed_admission,
            &receipt.observation,
            receipt.reviewer_completion.as_ref(),
            receipt.reviewer_raw_reply_hash.as_deref(),
        )?;
        let expected_hash = admitted_semantic_receipt_hash(
            &receipt.completed_admission,
            &receipt.observation,
            receipt.reviewer_completion.as_ref(),
            receipt.reviewer_raw_reply_hash.as_deref(),
        );
        if lock_sealed_reviews(&self.sealed_reviews).get(&receipt.authority_seal.review_id)
            != Some(&expected_hash)
        {
            return Err(SemanticNudgeEligibilityError::InvalidAdmittedReceipt);
        }
        receipt
            .reviewer_completion
            .as_ref()
            .ok_or(SemanticNudgeEligibilityError::ReviewProviderTerminalMissing)
    }
}

#[derive(Debug)]
pub struct AdmittedSemanticObservationReceipt {
    completed_admission: CompletedAdmission,
    observation: SemanticObservationReceipt,
    reviewer_completion: Option<CompletedProviderRequest>,
    reviewer_raw_reply_hash: Option<String>,
    authority_seal: SemanticAdmittedReceiptSeal,
}

impl AdmittedSemanticObservationReceipt {
    pub fn admission(&self) -> &AdmissionReceipt {
        &self.completed_admission.released().admission
    }

    pub fn observation(&self) -> &SemanticObservationReceipt {
        &self.observation
    }

    pub fn local_completion(&self) -> LocalCompletionKind {
        self.completed_admission.released().local_completion
    }

    pub fn reviewer_provider_request(&self) -> Option<&ProviderRequestKey> {
        self.reviewer_completion
            .as_ref()
            .map(|completion| &completion.request().key)
    }

    pub fn reviewer_provider_terminal(&self) -> Option<ProviderTerminalKind> {
        self.reviewer_completion
            .as_ref()
            .map(|completion| completion.terminal().kind)
    }

    pub fn has_intervention_authority(&self) -> bool {
        false
    }
}

fn admitted_semantic_receipt_hash(
    completed_admission: &CompletedAdmission,
    observation: &SemanticObservationReceipt,
    reviewer_completion: Option<&CompletedProviderRequest>,
    reviewer_raw_reply_hash: Option<&str>,
) -> String {
    let reviewer_completion = reviewer_completion.map(|completion| {
        (
            completion.admission(),
            completion.request(),
            completion.terminal(),
        )
    });
    canonical_sha256(&(
        "goose.semantic.admitted_receipt.v2",
        completed_admission.released(),
        observation,
        reviewer_completion,
        reviewer_raw_reply_hash,
    ))
}

fn validate_admitted_semantic_receipt_material(
    completed_admission: &CompletedAdmission,
    observation: &SemanticObservationReceipt,
    reviewer_completion: Option<&CompletedProviderRequest>,
    reviewer_raw_reply_hash: Option<&str>,
) -> Result<(), SemanticNudgeEligibilityError> {
    let released = completed_admission.released();
    let admission = &released.admission;
    if admission.role != WorkRole::SemanticJudgeObservation
        || admission.source.authority_scope != observation.authority_scope
        || admission.source.phase_epoch != observation.phase_epoch
        || admission.source.task_id != observation.task_id
        || admission.source.attempt != observation.attempt
        || admission.source.revision != observation.source_revision
        || !matches!(
            &admission.source.kind,
            SourceRevisionKind::Trace {
                trace_sequence,
                snapshot_hash,
            } if *trace_sequence == observation.source_revision
                && snapshot_hash == &observation.snapshot_hash
        )
    {
        return Err(SemanticNudgeEligibilityError::InvalidAdmittedReceipt);
    }
    let Some(completion) = reviewer_completion else {
        if !released.provider_not_started
            || !released.provider_terminals.is_empty()
            || reviewer_raw_reply_hash.is_some()
            || observation.reviewer_reply_hash.is_some()
            || released.local_completion != LocalCompletionKind::Error
        {
            return Err(SemanticNudgeEligibilityError::InvalidAdmittedReceipt);
        }
        return Ok(());
    };
    let request = completion.request();
    let terminal = completion.terminal();
    if completion.admission() != admission
        || request.admission_id != admission.admission_id
        || request.physical_host_id != admission.physical_host_id
        || request.model_instance_id != admission.model_instance_id
        || terminal.admission_id != request.admission_id
        || terminal.key != request.key
        || terminal.physical_host_id != request.physical_host_id
        || terminal.model_instance_id != request.model_instance_id
        || released.provider_not_started
        || released.provider_terminals.len() != 1
        || released.provider_terminals.first() != Some(terminal)
        || reviewer_raw_reply_hash != observation.reviewer_reply_hash.as_deref()
        || (released.local_completion == LocalCompletionKind::Success
            && terminal.kind != ProviderTerminalKind::Finished)
    {
        return Err(SemanticNudgeEligibilityError::InvalidAdmittedReceipt);
    }
    Ok(())
}

fn lock_sealed_reviews(
    sealed_reviews: &Arc<Mutex<HashMap<String, String>>>,
) -> MutexGuard<'_, HashMap<String, String>> {
    sealed_reviews
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One-shot authority to seal an observation capture against an exact live provider request.
///
/// It is non-cloneable, non-serializable, and has no public constructor. The brokered semantic
/// plane mints it only while borrowing an engine-owned [`StartedProviderRequest`].
#[derive(Debug)]
struct SemanticNudgeCapturePermit {
    authority_id: String,
    permit_id: String,
    source_provider_session: SemanticSourceProviderSessionBoundary,
}

#[derive(Clone)]
struct SemanticNudgeAuthority {
    inner: Arc<Mutex<SemanticNudgeLedger>>,
}

struct SemanticNudgeLedger {
    authority_id: String,
    registered_tasks: HashMap<String, String>,
    source_provider_sessions: HashMap<String, ExposedProviderRequestWitness>,
    unused_capture_permits: HashMap<String, CapturePermitRecord>,
    capture_by_snapshot: HashMap<String, SnapshotBindingRecord>,
    captures: HashMap<String, RegisteredNudgeCapture>,
    current_capture_by_task: HashMap<String, String>,
    latest_activity_by_task: HashMap<String, SemanticTraceRevision>,
    capabilities: HashMap<String, SemanticCapabilityRecord>,
}

// Semantic authority paths always take this ledger before pinning provider exposure state.
// Provider terminal/drop never enters this ledger, so the order is acyclic.

struct CapturePermitRecord {
    task_key: String,
    request_hash: String,
    provider_request_hash: String,
    source_session_hash: String,
}

struct SnapshotBindingRecord {
    task_key: String,
    trace_revision: u64,
}

struct RegisteredNudgeCapture {
    task_key: String,
    trace_revision: u64,
    snapshot_hash: String,
    provider_request_hash: String,
    review_consumed: bool,
}

struct SemanticCapabilityRecord {
    capture_id: String,
    state: SemanticCapabilityState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SemanticCapabilityState {
    Eligible,
    SpentDelivered,
    Invalidated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SemanticNudgeAuthorityError {
    TaskEvidenceConflict,
    TaskEvidenceNotRegistered,
    AuthorityMismatch,
    CapturePermitInvalid,
    SnapshotAlreadyBound,
    CaptureNotCurrent,
    SourceProviderNotLive,
    ReviewAlreadyConsumed,
    CapabilityUnknown,
    CapabilityAlreadySpent,
    DeliveryUnavailableAfterSpend,
    CancellationTerminalUnproven,
    InvalidEvidence(SemanticNudgeEligibilityError),
    InvalidCapture(String),
}

impl fmt::Display for SemanticNudgeAuthorityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TaskEvidenceConflict => {
                write!(
                    formatter,
                    "semantic task evidence conflicts with engine authority"
                )
            }
            Self::TaskEvidenceNotRegistered => {
                write!(formatter, "semantic task evidence is not registered")
            }
            Self::AuthorityMismatch => write!(formatter, "semantic authority id does not match"),
            Self::CapturePermitInvalid => {
                write!(
                    formatter,
                    "semantic capture permit is invalid or already consumed"
                )
            }
            Self::SnapshotAlreadyBound => write!(
                formatter,
                "semantic snapshot bytes are already bound to a provider session"
            ),
            Self::CaptureNotCurrent => {
                write!(formatter, "semantic capture is no longer the newest trace")
            }
            Self::SourceProviderNotLive => {
                write!(formatter, "semantic source provider session is not live")
            }
            Self::ReviewAlreadyConsumed => {
                write!(formatter, "semantic capture review was already consumed")
            }
            Self::CapabilityUnknown => write!(formatter, "semantic nudge capability is unknown"),
            Self::CapabilityAlreadySpent => {
                write!(formatter, "semantic nudge capability was already spent")
            }
            Self::DeliveryUnavailableAfterSpend => write!(
                formatter,
                "provider-pinned semantic nudge delivery is unavailable; capability was not spent"
            ),
            Self::CancellationTerminalUnproven => write!(
                formatter,
                "semantic nudge reserved cancellation but the exact source terminal was not proven"
            ),
            Self::InvalidEvidence(error) => error.fmt(formatter),
            Self::InvalidCapture(detail) => write!(formatter, "invalid semantic capture: {detail}"),
        }
    }
}

impl std::error::Error for SemanticNudgeAuthorityError {}

impl SemanticNudgeAuthority {
    fn new() -> Self {
        let authority_id = format!(
            "semantic-nudge-authority:{:032x}{:032x}",
            rand::random::<u128>(),
            rand::random::<u128>()
        );
        Self {
            inner: Arc::new(Mutex::new(SemanticNudgeLedger {
                authority_id,
                registered_tasks: HashMap::new(),
                source_provider_sessions: HashMap::new(),
                unused_capture_permits: HashMap::new(),
                capture_by_snapshot: HashMap::new(),
                captures: HashMap::new(),
                current_capture_by_task: HashMap::new(),
                latest_activity_by_task: HashMap::new(),
                capabilities: HashMap::new(),
            })),
        }
    }

    fn register_task_evidence(
        &self,
        engine_authority: EngineSemanticTaskAuthority,
    ) -> Result<SemanticTaskEvidenceCapability, SemanticNudgeAuthorityError> {
        let mut ledger = lock_nudge_ledger(&self.inner);
        let task_key = engine_authority.lineage_key().to_string();
        let engine_request_hash = engine_authority.request_hash().to_string();
        match ledger.registered_tasks.get(&task_key) {
            Some(registered) if registered != &engine_request_hash => {
                return Err(SemanticNudgeAuthorityError::TaskEvidenceConflict)
            }
            Some(_) => {}
            None => {
                ledger
                    .registered_tasks
                    .insert(task_key, engine_request_hash);
            }
        }
        SemanticTaskEvidenceCapability::mint(&ledger.authority_id, engine_authority)
            .map_err(SemanticNudgeAuthorityError::InvalidCapture)
    }

    fn mint_capture_permit_from_provider_start(
        &self,
        task_evidence: &SemanticTaskEvidenceCapability,
        request: &SemanticObservationCaptureRequest,
        provider_start: ProviderStartSession,
    ) -> Result<SemanticNudgeCapturePermit, SemanticNudgeAuthorityError> {
        let task_key = semantic_task_key(request);
        provider_start
            .ensure_live()
            .map_err(|_| SemanticNudgeAuthorityError::SourceProviderNotLive)?;
        let provider_session = {
            let mut ledger = lock_nudge_ledger(&self.inner);
            prune_closed_provider_authority(&mut ledger);
            if task_evidence.authority_id() != ledger.authority_id {
                return Err(SemanticNudgeAuthorityError::AuthorityMismatch);
            }
            if !task_evidence.matches(request)
                || ledger.registered_tasks.get(&task_key).map(String::as_str)
                    != Some(task_evidence.request_hash())
            {
                return Err(SemanticNudgeAuthorityError::TaskEvidenceNotRegistered);
            }
            let witness = provider_start
                .take_exposed_witness()
                .map_err(|error| SemanticNudgeAuthorityError::InvalidCapture(error.to_string()))?;
            let session = witness
                .bind_provider_start_session(&provider_start)
                .map_err(|error| SemanticNudgeAuthorityError::InvalidCapture(error.to_string()))?;
            let provider_request_hash = canonical_sha256(session.receipt());
            ledger
                .source_provider_sessions
                .insert(provider_request_hash, witness);
            session
        };
        let provider_request_hash = canonical_sha256(provider_session.receipt());
        let source_provider_session = SemanticSourceProviderSessionBoundary::from_provider_session(
            &request.activity_publisher,
            provider_session,
        )
        .map_err(SemanticNudgeAuthorityError::InvalidCapture)?;
        let permit_id = format!("semantic-capture-permit:{:032x}", rand::random::<u128>());
        let source_session_hash = source_provider_session.binding_hash().to_string();
        let mut ledger = lock_nudge_ledger(&self.inner);
        prune_closed_provider_authority(&mut ledger);
        if task_evidence.authority_id() != ledger.authority_id
            || ledger.registered_tasks.get(&task_key).map(String::as_str)
                != Some(task_evidence.request_hash())
        {
            return Err(SemanticNudgeAuthorityError::TaskEvidenceNotRegistered);
        }
        ledger
            .unused_capture_permits
            .retain(|_, record| record.task_key != task_key);
        ledger.unused_capture_permits.insert(
            permit_id.clone(),
            CapturePermitRecord {
                task_key,
                request_hash: task_evidence.request_hash().to_string(),
                provider_request_hash,
                source_session_hash: source_session_hash.clone(),
            },
        );
        Ok(SemanticNudgeCapturePermit {
            authority_id: ledger.authority_id.clone(),
            permit_id,
            source_provider_session,
        })
    }

    fn publish_activity(
        &self,
        task_evidence: &SemanticTaskEvidenceCapability,
        request: &SemanticObservationCaptureRequest,
        capture: &SemanticObservationCapture,
    ) -> Result<(), SemanticNudgeAuthorityError> {
        let revision = capture.revision();
        let task_key = semantic_task_key(request);
        let admitted_source = &request.activity_publisher.source;
        if revision.authority_scope != admitted_source.authority_scope
            || revision.phase_epoch != admitted_source.phase_epoch
            || revision.task_id != request.task_id
            || revision.attempt != request.attempt
        {
            return Err(SemanticNudgeAuthorityError::InvalidCapture(
                "published semantic activity does not match its scheduler-owned task boundary"
                    .to_string(),
            ));
        }
        let mut ledger = lock_nudge_ledger(&self.inner);
        prune_closed_provider_authority(&mut ledger);
        if task_evidence.authority_id() != ledger.authority_id
            || !task_evidence.matches(request)
            || ledger.registered_tasks.get(&task_key).map(String::as_str)
                != Some(task_evidence.request_hash())
        {
            return Err(SemanticNudgeAuthorityError::TaskEvidenceNotRegistered);
        }
        if let Some(current) = ledger.latest_activity_by_task.get(&task_key) {
            if revision.source_revision < current.source_revision {
                return Err(SemanticNudgeAuthorityError::CaptureNotCurrent);
            }
            if revision.source_revision == current.source_revision {
                return if revision == *current {
                    Ok(())
                } else {
                    Err(SemanticNudgeAuthorityError::InvalidCapture(format!(
                        "semantic activity revision {} changed immutable snapshot identity",
                        revision.source_revision
                    )))
                };
            }
        }
        prune_task_capture_authority(&mut ledger, &task_key, revision.source_revision);
        ledger.latest_activity_by_task.insert(task_key, revision);
        Ok(())
    }

    fn seal_capture(
        &self,
        capture: SemanticObservationCapture,
        request: &SemanticObservationCaptureRequest,
        task_evidence: &SemanticTaskEvidenceCapability,
        permit: SemanticNudgeCapturePermit,
    ) -> Result<BoundSemanticObservationCapture, SemanticNudgeAuthorityError> {
        let revision = capture.revision();
        let snapshot_key = canonical_sha256(&(
            "goose.semantic.snapshot_binding.v1",
            &revision.authority_scope,
            revision.phase_epoch,
            &revision.task_id,
            revision.attempt,
            revision.source_revision,
            &revision.snapshot_hash,
        ));
        let capture_id = format!("semantic-capture:{:032x}", rand::random::<u128>());
        let provider_session = permit.source_provider_session.provider_session();
        let mut ledger = lock_nudge_ledger(&self.inner);
        prune_closed_provider_authority(&mut ledger);
        let _source_pin = provider_session
            .try_pin()
            .map_err(|_| SemanticNudgeAuthorityError::SourceProviderNotLive)?;
        if permit.authority_id != ledger.authority_id
            || task_evidence.authority_id() != ledger.authority_id
        {
            return Err(SemanticNudgeAuthorityError::AuthorityMismatch);
        }
        let permit_record = ledger
            .unused_capture_permits
            .remove(&permit.permit_id)
            .ok_or(SemanticNudgeAuthorityError::CapturePermitInvalid)?;
        if permit_record.task_key != semantic_task_key(request)
            || permit_record.request_hash != task_evidence.request_hash()
            || permit_record.source_session_hash != permit.source_provider_session.binding_hash()
        {
            return Err(SemanticNudgeAuthorityError::CapturePermitInvalid);
        }
        if ledger.capture_by_snapshot.contains_key(&snapshot_key) {
            return Err(SemanticNudgeAuthorityError::SnapshotAlreadyBound);
        }
        if ledger.latest_activity_by_task.get(&permit_record.task_key) != Some(&revision) {
            return Err(SemanticNudgeAuthorityError::CaptureNotCurrent);
        }
        if let Some(current_id) = ledger.current_capture_by_task.get(&permit_record.task_key) {
            let current = ledger
                .captures
                .get(current_id)
                .expect("current semantic capture is registered");
            if current.trace_revision >= revision.source_revision {
                return Err(SemanticNudgeAuthorityError::CaptureNotCurrent);
            }
        }
        let bound = capture
            .bind_task_session(
                request,
                task_evidence,
                permit.source_provider_session,
                capture_id.clone(),
            )
            .map_err(SemanticNudgeAuthorityError::InvalidCapture)?;
        ledger.capture_by_snapshot.insert(
            snapshot_key,
            SnapshotBindingRecord {
                task_key: permit_record.task_key.clone(),
                trace_revision: revision.source_revision,
            },
        );
        ledger
            .current_capture_by_task
            .insert(permit_record.task_key.clone(), capture_id.clone());
        ledger.captures.insert(
            capture_id,
            RegisteredNudgeCapture {
                task_key: permit_record.task_key,
                trace_revision: revision.source_revision,
                snapshot_hash: revision.snapshot_hash,
                provider_request_hash: permit_record.provider_request_hash,
                review_consumed: false,
            },
        );
        Ok(bound)
    }

    fn issue(
        &self,
        bound_capture: BoundSemanticObservationCapture,
        receipt: AdmittedSemanticObservationReceipt,
        receipt_authority: &SemanticAdmittedReceiptAuthority,
    ) -> Result<Option<SemanticJudgeNudgeEligibility>, SemanticNudgeAuthorityError> {
        let authority_id = bound_capture.authority_id().to_string();
        let capture_id = bound_capture.capture_id().to_string();
        let trace_revision = bound_capture.nudge_boundary().trace_source_revision();
        let snapshot_hash = bound_capture.nudge_boundary().snapshot_hash().to_string();
        let eligibility =
            derive_semantic_judge_nudge_eligibility(bound_capture, receipt, receipt_authority)
                .map_err(SemanticNudgeAuthorityError::InvalidEvidence)?;
        let provider_session = eligibility.as_ref().map(|eligibility| {
            eligibility
                .boundary
                .source_provider_session()
                .provider_session()
        });
        let mut ledger = lock_nudge_ledger(&self.inner);
        prune_closed_provider_authority(&mut ledger);
        if authority_id != ledger.authority_id {
            return Err(SemanticNudgeAuthorityError::AuthorityMismatch);
        }
        let registered = ledger
            .captures
            .get(&capture_id)
            .ok_or(SemanticNudgeAuthorityError::CaptureNotCurrent)?;
        let task_key = registered.task_key.clone();
        if registered.review_consumed {
            return Err(SemanticNudgeAuthorityError::ReviewAlreadyConsumed);
        }
        if ledger.current_capture_by_task.get(&task_key) != Some(&capture_id) {
            return Err(SemanticNudgeAuthorityError::CaptureNotCurrent);
        }
        if !latest_activity_matches(
            ledger.latest_activity_by_task.get(&task_key),
            trace_revision,
            &snapshot_hash,
        ) {
            return Err(SemanticNudgeAuthorityError::CaptureNotCurrent);
        }
        let source_pin = provider_session
            .as_ref()
            .map(|session| session.try_pin())
            .transpose()
            .map_err(|_| SemanticNudgeAuthorityError::SourceProviderNotLive)?;
        ledger
            .captures
            .get_mut(&capture_id)
            .expect("registered semantic capture remains present")
            .review_consumed = true;
        if let Some(eligibility) = &eligibility {
            ledger.capabilities.insert(
                eligibility.evidence_receipt_hash.clone(),
                SemanticCapabilityRecord {
                    capture_id,
                    state: SemanticCapabilityState::Eligible,
                },
            );
        }
        drop(ledger);
        drop(source_pin);
        Ok(eligibility)
    }

    #[cfg(test)]
    fn redeem_record(
        &self,
        eligibility: &SemanticJudgeNudgeEligibility,
    ) -> Result<(), SemanticNudgeAuthorityError> {
        self.redeem_record_with_pin_hook(eligibility, || {})
    }

    #[cfg(test)]
    fn redeem_record_with_pin_hook(
        &self,
        eligibility: &SemanticJudgeNudgeEligibility,
        on_pinned_spend: impl FnOnce(),
    ) -> Result<(), SemanticNudgeAuthorityError> {
        self.redeem_record_inner(eligibility, None, on_pinned_spend)
            .map(drop)
    }

    fn redeem_record_at_capture(
        &self,
        eligibility: &SemanticJudgeNudgeEligibility,
        capture: ProviderNudgeSafetySnapshot,
    ) -> Result<ReservedSemanticNudge, SemanticNudgeAuthorityError> {
        let provider_session = eligibility
            .boundary
            .source_provider_session()
            .provider_session();
        let delivery = self
            .redeem_record_inner(eligibility, Some(capture), || {})?
            .expect("capture-bound semantic redemption returns its reserved delivery");
        Ok(ReservedSemanticNudge {
            provider_session,
            delivery,
        })
    }

    fn redeem_record_inner(
        &self,
        eligibility: &SemanticJudgeNudgeEligibility,
        capture: Option<ProviderNudgeSafetySnapshot>,
        on_pinned_spend: impl FnOnce(),
    ) -> Result<Option<Arc<dyn ProviderNudgeDelivery>>, SemanticNudgeAuthorityError> {
        let provider_session = eligibility
            .boundary
            .source_provider_session()
            .provider_session();
        let mut ledger = lock_nudge_ledger(&self.inner);
        if eligibility.boundary.authority_id() != ledger.authority_id {
            return Err(SemanticNudgeAuthorityError::AuthorityMismatch);
        }
        let capture_id = eligibility.boundary.capture_id();
        let task_key = semantic_boundary_task_key(&eligibility.boundary);
        if !latest_activity_matches(
            ledger.latest_activity_by_task.get(&task_key),
            eligibility.boundary.trace_source_revision(),
            eligibility.boundary.snapshot_hash(),
        ) {
            if let Some(record) = ledger
                .capabilities
                .get_mut(&eligibility.evidence_receipt_hash)
            {
                record.state = SemanticCapabilityState::Invalidated;
            }
            return Err(SemanticNudgeAuthorityError::CaptureNotCurrent);
        }
        let source_pin = match provider_session.try_pin() {
            Ok(pin) => pin,
            Err(_) => {
                if let Some(record) = ledger
                    .capabilities
                    .get_mut(&eligibility.evidence_receipt_hash)
                {
                    record.state = SemanticCapabilityState::Invalidated;
                }
                prune_closed_provider_authority(&mut ledger);
                return Err(SemanticNudgeAuthorityError::SourceProviderNotLive);
            }
        };
        drop(source_pin);
        let record = ledger
            .capabilities
            .get(&eligibility.evidence_receipt_hash)
            .ok_or(SemanticNudgeAuthorityError::CapabilityUnknown)?;
        if record.state != SemanticCapabilityState::Eligible {
            return Err(SemanticNudgeAuthorityError::CapabilityAlreadySpent);
        }
        let capture_is_current = record.capture_id == capture_id
            && ledger
                .current_capture_by_task
                .get(&task_key)
                .map(String::as_str)
                == Some(capture_id)
            && ledger.captures.get(capture_id).is_some_and(|capture| {
                capture.task_key == task_key
                    && capture.trace_revision == eligibility.boundary.trace_source_revision()
                    && capture.snapshot_hash == eligibility.boundary.snapshot_hash()
            });
        if !capture_is_current {
            ledger
                .capabilities
                .get_mut(&eligibility.evidence_receipt_hash)
                .expect("semantic capability remains registered")
                .state = SemanticCapabilityState::Invalidated;
            return Err(SemanticNudgeAuthorityError::CaptureNotCurrent);
        }
        let delivery = match capture {
            Some(capture) => Some(
                provider_session
                    .try_enqueue_nudge_at_capture(
                        eligibility.guidance.clone(),
                        capture,
                        on_pinned_spend,
                    )
                    .map_err(|_| SemanticNudgeAuthorityError::DeliveryUnavailableAfterSpend)?,
            ),
            None => {
                provider_session
                    .try_enqueue_nudge(eligibility.guidance.clone(), on_pinned_spend)
                    .map_err(|_| SemanticNudgeAuthorityError::DeliveryUnavailableAfterSpend)?;
                None
            }
        };
        ledger
            .capabilities
            .get_mut(&eligibility.evidence_receipt_hash)
            .expect("semantic capability remains registered")
            .state = SemanticCapabilityState::SpentDelivered;
        Ok(delivery)
    }
}

struct ReservedSemanticNudge {
    provider_session: Arc<LiveProviderRequestSession>,
    delivery: Arc<dyn ProviderNudgeDelivery>,
}

impl ReservedSemanticNudge {
    async fn confirm(self) -> Result<(), SemanticNudgeAuthorityError> {
        self.provider_session
            .confirm_reserved_nudge_terminal(self.delivery)
            .await
            .map(drop)
            .map_err(|_| SemanticNudgeAuthorityError::CancellationTerminalUnproven)
    }
}

fn latest_activity_matches(
    latest: Option<&SemanticTraceRevision>,
    trace_revision: u64,
    snapshot_hash: &str,
) -> bool {
    latest.is_some_and(|latest| {
        latest.source_revision == trace_revision && latest.snapshot_hash == snapshot_hash
    })
}

fn semantic_boundary_task_key(boundary: &SemanticNudgeBoundary) -> String {
    canonical_sha256(&(
        "goose.semantic.task_lineage.v1",
        &boundary.authority_scope().run_id,
        boundary.task_id(),
    ))
}

fn prune_task_capture_authority(
    ledger: &mut SemanticNudgeLedger,
    task_key: &str,
    newest_revision: u64,
) {
    let removed_capture_ids = ledger
        .captures
        .iter()
        .filter(|(_, capture)| capture.task_key == task_key)
        .map(|(capture_id, _)| capture_id.clone())
        .collect::<HashSet<_>>();
    ledger
        .captures
        .retain(|capture_id, _| !removed_capture_ids.contains(capture_id));
    ledger
        .current_capture_by_task
        .retain(|_, capture_id| !removed_capture_ids.contains(capture_id));
    ledger
        .capabilities
        .retain(|_, record| !removed_capture_ids.contains(&record.capture_id));
    ledger.capture_by_snapshot.retain(|_, binding| {
        binding.task_key != task_key || binding.trace_revision >= newest_revision
    });
}

fn prune_closed_provider_authority(ledger: &mut SemanticNudgeLedger) {
    let closed_session_hashes = ledger
        .source_provider_sessions
        .iter()
        .filter_map(|(session_hash, witness)| witness.try_pin().err().map(|_| session_hash.clone()))
        .collect::<HashSet<_>>();
    if closed_session_hashes.is_empty() {
        return;
    }
    ledger
        .source_provider_sessions
        .retain(|session_hash, _| !closed_session_hashes.contains(session_hash));
    ledger
        .unused_capture_permits
        .retain(|_, permit| !closed_session_hashes.contains(&permit.provider_request_hash));
    let removed_capture_ids = ledger
        .captures
        .iter()
        .filter(|(_, capture)| closed_session_hashes.contains(&capture.provider_request_hash))
        .map(|(capture_id, _)| capture_id.clone())
        .collect::<HashSet<_>>();
    ledger
        .captures
        .retain(|capture_id, _| !removed_capture_ids.contains(capture_id));
    ledger
        .current_capture_by_task
        .retain(|_, capture_id| !removed_capture_ids.contains(capture_id));
    ledger
        .capabilities
        .retain(|_, record| !removed_capture_ids.contains(&record.capture_id));
}

fn semantic_task_key(request: &SemanticObservationCaptureRequest) -> String {
    canonical_sha256(&(
        "goose.semantic.task_lineage.v1",
        &request.activity_publisher.source.authority_scope.run_id,
        &request.task_id,
    ))
}

fn lock_nudge_ledger(
    ledger: &Arc<Mutex<SemanticNudgeLedger>>,
) -> MutexGuard<'_, SemanticNudgeLedger> {
    ledger
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SemanticNudgeEligibilityError {
    InvalidAdmittedReceipt,
    ObservationBoundaryMismatch(String),
    SourceProviderBoundaryMismatch,
    ReviewWasNotSuccessful,
    ReviewProviderTerminalMissing,
    StaleObservation,
    InvalidNudgeObservation(String),
    MissingAcceptanceEvidence,
    MissingObservedStateEvidence,
}

impl fmt::Display for SemanticNudgeEligibilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAdmittedReceipt => write!(
                formatter,
                "semantic nudge review receipt is not an engine-sealed completion"
            ),
            Self::ObservationBoundaryMismatch(detail) => {
                write!(
                    formatter,
                    "semantic nudge observation boundary mismatch: {detail}"
                )
            }
            Self::SourceProviderBoundaryMismatch => write!(
                formatter,
                "semantic nudge source provider session does not match the sealed trace"
            ),
            Self::ReviewWasNotSuccessful => write!(
                formatter,
                "semantic nudge review did not complete successfully"
            ),
            Self::ReviewProviderTerminalMissing => write!(
                formatter,
                "semantic nudge review has no exact finished provider terminal"
            ),
            Self::StaleObservation => {
                write!(formatter, "semantic nudge observation is stale")
            }
            Self::InvalidNudgeObservation(detail) => {
                write!(formatter, "semantic nudge observation is invalid: {detail}")
            }
            Self::MissingAcceptanceEvidence => write!(
                formatter,
                "semantic nudge cites no criterion from the sealed acceptance slice"
            ),
            Self::MissingObservedStateEvidence => write!(
                formatter,
                "semantic nudge cites no sealed trace or owned-artifact state"
            ),
        }
    }
}

impl std::error::Error for SemanticNudgeEligibilityError {}

/// Evidence that one model-authored NUDGE is grounded in a real task/acceptance slice.
///
/// This value is deliberately non-cloneable and has no intervention method. It records the only
/// eligible delivery path and lets the eventual delivery hook compare an engine-current boundary;
/// atomic redemption remains outside this observation module.
#[derive(Debug)]
struct SemanticJudgeNudgeEligibility {
    boundary: SemanticNudgeBoundary,
    _task_slice: SemanticTaskAcceptanceSlice,
    guidance: String,
    _evidence: Vec<SemanticEvidenceCitation>,
    evidence_receipt_hash: String,
    _reviewer_provider_request_id: String,
}

/// Derive nudge eligibility from one exact admitted observation and its bound source evidence.
///
/// Non-NUDGE protocol actions remain observations and return `Ok(None)`. Neutral measurements may be
/// cited, but they cannot qualify the nudge: eligibility requires both a cited acceptance criterion
/// and cited trace or owned-artifact state.
fn derive_semantic_judge_nudge_eligibility(
    bound_capture: BoundSemanticObservationCapture,
    receipt: AdmittedSemanticObservationReceipt,
    receipt_authority: &SemanticAdmittedReceiptAuthority,
) -> std::result::Result<Option<SemanticJudgeNudgeEligibility>, SemanticNudgeEligibilityError> {
    let reviewer_completion = receipt_authority.verify(&receipt)?;
    let snapshot = bound_capture.snapshot();
    let expected_source = semantic_observation_task_version(snapshot);
    if receipt.admission().role != WorkRole::SemanticJudgeObservation
        || receipt.admission().source != expected_source
    {
        return Err(SemanticNudgeEligibilityError::ObservationBoundaryMismatch(
            "review admission does not match the exact trace source".to_string(),
        ));
    }
    if receipt.observation.authority_scope != *snapshot.authority_scope()
        || receipt.observation.phase_epoch != snapshot.phase_epoch()
        || receipt.observation.task_id != snapshot.task_id()
        || receipt.observation.attempt != snapshot.attempt()
        || receipt.observation.source_revision != snapshot.source_revision()
        || receipt.observation.snapshot_hash != snapshot.snapshot_hash()
    {
        return Err(SemanticNudgeEligibilityError::ObservationBoundaryMismatch(
            "observation receipt does not match the exact sealed snapshot".to_string(),
        ));
    }
    let boundary = bound_capture.nudge_boundary();
    if boundary.authority_scope() != snapshot.authority_scope()
        || boundary.phase_epoch() != snapshot.phase_epoch()
        || boundary.task_id() != snapshot.task_id()
        || boundary.attempt() != snapshot.attempt()
        || boundary.trace_source_revision() != snapshot.source_revision()
        || boundary.snapshot_hash() != snapshot.snapshot_hash()
    {
        return Err(SemanticNudgeEligibilityError::SourceProviderBoundaryMismatch);
    }
    if receipt.local_completion() != LocalCompletionKind::Success {
        return Err(SemanticNudgeEligibilityError::ReviewWasNotSuccessful);
    }
    let reviewer_provider_request = &reviewer_completion.request().key;
    let reviewer_provider_request_id = reviewer_provider_request.provider_request_id.as_str();
    if reviewer_provider_request_id.trim().is_empty()
        || reviewer_provider_request_id.trim() != reviewer_provider_request_id
        || reviewer_completion.terminal().kind != ProviderTerminalKind::Finished
    {
        return Err(SemanticNudgeEligibilityError::ReviewProviderTerminalMissing);
    }
    if receipt.observation.stale {
        return Err(SemanticNudgeEligibilityError::StaleObservation);
    }

    let reply = match &receipt.observation.decision {
        ParsedSemanticObservation::Parsed { reply }
            if matches!(reply.observation, SemanticObservationBody::Nudge { .. }) =>
        {
            reply
        }
        _ => return Ok(None),
    };
    if reply.protocol != SEMANTIC_OBSERVATION_PROTOCOL
        || reply.snapshot_hash != snapshot.snapshot_hash()
    {
        return Err(SemanticNudgeEligibilityError::InvalidNudgeObservation(
            "protocol or snapshot hash does not match".to_string(),
        ));
    }
    validate_reply(snapshot, reply).map_err(|error| {
        SemanticNudgeEligibilityError::InvalidNudgeObservation(error.to_string())
    })?;
    let (guidance, evidence) = match &reply.observation {
        SemanticObservationBody::Nudge {
            guidance, evidence, ..
        } => (guidance.clone(), evidence.clone()),
        _ => unreachable!("the typed action was checked above"),
    };

    let acceptance_sources: BTreeSet<String> = bound_capture
        .task_slice()
        .acceptance_oracle()
        .iter()
        .map(|criterion| format!("acceptance:{}", criterion.id))
        .collect();
    if !evidence
        .iter()
        .any(|citation| acceptance_sources.contains(&citation.source_id))
    {
        return Err(SemanticNudgeEligibilityError::MissingAcceptanceEvidence);
    }
    let mut observed_state_sources: BTreeSet<String> = snapshot
        .payload()
        .artifacts
        .iter()
        .map(|artifact| artifact.source_id.clone())
        .collect();
    observed_state_sources.insert(format!("trace:{}", snapshot.payload().trace.sequence));
    if !evidence
        .iter()
        .any(|citation| observed_state_sources.contains(&citation.source_id))
    {
        return Err(SemanticNudgeEligibilityError::MissingObservedStateEvidence);
    }

    let evidence_receipt_hash = semantic_nudge_evidence_hash(
        bound_capture.task_slice().binding_hash(),
        bound_capture.nudge_boundary(),
        receipt
            .observation
            .reviewer_reply_hash
            .as_deref()
            .ok_or_else(|| {
                SemanticNudgeEligibilityError::InvalidNudgeObservation(
                    "reviewer reply hash is missing".to_string(),
                )
            })?,
        reviewer_provider_request,
        &guidance,
        &evidence,
    );
    let (task_slice, boundary) = bound_capture.into_nudge_parts();
    Ok(Some(SemanticJudgeNudgeEligibility {
        boundary,
        _task_slice: task_slice,
        guidance,
        _evidence: evidence,
        evidence_receipt_hash,
        _reviewer_provider_request_id: reviewer_provider_request_id.to_string(),
    }))
}

fn semantic_nudge_evidence_hash(
    task_slice_hash: &str,
    boundary: &SemanticNudgeBoundary,
    reviewer_reply_hash: &str,
    reviewer_provider_request: &ProviderRequestKey,
    guidance: &str,
    evidence: &[SemanticEvidenceCitation],
) -> String {
    let value = serde_json::json!({
        "task_slice_hash": task_slice_hash,
        "run_id": boundary.authority_scope().run_id,
        "phase_lineage_id": boundary.authority_scope().phase_lineage_id,
        "phase_epoch": boundary.phase_epoch(),
        "task_id": boundary.task_id(),
        "attempt": boundary.attempt(),
        "task_source_revision": boundary.task_source_revision(),
        "trace_source_revision": boundary.trace_source_revision(),
        "snapshot_hash": boundary.snapshot_hash(),
        "source_provider_session_hash": boundary.source_provider_session().binding_hash(),
        "reviewer_reply_hash": reviewer_reply_hash,
        "reviewer_provider_request_ordinal": reviewer_provider_request.ordinal,
        "reviewer_provider_request_id": reviewer_provider_request.provider_request_id,
        "guidance": guidance,
        "evidence": evidence,
    });
    canonical_sha256(&value)
}

fn canonical_sha256(value: &impl Serialize) -> String {
    sha256_label(
        &serde_json::to_vec(value).expect("typed semantic authority material is JSON serializable"),
    )
}

fn raw_semantic_reply_hash(raw: &str) -> String {
    sha256_label(raw.as_bytes())
}

fn sha256_label(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

pub struct AdmittedSemanticObservationHandle {
    snapshot_hash: String,
    admission: AdmissionReceipt,
    completion: oneshot::Receiver<
        std::result::Result<AdmittedSemanticObservationReceipt, SemanticObservationAdmissionError>,
    >,
}

impl AdmittedSemanticObservationHandle {
    pub fn snapshot_hash(&self) -> &str {
        &self.snapshot_hash
    }

    pub fn admission(&self) -> &AdmissionReceipt {
        &self.admission
    }

    pub async fn wait(
        self,
    ) -> std::result::Result<AdmittedSemanticObservationReceipt, SemanticObservationAdmissionError>
    {
        self.completion.await.map_err(|_| {
            SemanticObservationAdmissionError::ObserverCompletionClosed {
                snapshot_hash: self.snapshot_hash,
            }
        })?
    }
}

#[derive(Clone, Debug)]
pub struct RejectedSemanticObservationAdmission {
    pub admission: Option<AdmissionReceipt>,
    pub rejection: SemanticObservationRejection,
}

pub enum SemanticObservationAdmissionSubmission {
    Started(AdmittedSemanticObservationHandle),
    Rejected(RejectedSemanticObservationAdmission),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticObservationAdmissionStage {
    PublishSource,
    RevalidateSource,
    PhysicalAdmission,
    ProviderNotStarted,
    LocalCompletion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticObservationAdmissionError {
    EventSinkMismatch,
    ObservationPlaneAlreadyBound,
    Broker {
        stage: SemanticObservationAdmissionStage,
        error: BrokerError,
    },
    ObserverCompletionClosed {
        snapshot_hash: String,
    },
    ProviderLifecycleUnresolved {
        admission_id: String,
        reason: String,
    },
}

impl fmt::Display for SemanticObservationAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventSinkMismatch => write!(
                formatter,
                "semantic observer and physical broker must share one event sink"
            ),
            Self::ObservationPlaneAlreadyBound => write!(
                formatter,
                "physical admission control already has a semantic observation plane"
            ),
            Self::Broker { stage, error } => {
                write!(formatter, "semantic observation {stage:?} failed: {error}")
            }
            Self::ObserverCompletionClosed { snapshot_hash } => write!(
                formatter,
                "semantic observer completion closed for snapshot `{snapshot_hash}`"
            ),
            Self::ProviderLifecycleUnresolved {
                admission_id,
                reason,
            } => write!(
                formatter,
                "semantic observation admission `{admission_id}` has unresolved provider lifecycle: {reason}"
            ),
        }
    }
}

impl std::error::Error for SemanticObservationAdmissionError {}

#[derive(Clone)]
pub struct BrokeredSemanticObservationPlane {
    control: PhysicalAdmissionControl,
    events: Arc<dyn EventSink>,
    observations: SemanticObservationPlane,
    task_lanes: Arc<AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
    nudge_authority: SemanticNudgeAuthority,
    admitted_receipt_authority: SemanticAdmittedReceiptAuthority,
}

impl BrokeredSemanticObservationPlane {
    pub fn new(
        control: PhysicalAdmissionControl,
        events: Arc<dyn EventSink>,
    ) -> std::result::Result<Self, SemanticObservationAdmissionError> {
        if !control.uses_sink(&events) {
            return Err(SemanticObservationAdmissionError::EventSinkMismatch);
        }
        if !control.claim_semantic_observation_plane() {
            return Err(SemanticObservationAdmissionError::ObservationPlaneAlreadyBound);
        }
        Ok(Self {
            control,
            events: events.clone(),
            observations: SemanticObservationPlane::new(events),
            task_lanes: Arc::new(AsyncMutex::new(HashMap::new())),
            nudge_authority: SemanticNudgeAuthority::new(),
            admitted_receipt_authority: SemanticAdmittedReceiptAuthority::new(),
        })
    }

    /// Register the scheduler-owned task/acceptance slice before a source provider request starts.
    ///
    /// The returned capability is opaque and scoped to this brokered plane. Registering conflicting
    /// evidence for the same engine task revision fails closed.
    pub(crate) fn register_scheduler_task_evidence(
        &self,
        engine_authority: EngineSemanticTaskAuthority,
        request: &SemanticObservationCaptureRequest,
    ) -> Result<SemanticTaskEvidenceCapability, String> {
        if !engine_authority.matches(request) {
            return Err(SemanticNudgeAuthorityError::TaskEvidenceNotRegistered.to_string());
        }
        self.nudge_authority
            .register_task_evidence(engine_authority)
            .map_err(|error| error.to_string())
    }

    /// Mint one non-cloneable capture permit while borrowing the exact engine-owned source request.
    #[cfg(test)]
    fn mint_semantic_nudge_capture_permit(
        &self,
        task_evidence: &SemanticTaskEvidenceCapability,
        request: &SemanticObservationCaptureRequest,
        started_provider_request: &StartedProviderRequest,
    ) -> Result<SemanticNudgeCapturePermit, SemanticNudgeAuthorityError> {
        let provider_start = started_provider_request
            .provider_start_session_for_test()
            .map_err(|_| SemanticNudgeAuthorityError::SourceProviderNotLive)?;
        self.nudge_authority
            .mint_capture_permit_from_provider_start(task_evidence, request, provider_start)
    }

    fn mint_semantic_nudge_capture_permit_from_provider_start(
        &self,
        task_evidence: &SemanticTaskEvidenceCapability,
        request: &SemanticObservationCaptureRequest,
        provider_start: ProviderStartSession,
    ) -> Result<SemanticNudgeCapturePermit, SemanticNudgeAuthorityError> {
        self.nudge_authority
            .mint_capture_permit_from_provider_start(task_evidence, request, provider_start)
    }

    /// Advance the engine-held activity head before any capture can be reviewed or sealed.
    pub(crate) fn publish_scheduler_activity(
        &self,
        task_evidence: &SemanticTaskEvidenceCapability,
        request: &SemanticObservationCaptureRequest,
        capture: &SemanticObservationCapture,
    ) -> Result<(), String> {
        self.nudge_authority
            .publish_activity(task_evidence, request, capture)
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn redeem_scheduler_nudge(
        &self,
        capture: SemanticObservationCapture,
        request: &SemanticObservationCaptureRequest,
        task_evidence: &SemanticTaskEvidenceCapability,
        provider_start: ProviderStartSession,
        receipt: AdmittedSemanticObservationReceipt,
    ) -> Result<bool, String> {
        let progress_at_capture = capture.provider_nudge_safety_snapshot();
        let permit = self
            .mint_semantic_nudge_capture_permit_from_provider_start(
                task_evidence,
                request,
                provider_start,
            )
            .map_err(|error| error.to_string())?;
        let bound = self
            .seal_semantic_nudge_capture(capture, request, task_evidence, permit)
            .map_err(|error| error.to_string())?;
        let Some(eligibility) = self
            .issue_semantic_nudge_eligibility(bound, receipt)
            .map_err(|error| error.to_string())?
        else {
            return Ok(false);
        };
        self.nudge_authority
            .redeem_record_at_capture(&eligibility, progress_at_capture)
            .map_err(|error| error.to_string())?
            .confirm()
            .await
            .map_err(|error| error.to_string())?;
        Ok(true)
    }

    /// Consume a one-shot provider-bound permit and seal observation bytes for eligibility review.
    fn seal_semantic_nudge_capture(
        &self,
        capture: SemanticObservationCapture,
        request: &SemanticObservationCaptureRequest,
        task_evidence: &SemanticTaskEvidenceCapability,
        permit: SemanticNudgeCapturePermit,
    ) -> Result<BoundSemanticObservationCapture, SemanticNudgeAuthorityError> {
        self.nudge_authority
            .seal_capture(capture, request, task_evidence, permit)
    }

    /// Issue at most one evidence-only nudge capability from an exact sealed capture and review.
    fn issue_semantic_nudge_eligibility(
        &self,
        bound_capture: BoundSemanticObservationCapture,
        receipt: AdmittedSemanticObservationReceipt,
    ) -> Result<Option<SemanticJudgeNudgeEligibility>, SemanticNudgeAuthorityError> {
        self.nudge_authority
            .issue(bound_capture, receipt, &self.admitted_receipt_authority)
    }

    /// Atomically consume a capability after checking current trace and source-session state.
    #[cfg(test)]
    fn redeem_existing_judge_nudge(
        &self,
        eligibility: &SemanticJudgeNudgeEligibility,
    ) -> Result<(), SemanticNudgeAuthorityError> {
        self.nudge_authority.redeem_record(eligibility)
    }

    pub async fn submit(
        &self,
        snapshot: SealedSemanticObservationSnapshot,
        policy: SemanticObservationAdmissionPolicy,
        reviewer: Arc<dyn AdmittedSemanticObservationReviewer>,
    ) -> std::result::Result<
        SemanticObservationAdmissionSubmission,
        SemanticObservationAdmissionError,
    > {
        self.submit_with_mode(snapshot, policy, reviewer, AdmissionMode::Queue)
            .await?
            .ok_or_else(
                || SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                    admission_id: "not-admitted".to_string(),
                    reason: "queued semantic observation unexpectedly produced no admission"
                        .to_string(),
                },
            )
    }

    /// Submit only when verified physical capacity is idle now. `None` means the opportunity was
    /// atomically withdrawn before admission; no provider-not-started receipt is fabricated because
    /// no provider lifecycle ever existed.
    pub async fn submit_if_idle(
        &self,
        snapshot: SealedSemanticObservationSnapshot,
        policy: SemanticObservationAdmissionPolicy,
        reviewer: Arc<dyn AdmittedSemanticObservationReviewer>,
    ) -> std::result::Result<
        Option<SemanticObservationAdmissionSubmission>,
        SemanticObservationAdmissionError,
    > {
        self.submit_with_mode(snapshot, policy, reviewer, AdmissionMode::ImmediateIdle)
            .await
    }

    async fn submit_with_mode(
        &self,
        snapshot: SealedSemanticObservationSnapshot,
        policy: SemanticObservationAdmissionPolicy,
        reviewer: Arc<dyn AdmittedSemanticObservationReviewer>,
        mode: AdmissionMode,
    ) -> std::result::Result<
        Option<SemanticObservationAdmissionSubmission>,
        SemanticObservationAdmissionError,
    > {
        let source = semantic_observation_task_version(&snapshot);
        self.control
            .set_source_revision(source.clone())
            .await
            .map_err(|error| SemanticObservationAdmissionError::Broker {
                stage: SemanticObservationAdmissionStage::PublishSource,
                error,
            })?;
        if let Err(rejection) = self.observations.register_current(&snapshot) {
            return Ok(Some(SemanticObservationAdmissionSubmission::Rejected(
                RejectedSemanticObservationAdmission {
                    admission: None,
                    rejection,
                },
            )));
        }
        self.events.emit(
            &crate::event::SwarmEvent::SemanticObservationSourcePublished {
                source: source.clone(),
            },
        );

        let task_lane = self.task_lane(&source.authority_key()).await;
        self.control
            .set_source_revision(source.clone())
            .await
            .map_err(|error| SemanticObservationAdmissionError::Broker {
                stage: SemanticObservationAdmissionStage::RevalidateSource,
                error,
            })?;
        if let Err(rejection) = self.observations.register_current(&snapshot) {
            return Ok(Some(SemanticObservationAdmissionSubmission::Rejected(
                RejectedSemanticObservationAdmission {
                    admission: None,
                    rejection,
                },
            )));
        }

        let work_id = semantic_observation_work_id(&snapshot);
        let opportunity = WorkOpportunity {
            work_id: work_id.clone(),
            role: WorkRole::SemanticJudgeObservation,
            priority: WorkRole::SemanticJudgeObservation.priority(),
            task_rank: policy.task_rank,
            source,
            eligible_logical_device_ids: policy.eligible_logical_device_ids,
            preferred_model_id: policy.preferred_model_id,
            excluded_logical_device_id: policy.excluded_logical_device_id,
        };
        let admitted = match mode {
            AdmissionMode::Queue => {
                Some(self.control.admit(opportunity).await.map_err(|error| {
                    SemanticObservationAdmissionError::Broker {
                        stage: SemanticObservationAdmissionStage::PhysicalAdmission,
                        error,
                    }
                })?)
            }
            AdmissionMode::ImmediateIdle => self
                .control
                .try_admit_idle(opportunity)
                .await
                .map_err(|error| SemanticObservationAdmissionError::Broker {
                    stage: SemanticObservationAdmissionStage::PhysicalAdmission,
                    error,
                })?,
        };
        let Some(admitted) = admitted else {
            self.events
                .emit(&crate::event::SwarmEvent::SemanticObservationDeferred {
                    task_id: snapshot.task_id().to_string(),
                    attempt: snapshot.attempt(),
                    source_revision: snapshot.source_revision(),
                    snapshot_hash: snapshot.snapshot_hash().to_string(),
                    reason: "no_verified_idle_provider_route".to_string(),
                });
            return Ok(None);
        };
        let admission = admitted.receipt().clone();
        let proof = Arc::new(Mutex::new(ProviderLifecycleProof::Pending));
        let lifecycle_reviewer = Arc::new(LifecycleBoundSemanticReviewer {
            inner: reviewer,
            admission: admission.clone(),
            lifecycle: admitted.lifecycle(),
            proof: proof.clone(),
        });

        let observation = match self.observations.submit(snapshot, lifecycle_reviewer) {
            SemanticObservationSubmission::Started(handle) => handle,
            SemanticObservationSubmission::Rejected(rejection) => {
                let admission_id = admission.admission_id.clone();
                let cleanup = tokio::spawn(close_rejected_admission(
                    admitted,
                    format!("semantic observation rejected before provider call: {rejection:?}"),
                ));
                cleanup.await.map_err(|error| {
                    SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                        admission_id,
                        reason: format!("pre-call rejection cleanup task failed: {error}"),
                    }
                })??;
                return Ok(Some(SemanticObservationAdmissionSubmission::Rejected(
                    RejectedSemanticObservationAdmission {
                        admission: Some(admission),
                        rejection,
                    },
                )));
            }
        };

        let (sender, completion) = oneshot::channel();
        let snapshot_hash = observation.snapshot_hash().to_string();
        let admission_for_handle = admission.clone();
        let admitted_receipt_authority = self.admitted_receipt_authority.clone();
        tokio::spawn(async move {
            let result = finalize_admitted_observation(
                admitted,
                observation,
                proof,
                admitted_receipt_authority,
            )
            .await;
            let _ = sender.send(result);
            drop(task_lane);
        });

        Ok(Some(SemanticObservationAdmissionSubmission::Started(
            AdmittedSemanticObservationHandle {
                snapshot_hash,
                admission: admission_for_handle,
                completion,
            },
        )))
    }

    async fn task_lane(&self, task_id: &str) -> OwnedMutexGuard<()> {
        let lane = {
            let mut lanes = self.task_lanes.lock().await;
            lanes
                .entry(task_id.to_string())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        lane.lock_owned().await
    }
}

#[derive(Clone, Copy)]
enum AdmissionMode {
    Queue,
    ImmediateIdle,
}

async fn close_rejected_admission(
    admitted: AdmittedWork,
    reason: String,
) -> std::result::Result<(), SemanticObservationAdmissionError> {
    admitted
        .lifecycle()
        .provider_not_started(reason)
        .await
        .map_err(|error| SemanticObservationAdmissionError::Broker {
            stage: SemanticObservationAdmissionStage::ProviderNotStarted,
            error,
        })?;
    admitted
        .complete_local(LocalCompletionKind::Error)
        .await
        .map_err(|error| SemanticObservationAdmissionError::Broker {
            stage: SemanticObservationAdmissionStage::LocalCompletion,
            error,
        })
}

pub fn semantic_observation_task_version(
    snapshot: &SealedSemanticObservationSnapshot,
) -> TaskVersion {
    TaskVersion {
        authority_scope: snapshot.authority_scope().clone(),
        phase_epoch: snapshot.phase_epoch(),
        task_id: snapshot.task_id().to_string(),
        attempt: snapshot.attempt(),
        revision: snapshot.source_revision(),
        kind: SourceRevisionKind::Trace {
            trace_sequence: snapshot.payload().trace.sequence,
            snapshot_hash: snapshot.snapshot_hash().to_string(),
        },
    }
}

fn semantic_observation_work_id(snapshot: &SealedSemanticObservationSnapshot) -> String {
    format!("semantic-observation:{}", snapshot.snapshot_hash())
}

#[derive(Debug)]
enum ProviderLifecycleProof {
    Pending,
    ProviderNotStarted,
    TerminalObserved {
        completion: Box<CompletedProviderRequest>,
        reviewer_raw_reply_hash: Option<String>,
    },
    Unresolved(String),
    Consumed,
}

struct LifecycleBoundSemanticReviewer {
    inner: Arc<dyn AdmittedSemanticObservationReviewer>,
    admission: AdmissionReceipt,
    lifecycle: ProviderLifecycle,
    proof: Arc<Mutex<ProviderLifecycleProof>>,
}

#[async_trait]
impl SemanticObservationReviewer for LifecycleBoundSemanticReviewer {
    async fn review(
        &self,
        observation: SemanticObservationRequest,
    ) -> std::result::Result<String, String> {
        let mut admitted_request = AdmittedSemanticObservationRequest {
            observation,
            admission: self.admission.clone(),
            provider_request_id: None,
        };
        if let Err(detail) = self.inner.verify_admission(&admitted_request) {
            let detail = format!("semantic provider preflight rejected admission: {detail}");
            match self.lifecycle.provider_not_started(detail.clone()).await {
                Ok(()) => self.set_proof(ProviderLifecycleProof::ProviderNotStarted),
                Err(close_error) => self.set_proof(ProviderLifecycleProof::Unresolved(format!(
                    "{detail}; provider-not-started was rejected: {close_error}"
                ))),
            }
            return Err(detail);
        }
        let started = match self.lifecycle.start_provider_request().await {
            Ok(started) => started,
            Err(error) => {
                let detail = format!("provider start rejected before semantic review: {error}");
                self.set_proof(ProviderLifecycleProof::Unresolved(detail.clone()));
                return Err(detail);
            }
        };
        admitted_request.provider_request_id =
            Some(started.receipt().key.provider_request_id.clone());

        let reviewer = self.inner.clone();
        let reviewed = match catch_future_unwind(
            started.scope_http(async move { reviewer.review(admitted_request).await }),
        )
        .await
        {
            Ok(reviewed) => reviewed,
            Err(()) => {
                let detail = "semantic provider task panicked without a reply".to_string();
                match finish_started_provider_request(started, ProviderTerminalKind::Failed).await {
                    Ok(completion) => {
                        self.set_proof(ProviderLifecycleProof::TerminalObserved {
                            completion: Box::new(completion),
                            reviewer_raw_reply_hash: None,
                        });
                    }
                    Err(close_error) => self.set_proof(ProviderLifecycleProof::Unresolved(
                        format!("{detail}; provider terminal was unresolved: {close_error}"),
                    )),
                }
                return Err(detail);
            }
        };
        let terminal_kind = match &reviewed {
            Ok(_) => ProviderTerminalKind::Finished,
            Err(AdmittedSemanticReviewError::TerminalFailure(_)) => ProviderTerminalKind::Failed,
            Err(AdmittedSemanticReviewError::LocalFailureAfterTerminal {
                provider_terminal,
                ..
            }) => *provider_terminal,
            Err(AdmittedSemanticReviewError::ProviderLifecycleUnresolved(detail)) => {
                self.set_proof(ProviderLifecycleProof::Unresolved(detail.clone()));
                return Err(detail.clone());
            }
        };
        let reviewer_raw_reply_hash = reviewed
            .as_ref()
            .ok()
            .map(|raw| raw_semantic_reply_hash(raw));
        match finish_started_provider_request(started, terminal_kind).await {
            Ok(completion) => {
                self.set_proof(ProviderLifecycleProof::TerminalObserved {
                    completion: Box::new(completion),
                    reviewer_raw_reply_hash,
                });
            }
            Err(error) => {
                let detail = format!("semantic provider terminal was rejected: {error}");
                self.set_proof(ProviderLifecycleProof::Unresolved(detail.clone()));
                return Err(detail);
            }
        }
        reviewed.map_err(|error| error.detail().to_string())
    }
}

async fn finish_started_provider_request(
    mut request: StartedProviderRequest,
    kind: ProviderTerminalKind,
) -> Result<CompletedProviderRequest, String> {
    loop {
        match request.provider_terminal_with_completion(kind).await {
            Ok(completion) => return Ok(completion),
            Err(ProviderLifecycleTransitionError::Retryable {
                error,
                request: retryable,
            }) => {
                request = *retryable;
                if matches!(
                    &error,
                    crate::control_plane::ProviderLifecycleOperationError::Lease(
                        crate::provider_lease::ProviderLeaseError::AuthorityContended
                    )
                ) {
                    tokio::task::yield_now().await;
                    continue;
                }
                return Err(error.to_string());
            }
            Err(ProviderLifecycleTransitionError::Fatal(error)) => return Err(error.to_string()),
        }
    }
}

async fn catch_future_unwind<F>(future: F) -> Result<F::Output, ()>
where
    F: Future,
{
    let mut future = Box::pin(future);
    std::future::poll_fn(|context| {
        match std::panic::catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(context))) {
            Ok(std::task::Poll::Ready(output)) => std::task::Poll::Ready(Ok(output)),
            Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
            Err(_) => std::task::Poll::Ready(Err(())),
        }
    })
    .await
}

impl LifecycleBoundSemanticReviewer {
    fn set_proof(&self, proof: ProviderLifecycleProof) {
        *lock_proof(&self.proof) = proof;
    }
}

async fn finalize_admitted_observation(
    admitted: AdmittedWork,
    observation: SemanticObservationHandle,
    proof: Arc<Mutex<ProviderLifecycleProof>>,
    admitted_receipt_authority: SemanticAdmittedReceiptAuthority,
) -> std::result::Result<AdmittedSemanticObservationReceipt, SemanticObservationAdmissionError> {
    let snapshot_hash = observation.snapshot_hash().to_string();
    let observation = match observation.wait().await {
        Ok(observation) => observation,
        Err(_) => {
            close_after_observer_loss(&admitted, &proof).await?;
            return Err(
                SemanticObservationAdmissionError::ObserverCompletionClosed { snapshot_hash },
            );
        }
    };
    let lifecycle_proof = take_lifecycle_proof(&proof);
    let (local_completion, reviewer_completion, reviewer_raw_reply_hash) = match lifecycle_proof {
        ProviderLifecycleProof::TerminalObserved {
            completion,
            reviewer_raw_reply_hash,
        } if completion.terminal().kind == ProviderTerminalKind::Finished => {
            let local_completion = if matches!(
                observation.decision.failure().map(|failure| &failure.kind),
                Some(SemanticProtocolFailureKind::ReviewerFailed)
            ) {
                LocalCompletionKind::Error
            } else {
                LocalCompletionKind::Success
            };
            (local_completion, Some(*completion), reviewer_raw_reply_hash)
        }
        ProviderLifecycleProof::TerminalObserved {
            completion,
            reviewer_raw_reply_hash,
        } => (
            LocalCompletionKind::Error,
            Some(*completion),
            reviewer_raw_reply_hash,
        ),
        ProviderLifecycleProof::ProviderNotStarted => (LocalCompletionKind::Error, None, None),
        ProviderLifecycleProof::Pending => {
            admitted
                .lifecycle()
                .provider_not_started("semantic observer returned without a provider start")
                .await
                .map_err(|error| SemanticObservationAdmissionError::Broker {
                    stage: SemanticObservationAdmissionStage::ProviderNotStarted,
                    error,
                })?;
            (LocalCompletionKind::Error, None, None)
        }
        ProviderLifecycleProof::Unresolved(reason) => {
            return Err(
                SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                    admission_id: admitted.receipt().admission_id.clone(),
                    reason,
                },
            )
        }
        ProviderLifecycleProof::Consumed => {
            return Err(
                SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                    admission_id: admitted.receipt().admission_id.clone(),
                    reason: "semantic reviewer lifecycle proof was already consumed".to_string(),
                },
            )
        }
    };
    let admission_id = admitted.receipt().admission_id.clone();
    let completed_admission = admitted
        .complete_local_with_completion(local_completion)
        .await
        .map_err(|error| SemanticObservationAdmissionError::Broker {
            stage: SemanticObservationAdmissionStage::LocalCompletion,
            error,
        })?;
    admitted_receipt_authority
        .seal(
            completed_admission,
            observation,
            reviewer_completion,
            reviewer_raw_reply_hash,
        )
        .map_err(
            |error| SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                admission_id,
                reason: error.to_string(),
            },
        )
}

async fn close_after_observer_loss(
    admitted: &AdmittedWork,
    proof: &Arc<Mutex<ProviderLifecycleProof>>,
) -> std::result::Result<(), SemanticObservationAdmissionError> {
    let lifecycle_proof = take_lifecycle_proof(proof);
    match lifecycle_proof {
        ProviderLifecycleProof::Pending => {
            admitted
                .lifecycle()
                .provider_not_started("semantic observer ended before the provider call")
                .await
                .map_err(|error| SemanticObservationAdmissionError::Broker {
                    stage: SemanticObservationAdmissionStage::ProviderNotStarted,
                    error,
                })?;
        }
        ProviderLifecycleProof::Unresolved(reason) => {
            return Err(
                SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                    admission_id: admitted.receipt().admission_id.clone(),
                    reason,
                },
            )
        }
        ProviderLifecycleProof::ProviderNotStarted
        | ProviderLifecycleProof::TerminalObserved { .. } => {}
        ProviderLifecycleProof::Consumed => {
            return Err(
                SemanticObservationAdmissionError::ProviderLifecycleUnresolved {
                    admission_id: admitted.receipt().admission_id.clone(),
                    reason: "semantic reviewer lifecycle proof was already consumed".to_string(),
                },
            )
        }
    }
    admitted
        .complete_local(LocalCompletionKind::Error)
        .await
        .map_err(|error| SemanticObservationAdmissionError::Broker {
            stage: SemanticObservationAdmissionStage::LocalCompletion,
            error,
        })
}

fn lock_proof(
    proof: &Arc<Mutex<ProviderLifecycleProof>>,
) -> MutexGuard<'_, ProviderLifecycleProof> {
    proof
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn take_lifecycle_proof(proof: &Arc<Mutex<ProviderLifecycleProof>>) -> ProviderLifecycleProof {
    let mut proof = lock_proof(proof);
    std::mem::replace(&mut *proof, ProviderLifecycleProof::Consumed)
}

#[cfg(test)]
mod authority_replay_tests;
