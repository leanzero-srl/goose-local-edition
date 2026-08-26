//! Typed production boundary between measured worker state and semantic observation.
//!
//! A summons is evidence that a trace changed, not a verdict about that change. This module has no
//! action-delivery API and cannot mutate scheduler state.

use crate::control_plane::{LiveProviderRequestSession, ProviderNudgeSafetySnapshot};
use crate::semantic_observation::{
    AcceptanceCriterionSnapshot, NeutralJudgeSignal, SealedSemanticObservationSnapshot,
};
use crate::{AdmissionReceipt, ProviderRequestReceipt, TaskVersion};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SemanticActivityPublisher {
    pub(crate) publisher_id: String,
    pub(crate) task_id: String,
    pub(crate) attempt: u32,
    pub(crate) admission_id: String,
    pub(crate) work_role: String,
    pub(crate) source_id: String,
    pub(crate) source: TaskVersion,
    pub(crate) fleet_snapshot_id: String,
    pub(crate) logical_device_id: String,
    pub(crate) model_id: String,
    pub(crate) physical_host_id: String,
    pub(crate) model_instance_id: String,
    pub(crate) provider_transport_id: String,
    pub(crate) capacity_evidence_id: String,
}

impl SemanticActivityPublisher {
    pub fn from_admission(admission: &AdmissionReceipt) -> Self {
        let work_role = serde_json::to_value(admission.role)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string());
        let source_id = canonical_digest(&admission.source);
        let capacity_evidence_id = canonical_digest(&admission.capacity_evidence);
        let mut publisher = Self {
            publisher_id: String::new(),
            task_id: admission.source.task_id.clone(),
            attempt: admission.source.attempt,
            admission_id: admission.admission_id.clone(),
            work_role,
            source_id,
            source: admission.source.clone(),
            fleet_snapshot_id: admission.fleet_snapshot_id.clone(),
            logical_device_id: admission.logical_device_id.clone(),
            model_id: admission.model_id.clone(),
            physical_host_id: admission.physical_host_id.clone(),
            model_instance_id: admission.model_instance_id.clone(),
            provider_transport_id: admission.provider_transport_id.clone(),
            capacity_evidence_id,
        };
        publisher.publisher_id = canonical_digest(&publisher.identity_fields());
        publisher
    }

    pub fn validate(&self) -> Result<(), String> {
        self.source.validate()?;
        let required = [
            ("publisher id", self.publisher_id.as_str()),
            ("task id", self.task_id.as_str()),
            ("admission id", self.admission_id.as_str()),
            ("work role", self.work_role.as_str()),
            ("source id", self.source_id.as_str()),
            ("fleet snapshot id", self.fleet_snapshot_id.as_str()),
            ("logical device id", self.logical_device_id.as_str()),
            ("model id", self.model_id.as_str()),
            ("physical host id", self.physical_host_id.as_str()),
            ("model instance id", self.model_instance_id.as_str()),
            ("provider transport id", self.provider_transport_id.as_str()),
            ("capacity evidence id", self.capacity_evidence_id.as_str()),
        ];
        if let Some((name, _)) = required
            .into_iter()
            .find(|(_, value)| value.trim().is_empty())
        {
            return Err(format!("semantic activity publisher {name} is empty"));
        }
        let expected = canonical_digest(&self.identity_fields());
        if self.task_id != self.source.task_id || self.attempt != self.source.attempt {
            return Err(
                "semantic activity publisher task/attempt does not match its source authority"
                    .into(),
            );
        }
        if self.source_id != canonical_digest(&self.source) {
            return Err(
                "semantic activity publisher source id does not match its source authority".into(),
            );
        }
        if self.publisher_id != expected {
            return Err("semantic activity publisher id does not match its sealed identity".into());
        }
        Ok(())
    }

    pub fn publisher_id(&self) -> &str {
        &self.publisher_id
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn admission_id(&self) -> &str {
        &self.admission_id
    }

    pub fn work_role(&self) -> &str {
        &self.work_role
    }

    pub fn source(&self) -> &TaskVersion {
        &self.source
    }

    pub fn logical_device_id(&self) -> &str {
        &self.logical_device_id
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn physical_host_id(&self) -> &str {
        &self.physical_host_id
    }

    pub fn model_instance_id(&self) -> &str {
        &self.model_instance_id
    }

    pub fn provider_transport_id(&self) -> &str {
        &self.provider_transport_id
    }

    fn identity_fields(&self) -> SemanticActivityPublisherIdentity<'_> {
        SemanticActivityPublisherIdentity {
            task_id: &self.task_id,
            attempt: self.attempt,
            admission_id: &self.admission_id,
            work_role: &self.work_role,
            source_id: &self.source_id,
            source: &self.source,
            fleet_snapshot_id: &self.fleet_snapshot_id,
            logical_device_id: &self.logical_device_id,
            model_id: &self.model_id,
            physical_host_id: &self.physical_host_id,
            model_instance_id: &self.model_instance_id,
            provider_transport_id: &self.provider_transport_id,
            capacity_evidence_id: &self.capacity_evidence_id,
        }
    }
}

#[derive(Serialize)]
struct SemanticActivityPublisherIdentity<'a> {
    task_id: &'a str,
    attempt: u32,
    admission_id: &'a str,
    work_role: &'a str,
    source_id: &'a str,
    source: &'a TaskVersion,
    fleet_snapshot_id: &'a str,
    logical_device_id: &'a str,
    model_id: &'a str,
    physical_host_id: &'a str,
    model_instance_id: &'a str,
    provider_transport_id: &'a str,
    capacity_evidence_id: &'a str,
}

fn canonical_digest(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("engine authority is JSON serializable");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

/// Exact provider request that owns the observed worker's current source session.
///
/// This boundary can only be minted from a broker-admitted build or repair task plus the provider
/// start receipt for that same admission and physical instance. Fleet idleness is intentionally not
/// an input: spare capacity may run an observer, but it cannot identify or authorize a target.
#[derive(Debug)]
pub struct SemanticSourceProviderSessionBoundary {
    publisher: SemanticActivityPublisher,
    provider_request: ProviderRequestReceipt,
    provider_session: Arc<LiveProviderRequestSession>,
    binding_hash: String,
}

impl SemanticSourceProviderSessionBoundary {
    pub(crate) fn from_provider_session(
        publisher: &SemanticActivityPublisher,
        provider_session: LiveProviderRequestSession,
    ) -> Result<Self, String> {
        let provider_request = provider_session.receipt();
        publisher.validate()?;
        if provider_request.admission_id != publisher.admission_id
            || provider_request.physical_host_id != publisher.physical_host_id
            || provider_request.model_instance_id != publisher.model_instance_id
        {
            return Err(
                "provider session does not match its semantic activity publisher".to_string(),
            );
        }
        let publisher = publisher.clone();
        let provider_request = provider_request.clone();
        let binding_hash = canonical_digest(&(&publisher, &provider_request));
        Ok(Self {
            publisher,
            provider_request,
            provider_session: Arc::new(provider_session),
            binding_hash,
        })
    }

    pub fn authority_scope(&self) -> &crate::AuthorityScope {
        &self.publisher.source.authority_scope
    }

    pub fn phase_epoch(&self) -> u64 {
        self.publisher.source.phase_epoch
    }

    pub fn task_id(&self) -> &str {
        &self.publisher.source.task_id
    }

    pub fn attempt(&self) -> u32 {
        self.publisher.source.attempt
    }

    pub fn task_source_revision(&self) -> u64 {
        self.publisher.source.revision
    }

    pub fn publisher_id(&self) -> &str {
        &self.publisher.publisher_id
    }

    pub fn admission_id(&self) -> &str {
        &self.provider_request.admission_id
    }

    pub fn provider_request_id(&self) -> &str {
        &self.provider_request.key.provider_request_id
    }

    pub fn provider_request_ordinal(&self) -> u32 {
        self.provider_request.key.ordinal
    }

    pub fn logical_device_id(&self) -> &str {
        &self.publisher.logical_device_id
    }

    pub fn model_id(&self) -> &str {
        &self.publisher.model_id
    }

    pub fn physical_host_id(&self) -> &str {
        &self.publisher.physical_host_id
    }

    pub fn model_instance_id(&self) -> &str {
        &self.publisher.model_instance_id
    }

    pub fn provider_transport_id(&self) -> &str {
        &self.publisher.provider_transport_id
    }

    pub fn binding_hash(&self) -> &str {
        &self.binding_hash
    }

    pub(crate) fn provider_session(&self) -> Arc<LiveProviderRequestSession> {
        Arc::clone(&self.provider_session)
    }

    fn matches_publisher(&self, publisher: &SemanticActivityPublisher) -> bool {
        &self.publisher == publisher
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SemanticObservationCaptureRequest {
    pub(crate) task_id: String,
    pub(crate) attempt: u32,
    pub(crate) task_rank: u64,
    pub(crate) goal: String,
    pub(crate) task_contract: String,
    pub(crate) owned_files: Vec<String>,
    pub(crate) contract_version: String,
    pub(crate) acceptance_oracle: Vec<AcceptanceCriterionSnapshot>,
    pub(crate) dependency_contract_versions: BTreeMap<String, String>,
    pub(crate) sibling_contract_versions: BTreeMap<String, String>,
    pub(crate) allowed_finding_routes: Vec<String>,
    pub(crate) running_logical_device_id: String,
    pub(crate) running_model_id: String,
    pub(crate) activity_publisher: SemanticActivityPublisher,
}

impl SemanticObservationCaptureRequest {
    /// Build portable observation input. This value has no control authority; the swarm scheduler
    /// must independently mint an opaque [`EngineSemanticTaskAuthority`] before the brokered plane
    /// will accept it for nudge-capable evidence.
    #[allow(clippy::too_many_arguments)]
    pub fn observation_only(
        task_id: String,
        attempt: u32,
        task_rank: u64,
        goal: String,
        task_contract: String,
        owned_files: Vec<String>,
        contract_version: String,
        acceptance_oracle: Vec<AcceptanceCriterionSnapshot>,
        dependency_contract_versions: BTreeMap<String, String>,
        sibling_contract_versions: BTreeMap<String, String>,
        allowed_finding_routes: Vec<String>,
        running_logical_device_id: String,
        running_model_id: String,
        activity_publisher: SemanticActivityPublisher,
    ) -> Result<Self, String> {
        let request = Self {
            task_id,
            attempt,
            task_rank,
            goal,
            task_contract,
            owned_files,
            contract_version,
            acceptance_oracle,
            dependency_contract_versions,
            sibling_contract_versions,
            allowed_finding_routes,
            running_logical_device_id,
            running_model_id,
            activity_publisher,
        };
        request.validate_boundary()?;
        Ok(request)
    }

    // Every argument is a separately validated scheduler authority field; a loose aggregate would
    // make it easier to mix fields from different task attempts at this sealing boundary.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_scheduler_state(
        task_id: String,
        attempt: u32,
        task_rank: u64,
        goal: String,
        task_contract: String,
        owned_files: Vec<String>,
        contract_version: String,
        acceptance_oracle: Vec<AcceptanceCriterionSnapshot>,
        dependency_contract_versions: BTreeMap<String, String>,
        sibling_contract_versions: BTreeMap<String, String>,
        allowed_finding_routes: Vec<String>,
        running_logical_device_id: String,
        running_model_id: String,
        activity_publisher: SemanticActivityPublisher,
    ) -> Result<Self, String> {
        Self::observation_only(
            task_id,
            attempt,
            task_rank,
            goal,
            task_contract,
            owned_files,
            contract_version,
            acceptance_oracle,
            dependency_contract_versions,
            sibling_contract_versions,
            allowed_finding_routes,
            running_logical_device_id,
            running_model_id,
            activity_publisher,
        )
    }

    fn validate_boundary(&self) -> Result<(), String> {
        self.activity_publisher.validate()?;
        if self.task_id != self.activity_publisher.task_id
            || self.attempt != self.activity_publisher.attempt
            || self.running_logical_device_id != self.activity_publisher.logical_device_id
            || self.running_model_id != self.activity_publisher.model_id
        {
            return Err(
                "semantic capture request does not match its admitted activity publisher"
                    .to_string(),
            );
        }
        Ok(())
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn task_rank(&self) -> u64 {
        self.task_rank
    }

    pub fn goal(&self) -> &str {
        &self.goal
    }

    pub fn task_contract(&self) -> &str {
        &self.task_contract
    }

    pub fn owned_files(&self) -> &[String] {
        &self.owned_files
    }

    pub fn contract_version(&self) -> &str {
        &self.contract_version
    }

    pub fn acceptance_oracle(&self) -> &[AcceptanceCriterionSnapshot] {
        &self.acceptance_oracle
    }

    pub fn dependency_contract_versions(&self) -> &BTreeMap<String, String> {
        &self.dependency_contract_versions
    }

    pub fn sibling_contract_versions(&self) -> &BTreeMap<String, String> {
        &self.sibling_contract_versions
    }

    pub fn allowed_finding_routes(&self) -> &[String] {
        &self.allowed_finding_routes
    }

    pub fn running_logical_device_id(&self) -> &str {
        &self.running_logical_device_id
    }

    pub fn running_model_id(&self) -> &str {
        &self.running_model_id
    }

    pub fn activity_publisher(&self) -> &SemanticActivityPublisher {
        &self.activity_publisher
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceStateMeasurement {
    pub measurement_hash: String,
    pub tool_calls: u64,
    pub failed_tool_calls: u64,
    pub malformed_tool_calls: u64,
    pub pending_tool_calls: u64,
    pub thinking_chars: u64,
    pub recurrence_window_chars: u32,
    pub recurrence_observed_windows: u64,
    pub recurrence_repeated_windows: u64,
    pub recurrence_repeat_share: f64,
    #[serde(default)]
    pub provider_stream_revision: u64,
    #[serde(default)]
    pub provider_stream_chunks: u64,
    #[serde(default)]
    pub provider_stream_bytes: u64,
    #[serde(default)]
    pub provider_structured_output_chunks: u64,
    #[serde(default)]
    pub provider_structured_output_bytes: u64,
    #[serde(default)]
    pub provider_last_progress_elapsed_ms: u64,
    #[serde(default)]
    pub provider_structured_output_active: bool,
    pub artifact_version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticObservationSummonsSignal {
    TraceStateAdvanced {
        source_id: String,
        measurement: TraceStateMeasurement,
        provenance: String,
    },
}

impl SemanticObservationSummonsSignal {
    pub fn source_id(&self) -> &str {
        match self {
            Self::TraceStateAdvanced { source_id, .. } => source_id,
        }
    }

    pub fn neutral_signal(&self) -> NeutralJudgeSignal {
        match self {
            Self::TraceStateAdvanced {
                source_id,
                measurement,
                provenance,
            } => NeutralJudgeSignal {
                source_id: source_id.clone(),
                kind: "trace_state_advanced".to_string(),
                value: serde_json::to_value(measurement)
                    .expect("typed trace measurement is JSON serializable"),
                provenance: provenance.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SemanticTraceRevision {
    pub authority_scope: crate::AuthorityScope,
    pub phase_epoch: u64,
    pub task_id: String,
    pub attempt: u32,
    pub source_revision: u64,
    pub snapshot_hash: String,
}

#[derive(Debug)]
pub struct SemanticObservationCapture {
    snapshot: SealedSemanticObservationSnapshot,
    summons: SemanticObservationSummonsSignal,
}

/// Engine-minted task/acceptance authority used only while sealing a nudge-capable capture.
///
/// The ordinary capture request remains a portable observation input. Its contents cannot become
/// control evidence unless they match this opaque capability issued by the engine-held authority.
#[derive(Debug)]
pub(crate) struct SemanticTaskEvidenceCapability {
    authority_id: String,
    request_hash: String,
}

/// Scheduler-owned authority for one exact task contract and admitted execution route.
///
/// This is deliberately separate from [`SemanticObservationCaptureRequest`]. The latter remains a
/// portable observation input and is caller-constructible; it cannot establish control provenance.
/// Only engine code may mint this opaque value at the point where the scheduler still owns the DAG
/// task, acceptance oracle, and admitted activity publisher.
#[derive(Debug)]
pub(crate) struct EngineSemanticTaskAuthority {
    request_hash: String,
    lineage_key: String,
}

impl EngineSemanticTaskAuthority {
    pub(crate) fn mint_from_scheduler_state(
        request: &SemanticObservationCaptureRequest,
    ) -> Result<Self, String> {
        request.activity_publisher.validate()?;
        if request.task_id != request.activity_publisher.task_id
            || request.attempt != request.activity_publisher.attempt
            || request.running_logical_device_id != request.activity_publisher.logical_device_id
            || request.running_model_id != request.activity_publisher.model_id
        {
            return Err(
                "semantic scheduler task authority does not match its admitted activity publisher"
                    .to_string(),
            );
        }
        Ok(Self {
            request_hash: canonical_digest(&SemanticTaskEvidenceIdentity::new(request)),
            lineage_key: canonical_digest(&(
                "goose.semantic.task_lineage.v1",
                &request.activity_publisher.source.authority_scope.run_id,
                &request.task_id,
            )),
        })
    }

    pub(crate) fn request_hash(&self) -> &str {
        &self.request_hash
    }

    pub(crate) fn matches(&self, request: &SemanticObservationCaptureRequest) -> bool {
        self.request_hash == canonical_digest(&SemanticTaskEvidenceIdentity::new(request))
    }

    pub(crate) fn lineage_key(&self) -> &str {
        &self.lineage_key
    }
}

impl SemanticTaskEvidenceCapability {
    pub(crate) fn mint(
        authority_id: &str,
        engine_authority: EngineSemanticTaskAuthority,
    ) -> Result<Self, String> {
        if authority_id.trim().is_empty() || authority_id.trim() != authority_id {
            return Err("semantic task evidence authority id is invalid".to_string());
        }
        Ok(Self {
            authority_id: authority_id.to_string(),
            request_hash: engine_authority.request_hash,
        })
    }

    pub(crate) fn matches(&self, request: &SemanticObservationCaptureRequest) -> bool {
        self.request_hash == canonical_digest(&SemanticTaskEvidenceIdentity::new(request))
    }

    pub(crate) fn authority_id(&self) -> &str {
        &self.authority_id
    }

    pub(crate) fn request_hash(&self) -> &str {
        &self.request_hash
    }
}

#[derive(Serialize)]
struct SemanticTaskEvidenceIdentity<'a> {
    domain: &'static str,
    task_id: &'a str,
    attempt: u32,
    task_rank: u64,
    goal: &'a str,
    task_contract: &'a str,
    owned_files: &'a [String],
    contract_version: &'a str,
    acceptance_oracle: &'a [AcceptanceCriterionSnapshot],
    dependency_contract_versions: &'a BTreeMap<String, String>,
    sibling_contract_versions: &'a BTreeMap<String, String>,
    allowed_finding_routes: &'a [String],
    running_logical_device_id: &'a str,
    running_model_id: &'a str,
    activity_publisher: &'a SemanticActivityPublisher,
}

impl<'a> SemanticTaskEvidenceIdentity<'a> {
    fn new(request: &'a SemanticObservationCaptureRequest) -> Self {
        Self {
            domain: "goose.semantic.task_evidence.v1",
            task_id: &request.task_id,
            attempt: request.attempt,
            task_rank: request.task_rank,
            goal: &request.goal,
            task_contract: &request.task_contract,
            owned_files: &request.owned_files,
            contract_version: &request.contract_version,
            acceptance_oracle: &request.acceptance_oracle,
            dependency_contract_versions: &request.dependency_contract_versions,
            sibling_contract_versions: &request.sibling_contract_versions,
            allowed_finding_routes: &request.allowed_finding_routes,
            running_logical_device_id: &request.running_logical_device_id,
            running_model_id: &request.running_model_id,
            activity_publisher: &request.activity_publisher,
        }
    }
}

/// Canonical task and acceptance evidence that the snapshot producer was asked to observe.
///
/// Fields are private so this slice cannot be assembled from an idle-node count or an unbound model
/// reply. It is minted only after the capture is checked against the engine request and source
/// provider session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticTaskAcceptanceSlice {
    task_rank: u64,
    goal: String,
    task_contract: String,
    owned_files: Vec<String>,
    contract_version: String,
    acceptance_oracle: Vec<AcceptanceCriterionSnapshot>,
    dependency_contract_versions: BTreeMap<String, String>,
    sibling_contract_versions: BTreeMap<String, String>,
    allowed_finding_routes: Vec<String>,
    binding_hash: String,
}

impl SemanticTaskAcceptanceSlice {
    pub fn task_rank(&self) -> u64 {
        self.task_rank
    }

    pub fn goal(&self) -> &str {
        &self.goal
    }

    pub fn task_contract(&self) -> &str {
        &self.task_contract
    }

    pub fn owned_files(&self) -> &[String] {
        &self.owned_files
    }

    pub fn contract_version(&self) -> &str {
        &self.contract_version
    }

    pub fn acceptance_oracle(&self) -> &[AcceptanceCriterionSnapshot] {
        &self.acceptance_oracle
    }

    pub fn dependency_contract_versions(&self) -> &BTreeMap<String, String> {
        &self.dependency_contract_versions
    }

    pub fn sibling_contract_versions(&self) -> &BTreeMap<String, String> {
        &self.sibling_contract_versions
    }

    pub fn allowed_finding_routes(&self) -> &[String] {
        &self.allowed_finding_routes
    }

    pub fn binding_hash(&self) -> &str {
        &self.binding_hash
    }
}

/// Exact immutable boundary a later nudge eligibility must still match at delivery time.
#[derive(Debug)]
pub struct SemanticNudgeBoundary {
    authority_id: String,
    capture_id: String,
    authority_scope: crate::AuthorityScope,
    phase_epoch: u64,
    task_id: String,
    attempt: u32,
    task_source_revision: u64,
    trace_source_revision: u64,
    snapshot_hash: String,
    source_provider_session: SemanticSourceProviderSessionBoundary,
}

impl SemanticNudgeBoundary {
    pub(crate) fn authority_id(&self) -> &str {
        &self.authority_id
    }

    pub(crate) fn capture_id(&self) -> &str {
        &self.capture_id
    }

    pub fn authority_scope(&self) -> &crate::AuthorityScope {
        &self.authority_scope
    }

    pub fn phase_epoch(&self) -> u64 {
        self.phase_epoch
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn task_source_revision(&self) -> u64 {
        self.task_source_revision
    }

    pub fn trace_source_revision(&self) -> u64 {
        self.trace_source_revision
    }

    pub fn snapshot_hash(&self) -> &str {
        &self.snapshot_hash
    }

    pub fn source_provider_session(&self) -> &SemanticSourceProviderSessionBoundary {
        &self.source_provider_session
    }
}

/// Observation capture whose immutable bytes have been checked against a real task/acceptance slice
/// and the provider request currently producing that task's source trace.
///
/// The ordinary [`SemanticObservationCapture`] remains valid observation-only evidence. Only this
/// wrapper can participate in nudge eligibility.
#[derive(Debug)]
pub struct BoundSemanticObservationCapture {
    capture: SemanticObservationCapture,
    task_slice: SemanticTaskAcceptanceSlice,
    nudge_boundary: SemanticNudgeBoundary,
}

impl BoundSemanticObservationCapture {
    pub fn snapshot(&self) -> &SealedSemanticObservationSnapshot {
        self.capture.snapshot()
    }

    pub fn task_slice(&self) -> &SemanticTaskAcceptanceSlice {
        &self.task_slice
    }

    pub fn nudge_boundary(&self) -> &SemanticNudgeBoundary {
        &self.nudge_boundary
    }

    pub(crate) fn authority_id(&self) -> &str {
        self.nudge_boundary.authority_id()
    }

    pub(crate) fn capture_id(&self) -> &str {
        self.nudge_boundary.capture_id()
    }

    pub(crate) fn into_nudge_parts(self) -> (SemanticTaskAcceptanceSlice, SemanticNudgeBoundary) {
        (self.task_slice, self.nudge_boundary)
    }
}

impl SemanticObservationCapture {
    pub fn new(
        snapshot: SealedSemanticObservationSnapshot,
        summons: SemanticObservationSummonsSignal,
    ) -> Result<Self, String> {
        let neutral = summons.neutral_signal();
        let matching = snapshot
            .payload()
            .neutral_signals
            .iter()
            .filter(|signal| signal.source_id == summons.source_id())
            .collect::<Vec<_>>();
        if matching.len() != 1 || matching[0] != &neutral {
            return Err(format!(
                "semantic summons signal `{}` is not sealed exactly once into the snapshot",
                summons.source_id()
            ));
        }
        Ok(Self { snapshot, summons })
    }

    pub fn snapshot(&self) -> &SealedSemanticObservationSnapshot {
        &self.snapshot
    }

    pub fn summons(&self) -> &SemanticObservationSummonsSignal {
        &self.summons
    }

    pub(crate) fn provider_nudge_safety_snapshot(&self) -> ProviderNudgeSafetySnapshot {
        let SemanticObservationSummonsSignal::TraceStateAdvanced { measurement, .. } =
            &self.summons;
        ProviderNudgeSafetySnapshot {
            provider_stream_revision: measurement.provider_stream_revision,
            provider_stream_chunks: measurement.provider_stream_chunks,
            provider_stream_bytes: measurement.provider_stream_bytes,
            provider_structured_output_chunks: measurement.provider_structured_output_chunks,
            provider_structured_output_bytes: measurement.provider_structured_output_bytes,
            provider_last_progress_elapsed_ms: measurement.provider_last_progress_elapsed_ms,
            provider_structured_output_active: measurement.provider_structured_output_active,
        }
    }

    pub fn revision(&self) -> SemanticTraceRevision {
        SemanticTraceRevision {
            authority_scope: self.snapshot.authority_scope().clone(),
            phase_epoch: self.snapshot.phase_epoch(),
            task_id: self.snapshot.task_id().to_string(),
            attempt: self.snapshot.attempt(),
            source_revision: self.snapshot.source_revision(),
            snapshot_hash: self.snapshot.snapshot_hash().to_string(),
        }
    }

    pub fn into_snapshot(self) -> SealedSemanticObservationSnapshot {
        self.snapshot
    }

    pub(crate) fn bind_task_session(
        self,
        request: &SemanticObservationCaptureRequest,
        task_evidence: &SemanticTaskEvidenceCapability,
        source_provider_session: SemanticSourceProviderSessionBoundary,
        capture_id: String,
    ) -> Result<BoundSemanticObservationCapture, String> {
        if capture_id.trim().is_empty() || capture_id.trim() != capture_id {
            return Err("semantic capture id is invalid".to_string());
        }
        if !task_evidence.matches(request) {
            return Err(
                "semantic capture request does not match its engine-minted task evidence"
                    .to_string(),
            );
        }
        request.activity_publisher.validate()?;
        if !source_provider_session.matches_publisher(&request.activity_publisher) {
            return Err(
                "semantic capture source provider session does not match its activity publisher"
                    .to_string(),
            );
        }
        if request.task_id != request.activity_publisher.task_id
            || request.attempt != request.activity_publisher.attempt
            || request.running_logical_device_id != request.activity_publisher.logical_device_id
            || request.running_model_id != request.activity_publisher.model_id
        {
            return Err(
                "semantic capture request does not match its engine-minted activity publisher"
                    .to_string(),
            );
        }

        let snapshot = self.snapshot();
        let payload = snapshot.payload();
        let measurement_artifact_version = match self.summons() {
            SemanticObservationSummonsSignal::TraceStateAdvanced { measurement, .. } => {
                &measurement.artifact_version
            }
        };
        if payload.artifact_version != *measurement_artifact_version {
            return Err(
                "semantic capture artifact version does not match its typed summons".to_string(),
            );
        }
        if payload.neutral_signals.len() != 1 {
            return Err(
                "nudge-capable semantic capture contains unbound neutral signals".to_string(),
            );
        }
        if snapshot.authority_scope() != &request.activity_publisher.source.authority_scope
            || snapshot.phase_epoch() != request.activity_publisher.source.phase_epoch
            || snapshot.task_id() != request.task_id
            || snapshot.attempt() != request.attempt
        {
            return Err(
                "semantic capture snapshot does not match the requested run/phase/task boundary"
                    .to_string(),
            );
        }
        if payload.goal != request.goal
            || payload.task_contract != request.task_contract
            || payload.contract_version != request.contract_version
            || payload.dependency_contract_versions != request.dependency_contract_versions
            || payload.sibling_contract_versions != request.sibling_contract_versions
        {
            return Err(
                "semantic capture snapshot substituted task or contract evidence".to_string(),
            );
        }

        let mut acceptance_oracle = request.acceptance_oracle.clone();
        acceptance_oracle.sort_by(|left, right| left.id.cmp(&right.id));
        if acceptance_oracle.is_empty() {
            return Err(
                "semantic nudge evidence requires a non-empty sealed acceptance oracle".to_string(),
            );
        }
        if payload.acceptance_oracle != acceptance_oracle {
            return Err("semantic capture snapshot substituted acceptance evidence".to_string());
        }

        let mut allowed_finding_routes = request.allowed_finding_routes.clone();
        allowed_finding_routes.sort();
        if payload.allowed_finding_routes != allowed_finding_routes {
            return Err("semantic capture snapshot substituted finding routes".to_string());
        }

        let mut owned_files = request.owned_files.clone();
        owned_files.sort();
        if owned_files.iter().any(|path| {
            path.trim().is_empty()
                || path.trim() != path
                || std::path::Path::new(path)
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
        }) || owned_files.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err(
                "semantic task slice has an empty, absolute, padded, or duplicate owned path"
                    .to_string(),
            );
        }
        let owned_paths = owned_files
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        let mut artifact_paths = Vec::with_capacity(payload.artifacts.len());
        for artifact in &payload.artifacts {
            if artifact.source_id.trim().is_empty()
                || artifact.source_id.trim() != artifact.source_id
                || !owned_paths.contains(&artifact.path)
            {
                return Err(
                    "semantic capture artifact is not a sealed owned-path snapshot".to_string(),
                );
            }
            artifact_paths.push(artifact.path.clone());
        }
        artifact_paths.sort();
        if artifact_paths.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("semantic capture has duplicate owned artifacts".to_string());
        }

        let task_slice_identity = SemanticTaskAcceptanceSliceIdentity {
            task_rank: request.task_rank,
            goal: &request.goal,
            task_contract: &request.task_contract,
            owned_files: &owned_files,
            contract_version: &request.contract_version,
            acceptance_oracle: &acceptance_oracle,
            dependency_contract_versions: &request.dependency_contract_versions,
            sibling_contract_versions: &request.sibling_contract_versions,
            allowed_finding_routes: &allowed_finding_routes,
            activity_publisher_id: &request.activity_publisher.publisher_id,
            source_provider_session_hash: source_provider_session.binding_hash(),
            task_evidence_authority_id: task_evidence.authority_id(),
        };
        let binding_hash = canonical_digest(&task_slice_identity);
        let task_slice = SemanticTaskAcceptanceSlice {
            task_rank: request.task_rank,
            goal: request.goal.clone(),
            task_contract: request.task_contract.clone(),
            owned_files,
            contract_version: request.contract_version.clone(),
            acceptance_oracle,
            dependency_contract_versions: request.dependency_contract_versions.clone(),
            sibling_contract_versions: request.sibling_contract_versions.clone(),
            allowed_finding_routes,
            binding_hash,
        };
        let nudge_boundary = SemanticNudgeBoundary {
            authority_id: task_evidence.authority_id().to_string(),
            capture_id,
            authority_scope: snapshot.authority_scope().clone(),
            phase_epoch: snapshot.phase_epoch(),
            task_id: snapshot.task_id().to_string(),
            attempt: snapshot.attempt(),
            task_source_revision: source_provider_session.task_source_revision(),
            trace_source_revision: snapshot.source_revision(),
            snapshot_hash: snapshot.snapshot_hash().to_string(),
            source_provider_session,
        };
        Ok(BoundSemanticObservationCapture {
            capture: self,
            task_slice,
            nudge_boundary,
        })
    }
}

#[derive(Serialize)]
struct SemanticTaskAcceptanceSliceIdentity<'a> {
    task_rank: u64,
    goal: &'a str,
    task_contract: &'a str,
    owned_files: &'a [String],
    contract_version: &'a str,
    acceptance_oracle: &'a [AcceptanceCriterionSnapshot],
    dependency_contract_versions: &'a BTreeMap<String, String>,
    sibling_contract_versions: &'a BTreeMap<String, String>,
    allowed_finding_routes: &'a [String],
    activity_publisher_id: &'a str,
    source_provider_session_hash: &'a str,
    task_evidence_authority_id: &'a str,
}

#[async_trait]
pub trait SemanticObservationSnapshotProducer: Send + Sync {
    async fn capture(
        &self,
        request: SemanticObservationCaptureRequest,
    ) -> Result<Option<SemanticObservationCapture>, String>;
}
