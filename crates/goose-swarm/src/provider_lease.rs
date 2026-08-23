//! Cross-process physical provider capacity authority.
//!
//! A lease is reserved before any provider transport can be touched, then durably exposed
//! immediately before the single-attempt provider call. Only an exact provider terminal may
//! release an exposed lease. Dropping a Rust future, handle, process, or stream never does.

use crate::broker::{
    AdmissionReceipt, HostCapacityEvidence, PhysicalFleetSnapshot, ProviderRequestReceipt,
    ProviderTerminalKind, ProviderTerminalReceipt, TaskVersion, VerifiedPhysicalLane, WorkPriority,
    WorkRole,
};
use fs2::FileExt;
use goose_provider_types::base::{
    scope_provider_http_exposure, ProviderHttpExposureBoundary, ProviderHttpProtocol,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const WAL_SCHEMA_VERSION: u32 = 1;
const GENESIS_HASH: &str = "genesis";
const LOCK_FILE_NAME: &str = "control.lock";
const WAL_FILE_NAME: &str = "authority.jsonl";
const CHECKPOINT_FILE_NAME: &str = "authority.checkpoint";
const AUTHORITY_DIRECTORY_NAME: &str = "physical-provider-authority-v1";
const INITIALIZATION_LOCK_SUFFIX: &str = ".init.lock";
const INITIALIZATION_READY: &[u8] = b"provider-authority-ready-v1\n";
#[cfg(test)]
const CHECKPOINT_KILL_POINT_ENV: &str = "GOOSE_PROVIDER_LEASE_CHECKPOINT_KILL_POINT";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseWorkPriority {
    AuxiliaryEvidence,
    Implementation,
    CriticalPath,
}

impl From<WorkPriority> for LeaseWorkPriority {
    fn from(value: WorkPriority) -> Self {
        match value {
            WorkPriority::AuxiliaryEvidence => Self::AuxiliaryEvidence,
            WorkPriority::Implementation => Self::Implementation,
            WorkPriority::CriticalPath => Self::CriticalPath,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseWorkRole {
    Build,
    Repair,
    ResearchEvidence,
    PlanningAuthority,
    RuntimeAcceptanceReview,
    CompletedArtifactReview,
    SemanticJudgeObservation,
    ContractReview,
    AcceptanceOracle,
}

impl From<WorkRole> for LeaseWorkRole {
    fn from(value: WorkRole) -> Self {
        match value {
            WorkRole::Build => Self::Build,
            WorkRole::Repair => Self::Repair,
            WorkRole::ResearchEvidence => Self::ResearchEvidence,
            WorkRole::PlanningAuthority => Self::PlanningAuthority,
            WorkRole::RuntimeAcceptanceReview => Self::RuntimeAcceptanceReview,
            WorkRole::CompletedArtifactReview => Self::CompletedArtifactReview,
            WorkRole::SemanticJudgeObservation => Self::SemanticJudgeObservation,
            WorkRole::ContractReview => Self::ContractReview,
            WorkRole::AcceptanceOracle => Self::AcceptanceOracle,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LeaseHostCapacityEvidence {
    ProbeSingleStream {
        probe_epoch: String,
    },
    MeasuredProfile {
        profile_hash: String,
        profile_key: String,
        max_concurrent: u32,
    },
}

impl LeaseHostCapacityEvidence {
    pub fn max_concurrent(&self) -> u32 {
        match self {
            Self::ProbeSingleStream { .. } => 1,
            Self::MeasuredProfile { max_concurrent, .. } => *max_concurrent,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::ProbeSingleStream { probe_epoch } => {
                validate_exact_text("capacity probe epoch", probe_epoch)
            }
            Self::MeasuredProfile {
                profile_hash,
                profile_key,
                max_concurrent,
            } => {
                validate_exact_text("capacity profile hash", profile_hash)?;
                validate_exact_text("capacity profile key", profile_key)?;
                if *max_concurrent == 0 {
                    return Err("host capacity is zero".to_string());
                }
                Ok(())
            }
        }
    }
}

impl From<&HostCapacityEvidence> for LeaseHostCapacityEvidence {
    fn from(value: &HostCapacityEvidence) -> Self {
        match value {
            HostCapacityEvidence::ProbeSingleStream { probe_epoch } => Self::ProbeSingleStream {
                probe_epoch: probe_epoch.clone(),
            },
            HostCapacityEvidence::MeasuredProfile {
                profile_hash,
                profile_key,
                max_concurrent,
            } => Self::MeasuredProfile {
                profile_hash: profile_hash.clone(),
                profile_key: profile_key.clone(),
                max_concurrent: *max_concurrent,
            },
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct SealedProviderLeaseAuthority {
    fleet_snapshot_id: String,
    lanes_by_logical_device: HashMap<String, VerifiedPhysicalLane>,
    protocol_by_transport: HashMap<String, ProviderHttpProtocol>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedProviderProtocolRoute {
    provider_transport_id: String,
    protocol: ProviderHttpProtocol,
}

impl VerifiedProviderProtocolRoute {
    pub fn new(
        provider_transport_id: impl Into<String>,
        protocol: ProviderHttpProtocol,
    ) -> Result<Self, ProviderLeaseError> {
        let provider_transport_id = provider_transport_id.into();
        if !is_canonical_digest(&provider_transport_id) {
            return Err(ProviderLeaseError::InvalidClaim(
                "provider protocol route transport is not a canonical sha256 digest".to_string(),
            ));
        }
        Ok(Self {
            provider_transport_id,
            protocol,
        })
    }

    pub fn provider_transport_id(&self) -> &str {
        &self.provider_transport_id
    }

    pub fn protocol(&self) -> ProviderHttpProtocol {
        self.protocol
    }
}

impl SealedProviderLeaseAuthority {
    pub fn from_fleet_snapshot(
        snapshot: &PhysicalFleetSnapshot,
        protocol_routes: impl IntoIterator<Item = VerifiedProviderProtocolRoute>,
    ) -> Result<Self, ProviderLeaseError> {
        let validated =
            PhysicalFleetSnapshot::new(snapshot.snapshot_id.clone(), snapshot.lanes.clone())
                .map_err(|error| {
                    ProviderLeaseError::InvalidClaim(format!(
                        "physical fleet authority is invalid: {error}"
                    ))
                })?;
        let lanes_by_logical_device = validated
            .lanes
            .into_iter()
            .map(|lane| (lane.logical_device_id.clone(), lane))
            .collect::<HashMap<_, _>>();
        let lane_transports = lanes_by_logical_device
            .values()
            .map(|lane| lane.provider_transport_id.as_str())
            .collect::<HashSet<_>>();
        let mut protocol_by_transport = HashMap::new();
        for route in protocol_routes {
            if !lane_transports.contains(route.provider_transport_id.as_str()) {
                return Err(ProviderLeaseError::InvalidClaim(
                    "provider protocol route is absent from the sealed fleet snapshot".to_string(),
                ));
            }
            if protocol_by_transport
                .insert(route.provider_transport_id, route.protocol)
                .is_some()
            {
                return Err(ProviderLeaseError::InvalidClaim(
                    "provider transport has duplicate protocol routes".to_string(),
                ));
            }
        }
        if lane_transports
            .iter()
            .any(|transport| !protocol_by_transport.contains_key(*transport))
        {
            return Err(ProviderLeaseError::InvalidClaim(
                "sealed fleet transport has no verified provider protocol route".to_string(),
            ));
        }
        Ok(Self {
            fleet_snapshot_id: validated.snapshot_id,
            lanes_by_logical_device,
            protocol_by_transport,
        })
    }

    fn lane_for_admission(
        &self,
        admission: &AdmissionReceipt,
    ) -> Result<&VerifiedPhysicalLane, ProviderLeaseError> {
        if admission.fleet_snapshot_id != self.fleet_snapshot_id {
            return Err(ProviderLeaseError::InvalidClaim(
                "admission is outside the sealed fleet snapshot".to_string(),
            ));
        }
        let lane = self
            .lanes_by_logical_device
            .get(&admission.logical_device_id)
            .ok_or_else(|| {
                ProviderLeaseError::InvalidClaim(
                    "admission lane is absent from the sealed fleet snapshot".to_string(),
                )
            })?;
        if lane.model_id != admission.model_id
            || lane.host_id != admission.physical_host_id
            || lane.model_instance_id != admission.model_instance_id
            || lane.provider_transport_id != admission.provider_transport_id
            || lane.route_evidence_id != admission.route_evidence_id
            || lane.capacity_evidence != admission.capacity_evidence
        {
            return Err(ProviderLeaseError::InvalidClaim(
                "admission route differs from the sealed fleet authority".to_string(),
            ));
        }
        Ok(lane)
    }

    fn protocol_for_lane(
        &self,
        lane: &VerifiedPhysicalLane,
    ) -> Result<ProviderHttpProtocol, ProviderLeaseError> {
        self.protocol_by_transport
            .get(&lane.provider_transport_id)
            .copied()
            .ok_or_else(|| {
                ProviderLeaseError::InvalidClaim(
                    "admission transport has no sealed provider protocol".to_string(),
                )
            })
    }
}

/// Immutable resource and source identity for one exact provider request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderLeaseClaim {
    admission_id: String,
    admission_sequence: u64,
    work_id: String,
    work_role: LeaseWorkRole,
    work_priority: LeaseWorkPriority,
    task_rank: u64,
    source: TaskVersion,
    fleet_snapshot_id: String,
    logical_device_id: String,
    model_id: String,
    physical_host_id: String,
    model_instance_id: String,
    provider_transport_id: String,
    provider_protocol_id: String,
    route_evidence_id: String,
    host_capacity_evidence: LeaseHostCapacityEvidence,
    advertised_instance_capacity: u32,
    queue_sequence: u64,
    provider_request_ordinal: u32,
    provider_request_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PersistedProviderLeaseClaim {
    admission_id: String,
    admission_sequence: u64,
    work_id: String,
    work_role: LeaseWorkRole,
    work_priority: LeaseWorkPriority,
    task_rank: u64,
    source: TaskVersion,
    fleet_snapshot_id: String,
    logical_device_id: String,
    model_id: String,
    physical_host_id: String,
    model_instance_id: String,
    provider_transport_id: String,
    provider_protocol_id: String,
    route_evidence_id: String,
    host_capacity_evidence: LeaseHostCapacityEvidence,
    advertised_instance_capacity: u32,
    queue_sequence: u64,
    provider_request_ordinal: u32,
    provider_request_id: String,
}

impl From<&ProviderLeaseClaim> for PersistedProviderLeaseClaim {
    fn from(claim: &ProviderLeaseClaim) -> Self {
        Self {
            admission_id: claim.admission_id.clone(),
            admission_sequence: claim.admission_sequence,
            work_id: claim.work_id.clone(),
            work_role: claim.work_role,
            work_priority: claim.work_priority,
            task_rank: claim.task_rank,
            source: claim.source.clone(),
            fleet_snapshot_id: claim.fleet_snapshot_id.clone(),
            logical_device_id: claim.logical_device_id.clone(),
            model_id: claim.model_id.clone(),
            physical_host_id: claim.physical_host_id.clone(),
            model_instance_id: claim.model_instance_id.clone(),
            provider_transport_id: claim.provider_transport_id.clone(),
            provider_protocol_id: claim.provider_protocol_id.clone(),
            route_evidence_id: claim.route_evidence_id.clone(),
            host_capacity_evidence: claim.host_capacity_evidence.clone(),
            advertised_instance_capacity: claim.advertised_instance_capacity,
            queue_sequence: claim.queue_sequence,
            provider_request_ordinal: claim.provider_request_ordinal,
            provider_request_id: claim.provider_request_id.clone(),
        }
    }
}

impl From<PersistedProviderLeaseClaim> for ProviderLeaseClaim {
    fn from(claim: PersistedProviderLeaseClaim) -> Self {
        Self {
            admission_id: claim.admission_id,
            admission_sequence: claim.admission_sequence,
            work_id: claim.work_id,
            work_role: claim.work_role,
            work_priority: claim.work_priority,
            task_rank: claim.task_rank,
            source: claim.source,
            fleet_snapshot_id: claim.fleet_snapshot_id,
            logical_device_id: claim.logical_device_id,
            model_id: claim.model_id,
            physical_host_id: claim.physical_host_id,
            model_instance_id: claim.model_instance_id,
            provider_transport_id: claim.provider_transport_id,
            provider_protocol_id: claim.provider_protocol_id,
            route_evidence_id: claim.route_evidence_id,
            host_capacity_evidence: claim.host_capacity_evidence,
            advertised_instance_capacity: claim.advertised_instance_capacity,
            queue_sequence: claim.queue_sequence,
            provider_request_ordinal: claim.provider_request_ordinal,
            provider_request_id: claim.provider_request_id,
        }
    }
}

impl ProviderLeaseClaim {
    pub fn from_authority(
        authority: &SealedProviderLeaseAuthority,
        admission: &AdmissionReceipt,
        request: &ProviderRequestReceipt,
    ) -> Result<Self, ProviderLeaseError> {
        let lane = authority.lane_for_admission(admission)?;
        let protocol = authority.protocol_for_lane(lane)?;
        let claim = Self {
            admission_id: admission.admission_id.clone(),
            admission_sequence: admission.admission_sequence,
            work_id: admission.work_id.clone(),
            work_role: admission.role.into(),
            work_priority: admission.priority.into(),
            task_rank: admission.task_rank,
            source: admission.source.clone(),
            fleet_snapshot_id: admission.fleet_snapshot_id.clone(),
            logical_device_id: admission.logical_device_id.clone(),
            model_id: admission.model_id.clone(),
            physical_host_id: admission.physical_host_id.clone(),
            model_instance_id: admission.model_instance_id.clone(),
            provider_transport_id: admission.provider_transport_id.clone(),
            provider_protocol_id: protocol.authority_id().to_string(),
            route_evidence_id: admission.route_evidence_id.clone(),
            host_capacity_evidence: (&admission.capacity_evidence).into(),
            advertised_instance_capacity: lane.advertised_instance_capacity,
            queue_sequence: admission.queue_sequence,
            provider_request_ordinal: request.key.ordinal,
            provider_request_id: request.key.provider_request_id.clone(),
        };
        claim.validate()?;
        if request.admission_id != claim.admission_id
            || request.physical_host_id != claim.physical_host_id
            || request.model_instance_id != claim.model_instance_id
        {
            return Err(ProviderLeaseError::InvalidClaim(
                "provider request does not match its admission identity".to_string(),
            ));
        }
        Ok(claim)
    }

    pub fn admission_id(&self) -> &str {
        &self.admission_id
    }

    pub fn provider_request_id(&self) -> &str {
        &self.provider_request_id
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

    pub fn provider_protocol_id(&self) -> &str {
        &self.provider_protocol_id
    }

    pub fn provider_http_protocol(&self) -> ProviderHttpProtocol {
        ProviderHttpProtocol::from_authority_id(&self.provider_protocol_id)
            .expect("validated provider lease claims carry a known protocol")
    }

    pub fn advertised_instance_capacity(&self) -> u32 {
        self.advertised_instance_capacity
    }

    pub fn validate(&self) -> Result<(), ProviderLeaseError> {
        self.source
            .validate()
            .map_err(ProviderLeaseError::InvalidClaim)?;
        for (name, value) in [
            ("admission id", self.admission_id.as_str()),
            ("work id", self.work_id.as_str()),
            ("fleet snapshot id", self.fleet_snapshot_id.as_str()),
            ("logical device id", self.logical_device_id.as_str()),
            ("model id", self.model_id.as_str()),
            ("physical host id", self.physical_host_id.as_str()),
            ("model instance id", self.model_instance_id.as_str()),
            ("provider transport id", self.provider_transport_id.as_str()),
            ("provider protocol id", self.provider_protocol_id.as_str()),
            ("route evidence id", self.route_evidence_id.as_str()),
            ("provider request id", self.provider_request_id.as_str()),
        ] {
            validate_exact_text(name, value).map_err(ProviderLeaseError::InvalidClaim)?;
        }
        if !is_canonical_digest(&self.provider_transport_id) {
            return Err(ProviderLeaseError::InvalidClaim(
                "provider transport id is not a canonical sha256 digest".to_string(),
            ));
        }
        if ProviderHttpProtocol::from_authority_id(&self.provider_protocol_id).is_none() {
            return Err(ProviderLeaseError::InvalidClaim(
                "provider protocol is not recognized by the lease authority".to_string(),
            ));
        }
        self.host_capacity_evidence
            .validate()
            .map_err(ProviderLeaseError::InvalidClaim)?;
        if self.advertised_instance_capacity == 0 {
            return Err(ProviderLeaseError::InvalidClaim(
                "advertised instance capacity is zero".to_string(),
            ));
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, ProviderLeaseError> {
        hash_serializable(self).map_err(ProviderLeaseError::CorruptWal)
    }

    fn request_identity(&self) -> RequestIdentity {
        RequestIdentity {
            run_id: self.source.authority_scope.run_id.clone(),
            phase_lineage_id: self.source.authority_scope.phase_lineage_id.clone(),
            admission_id: self.admission_id.clone(),
            ordinal: self.provider_request_ordinal,
            provider_request_id: self.provider_request_id.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLeaseBusyKind {
    AuthorityLock,
    HostCapacity,
    InstanceCapacity,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderLeaseBusy {
    pub kind: ProviderLeaseBusyKind,
    pub physical_host_id: String,
    pub model_instance_id: String,
    pub permits_held: u32,
    pub capacity: u32,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ProviderLeaseTry {
    Acquired(ReservedProviderLease),
    Busy(ProviderLeaseBusy),
}

#[derive(Debug, Eq, PartialEq)]
pub struct ReservedProviderLease {
    lease_id: String,
    owner_id: String,
    claim_digest: String,
    claim: Box<ProviderLeaseClaim>,
    reservation_sequence: u64,
}

impl ReservedProviderLease {
    pub fn lease_id(&self) -> &str {
        &self.lease_id
    }

    pub fn claim(&self) -> &ProviderLeaseClaim {
        &self.claim
    }

    pub fn claim_digest(&self) -> &str {
        &self.claim_digest
    }

    pub fn reservation_sequence(&self) -> u64 {
        self.reservation_sequence
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ExposedProviderLease {
    lease_id: String,
    owner_id: String,
    claim_digest: String,
    claim: Box<ProviderLeaseClaim>,
    reservation_sequence: u64,
    exposure_sequence: u64,
}

impl ExposedProviderLease {
    pub fn lease_id(&self) -> &str {
        &self.lease_id
    }

    pub fn claim(&self) -> &ProviderLeaseClaim {
        &self.claim
    }

    pub fn claim_digest(&self) -> &str {
        &self.claim_digest
    }

    pub fn reservation_sequence(&self) -> u64 {
        self.reservation_sequence
    }

    pub fn exposure_sequence(&self) -> u64 {
        self.exposure_sequence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderLeaseReleaseReceipt {
    pub lease_id: String,
    pub wal_sequence: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLeaseStatus {
    Reserved,
    Exposed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveProviderLeaseSnapshot {
    pub lease_id: String,
    pub owner_id: String,
    pub claim_digest: String,
    pub claim: ProviderLeaseClaim,
    pub status: ProviderLeaseStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderLeaseAuthoritySnapshot {
    pub next_sequence: u64,
    pub active: Vec<ActiveProviderLeaseSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderLeaseError {
    AuthorityContended,
    InvalidClaim(String),
    InvalidTransition(String),
    ReceiptMismatch(String),
    ConflictingEvidence(String),
    CorruptWal(String),
    UnsafeControlPath(String),
    Io(String),
    UnsupportedPlatform,
}

impl ProviderLeaseError {
    fn latches_authority(&self) -> bool {
        matches!(
            self,
            Self::CorruptWal(_) | Self::UnsafeControlPath(_) | Self::Io(_)
        )
    }
}

impl std::fmt::Display for ProviderLeaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthorityContended => write!(formatter, "provider lease authority is contended"),
            Self::InvalidClaim(reason) => {
                write!(formatter, "invalid provider lease claim: {reason}")
            }
            Self::InvalidTransition(reason) => {
                write!(formatter, "invalid provider lease transition: {reason}")
            }
            Self::ReceiptMismatch(reason) => {
                write!(formatter, "provider lease receipt mismatch: {reason}")
            }
            Self::ConflictingEvidence(reason) => {
                write!(
                    formatter,
                    "conflicting provider resource evidence: {reason}"
                )
            }
            Self::CorruptWal(reason) => {
                write!(formatter, "provider lease WAL is corrupt: {reason}")
            }
            Self::UnsafeControlPath(reason) => {
                write!(formatter, "unsafe provider lease control path: {reason}")
            }
            Self::Io(reason) => write!(formatter, "provider lease authority I/O failed: {reason}"),
            Self::UnsupportedPlatform => {
                write!(
                    formatter,
                    "provider lease authority requires macOS or Linux"
                )
            }
        }
    }
}

impl std::error::Error for ProviderLeaseError {}

#[derive(Debug, Eq, PartialEq)]
pub enum ProviderLeaseTransitionError<H> {
    Retryable {
        error: ProviderLeaseError,
        handle: Box<H>,
    },
    Fatal(ProviderLeaseError),
}

impl<H> ProviderLeaseTransitionError<H> {
    pub fn error(&self) -> &ProviderLeaseError {
        match self {
            Self::Retryable { error, .. } | Self::Fatal(error) => error,
        }
    }

    pub fn into_retryable_handle(self) -> Option<H> {
        match self {
            Self::Retryable { handle, .. } => Some(*handle),
            Self::Fatal(_) => None,
        }
    }
}

pub trait PhysicalProviderLeaseAuthority: Send + Sync {
    fn try_reserve(
        &self,
        claim: ProviderLeaseClaim,
    ) -> Result<ProviderLeaseTry, ProviderLeaseError>;

    fn expose(
        &self,
        reserved: ReservedProviderLease,
    ) -> Result<ExposedProviderLease, ProviderLeaseTransitionError<ReservedProviderLease>>;

    fn abandon_reserved(
        &self,
        reserved: ReservedProviderLease,
        reason: &str,
    ) -> Result<ProviderLeaseReleaseReceipt, ProviderLeaseTransitionError<ReservedProviderLease>>;

    fn provider_terminal(
        &self,
        exposed: ExposedProviderLease,
        terminal: &ProviderTerminalReceipt,
    ) -> Result<ProviderLeaseReleaseReceipt, ProviderLeaseTransitionError<ExposedProviderLease>>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderLeaseWaitPolicy {
    busy_retry_delay: Duration,
}

impl ProviderLeaseWaitPolicy {
    pub const fn new(busy_retry_delay: Duration) -> Self {
        Self { busy_retry_delay }
    }

    pub const fn busy_retry_delay(self) -> Duration {
        self.busy_retry_delay
    }
}

impl Default for ProviderLeaseWaitPolicy {
    fn default() -> Self {
        Self::new(Duration::from_millis(25))
    }
}

#[derive(Clone)]
pub struct RunScopedProviderLeaseAuthority {
    inner: Arc<RunScopedProviderLeaseInner>,
}

struct RunScopedProviderLeaseInner {
    physical: Arc<dyn PhysicalProviderLeaseAuthority>,
    sealed: SealedProviderLeaseAuthority,
    wait_policy: ProviderLeaseWaitPolicy,
    records: Mutex<HashMap<RequestIdentity, RuntimeLeaseRecord>>,
}

struct RuntimeLeaseRecord {
    claim_digest: String,
    state: RuntimeLeaseState,
}

enum RuntimeLeaseState {
    Acquiring,
    Reserved(ReservedProviderLease),
    Exposing,
    Exposed(ExposedProviderLease),
    Abandoning,
    Terminalizing,
    Released(RuntimeLeaseRelease),
    Failed(ProviderLeaseError),
}

enum RuntimeLeaseRelease {
    Abandoned {
        reason: String,
        receipt: ProviderLeaseReleaseReceipt,
    },
    Terminal {
        terminal: ProviderTerminalReceipt,
        receipt: ProviderLeaseReleaseReceipt,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderLeaseBoundaryStatus {
    Reserved,
    Exposed,
    Abandoned,
    Terminal,
    Transitioning,
    Failed,
}

#[derive(Clone)]
pub struct ProviderLeaseHttpBoundary {
    authority: RunScopedProviderLeaseAuthority,
    request_identity: RequestIdentity,
    claim_digest: String,
    protocol: ProviderHttpProtocol,
    transport_identity: String,
}

impl std::fmt::Debug for ProviderLeaseHttpBoundary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderLeaseHttpBoundary")
            .field("request_identity", &self.request_identity)
            .field("claim_digest", &self.claim_digest)
            .field("protocol", &self.protocol)
            .field("transport_identity", &self.transport_identity)
            .finish_non_exhaustive()
    }
}

struct ScopedProviderLeaseExposure {
    authority: RunScopedProviderLeaseAuthority,
    request_identity: RequestIdentity,
    claim_digest: String,
}

impl ProviderHttpExposureBoundary for ScopedProviderLeaseExposure {
    fn expose(
        &self,
        protocol: ProviderHttpProtocol,
        transport_identity: &str,
    ) -> Result<(), String> {
        self.authority
            .expose_http_request(
                &self.request_identity,
                &self.claim_digest,
                protocol,
                transport_identity,
            )
            .map_err(|error| error.to_string())
    }
}

impl ProviderLeaseHttpBoundary {
    pub fn protocol(&self) -> ProviderHttpProtocol {
        self.protocol
    }

    pub fn transport_identity(&self) -> &str {
        &self.transport_identity
    }

    pub fn status(&self) -> Result<ProviderLeaseBoundaryStatus, ProviderLeaseError> {
        self.authority
            .boundary_status(&self.request_identity, &self.claim_digest)
    }

    pub async fn scope_http<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        let exposure = ScopedProviderLeaseExposure {
            authority: self.authority.clone(),
            request_identity: self.request_identity.clone(),
            claim_digest: self.claim_digest.clone(),
        };
        scope_provider_http_exposure(Arc::new(exposure), future).await
    }

    pub(crate) fn abandon_reserved(
        &self,
        reason: &str,
    ) -> Result<ProviderLeaseReleaseReceipt, ProviderLeaseError> {
        self.authority
            .abandon_reserved(&self.request_identity, &self.claim_digest, reason)
    }

    pub(crate) fn provider_terminal(
        &self,
        terminal: &ProviderTerminalReceipt,
    ) -> Result<ProviderLeaseReleaseReceipt, ProviderLeaseError> {
        self.authority
            .provider_terminal(&self.request_identity, &self.claim_digest, terminal)
    }
}

impl RunScopedProviderLeaseAuthority {
    pub fn new(
        physical: Arc<dyn PhysicalProviderLeaseAuthority>,
        sealed: SealedProviderLeaseAuthority,
    ) -> Self {
        Self::new_with_wait_policy(physical, sealed, ProviderLeaseWaitPolicy::default())
    }

    pub fn new_with_wait_policy(
        physical: Arc<dyn PhysicalProviderLeaseAuthority>,
        sealed: SealedProviderLeaseAuthority,
        wait_policy: ProviderLeaseWaitPolicy,
    ) -> Self {
        Self {
            inner: Arc::new(RunScopedProviderLeaseInner {
                physical,
                sealed,
                wait_policy,
                records: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub async fn reserve_provider_request(
        &self,
        admission: &AdmissionReceipt,
        request: &ProviderRequestReceipt,
    ) -> Result<ProviderLeaseHttpBoundary, ProviderLeaseError> {
        let claim = ProviderLeaseClaim::from_authority(&self.inner.sealed, admission, request)?;
        let request_identity = claim.request_identity();
        let claim_digest = claim.digest()?;
        let protocol = claim.provider_http_protocol();
        let transport_identity = claim.provider_transport_id().to_string();
        {
            let mut records = lock_runtime_records(&self.inner.records);
            if records.contains_key(&request_identity) {
                return Err(ProviderLeaseError::InvalidTransition(
                    "provider request already has a run-scoped lease record".to_string(),
                ));
            }
            records.insert(
                request_identity.clone(),
                RuntimeLeaseRecord {
                    claim_digest: claim_digest.clone(),
                    state: RuntimeLeaseState::Acquiring,
                },
            );
        }

        let mut acquisition = RuntimeAcquisitionGuard {
            inner: self.inner.clone(),
            request_identity: request_identity.clone(),
            claim_digest: claim_digest.clone(),
            armed: true,
        };
        loop {
            match self.inner.physical.try_reserve(claim.clone()) {
                Ok(ProviderLeaseTry::Acquired(reserved)) => {
                    let mut records = lock_runtime_records(&self.inner.records);
                    let record = matching_runtime_record_mut(
                        &mut records,
                        &request_identity,
                        &claim_digest,
                    )?;
                    if !matches!(record.state, RuntimeLeaseState::Acquiring) {
                        return Err(ProviderLeaseError::ConflictingEvidence(
                            "run-scoped reservation state changed during acquisition".to_string(),
                        ));
                    }
                    record.state = RuntimeLeaseState::Reserved(reserved);
                    acquisition.armed = false;
                    return Ok(ProviderLeaseHttpBoundary {
                        authority: self.clone(),
                        request_identity,
                        claim_digest,
                        protocol,
                        transport_identity,
                    });
                }
                Ok(ProviderLeaseTry::Busy(busy)) => match busy.kind {
                    ProviderLeaseBusyKind::AuthorityLock => tokio::task::yield_now().await,
                    ProviderLeaseBusyKind::HostCapacity
                    | ProviderLeaseBusyKind::InstanceCapacity => {
                        let delay = self.inner.wait_policy.busy_retry_delay();
                        if delay.is_zero() {
                            tokio::task::yield_now().await;
                        } else {
                            tokio::time::sleep(delay).await;
                        }
                    }
                },
                Err(error) => {
                    let mut records = lock_runtime_records(&self.inner.records);
                    if let Ok(record) =
                        matching_runtime_record_mut(&mut records, &request_identity, &claim_digest)
                    {
                        record.state = RuntimeLeaseState::Failed(error.clone());
                    }
                    acquisition.armed = false;
                    return Err(error);
                }
            }
        }
    }

    fn expose_http_request(
        &self,
        request_identity: &RequestIdentity,
        claim_digest: &str,
        protocol: ProviderHttpProtocol,
        transport_identity: &str,
    ) -> Result<(), ProviderLeaseError> {
        let reserved = {
            let mut records = lock_runtime_records(&self.inner.records);
            let record = matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
            let expected = match &record.state {
                RuntimeLeaseState::Reserved(reserved) => reserved.claim(),
                RuntimeLeaseState::Failed(error) => return Err(error.clone()),
                RuntimeLeaseState::Exposed(_) => {
                    return Err(ProviderLeaseError::InvalidTransition(
                        "provider request was already exposed; a second POST is unsafe".to_string(),
                    ));
                }
                RuntimeLeaseState::Released(_) => {
                    return Err(ProviderLeaseError::InvalidTransition(
                        "provider request was already released".to_string(),
                    ));
                }
                RuntimeLeaseState::Acquiring
                | RuntimeLeaseState::Exposing
                | RuntimeLeaseState::Abandoning
                | RuntimeLeaseState::Terminalizing => {
                    return Err(ProviderLeaseError::AuthorityContended);
                }
            };
            if protocol != expected.provider_http_protocol() {
                return Err(ProviderLeaseError::ReceiptMismatch(
                    "actual HTTP protocol differs from the sealed provider route".to_string(),
                ));
            }
            if transport_identity != expected.provider_transport_id() {
                return Err(ProviderLeaseError::ReceiptMismatch(
                    "actual HTTP transport differs from the sealed provider route".to_string(),
                ));
            }
            match std::mem::replace(&mut record.state, RuntimeLeaseState::Exposing) {
                RuntimeLeaseState::Reserved(reserved) => reserved,
                _ => unreachable!("reserved state was matched before transition"),
            }
        };

        match self.inner.physical.expose(reserved) {
            Ok(exposed) => {
                let mut records = lock_runtime_records(&self.inner.records);
                let record =
                    matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
                if !matches!(record.state, RuntimeLeaseState::Exposing) {
                    record.state =
                        RuntimeLeaseState::Failed(ProviderLeaseError::ConflictingEvidence(
                            "run-scoped exposure state changed during transition".to_string(),
                        ));
                    return Err(ProviderLeaseError::ConflictingEvidence(
                        "run-scoped exposure state changed during transition".to_string(),
                    ));
                }
                record.state = RuntimeLeaseState::Exposed(exposed);
                Ok(())
            }
            Err(ProviderLeaseTransitionError::Retryable { error, handle }) => {
                let mut records = lock_runtime_records(&self.inner.records);
                let record =
                    matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
                record.state = RuntimeLeaseState::Reserved(*handle);
                Err(error)
            }
            Err(ProviderLeaseTransitionError::Fatal(error)) => {
                let mut records = lock_runtime_records(&self.inner.records);
                let record =
                    matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
                record.state = RuntimeLeaseState::Failed(error.clone());
                Err(error)
            }
        }
    }

    fn abandon_reserved(
        &self,
        request_identity: &RequestIdentity,
        claim_digest: &str,
        reason: &str,
    ) -> Result<ProviderLeaseReleaseReceipt, ProviderLeaseError> {
        let reserved = {
            let mut records = lock_runtime_records(&self.inner.records);
            let record = matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
            match &record.state {
                RuntimeLeaseState::Released(RuntimeLeaseRelease::Abandoned {
                    reason: recorded,
                    receipt,
                }) if recorded == reason => return Ok(receipt.clone()),
                RuntimeLeaseState::Released(_) => {
                    return Err(ProviderLeaseError::InvalidTransition(
                        "provider request already has a different release".to_string(),
                    ));
                }
                RuntimeLeaseState::Failed(error) => return Err(error.clone()),
                RuntimeLeaseState::Exposed(_) => {
                    return Err(ProviderLeaseError::InvalidTransition(
                        "an exposed provider request cannot be abandoned as reserved".to_string(),
                    ));
                }
                RuntimeLeaseState::Acquiring
                | RuntimeLeaseState::Exposing
                | RuntimeLeaseState::Abandoning
                | RuntimeLeaseState::Terminalizing => {
                    return Err(ProviderLeaseError::AuthorityContended);
                }
                RuntimeLeaseState::Reserved(_) => {}
            }
            match std::mem::replace(&mut record.state, RuntimeLeaseState::Abandoning) {
                RuntimeLeaseState::Reserved(reserved) => reserved,
                _ => unreachable!("reserved state was matched before abandon"),
            }
        };

        match self.inner.physical.abandon_reserved(reserved, reason) {
            Ok(receipt) => {
                let mut records = lock_runtime_records(&self.inner.records);
                let record =
                    matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
                record.state = RuntimeLeaseState::Released(RuntimeLeaseRelease::Abandoned {
                    reason: reason.to_string(),
                    receipt: receipt.clone(),
                });
                Ok(receipt)
            }
            Err(ProviderLeaseTransitionError::Retryable { error, handle }) => {
                let mut records = lock_runtime_records(&self.inner.records);
                let record =
                    matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
                record.state = RuntimeLeaseState::Reserved(*handle);
                Err(error)
            }
            Err(ProviderLeaseTransitionError::Fatal(error)) => {
                let mut records = lock_runtime_records(&self.inner.records);
                let record =
                    matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
                record.state = RuntimeLeaseState::Failed(error.clone());
                Err(error)
            }
        }
    }

    fn provider_terminal(
        &self,
        request_identity: &RequestIdentity,
        claim_digest: &str,
        terminal: &ProviderTerminalReceipt,
    ) -> Result<ProviderLeaseReleaseReceipt, ProviderLeaseError> {
        let exposed = {
            let mut records = lock_runtime_records(&self.inner.records);
            let record = matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
            match &record.state {
                RuntimeLeaseState::Released(RuntimeLeaseRelease::Terminal {
                    terminal: recorded,
                    receipt,
                }) if recorded == terminal => return Ok(receipt.clone()),
                RuntimeLeaseState::Released(_) => {
                    return Err(ProviderLeaseError::ReceiptMismatch(
                        "provider request already has a different release receipt".to_string(),
                    ));
                }
                RuntimeLeaseState::Failed(error) => return Err(error.clone()),
                RuntimeLeaseState::Reserved(_) => {
                    return Err(ProviderLeaseError::InvalidTransition(
                        "provider terminal cannot release a request before HTTP exposure"
                            .to_string(),
                    ));
                }
                RuntimeLeaseState::Acquiring
                | RuntimeLeaseState::Exposing
                | RuntimeLeaseState::Abandoning
                | RuntimeLeaseState::Terminalizing => {
                    return Err(ProviderLeaseError::AuthorityContended);
                }
                RuntimeLeaseState::Exposed(_) => {}
            }
            match std::mem::replace(&mut record.state, RuntimeLeaseState::Terminalizing) {
                RuntimeLeaseState::Exposed(exposed) => exposed,
                _ => unreachable!("exposed state was matched before terminal"),
            }
        };

        match self.inner.physical.provider_terminal(exposed, terminal) {
            Ok(receipt) => {
                let mut records = lock_runtime_records(&self.inner.records);
                let record =
                    matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
                record.state = RuntimeLeaseState::Released(RuntimeLeaseRelease::Terminal {
                    terminal: terminal.clone(),
                    receipt: receipt.clone(),
                });
                Ok(receipt)
            }
            Err(ProviderLeaseTransitionError::Retryable { error, handle }) => {
                let mut records = lock_runtime_records(&self.inner.records);
                let record =
                    matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
                record.state = RuntimeLeaseState::Exposed(*handle);
                Err(error)
            }
            Err(ProviderLeaseTransitionError::Fatal(error)) => {
                let mut records = lock_runtime_records(&self.inner.records);
                let record =
                    matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
                record.state = RuntimeLeaseState::Failed(error.clone());
                Err(error)
            }
        }
    }

    fn boundary_status(
        &self,
        request_identity: &RequestIdentity,
        claim_digest: &str,
    ) -> Result<ProviderLeaseBoundaryStatus, ProviderLeaseError> {
        let mut records = lock_runtime_records(&self.inner.records);
        let record = matching_runtime_record_mut(&mut records, request_identity, claim_digest)?;
        Ok(match &record.state {
            RuntimeLeaseState::Reserved(_) => ProviderLeaseBoundaryStatus::Reserved,
            RuntimeLeaseState::Exposed(_) => ProviderLeaseBoundaryStatus::Exposed,
            RuntimeLeaseState::Released(RuntimeLeaseRelease::Abandoned { .. }) => {
                ProviderLeaseBoundaryStatus::Abandoned
            }
            RuntimeLeaseState::Released(RuntimeLeaseRelease::Terminal { .. }) => {
                ProviderLeaseBoundaryStatus::Terminal
            }
            RuntimeLeaseState::Failed(_) => ProviderLeaseBoundaryStatus::Failed,
            RuntimeLeaseState::Acquiring
            | RuntimeLeaseState::Exposing
            | RuntimeLeaseState::Abandoning
            | RuntimeLeaseState::Terminalizing => ProviderLeaseBoundaryStatus::Transitioning,
        })
    }
}

struct RuntimeAcquisitionGuard {
    inner: Arc<RunScopedProviderLeaseInner>,
    request_identity: RequestIdentity,
    claim_digest: String,
    armed: bool,
}

impl Drop for RuntimeAcquisitionGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut records = lock_runtime_records(&self.inner.records);
        if records.get(&self.request_identity).is_some_and(|record| {
            record.claim_digest == self.claim_digest
                && matches!(record.state, RuntimeLeaseState::Acquiring)
        }) {
            records.remove(&self.request_identity);
        }
    }
}

fn lock_runtime_records(
    records: &Mutex<HashMap<RequestIdentity, RuntimeLeaseRecord>>,
) -> MutexGuard<'_, HashMap<RequestIdentity, RuntimeLeaseRecord>> {
    records
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn matching_runtime_record_mut<'a>(
    records: &'a mut HashMap<RequestIdentity, RuntimeLeaseRecord>,
    request_identity: &RequestIdentity,
    claim_digest: &str,
) -> Result<&'a mut RuntimeLeaseRecord, ProviderLeaseError> {
    let record = records.get_mut(request_identity).ok_or_else(|| {
        ProviderLeaseError::InvalidTransition(
            "provider request has no run-scoped lease record".to_string(),
        )
    })?;
    if record.claim_digest != claim_digest {
        return Err(ProviderLeaseError::ConflictingEvidence(
            "provider request lease claim digest changed".to_string(),
        ));
    }
    Ok(record)
}

/// One process-local handle onto the shared, fixed-root WAL authority.
pub struct GlobalProviderLeaseAuthority {
    root: PathBuf,
    root_identity: FileIdentity,
    owner_id: String,
    files: Mutex<AuthorityFiles>,
}

struct AuthorityFiles {
    initialization_lock_path: PathBuf,
    initialization_lock_file: File,
    initialization_lock_identity: FileIdentity,
    lock_path: PathBuf,
    lock_file: File,
    lock_identity: FileIdentity,
    wal_path: PathBuf,
    wal_file: File,
    wal_identity: FileIdentity,
    checkpoint_path: PathBuf,
    failure: Option<ProviderLeaseError>,
}

impl AuthorityFiles {
    fn replay(&mut self) -> Result<WalState, ProviderLeaseError> {
        replay_wal(&mut self.wal_file, &self.checkpoint_path)
    }

    fn append(
        &mut self,
        wal: &mut WalState,
        material: WalMaterial,
    ) -> Result<(), ProviderLeaseError> {
        append_material(&mut self.wal_file, &self.checkpoint_path, wal, material)
    }
}

impl GlobalProviderLeaseAuthority {
    /// Opens the single platform state root shared by all goose processes for this user.
    pub fn open_fixed_root() -> Result<Self, ProviderLeaseError> {
        #[cfg(not(unix))]
        {
            return Err(ProviderLeaseError::UnsupportedPlatform);
        }

        #[cfg(unix)]
        {
            Self::open_at_root(fixed_control_root()?)
        }
    }

    /// Opens an explicit isolated root for hermetic authority tests.
    #[doc(hidden)]
    pub fn open_test_root(root: impl AsRef<Path>) -> Result<Self, ProviderLeaseError> {
        Self::open_at_root(root.as_ref().to_path_buf())
    }

    fn open_at_root(root: PathBuf) -> Result<Self, ProviderLeaseError> {
        #[cfg(not(unix))]
        {
            let _ = root;
            return Err(ProviderLeaseError::UnsupportedPlatform);
        }

        #[cfg(unix)]
        {
            if !root.is_absolute() {
                return Err(ProviderLeaseError::UnsafeControlPath(
                    "control root is not absolute".to_string(),
                ));
            }
            let root = normalize_control_root(&root)?;
            let root_parent = root.parent().ok_or_else(|| {
                ProviderLeaseError::UnsafeControlPath("control root has no parent".to_string())
            })?;
            std::fs::create_dir_all(root_parent).map_err(|error| {
                ProviderLeaseError::Io(format!("cannot create control-root parent: {error}"))
            })?;
            let initialization_lock_path = initialization_lock_path(&root)?;
            let (initialization_lock_file, _initialization_lock_created) =
                open_or_create_secure_file(&initialization_lock_path, FileAccess::Rewrite)?;
            let initialization_lock_identity =
                verify_secure_file(&initialization_lock_path, &initialization_lock_file)?;
            FileExt::lock_exclusive(&initialization_lock_file).map_err(|error| {
                ProviderLeaseError::Io(format!("cannot lock authority initialization: {error}"))
            })?;
            let initialization_state = read_initialization_state(&initialization_lock_file)?;
            let recovering_initialization = initialization_state == InitializationState::Recover;
            let (root_identity, _) = secure_control_root(&root, recovering_initialization)?;
            let lock_path = root.join(LOCK_FILE_NAME);
            let lock_file = if recovering_initialization {
                open_or_create_secure_file(&lock_path, FileAccess::Rewrite)?.0
            } else {
                open_secure_file(&lock_path, false, FileAccess::Rewrite)?
            };
            let lock_identity = verify_secure_file(&lock_path, &lock_file)?;
            let wal_path = root.join(WAL_FILE_NAME);
            let mut wal_file = if recovering_initialization {
                open_or_create_secure_file(&wal_path, FileAccess::Append)?.0
            } else {
                open_secure_file(&wal_path, false, FileAccess::Append)?
            };
            let wal_identity = verify_secure_file(&wal_path, &wal_file)?;
            let checkpoint_path = root.join(CHECKPOINT_FILE_NAME);
            FileExt::lock_exclusive(&lock_file).map_err(|error| {
                ProviderLeaseError::Io(format!("cannot lock authority: {error}"))
            })?;
            let replay_result = (|| {
                verify_no_symlink_components(&root)?;
                verify_identity(&root, root_identity, true)?;
                verify_expected_file(
                    &initialization_lock_path,
                    &initialization_lock_file,
                    initialization_lock_identity,
                )?;
                verify_expected_file(&lock_path, &lock_file, lock_identity)?;
                verify_expected_file(&wal_path, &wal_file, wal_identity)?;
                cleanup_checkpoint_temps(&root)?;
                if recovering_initialization {
                    let wal_length = wal_file
                        .metadata()
                        .map_err(|error| {
                            ProviderLeaseError::Io(format!(
                                "cannot inspect recovering WAL: {error}"
                            ))
                        })?
                        .len();
                    if wal_length != 0 {
                        return Err(ProviderLeaseError::CorruptWal(
                            "incomplete first initialization contains provider transitions"
                                .to_string(),
                        ));
                    }
                    match std::fs::symlink_metadata(&checkpoint_path) {
                        Ok(_) => {
                            let checkpoint = read_checkpoint_path(&checkpoint_path)?;
                            if checkpoint.next_sequence != 0
                                || checkpoint.entry_hash != GENESIS_HASH
                            {
                                return Err(ProviderLeaseError::CorruptWal(
                                    "incomplete first initialization has a non-genesis checkpoint"
                                        .to_string(),
                                ));
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            write_checkpoint_atomic(&checkpoint_path, &WalState::genesis())?;
                        }
                        Err(error) => {
                            return Err(ProviderLeaseError::UnsafeControlPath(format!(
                                "cannot inspect recovering checkpoint: {error}"
                            )));
                        }
                    }
                }
                replay_wal(&mut wal_file, &checkpoint_path)
            })();
            let unlock_result = FileExt::unlock(&lock_file).map_err(|error| {
                ProviderLeaseError::Io(format!("cannot unlock authority: {error}"))
            });
            replay_result?;
            unlock_result?;
            if recovering_initialization {
                write_initialization_ready(&initialization_lock_file)?;
            }
            FileExt::unlock(&initialization_lock_file).map_err(|error| {
                ProviderLeaseError::Io(format!("cannot unlock authority initialization: {error}"))
            })?;
            Ok(Self {
                root,
                root_identity,
                owner_id: random_id("owner"),
                files: Mutex::new(AuthorityFiles {
                    initialization_lock_path,
                    initialization_lock_file,
                    initialization_lock_identity,
                    lock_path,
                    lock_file,
                    lock_identity,
                    wal_path,
                    wal_file,
                    wal_identity,
                    checkpoint_path,
                    failure: None,
                }),
            })
        }
    }

    pub fn control_root(&self) -> &Path {
        &self.root
    }

    pub fn snapshot(&self) -> Result<ProviderLeaseAuthoritySnapshot, ProviderLeaseError> {
        match self.with_wal(|_, wal| Ok(wal.snapshot()))? {
            TransactionResult::Completed(snapshot) => Ok(snapshot),
            TransactionResult::Contended => Err(ProviderLeaseError::AuthorityContended),
        }
    }

    fn with_wal<T>(
        &self,
        operation: impl FnOnce(&mut AuthorityFiles, &mut WalState) -> Result<T, ProviderLeaseError>,
    ) -> Result<TransactionResult<T>, ProviderLeaseError> {
        let mut files = lock(&self.files);
        if let Some(error) = files.failure.clone() {
            return Err(error);
        }
        match FileExt::try_lock_exclusive(&files.lock_file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Ok(TransactionResult::Contended);
            }
            Err(error) => {
                let error = ProviderLeaseError::Io(format!("cannot lock authority: {error}"));
                files.failure = Some(error.clone());
                return Err(error);
            }
        }

        let result = (|| {
            verify_no_symlink_components(&self.root)?;
            verify_identity(&self.root, self.root_identity, true)?;
            verify_expected_file(
                &files.initialization_lock_path,
                &files.initialization_lock_file,
                files.initialization_lock_identity,
            )?;
            if read_initialization_state(&files.initialization_lock_file)?
                != InitializationState::Ready
            {
                return Err(ProviderLeaseError::UnsafeControlPath(
                    "authority initialization marker became empty".to_string(),
                ));
            }
            verify_expected_file(&files.lock_path, &files.lock_file, files.lock_identity)?;
            verify_expected_file(&files.wal_path, &files.wal_file, files.wal_identity)?;
            cleanup_checkpoint_temps(&self.root)?;
            let mut wal = files.replay()?;
            operation(&mut files, &mut wal)
        })();
        let unlock_result = FileExt::unlock(&files.lock_file)
            .map_err(|error| ProviderLeaseError::Io(format!("cannot unlock authority: {error}")));
        let result = match (result, unlock_result) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        };
        if let Err(error) = &result {
            if error.latches_authority() {
                files.failure = Some(error.clone());
            }
        }
        result.map(TransactionResult::Completed)
    }
}

impl PhysicalProviderLeaseAuthority for GlobalProviderLeaseAuthority {
    fn try_reserve(
        &self,
        claim: ProviderLeaseClaim,
    ) -> Result<ProviderLeaseTry, ProviderLeaseError> {
        claim.validate()?;
        let claim_digest = claim.digest()?;
        let host_id = claim.physical_host_id.clone();
        let instance_id = claim.model_instance_id.clone();
        match self.with_wal(|files, wal| {
            wal.validate_resource_binding(&claim)?;
            if wal.seen_requests.contains_key(&claim.request_identity()) {
                return Err(ProviderLeaseError::InvalidTransition(
                    "provider request identity was already reserved".to_string(),
                ));
            }
            let host_used = wal.host_occupancy(&host_id);
            let host_capacity = claim.host_capacity_evidence.max_concurrent();
            if host_used >= host_capacity {
                return Ok(ProviderLeaseTry::Busy(ProviderLeaseBusy {
                    kind: ProviderLeaseBusyKind::HostCapacity,
                    physical_host_id: host_id.clone(),
                    model_instance_id: instance_id.clone(),
                    permits_held: host_used,
                    capacity: host_capacity,
                }));
            }
            let instance_used = wal.instance_occupancy(&host_id, &instance_id);
            if instance_used >= claim.advertised_instance_capacity {
                return Ok(ProviderLeaseTry::Busy(ProviderLeaseBusy {
                    kind: ProviderLeaseBusyKind::InstanceCapacity,
                    physical_host_id: host_id.clone(),
                    model_instance_id: instance_id.clone(),
                    permits_held: instance_used,
                    capacity: claim.advertised_instance_capacity,
                }));
            }
            let lease_id = random_id("lease");
            let sequence = wal.next_sequence;
            let material = WalMaterial {
                schema_version: WAL_SCHEMA_VERSION,
                sequence,
                previous_hash: wal.previous_hash.clone(),
                transition: WalTransition::Reserved,
                lease_id: lease_id.clone(),
                owner_id: self.owner_id.clone(),
                claim_digest: claim_digest.clone(),
                claim: Some(PersistedProviderLeaseClaim::from(&claim)),
                terminal: None,
                reason: None,
            };
            files.append(wal, material)?;
            Ok(ProviderLeaseTry::Acquired(ReservedProviderLease {
                lease_id,
                owner_id: self.owner_id.clone(),
                claim_digest,
                claim: Box::new(claim.clone()),
                reservation_sequence: sequence,
            }))
        })? {
            TransactionResult::Completed(result) => Ok(result),
            TransactionResult::Contended => Ok(ProviderLeaseTry::Busy(ProviderLeaseBusy {
                kind: ProviderLeaseBusyKind::AuthorityLock,
                physical_host_id: host_id,
                model_instance_id: instance_id,
                permits_held: 0,
                capacity: 0,
            })),
        }
    }

    fn expose(
        &self,
        reserved: ReservedProviderLease,
    ) -> Result<ExposedProviderLease, ProviderLeaseTransitionError<ReservedProviderLease>> {
        let result = self.with_wal(|files, wal| {
            wal.validate_token(
                &reserved.lease_id,
                &reserved.owner_id,
                &reserved.claim_digest,
                ProviderLeaseStatus::Reserved,
            )?;
            let sequence = wal.next_sequence;
            files.append(
                wal,
                WalMaterial {
                    schema_version: WAL_SCHEMA_VERSION,
                    sequence,
                    previous_hash: wal.previous_hash.clone(),
                    transition: WalTransition::Exposed,
                    lease_id: reserved.lease_id.clone(),
                    owner_id: reserved.owner_id.clone(),
                    claim_digest: reserved.claim_digest.clone(),
                    claim: None,
                    terminal: None,
                    reason: None,
                },
            )?;
            Ok(ExposedProviderLease {
                lease_id: reserved.lease_id.clone(),
                owner_id: reserved.owner_id.clone(),
                claim_digest: reserved.claim_digest.clone(),
                claim: reserved.claim.clone(),
                reservation_sequence: reserved.reservation_sequence,
                exposure_sequence: sequence,
            })
        });
        finish_transition(result, reserved)
    }

    fn abandon_reserved(
        &self,
        reserved: ReservedProviderLease,
        reason: &str,
    ) -> Result<ProviderLeaseReleaseReceipt, ProviderLeaseTransitionError<ReservedProviderLease>>
    {
        if let Err(reason) = validate_exact_text("reserved-abandon reason", reason) {
            return Err(ProviderLeaseTransitionError::Fatal(
                ProviderLeaseError::InvalidTransition(reason),
            ));
        }
        let result = self.with_wal(|files, wal| {
            wal.validate_token(
                &reserved.lease_id,
                &reserved.owner_id,
                &reserved.claim_digest,
                ProviderLeaseStatus::Reserved,
            )?;
            let sequence = wal.next_sequence;
            files.append(
                wal,
                WalMaterial {
                    schema_version: WAL_SCHEMA_VERSION,
                    sequence,
                    previous_hash: wal.previous_hash.clone(),
                    transition: WalTransition::AbandonedReserved,
                    lease_id: reserved.lease_id.clone(),
                    owner_id: reserved.owner_id.clone(),
                    claim_digest: reserved.claim_digest.clone(),
                    claim: None,
                    terminal: None,
                    reason: Some(reason.to_string()),
                },
            )?;
            Ok(ProviderLeaseReleaseReceipt {
                lease_id: reserved.lease_id.clone(),
                wal_sequence: sequence,
            })
        });
        finish_transition(result, reserved)
    }

    fn provider_terminal(
        &self,
        exposed: ExposedProviderLease,
        terminal: &ProviderTerminalReceipt,
    ) -> Result<ProviderLeaseReleaseReceipt, ProviderLeaseTransitionError<ExposedProviderLease>>
    {
        let terminal = LeaseTerminalRecord::from_receipt(terminal, &exposed.claim)
            .map_err(ProviderLeaseTransitionError::Fatal)?;
        let result = self.with_wal(|files, wal| {
            wal.validate_token(
                &exposed.lease_id,
                &exposed.owner_id,
                &exposed.claim_digest,
                ProviderLeaseStatus::Exposed,
            )?;
            let sequence = wal.next_sequence;
            files.append(
                wal,
                WalMaterial {
                    schema_version: WAL_SCHEMA_VERSION,
                    sequence,
                    previous_hash: wal.previous_hash.clone(),
                    transition: WalTransition::ProviderTerminal,
                    lease_id: exposed.lease_id.clone(),
                    owner_id: exposed.owner_id.clone(),
                    claim_digest: exposed.claim_digest.clone(),
                    claim: None,
                    terminal: Some(terminal.clone()),
                    reason: None,
                },
            )?;
            Ok(ProviderLeaseReleaseReceipt {
                lease_id: exposed.lease_id.clone(),
                wal_sequence: sequence,
            })
        });
        finish_transition(result, exposed)
    }
}

enum TransactionResult<T> {
    Completed(T),
    Contended,
}

fn finish_transition<T, H>(
    result: Result<TransactionResult<T>, ProviderLeaseError>,
    handle: H,
) -> Result<T, ProviderLeaseTransitionError<H>> {
    match result {
        Ok(TransactionResult::Completed(value)) => Ok(value),
        Ok(TransactionResult::Contended) => Err(ProviderLeaseTransitionError::Retryable {
            error: ProviderLeaseError::AuthorityContended,
            handle: Box::new(handle),
        }),
        Err(error) => Err(ProviderLeaseTransitionError::Fatal(error)),
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
struct RequestIdentity {
    run_id: String,
    phase_lineage_id: String,
    admission_id: String,
    ordinal: u32,
    provider_request_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum WalTransition {
    Reserved,
    Exposed,
    AbandonedReserved,
    ProviderTerminal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum LeaseTerminalKind {
    Finished,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LeaseTerminalRecord {
    admission_id: String,
    provider_request_ordinal: u32,
    provider_request_id: String,
    physical_host_id: String,
    model_instance_id: String,
    kind: LeaseTerminalKind,
}

impl LeaseTerminalRecord {
    fn from_receipt(
        terminal: &ProviderTerminalReceipt,
        claim: &ProviderLeaseClaim,
    ) -> Result<Self, ProviderLeaseError> {
        let record = Self {
            admission_id: terminal.admission_id.clone(),
            provider_request_ordinal: terminal.key.ordinal,
            provider_request_id: terminal.key.provider_request_id.clone(),
            physical_host_id: terminal.physical_host_id.clone(),
            model_instance_id: terminal.model_instance_id.clone(),
            kind: match terminal.kind {
                ProviderTerminalKind::Finished => LeaseTerminalKind::Finished,
                ProviderTerminalKind::Failed => LeaseTerminalKind::Failed,
                ProviderTerminalKind::Cancelled => LeaseTerminalKind::Cancelled,
            },
        };
        record.validate_claim(claim)?;
        Ok(record)
    }

    fn validate_claim(&self, claim: &ProviderLeaseClaim) -> Result<(), ProviderLeaseError> {
        if self.admission_id != claim.admission_id
            || self.provider_request_ordinal != claim.provider_request_ordinal
            || self.provider_request_id != claim.provider_request_id
            || self.physical_host_id != claim.physical_host_id
            || self.model_instance_id != claim.model_instance_id
        {
            return Err(ProviderLeaseError::ReceiptMismatch(
                "terminal does not match the exposed provider request".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WalMaterial {
    schema_version: u32,
    sequence: u64,
    previous_hash: String,
    transition: WalTransition,
    lease_id: String,
    owner_id: String,
    claim_digest: String,
    claim: Option<PersistedProviderLeaseClaim>,
    terminal: Option<LeaseTerminalRecord>,
    reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WalRecord {
    #[serde(flatten)]
    material: WalMaterial,
    entry_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CheckpointMaterial {
    schema_version: u32,
    next_sequence: u64,
    entry_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CheckpointRecord {
    #[serde(flatten)]
    material: CheckpointMaterial,
    checkpoint_hash: String,
}

#[derive(Clone, Debug)]
struct ActiveLease {
    owner_id: String,
    claim_digest: String,
    claim: ProviderLeaseClaim,
    status: ProviderLeaseStatus,
}

#[derive(Default)]
struct WalState {
    next_sequence: u64,
    previous_hash: String,
    seen_leases: HashSet<String>,
    seen_requests: HashMap<RequestIdentity, String>,
    active: HashMap<String, ActiveLease>,
}

impl WalState {
    fn genesis() -> Self {
        Self {
            previous_hash: GENESIS_HASH.to_string(),
            ..Self::default()
        }
    }

    fn apply(&mut self, material: &WalMaterial) -> Result<(), ProviderLeaseError> {
        if material.schema_version != WAL_SCHEMA_VERSION
            || material.sequence != self.next_sequence
            || material.previous_hash != self.previous_hash
        {
            return Err(ProviderLeaseError::CorruptWal(format!(
                "record {} breaks schema, sequence, or previous-hash continuity",
                material.sequence
            )));
        }
        validate_identifier("lease id", &material.lease_id)?;
        validate_identifier("owner id", &material.owner_id)?;
        if !is_canonical_digest(&material.claim_digest) {
            return Err(ProviderLeaseError::CorruptWal(format!(
                "record {} has an invalid claim digest",
                material.sequence
            )));
        }
        match material.transition {
            WalTransition::Reserved => {
                if material.terminal.is_some() || material.reason.is_some() {
                    return Err(ProviderLeaseError::CorruptWal(
                        "reservation carries terminal or reason material".to_string(),
                    ));
                }
                let claim = ProviderLeaseClaim::from(material.claim.clone().ok_or_else(|| {
                    ProviderLeaseError::CorruptWal("reservation has no claim".to_string())
                })?);
                claim.validate().map_err(|error| {
                    ProviderLeaseError::CorruptWal(format!("reservation claim is invalid: {error}"))
                })?;
                if claim.digest()? != material.claim_digest {
                    return Err(ProviderLeaseError::CorruptWal(
                        "reservation claim digest changed".to_string(),
                    ));
                }
                if !self.seen_leases.insert(material.lease_id.clone()) {
                    return Err(ProviderLeaseError::CorruptWal(
                        "lease id was reserved twice".to_string(),
                    ));
                }
                let request_identity = claim.request_identity();
                if self
                    .seen_requests
                    .insert(request_identity, material.lease_id.clone())
                    .is_some()
                {
                    return Err(ProviderLeaseError::CorruptWal(
                        "provider request identity was reserved twice".to_string(),
                    ));
                }
                self.active.insert(
                    material.lease_id.clone(),
                    ActiveLease {
                        owner_id: material.owner_id.clone(),
                        claim_digest: material.claim_digest.clone(),
                        claim,
                        status: ProviderLeaseStatus::Reserved,
                    },
                );
            }
            WalTransition::Exposed => {
                ensure_followup_shape(material, false, false)?;
                let active = self.active.get_mut(&material.lease_id).ok_or_else(|| {
                    ProviderLeaseError::CorruptWal("exposure has no reservation".to_string())
                })?;
                validate_active_material(active, material)?;
                if active.status != ProviderLeaseStatus::Reserved {
                    return Err(ProviderLeaseError::CorruptWal(
                        "lease was exposed twice".to_string(),
                    ));
                }
                active.status = ProviderLeaseStatus::Exposed;
            }
            WalTransition::AbandonedReserved => {
                ensure_followup_shape(material, false, true)?;
                let active = self.active.get(&material.lease_id).ok_or_else(|| {
                    ProviderLeaseError::CorruptWal("abandon has no reservation".to_string())
                })?;
                validate_active_material(active, material)?;
                if active.status != ProviderLeaseStatus::Reserved {
                    return Err(ProviderLeaseError::CorruptWal(
                        "an exposed lease was abandoned without a provider terminal".to_string(),
                    ));
                }
                validate_exact_text(
                    "reserved-abandon reason",
                    material.reason.as_deref().unwrap_or_default(),
                )
                .map_err(ProviderLeaseError::CorruptWal)?;
                self.active.remove(&material.lease_id);
            }
            WalTransition::ProviderTerminal => {
                ensure_followup_shape(material, true, false)?;
                let active = self.active.get(&material.lease_id).ok_or_else(|| {
                    ProviderLeaseError::CorruptWal("terminal has no exposed lease".to_string())
                })?;
                validate_active_material(active, material)?;
                if active.status != ProviderLeaseStatus::Exposed {
                    return Err(ProviderLeaseError::CorruptWal(
                        "terminal arrived before transport exposure".to_string(),
                    ));
                }
                material
                    .terminal
                    .as_ref()
                    .expect("terminal shape was checked")
                    .validate_claim(&active.claim)
                    .map_err(|error| ProviderLeaseError::CorruptWal(error.to_string()))?;
                self.active.remove(&material.lease_id);
            }
        }
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| ProviderLeaseError::CorruptWal("sequence overflowed".to_string()))?;
        Ok(())
    }

    fn validate_resource_binding(
        &self,
        claim: &ProviderLeaseClaim,
    ) -> Result<(), ProviderLeaseError> {
        for active in self
            .active
            .values()
            .filter(|active| active.claim.physical_host_id == claim.physical_host_id)
        {
            if active.claim.host_capacity_evidence != claim.host_capacity_evidence {
                return Err(ProviderLeaseError::ConflictingEvidence(format!(
                    "host `{}` has different live capacity evidence",
                    claim.physical_host_id
                )));
            }
            if active.claim.model_instance_id == claim.model_instance_id
                && (active.claim.advertised_instance_capacity != claim.advertised_instance_capacity
                    || active.claim.model_id != claim.model_id
                    || active.claim.provider_transport_id != claim.provider_transport_id
                    || active.claim.provider_protocol_id != claim.provider_protocol_id
                    || active.claim.route_evidence_id != claim.route_evidence_id)
            {
                return Err(ProviderLeaseError::ConflictingEvidence(format!(
                    "model instance `{}` on host `{}` has different live route evidence",
                    claim.model_instance_id, claim.physical_host_id
                )));
            }
        }
        Ok(())
    }

    fn validate_token(
        &self,
        lease_id: &str,
        owner_id: &str,
        claim_digest: &str,
        expected_status: ProviderLeaseStatus,
    ) -> Result<(), ProviderLeaseError> {
        let active = self.active.get(lease_id).ok_or_else(|| {
            ProviderLeaseError::InvalidTransition("lease is not active".to_string())
        })?;
        if active.owner_id != owner_id || active.claim_digest != claim_digest {
            return Err(ProviderLeaseError::ReceiptMismatch(
                "lease token changed owner or claim digest".to_string(),
            ));
        }
        if active.status != expected_status {
            return Err(ProviderLeaseError::InvalidTransition(format!(
                "lease is {:?}, expected {expected_status:?}",
                active.status
            )));
        }
        Ok(())
    }

    fn host_occupancy(&self, host_id: &str) -> u32 {
        self.active
            .values()
            .filter(|active| active.claim.physical_host_id == host_id)
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    fn instance_occupancy(&self, host_id: &str, instance_id: &str) -> u32 {
        self.active
            .values()
            .filter(|active| {
                active.claim.physical_host_id == host_id
                    && active.claim.model_instance_id == instance_id
            })
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    fn snapshot(&self) -> ProviderLeaseAuthoritySnapshot {
        let mut active: Vec<_> = self
            .active
            .iter()
            .map(|(lease_id, active)| ActiveProviderLeaseSnapshot {
                lease_id: lease_id.clone(),
                owner_id: active.owner_id.clone(),
                claim_digest: active.claim_digest.clone(),
                claim: active.claim.clone(),
                status: active.status,
            })
            .collect();
        active.sort_by(|left, right| left.lease_id.cmp(&right.lease_id));
        ProviderLeaseAuthoritySnapshot {
            next_sequence: self.next_sequence,
            active,
        }
    }
}

fn ensure_followup_shape(
    material: &WalMaterial,
    terminal_required: bool,
    reason_required: bool,
) -> Result<(), ProviderLeaseError> {
    if material.claim.is_some()
        || material.terminal.is_some() != terminal_required
        || material.reason.is_some() != reason_required
    {
        return Err(ProviderLeaseError::CorruptWal(format!(
            "record {} has invalid transition material",
            material.sequence
        )));
    }
    Ok(())
}

fn validate_active_material(
    active: &ActiveLease,
    material: &WalMaterial,
) -> Result<(), ProviderLeaseError> {
    if active.owner_id != material.owner_id || active.claim_digest != material.claim_digest {
        return Err(ProviderLeaseError::CorruptWal(
            "follow-up changed lease owner or claim digest".to_string(),
        ));
    }
    Ok(())
}

fn append_material(
    wal_file: &mut File,
    checkpoint_path: &Path,
    wal: &mut WalState,
    material: WalMaterial,
) -> Result<(), ProviderLeaseError> {
    let entry_hash = hash_serializable(&material).map_err(ProviderLeaseError::CorruptWal)?;
    let record = WalRecord {
        material,
        entry_hash: entry_hash.clone(),
    };
    let mut encoded = serde_json::to_vec(&record)
        .map_err(|error| ProviderLeaseError::CorruptWal(error.to_string()))?;
    encoded.push(b'\n');
    wal_file
        .write_all(&encoded)
        .map_err(|error| ProviderLeaseError::Io(format!("cannot append WAL: {error}")))?;
    wal_file
        .sync_all()
        .map_err(|error| ProviderLeaseError::Io(format!("cannot sync WAL: {error}")))?;
    checkpoint_kill_point("after_wal_sync");
    wal.apply(&record.material)?;
    wal.previous_hash = entry_hash;
    write_checkpoint_atomic(checkpoint_path, wal)?;
    Ok(())
}

fn replay_wal(wal_file: &mut File, checkpoint_path: &Path) -> Result<WalState, ProviderLeaseError> {
    let checkpoint = read_checkpoint_path(checkpoint_path)?;
    wal_file
        .seek(SeekFrom::Start(0))
        .map_err(|error| ProviderLeaseError::Io(format!("cannot seek WAL: {error}")))?;
    let mut bytes = Vec::new();
    wal_file
        .read_to_end(&mut bytes)
        .map_err(|error| ProviderLeaseError::Io(format!("cannot read WAL: {error}")))?;
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(ProviderLeaseError::CorruptWal(
            "final record is torn".to_string(),
        ));
    }
    let mut wal = WalState::genesis();
    let mut checkpoint_matches_prefix =
        checkpoint.next_sequence == 0 && checkpoint.entry_hash == GENESIS_HASH;
    let content = bytes.strip_suffix(b"\n").unwrap_or(bytes.as_slice());
    if !bytes.is_empty() && content.is_empty() {
        return Err(ProviderLeaseError::CorruptWal(
            "record 1 is blank".to_string(),
        ));
    }
    if !content.is_empty() {
        for (index, line) in content.split(|byte| *byte == b'\n').enumerate() {
            if line.is_empty() {
                return Err(ProviderLeaseError::CorruptWal(format!(
                    "record {} is blank",
                    index + 1
                )));
            }
            let record: WalRecord = serde_json::from_slice(line).map_err(|error| {
                ProviderLeaseError::CorruptWal(format!("record {} is invalid: {error}", index + 1))
            })?;
            let expected_hash =
                hash_serializable(&record.material).map_err(ProviderLeaseError::CorruptWal)?;
            if expected_hash != record.entry_hash {
                return Err(ProviderLeaseError::CorruptWal(format!(
                    "record {} hash changed",
                    index + 1
                )));
            }
            wal.apply(&record.material)?;
            wal.previous_hash = record.entry_hash;
            if wal.next_sequence == checkpoint.next_sequence
                && wal.previous_hash == checkpoint.entry_hash
            {
                checkpoint_matches_prefix = true;
            }
        }
    }
    if !checkpoint_matches_prefix || checkpoint.next_sequence > wal.next_sequence {
        return Err(ProviderLeaseError::CorruptWal(
            "WAL was rolled back or diverged from its durable checkpoint".to_string(),
        ));
    }
    if checkpoint.next_sequence != wal.next_sequence || checkpoint.entry_hash != wal.previous_hash {
        write_checkpoint_atomic(checkpoint_path, &wal)?;
    }
    wal_file
        .seek(SeekFrom::End(0))
        .map_err(|error| ProviderLeaseError::Io(format!("cannot seek WAL end: {error}")))?;
    Ok(wal)
}

fn read_checkpoint_path(path: &Path) -> Result<CheckpointMaterial, ProviderLeaseError> {
    let mut file = open_secure_file(path, false, FileAccess::Rewrite)?;
    verify_secure_file(path, &file)?;
    read_checkpoint(&mut file)
}

fn read_checkpoint(file: &mut File) -> Result<CheckpointMaterial, ProviderLeaseError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| ProviderLeaseError::Io(format!("cannot seek checkpoint: {error}")))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| ProviderLeaseError::Io(format!("cannot read checkpoint: {error}")))?;
    if !bytes.ends_with(b"\n") || bytes[..bytes.len().saturating_sub(1)].contains(&b'\n') {
        return Err(ProviderLeaseError::CorruptWal(
            "checkpoint is empty, torn, or has multiple records".to_string(),
        ));
    }
    let record: CheckpointRecord = serde_json::from_slice(&bytes[..bytes.len() - 1])
        .map_err(|error| ProviderLeaseError::CorruptWal(format!("invalid checkpoint: {error}")))?;
    let expected_hash =
        hash_serializable(&record.material).map_err(ProviderLeaseError::CorruptWal)?;
    if expected_hash != record.checkpoint_hash
        || record.material.schema_version != WAL_SCHEMA_VERSION
        || (record.material.next_sequence == 0 && record.material.entry_hash != GENESIS_HASH)
        || (record.material.next_sequence > 0 && !is_canonical_digest(&record.material.entry_hash))
    {
        return Err(ProviderLeaseError::CorruptWal(
            "checkpoint hash, schema, sequence, or head is invalid".to_string(),
        ));
    }
    Ok(record.material)
}

fn write_checkpoint_atomic(path: &Path, wal: &WalState) -> Result<(), ProviderLeaseError> {
    let material = CheckpointMaterial {
        schema_version: WAL_SCHEMA_VERSION,
        next_sequence: wal.next_sequence,
        entry_hash: wal.previous_hash.clone(),
    };
    let checkpoint_hash = hash_serializable(&material).map_err(ProviderLeaseError::CorruptWal)?;
    let record = CheckpointRecord {
        material,
        checkpoint_hash,
    };
    let mut encoded = serde_json::to_vec(&record)
        .map_err(|error| ProviderLeaseError::CorruptWal(error.to_string()))?;
    encoded.push(b'\n');
    let parent = path.parent().ok_or_else(|| {
        ProviderLeaseError::UnsafeControlPath("checkpoint has no parent".to_string())
    })?;
    let temporary = parent.join(format!(
        ".{CHECKPOINT_FILE_NAME}.{}.tmp",
        random_id("replace")
    ));
    let mut file = open_secure_file(&temporary, true, FileAccess::Rewrite)?;
    file.write_all(&encoded)
        .map_err(|error| ProviderLeaseError::Io(format!("cannot write checkpoint: {error}")))?;
    file.sync_all()
        .map_err(|error| ProviderLeaseError::Io(format!("cannot sync checkpoint: {error}")))?;
    checkpoint_kill_point("after_checkpoint_temp_sync");
    std::fs::rename(&temporary, path).map_err(|error| {
        ProviderLeaseError::Io(format!("cannot atomically replace checkpoint: {error}"))
    })?;
    checkpoint_kill_point("after_checkpoint_rename");
    sync_directory(parent)?;
    checkpoint_kill_point("after_checkpoint_directory_sync");
    let installed = open_secure_file(path, false, FileAccess::Rewrite)?;
    verify_secure_file(path, &installed)?;
    Ok(())
}

fn cleanup_checkpoint_temps(root: &Path) -> Result<(), ProviderLeaseError> {
    let prefix = format!(".{CHECKPOINT_FILE_NAME}.");
    let mut removed = false;
    for entry in std::fs::read_dir(root).map_err(|error| {
        ProviderLeaseError::Io(format!("cannot inspect checkpoint directory: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            ProviderLeaseError::Io(format!("cannot inspect checkpoint debris: {error}"))
        })?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with(&prefix) || !name.ends_with(".tmp") {
            continue;
        }
        let path = entry.path();
        let file = open_secure_file(&path, false, FileAccess::Rewrite)?;
        verify_secure_file(&path, &file)?;
        std::fs::remove_file(&path).map_err(|error| {
            ProviderLeaseError::Io(format!("cannot remove checkpoint debris: {error}"))
        })?;
        removed = true;
    }
    if removed {
        sync_directory(root)?;
    }
    Ok(())
}

fn checkpoint_kill_point(name: &str) {
    #[cfg(test)]
    if std::env::var(CHECKPOINT_KILL_POINT_ENV).as_deref() == Ok(name) {
        unsafe {
            libc::kill(libc::getpid(), libc::SIGKILL);
        }
    }
    #[cfg(not(test))]
    let _ = name;
}

fn validate_exact_text(name: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{name} is empty"));
    }
    if value.trim() != value {
        return Err(format!("{name} has surrounding whitespace"));
    }
    Ok(())
}

fn validate_identifier(name: &str, value: &str) -> Result<(), ProviderLeaseError> {
    let Some(digest) = value.split_once(':').map(|(_, digest)| digest) else {
        return Err(ProviderLeaseError::CorruptWal(format!(
            "{name} is not namespaced"
        )));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProviderLeaseError::CorruptWal(format!(
            "{name} is not canonical"
        )));
    }
    Ok(())
}

fn is_canonical_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn hash_serializable(value: &impl Serialize) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    Ok(encode_digest(Sha256::digest(bytes)))
}

fn encode_digest(bytes: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in bytes.as_ref() {
        write!(&mut encoded, "{byte:02x}").expect("writing to a string cannot fail");
    }
    encoded
}

fn random_id(namespace: &str) -> String {
    use std::fmt::Write as _;

    let random: [u8; 32] = rand::random();
    let mut encoded = String::with_capacity(namespace.len() + 65);
    encoded.push_str(namespace);
    encoded.push(':');
    for byte in random {
        write!(&mut encoded, "{byte:02x}").expect("writing to a string cannot fail");
    }
    encoded
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Copy)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

#[derive(Clone, Copy)]
enum FileAccess {
    Append,
    Rewrite,
}

#[cfg(unix)]
fn fixed_control_root() -> Result<PathBuf, ProviderLeaseError> {
    let home = effective_user_home()?;
    #[cfg(target_os = "macos")]
    let state = home.join("Library/Application Support/Block/goose");
    #[cfg(not(target_os = "macos"))]
    let state = home.join(".local/state/goose");
    Ok(state.join(AUTHORITY_DIRECTORY_NAME))
}

#[cfg(unix)]
fn effective_user_home() -> Result<PathBuf, ProviderLeaseError> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;

    let effective_uid = unsafe { libc::geteuid() };
    let mut buffer = vec![0_u8; 1024];
    loop {
        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                effective_uid,
                record.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE {
            let next = buffer.len().checked_mul(2).ok_or_else(|| {
                ProviderLeaseError::UnsafeControlPath(
                    "effective-user record is too large".to_string(),
                )
            })?;
            buffer.resize(next, 0);
            continue;
        }
        if status != 0 {
            return Err(ProviderLeaseError::UnsafeControlPath(format!(
                "cannot resolve effective-user home: OS error {status}"
            )));
        }
        if result.is_null() {
            return Err(ProviderLeaseError::UnsafeControlPath(
                "effective user has no passwd record".to_string(),
            ));
        }
        let record = unsafe { record.assume_init() };
        if record.pw_dir.is_null() {
            return Err(ProviderLeaseError::UnsafeControlPath(
                "effective user has no home directory".to_string(),
            ));
        }
        let bytes = unsafe { CStr::from_ptr(record.pw_dir) }.to_bytes();
        if bytes.is_empty() {
            return Err(ProviderLeaseError::UnsafeControlPath(
                "effective user has an empty home directory".to_string(),
            ));
        }
        let home = PathBuf::from(OsStr::from_bytes(bytes));
        if !home.is_absolute() {
            return Err(ProviderLeaseError::UnsafeControlPath(
                "effective-user home is not absolute".to_string(),
            ));
        }
        return Ok(home);
    }
}

#[cfg(unix)]
fn normalize_control_root(path: &Path) -> Result<PathBuf, ProviderLeaseError> {
    let name = path.file_name().ok_or_else(|| {
        ProviderLeaseError::UnsafeControlPath("control root has no final component".to_string())
    })?;
    let mut existing = path.parent().ok_or_else(|| {
        ProviderLeaseError::UnsafeControlPath("control root has no parent".to_string())
    })?;
    let mut missing = vec![name.to_os_string()];
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            ProviderLeaseError::UnsafeControlPath(
                "control root has no existing ancestor".to_string(),
            )
        })?;
        missing.push(name.to_os_string());
        existing = existing.parent().ok_or_else(|| {
            ProviderLeaseError::UnsafeControlPath(
                "control root has no existing ancestor".to_string(),
            )
        })?;
    }
    let mut normalized = existing.canonicalize().map_err(|error| {
        ProviderLeaseError::UnsafeControlPath(format!(
            "cannot resolve control-root ancestors: {error}"
        ))
    })?;
    for component in missing.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

#[cfg(unix)]
fn initialization_lock_path(root: &Path) -> Result<PathBuf, ProviderLeaseError> {
    let name = root.file_name().ok_or_else(|| {
        ProviderLeaseError::UnsafeControlPath("control root has no final component".to_string())
    })?;
    let mut lock_name = name.to_os_string();
    lock_name.push(INITIALIZATION_LOCK_SUFFIX);
    Ok(root.with_file_name(lock_name))
}

#[cfg(unix)]
fn secure_control_root(
    path: &Path,
    allow_initialize: bool,
) -> Result<(FileIdentity, bool), ProviderLeaseError> {
    use std::os::unix::fs::PermissionsExt;

    verify_no_symlink_components(path)?;
    let created;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(ProviderLeaseError::UnsafeControlPath(
                    "control root is not a real directory".to_string(),
                ));
            }
            created = false;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !allow_initialize {
                return Err(ProviderLeaseError::UnsafeControlPath(
                    "initialized control root is missing".to_string(),
                ));
            }
            let parent = path.parent().ok_or_else(|| {
                ProviderLeaseError::UnsafeControlPath("control root has no parent".to_string())
            })?;
            std::fs::create_dir_all(parent).map_err(|error| {
                ProviderLeaseError::Io(format!("cannot create control-root parent: {error}"))
            })?;
            match std::fs::create_dir(path) {
                Ok(()) => {
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                        .map_err(|error| {
                            ProviderLeaseError::Io(format!("cannot secure control root: {error}"))
                        })?;
                    sync_directory(parent)?;
                    created = true;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    created = false;
                }
                Err(error) => {
                    return Err(ProviderLeaseError::Io(format!(
                        "cannot create control root: {error}"
                    )));
                }
            }
        }
        Err(error) => {
            return Err(ProviderLeaseError::Io(format!(
                "cannot inspect control root: {error}"
            )));
        }
    }
    verify_no_symlink_components(path)?;
    let identity = verify_identity(path, file_identity(path)?, true)?;
    Ok((identity, created))
}

#[cfg(unix)]
fn open_or_create_secure_file(
    path: &Path,
    access: FileAccess,
) -> Result<(File, bool), ProviderLeaseError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut create = OpenOptions::new();
    create.read(true).create_new(true).mode(0o600);
    match access {
        FileAccess::Append => {
            create.append(true);
        }
        FileAccess::Rewrite => {
            create.write(true);
        }
    }
    create.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    match create.open(path) {
        Ok(file) => {
            file.sync_all().map_err(|error| {
                ProviderLeaseError::Io(format!("cannot sync new control file: {error}"))
            })?;
            if let Some(parent) = path.parent() {
                sync_directory(parent)?;
            }
            Ok((file, true))
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            open_secure_file(path, false, access).map(|file| (file, false))
        }
        Err(error) => Err(ProviderLeaseError::Io(format!(
            "cannot create control file: {error}"
        ))),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InitializationState {
    Ready,
    Recover,
}

#[cfg(unix)]
fn read_initialization_state(mut file: &File) -> Result<InitializationState, ProviderLeaseError> {
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        ProviderLeaseError::Io(format!("cannot seek initialization marker: {error}"))
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        ProviderLeaseError::Io(format!("cannot read initialization marker: {error}"))
    })?;
    if bytes == INITIALIZATION_READY {
        return Ok(InitializationState::Ready);
    }
    if INITIALIZATION_READY.starts_with(&bytes) {
        return Ok(InitializationState::Recover);
    }
    Err(ProviderLeaseError::UnsafeControlPath(
        "authority initialization marker is corrupt".to_string(),
    ))
}

#[cfg(not(unix))]
fn read_initialization_state(_file: &File) -> Result<InitializationState, ProviderLeaseError> {
    Err(ProviderLeaseError::UnsupportedPlatform)
}

#[cfg(unix)]
fn write_initialization_ready(mut file: &File) -> Result<(), ProviderLeaseError> {
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        ProviderLeaseError::Io(format!("cannot seek initialization marker: {error}"))
    })?;
    file.set_len(0).map_err(|error| {
        ProviderLeaseError::Io(format!("cannot truncate initialization marker: {error}"))
    })?;
    file.write_all(INITIALIZATION_READY).map_err(|error| {
        ProviderLeaseError::Io(format!("cannot write initialization marker: {error}"))
    })?;
    file.sync_all().map_err(|error| {
        ProviderLeaseError::Io(format!("cannot sync initialization marker: {error}"))
    })
}

#[cfg(unix)]
fn open_secure_file(
    path: &Path,
    create: bool,
    access: FileAccess,
) -> Result<File, ProviderLeaseError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options.read(true);
    match access {
        FileAccess::Append => {
            options.append(true);
        }
        FileAccess::Rewrite => {
            options.write(true);
        }
    }
    if create {
        options.create_new(true).mode(0o600);
    }
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path).map_err(|error| {
        if create {
            ProviderLeaseError::Io(format!("cannot create control file: {error}"))
        } else {
            ProviderLeaseError::UnsafeControlPath(format!(
                "cannot securely open existing control file: {error}"
            ))
        }
    })?;
    if create {
        file.sync_all().map_err(|error| {
            ProviderLeaseError::Io(format!("cannot sync new control file: {error}"))
        })?;
    }
    Ok(file)
}

#[cfg(unix)]
fn verify_no_symlink_components(path: &Path) -> Result<(), ProviderLeaseError> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component.as_os_str());
        match std::fs::symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ProviderLeaseError::UnsafeControlPath(format!(
                    "control path contains symlink component `{}`",
                    prefix.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(ProviderLeaseError::UnsafeControlPath(format!(
                    "cannot inspect control path component `{}`: {error}",
                    prefix.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_no_symlink_components(_path: &Path) -> Result<(), ProviderLeaseError> {
    Err(ProviderLeaseError::UnsupportedPlatform)
}

#[cfg(unix)]
fn verify_secure_file(path: &Path, file: &File) -> Result<FileIdentity, ProviderLeaseError> {
    use std::os::unix::fs::MetadataExt;

    let open = file
        .metadata()
        .map_err(|error| ProviderLeaseError::Io(format!("cannot inspect control file: {error}")))?;
    let linked = std::fs::symlink_metadata(path).map_err(|error| {
        ProviderLeaseError::UnsafeControlPath(format!("cannot inspect control path: {error}"))
    })?;
    if !open.is_file()
        || !linked.is_file()
        || open.uid() != unsafe { libc::geteuid() }
        || linked.uid() != unsafe { libc::geteuid() }
        || open.mode() & 0o7777 != 0o600
        || linked.mode() & 0o7777 != 0o600
        || open.nlink() != 1
        || linked.nlink() != 1
        || open.dev() != linked.dev()
        || open.ino() != linked.ino()
    {
        return Err(ProviderLeaseError::UnsafeControlPath(
            "control file identity, owner, permissions, or link count is unsafe".to_string(),
        ));
    }
    Ok(FileIdentity {
        device: open.dev(),
        inode: open.ino(),
    })
}

#[cfg(unix)]
fn verify_expected_file(
    path: &Path,
    file: &File,
    expected: FileIdentity,
) -> Result<(), ProviderLeaseError> {
    let observed = verify_secure_file(path, file)?;
    if observed.device != expected.device || observed.inode != expected.inode {
        return Err(ProviderLeaseError::UnsafeControlPath(
            "control file was replaced".to_string(),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_expected_file(
    _path: &Path,
    _file: &File,
    _expected: FileIdentity,
) -> Result<(), ProviderLeaseError> {
    Err(ProviderLeaseError::UnsupportedPlatform)
}

fn file_identity(path: &Path) -> Result<FileIdentity, ProviderLeaseError> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        ProviderLeaseError::UnsafeControlPath(format!("cannot inspect path: {error}"))
    })?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(unix)]
fn verify_identity(
    path: &Path,
    expected: FileIdentity,
    directory: bool,
) -> Result<FileIdentity, ProviderLeaseError> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        ProviderLeaseError::UnsafeControlPath(format!("cannot inspect path: {error}"))
    })?;
    if metadata.file_type().is_symlink()
        || metadata.is_dir() != directory
        || metadata.uid() != unsafe { libc::geteuid() }
        || (directory && metadata.mode() & 0o7777 != 0o700)
        || metadata.dev() != expected.device
        || metadata.ino() != expected.inode
    {
        return Err(ProviderLeaseError::UnsafeControlPath(
            "control path identity, owner, or permissions changed".to_string(),
        ));
    }
    Ok(expected)
}

#[cfg(not(unix))]
fn verify_identity(
    _path: &Path,
    _expected: FileIdentity,
    _directory: bool,
) -> Result<FileIdentity, ProviderLeaseError> {
    Err(ProviderLeaseError::UnsupportedPlatform)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), ProviderLeaseError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| ProviderLeaseError::Io(format!("cannot sync control directory: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::{
        AuthorityScope, ProviderRequestKey, SourceRevisionKind, WorkPriority, WorkRole,
    };
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};

    const TRANSPORT: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const CHILD_MODE_ENV: &str = "GOOSE_PROVIDER_LEASE_TEST_CHILD_MODE";
    const CHILD_ROOT_ENV: &str = "GOOSE_PROVIDER_LEASE_TEST_ROOT";
    const CHILD_RESULT_ENV: &str = "GOOSE_PROVIDER_LEASE_TEST_RESULT";
    const CHILD_START_ENV: &str = "GOOSE_PROVIDER_LEASE_TEST_START";
    const CHILD_REQUEST_ENV: &str = "GOOSE_PROVIDER_LEASE_TEST_REQUEST";
    const CHILD_CAPACITY_ENV: &str = "GOOSE_PROVIDER_LEASE_TEST_CAPACITY";

    fn control_root() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("authority");
        (temp, root)
    }

    fn open_authority(root: &Path) -> GlobalProviderLeaseAuthority {
        loop {
            match GlobalProviderLeaseAuthority::open_test_root(root) {
                Ok(authority) => return authority,
                Err(ProviderLeaseError::AuthorityContended) => std::thread::yield_now(),
                Err(error) => panic!("cannot open test authority: {error}"),
            }
        }
    }

    fn claim(
        request_id: &str,
        host: &str,
        instance: &str,
        host_capacity: u32,
        instance_capacity: u32,
    ) -> ProviderLeaseClaim {
        let source = TaskVersion {
            authority_scope: AuthorityScope::new("run-a", "build"),
            phase_epoch: 0,
            task_id: format!("task-{request_id}"),
            attempt: 0,
            revision: 1,
            kind: SourceRevisionKind::TaskAttempt,
        };
        let capacity_evidence = HostCapacityEvidence::MeasuredProfile {
            profile_hash: format!("profile-{host}-{host_capacity}"),
            profile_key: format!("runtime-{host}-{host_capacity}"),
            max_concurrent: host_capacity,
        };
        let admission = AdmissionReceipt {
            admission_id: format!("admission-{request_id}"),
            work_id: format!("work-{request_id}"),
            role: WorkRole::Build,
            priority: WorkPriority::Implementation,
            task_rank: 1,
            source,
            fleet_snapshot_id: "fleet-a".to_string(),
            logical_device_id: format!("device-{instance}"),
            model_id: format!("model-{instance}"),
            physical_host_id: host.to_string(),
            model_instance_id: instance.to_string(),
            provider_transport_id: TRANSPORT.to_string(),
            route_evidence_id: format!("route-{host}-{instance}"),
            capacity_evidence: capacity_evidence.clone(),
            queue_sequence: 1,
            admission_sequence: 1,
        };
        let request = ProviderRequestReceipt {
            admission_id: admission.admission_id.clone(),
            key: ProviderRequestKey {
                ordinal: 0,
                provider_request_id: request_id.to_string(),
            },
            physical_host_id: host.to_string(),
            model_instance_id: instance.to_string(),
        };
        let fleet = PhysicalFleetSnapshot::new(
            "fleet-a",
            vec![VerifiedPhysicalLane {
                logical_device_id: admission.logical_device_id.clone(),
                model_id: admission.model_id.clone(),
                host_id: host.to_string(),
                model_instance_id: instance.to_string(),
                provider_transport_id: TRANSPORT.to_string(),
                advertised_instance_capacity: instance_capacity,
                routing_weight: 1,
                capacity_evidence,
                route_evidence_id: admission.route_evidence_id.clone(),
            }],
        )
        .unwrap();
        let authority = SealedProviderLeaseAuthority::from_fleet_snapshot(
            &fleet,
            [VerifiedProviderProtocolRoute::new(
                TRANSPORT,
                ProviderHttpProtocol::OpenAiChatCompletions,
            )
            .unwrap()],
        )
        .unwrap();
        ProviderLeaseClaim::from_authority(&authority, &admission, &request).unwrap()
    }

    fn reserve(
        authority: &GlobalProviderLeaseAuthority,
        claim: ProviderLeaseClaim,
    ) -> ProviderLeaseTry {
        loop {
            match authority.try_reserve(claim.clone()).unwrap() {
                ProviderLeaseTry::Busy(busy)
                    if busy.kind == ProviderLeaseBusyKind::AuthorityLock =>
                {
                    std::thread::yield_now();
                }
                result => return result,
            }
        }
    }

    fn acquired(result: ProviderLeaseTry) -> ReservedProviderLease {
        match result {
            ProviderLeaseTry::Acquired(lease) => lease,
            ProviderLeaseTry::Busy(busy) => panic!("expected lease, got {busy:?}"),
        }
    }

    fn terminal(exposed: &ExposedProviderLease) -> ProviderTerminalReceipt {
        ProviderTerminalReceipt {
            admission_id: exposed.claim.admission_id.clone(),
            key: ProviderRequestKey {
                ordinal: exposed.claim.provider_request_ordinal,
                provider_request_id: exposed.claim.provider_request_id.clone(),
            },
            physical_host_id: exposed.claim.physical_host_id.clone(),
            model_instance_id: exposed.claim.model_instance_id.clone(),
            kind: ProviderTerminalKind::Finished,
        }
    }

    fn duplicate_reserved_for_adversarial_replay(
        reserved: &ReservedProviderLease,
    ) -> ReservedProviderLease {
        ReservedProviderLease {
            lease_id: reserved.lease_id.clone(),
            owner_id: reserved.owner_id.clone(),
            claim_digest: reserved.claim_digest.clone(),
            claim: reserved.claim.clone(),
            reservation_sequence: reserved.reservation_sequence,
        }
    }

    fn duplicate_exposed_for_adversarial_replay(
        exposed: &ExposedProviderLease,
    ) -> ExposedProviderLease {
        ExposedProviderLease {
            lease_id: exposed.lease_id.clone(),
            owner_id: exposed.owner_id.clone(),
            claim_digest: exposed.claim_digest.clone(),
            claim: exposed.claim.clone(),
            reservation_sequence: exposed.reservation_sequence,
            exposure_sequence: exposed.exposure_sequence,
        }
    }

    fn wait_for_path(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            assert!(Instant::now() < deadline, "timed out waiting for test path");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn provider_lease_child_process() {
        let Ok(mode) = std::env::var(CHILD_MODE_ENV) else {
            return;
        };
        let root = PathBuf::from(std::env::var_os(CHILD_ROOT_ENV).unwrap());
        let result_path = PathBuf::from(std::env::var_os(CHILD_RESULT_ENV).unwrap());
        if mode == "hold_control_lock" {
            let release = PathBuf::from(std::env::var_os(CHILD_START_ENV).unwrap());
            let lock_path = root.join(LOCK_FILE_NAME);
            let lock_file = open_secure_file(&lock_path, false, FileAccess::Rewrite).unwrap();
            verify_secure_file(&lock_path, &lock_file).unwrap();
            FileExt::lock_exclusive(&lock_file).unwrap();
            std::fs::write(result_path, "locked").unwrap();
            wait_for_path(&release);
            FileExt::unlock(&lock_file).unwrap();
            return;
        }
        let request_id = std::env::var(CHILD_REQUEST_ENV).unwrap();
        let capacity: u32 = std::env::var(CHILD_CAPACITY_ENV).unwrap().parse().unwrap();
        if let Some(start) = std::env::var_os(CHILD_START_ENV) {
            wait_for_path(Path::new(&start));
        }
        let authority = open_authority(&root);
        let result = reserve(
            &authority,
            claim(
                &request_id,
                "host-child",
                "instance-child",
                capacity,
                capacity,
            ),
        );
        match mode.as_str() {
            "reserve" => {
                let outcome = match result {
                    ProviderLeaseTry::Acquired(_) => "acquired",
                    ProviderLeaseTry::Busy(ProviderLeaseBusy {
                        kind: ProviderLeaseBusyKind::HostCapacity,
                        ..
                    }) => "busy",
                    other => panic!("unexpected child reservation result: {other:?}"),
                };
                std::fs::write(result_path, outcome).unwrap();
            }
            "expose_and_wait" => {
                let reserved = acquired(result);
                let _exposed = authority.expose(reserved).unwrap();
                std::fs::write(result_path, "exposed").unwrap();
                loop {
                    std::thread::park_timeout(Duration::from_secs(60));
                }
            }
            other => panic!("unknown provider-lease child mode `{other}`"),
        }
    }

    fn cross_process_race(capacity: u32, contenders: usize) -> Vec<String> {
        let (_temp, root) = control_root();
        drop(open_authority(&root));
        let test_space = root.parent().unwrap();
        let start = test_space.join("start");
        let executable = std::env::current_exe().unwrap();
        let mut children = Vec::new();
        let mut results = Vec::new();
        for index in 0..contenders {
            let cwd = test_space.join(format!("cwd-{index}"));
            std::fs::create_dir(&cwd).unwrap();
            let result = test_space.join(format!("result-{index}"));
            let child = Command::new(&executable)
                .arg("--exact")
                .arg("provider_lease::tests::provider_lease_child_process")
                .arg("--nocapture")
                .env(CHILD_MODE_ENV, "reserve")
                .env(CHILD_ROOT_ENV, &root)
                .env(CHILD_RESULT_ENV, &result)
                .env(CHILD_START_ENV, &start)
                .env(CHILD_REQUEST_ENV, format!("child-{index}"))
                .env(CHILD_CAPACITY_ENV, capacity.to_string())
                .current_dir(cwd)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            children.push(child);
            results.push(result);
        }
        std::fs::write(&start, "start").unwrap();
        for child in children {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "child process failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        results
            .into_iter()
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect()
    }

    #[test]
    fn cap_one_race_grants_exactly_one_across_authority_instances() {
        let (_temp, root) = control_root();
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = ["one", "two"]
            .into_iter()
            .map(|request_id| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let authority = open_authority(&root);
                    barrier.wait();
                    reserve(&authority, claim(request_id, "host-a", "instance-a", 1, 1))
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, ProviderLeaseTry::Acquired(_)))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    ProviderLeaseTry::Busy(ProviderLeaseBusy {
                        kind: ProviderLeaseBusyKind::HostCapacity,
                        ..
                    })
                ))
                .count(),
            1
        );
    }

    #[test]
    fn cap_one_race_is_global_across_processes_and_working_directories() {
        let outcomes = cross_process_race(1, 4);
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| *outcome == "acquired")
                .count(),
            1
        );
        assert_eq!(
            outcomes.iter().filter(|outcome| *outcome == "busy").count(),
            3
        );
    }

    #[test]
    fn cap_n_race_never_grants_more_than_the_global_capacity() {
        let outcomes = cross_process_race(3, 8);
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| *outcome == "acquired")
                .count(),
            3
        );
        assert_eq!(
            outcomes.iter().filter(|outcome| *outcome == "busy").count(),
            5
        );
    }

    #[test]
    fn host_and_instance_caps_are_both_global() {
        let (_temp, root) = control_root();
        let first = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        let second = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        let a = acquired(reserve(&first, claim("a", "host-a", "instance-a", 2, 1)));
        let b = acquired(reserve(&second, claim("b", "host-a", "instance-b", 2, 1)));
        assert!(matches!(
            reserve(&first, claim("c", "host-a", "instance-a", 2, 1)),
            ProviderLeaseTry::Busy(ProviderLeaseBusy {
                kind: ProviderLeaseBusyKind::HostCapacity,
                permits_held: 2,
                capacity: 2,
                ..
            })
        ));
        first.abandon_reserved(a, "not exposed").unwrap();
        assert!(matches!(
            reserve(&first, claim("d", "host-a", "instance-b", 2, 1)),
            ProviderLeaseTry::Busy(ProviderLeaseBusy {
                kind: ProviderLeaseBusyKind::InstanceCapacity,
                permits_held: 1,
                capacity: 1,
                ..
            })
        ));
        second.abandon_reserved(b, "not exposed").unwrap();
    }

    #[test]
    fn exposed_handle_drop_and_authority_reopen_do_not_release_capacity() {
        let (_temp, root) = control_root();
        let first = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        let reserved = acquired(reserve(
            &first,
            claim("first", "host-a", "instance-a", 1, 1),
        ));
        let exposed = first.expose(reserved).unwrap();
        drop(exposed);
        drop(first);

        let reopened = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        let snapshot = reopened.snapshot().unwrap();
        assert_eq!(snapshot.active.len(), 1);
        assert_eq!(snapshot.active[0].status, ProviderLeaseStatus::Exposed);
        assert!(matches!(
            reserve(&reopened, claim("second", "host-a", "instance-a", 1, 1)),
            ProviderLeaseTry::Busy(ProviderLeaseBusy {
                kind: ProviderLeaseBusyKind::HostCapacity,
                ..
            })
        ));
    }

    #[test]
    fn dropping_a_reserved_handle_does_not_implicitly_abandon_it() {
        let (_temp, root) = control_root();
        let authority = open_authority(&root);
        let reserved = acquired(reserve(
            &authority,
            claim("reserved-drop", "host-a", "instance-a", 1, 1),
        ));
        drop(reserved);
        let snapshot = authority.snapshot().unwrap();
        assert_eq!(snapshot.active.len(), 1);
        assert_eq!(snapshot.active[0].status, ProviderLeaseStatus::Reserved);
        assert!(matches!(
            reserve(
                &authority,
                claim("after-reserved-drop", "host-a", "instance-a", 1, 1)
            ),
            ProviderLeaseTry::Busy(ProviderLeaseBusy {
                kind: ProviderLeaseBusyKind::HostCapacity,
                ..
            })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn sigkill_after_exposure_reconstructs_an_occupied_lease() {
        let (_temp, root) = control_root();
        drop(open_authority(&root));
        let ready = root.parent().unwrap().join("child-exposed");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("provider_lease::tests::provider_lease_child_process")
            .arg("--nocapture")
            .env(CHILD_MODE_ENV, "expose_and_wait")
            .env(CHILD_ROOT_ENV, &root)
            .env(CHILD_RESULT_ENV, &ready)
            .env(CHILD_REQUEST_ENV, "killed-child")
            .env(CHILD_CAPACITY_ENV, "1")
            .current_dir(root.parent().unwrap())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            assert!(
                child.try_wait().unwrap().is_none(),
                "child exited before exposing its provider lease"
            );
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("timed out waiting for the exposed child lease");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        child.kill().unwrap();
        child.wait().unwrap();

        let authority = open_authority(&root);
        let snapshot = authority.snapshot().unwrap();
        assert_eq!(snapshot.active.len(), 1);
        assert_eq!(snapshot.active[0].status, ProviderLeaseStatus::Exposed);
        assert!(matches!(
            reserve(
                &authority,
                claim("after-kill", "host-child", "instance-child", 1, 1)
            ),
            ProviderLeaseTry::Busy(ProviderLeaseBusy {
                kind: ProviderLeaseBusyKind::HostCapacity,
                ..
            })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_replacement_kill_points_reconcile_from_the_durable_wal() {
        for (index, kill_point) in [
            "after_wal_sync",
            "after_checkpoint_temp_sync",
            "after_checkpoint_rename",
            "after_checkpoint_directory_sync",
        ]
        .into_iter()
        .enumerate()
        {
            let (_temp, root) = control_root();
            drop(open_authority(&root));
            let result = root.parent().unwrap().join(format!("kill-result-{index}"));
            let status = Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg("provider_lease::tests::provider_lease_child_process")
                .arg("--nocapture")
                .env(CHILD_MODE_ENV, "reserve")
                .env(CHILD_ROOT_ENV, &root)
                .env(CHILD_RESULT_ENV, result)
                .env(CHILD_REQUEST_ENV, format!("checkpoint-kill-{index}"))
                .env(CHILD_CAPACITY_ENV, "1")
                .env(CHECKPOINT_KILL_POINT_ENV, kill_point)
                .current_dir(root.parent().unwrap())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(
                !status.success(),
                "kill point {kill_point} did not kill child"
            );

            let authority = open_authority(&root);
            let snapshot = authority.snapshot().unwrap();
            assert_eq!(snapshot.active.len(), 1, "kill point {kill_point}");
            assert_eq!(
                snapshot.active[0].status,
                ProviderLeaseStatus::Reserved,
                "kill point {kill_point}"
            );
            assert!(matches!(
                reserve(
                    &authority,
                    claim(
                        &format!("after-checkpoint-kill-{index}"),
                        "host-child",
                        "instance-child",
                        1,
                        1
                    )
                ),
                ProviderLeaseTry::Busy(ProviderLeaseBusy {
                    kind: ProviderLeaseBusyKind::HostCapacity,
                    ..
                })
            ));
            let temporary_prefix = format!(".{CHECKPOINT_FILE_NAME}.");
            assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
                let name = entry.unwrap().file_name();
                !name.to_string_lossy().starts_with(&temporary_prefix)
            }));
        }
    }

    #[cfg(unix)]
    #[test]
    fn retryable_contention_returns_the_consumed_handle_for_an_exact_retry() {
        let (_temp, root) = control_root();
        let authority = open_authority(&root);
        let reserved = acquired(reserve(
            &authority,
            claim("retryable", "host-a", "instance-a", 1, 1),
        ));
        let locked = root.parent().unwrap().join("control-lock-held");
        let release = root.parent().unwrap().join("release-control-lock");
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("provider_lease::tests::provider_lease_child_process")
            .arg("--nocapture")
            .env(CHILD_MODE_ENV, "hold_control_lock")
            .env(CHILD_ROOT_ENV, &root)
            .env(CHILD_RESULT_ENV, &locked)
            .env(CHILD_START_ENV, &release)
            .current_dir(root.parent().unwrap())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        wait_for_path(&locked);

        let reserved = match authority.expose(reserved) {
            Err(ProviderLeaseTransitionError::Retryable { error, handle }) => {
                assert_eq!(error, ProviderLeaseError::AuthorityContended);
                *handle
            }
            other => panic!("expected retryable authority contention, got {other:?}"),
        };
        std::fs::write(&release, "release").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "lock-holder child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let exposed = authority.expose(reserved).unwrap();
        let terminal_receipt = terminal(&exposed);
        authority
            .provider_terminal(exposed, &terminal_receipt)
            .unwrap();
        assert!(authority.snapshot().unwrap().active.is_empty());
    }

    #[test]
    fn only_reserved_can_be_abandoned_and_only_exposed_can_be_terminal() {
        let (_temp, root) = control_root();
        let authority = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        let reserved = acquired(reserve(
            &authority,
            claim("first", "host-a", "instance-a", 1, 1),
        ));
        let stale_abandon = duplicate_reserved_for_adversarial_replay(&reserved);
        let stale_expose = duplicate_reserved_for_adversarial_replay(&reserved);
        let exposed = authority.expose(reserved).unwrap();
        assert!(matches!(
            authority.abandon_reserved(stale_abandon, "late abandon"),
            Err(ProviderLeaseTransitionError::Fatal(
                ProviderLeaseError::InvalidTransition(_)
            ))
        ));
        assert!(matches!(
            authority.expose(stale_expose),
            Err(ProviderLeaseTransitionError::Fatal(
                ProviderLeaseError::InvalidTransition(_)
            ))
        ));
        let terminal_receipt = terminal(&exposed);
        let stale_terminal = duplicate_exposed_for_adversarial_replay(&exposed);
        authority
            .provider_terminal(exposed, &terminal_receipt)
            .unwrap();
        assert!(authority.snapshot().unwrap().active.is_empty());
        assert!(matches!(
            authority.provider_terminal(stale_terminal, &terminal_receipt),
            Err(ProviderLeaseTransitionError::Fatal(
                ProviderLeaseError::InvalidTransition(_)
            ))
        ));
    }

    #[test]
    fn wrong_terminal_and_duplicate_request_are_rejected_without_release() {
        let (_temp, root) = control_root();
        let authority = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        let original_claim = claim("first", "host-a", "instance-a", 1, 1);
        let reserved = acquired(reserve(&authority, original_claim.clone()));
        let mut forged_reservation = duplicate_reserved_for_adversarial_replay(&reserved);
        forged_reservation.owner_id = random_id("owner");
        assert!(matches!(
            authority.expose(forged_reservation),
            Err(ProviderLeaseTransitionError::Fatal(
                ProviderLeaseError::ReceiptMismatch(_)
            ))
        ));
        let exposed = authority.expose(reserved).unwrap();
        let mut wrong = terminal(&exposed);
        wrong.key.provider_request_id = "forged".to_string();
        let wrong_terminal_handle = duplicate_exposed_for_adversarial_replay(&exposed);
        assert!(matches!(
            authority.provider_terminal(wrong_terminal_handle, &wrong),
            Err(ProviderLeaseTransitionError::Fatal(
                ProviderLeaseError::ReceiptMismatch(_)
            ))
        ));
        assert_eq!(authority.snapshot().unwrap().active.len(), 1);
        let terminal_receipt = terminal(&exposed);
        authority
            .provider_terminal(exposed, &terminal_receipt)
            .unwrap();
        assert!(matches!(
            authority.try_reserve(original_claim),
            Err(ProviderLeaseError::InvalidTransition(_))
        ));
    }

    #[test]
    fn conflicting_live_capacity_or_route_evidence_fails_closed() {
        let (_temp, root) = control_root();
        let authority = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        let _reserved = acquired(reserve(
            &authority,
            claim("first", "host-a", "instance-a", 2, 2),
        ));
        assert!(matches!(
            authority.try_reserve(claim("second", "host-a", "instance-b", 3, 2)),
            Err(ProviderLeaseError::ConflictingEvidence(_))
        ));
        let mut route_conflict = claim("third", "host-a", "instance-a", 2, 2);
        route_conflict.provider_protocol_id = "openai.responses.v1".to_string();
        assert!(matches!(
            authority.try_reserve(route_conflict),
            Err(ProviderLeaseError::ConflictingEvidence(_))
        ));
    }

    #[test]
    fn torn_blank_or_replaced_authority_evidence_latches() {
        let (_temp, root) = control_root();
        let authority = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        let wal = root.join(WAL_FILE_NAME);
        OpenOptions::new()
            .append(true)
            .open(&wal)
            .unwrap()
            .write_all(b"torn")
            .unwrap();
        assert!(matches!(
            authority.snapshot(),
            Err(ProviderLeaseError::CorruptWal(_))
        ));
        assert!(matches!(
            authority.try_reserve(claim("one", "host-a", "instance-a", 1, 1)),
            Err(ProviderLeaseError::CorruptWal(_))
        ));

        let (_temp, root) = control_root();
        let authority = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        let wal = root.join(WAL_FILE_NAME);
        std::fs::rename(&wal, root.join("old-wal")).unwrap();
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&wal)
            .unwrap();
        std::fs::set_permissions(&wal, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            authority.snapshot(),
            Err(ProviderLeaseError::UnsafeControlPath(_))
        ));

        let (_temp, root) = control_root();
        let authority = open_authority(&root);
        OpenOptions::new()
            .append(true)
            .open(root.join(WAL_FILE_NAME))
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        assert!(matches!(
            authority.snapshot(),
            Err(ProviderLeaseError::CorruptWal(_))
        ));

        let (_temp, root) = control_root();
        let authority = open_authority(&root);
        std::fs::write(root.join(CHECKPOINT_FILE_NAME), b"torn-checkpoint").unwrap();
        assert!(matches!(
            authority.snapshot(),
            Err(ProviderLeaseError::CorruptWal(_))
        ));
    }

    #[test]
    fn valid_prefix_wal_rollback_is_rejected_by_the_durable_checkpoint() {
        let (_temp, root) = control_root();
        let authority = open_authority(&root);
        let reserved = acquired(reserve(
            &authority,
            claim("rollback", "host-a", "instance-a", 1, 1),
        ));
        let reserved_prefix = std::fs::read(root.join(WAL_FILE_NAME)).unwrap();
        let _exposed = authority.expose(reserved).unwrap();
        drop(authority);

        std::fs::write(root.join(WAL_FILE_NAME), reserved_prefix).unwrap();
        assert!(matches!(
            GlobalProviderLeaseAuthority::open_test_root(&root),
            Err(ProviderLeaseError::CorruptWal(_))
        ));
    }

    #[test]
    fn checkpoint_lag_reconciles_only_forward_to_a_valid_wal_head() {
        let (_temp, root) = control_root();
        let authority = open_authority(&root);
        let reserved = acquired(reserve(
            &authority,
            claim("checkpoint-lag", "host-a", "instance-a", 1, 1),
        ));
        let reserved_checkpoint = std::fs::read(root.join(CHECKPOINT_FILE_NAME)).unwrap();
        let _exposed = authority.expose(reserved).unwrap();
        drop(authority);

        std::fs::write(root.join(CHECKPOINT_FILE_NAME), reserved_checkpoint).unwrap();
        let reopened = open_authority(&root);
        let snapshot = reopened.snapshot().unwrap();
        assert_eq!(snapshot.active.len(), 1);
        assert_eq!(snapshot.active[0].status, ProviderLeaseStatus::Exposed);
        let checkpoint = read_checkpoint(
            &mut OpenOptions::new()
                .read(true)
                .write(true)
                .open(root.join(CHECKPOINT_FILE_NAME))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(checkpoint.next_sequence, snapshot.next_sequence);
    }

    #[cfg(unix)]
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[cfg(unix)]
    #[test]
    fn symlink_hardlink_and_unsafe_permissions_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
        let linked_root = temp.path().join("linked-authority");
        symlink(&target, &linked_root).unwrap();
        assert!(matches!(
            GlobalProviderLeaseAuthority::open_test_root(&linked_root),
            Err(ProviderLeaseError::UnsafeControlPath(_))
        ));

        let unsafe_root = temp.path().join("unsafe-authority");
        drop(open_authority(&unsafe_root));
        std::fs::set_permissions(&unsafe_root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            GlobalProviderLeaseAuthority::open_test_root(&unsafe_root),
            Err(ProviderLeaseError::UnsafeControlPath(_))
        ));

        let root = temp.path().join("hardlink-authority");
        let authority = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        std::fs::hard_link(root.join(WAL_FILE_NAME), root.join("wal-link")).unwrap();
        assert!(matches!(
            authority.snapshot(),
            Err(ProviderLeaseError::UnsafeControlPath(_))
        ));

        let root = temp.path().join("file-mode-authority");
        let authority = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        std::fs::set_permissions(
            root.join(WAL_FILE_NAME),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(matches!(
            authority.snapshot(),
            Err(ProviderLeaseError::UnsafeControlPath(_))
        ));

        let root = temp.path().join("checkpoint-hardlink-authority");
        let authority = open_authority(&root);
        std::fs::hard_link(
            root.join(CHECKPOINT_FILE_NAME),
            root.join("checkpoint-link"),
        )
        .unwrap();
        assert!(matches!(
            authority.snapshot(),
            Err(ProviderLeaseError::UnsafeControlPath(_))
        ));

        let root = temp.path().join("lock-symlink-authority");
        drop(open_authority(&root));
        let lock = root.join(LOCK_FILE_NAME);
        let old_lock = root.join("old-lock");
        std::fs::rename(&lock, &old_lock).unwrap();
        symlink(&old_lock, &lock).unwrap();
        assert!(matches!(
            GlobalProviderLeaseAuthority::open_test_root(&root),
            Err(ProviderLeaseError::UnsafeControlPath(_))
        ));

        let root = temp.path().join("missing-root-authority");
        drop(open_authority(&root));
        std::fs::rename(&root, temp.path().join("removed-authority")).unwrap();
        assert!(matches!(
            GlobalProviderLeaseAuthority::open_test_root(&root),
            Err(ProviderLeaseError::UnsafeControlPath(_))
        ));

        let root = temp.path().join("unsafe-initialization-marker-authority");
        drop(open_authority(&root));
        let initialization_lock = initialization_lock_path(&root).unwrap();
        std::fs::set_permissions(initialization_lock, std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(matches!(
            GlobalProviderLeaseAuthority::open_test_root(&root),
            Err(ProviderLeaseError::UnsafeControlPath(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn abandoned_first_initialization_is_recovered_under_the_initialization_lock() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("abandoned-authority");
        let initialization_lock = initialization_lock_path(&root).unwrap();
        let (marker, created) =
            open_or_create_secure_file(&initialization_lock, FileAccess::Rewrite).unwrap();
        assert!(created);
        drop(marker);
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        drop(
            open_or_create_secure_file(&root.join(LOCK_FILE_NAME), FileAccess::Rewrite)
                .unwrap()
                .0,
        );

        let authority = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        assert_eq!(
            std::fs::read(&initialization_lock).unwrap(),
            INITIALIZATION_READY
        );
        assert!(root.join(WAL_FILE_NAME).is_file());
        assert!(root.join(CHECKPOINT_FILE_NAME).is_file());
        assert!(authority.snapshot().unwrap().active.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn interrupted_ready_marker_is_recovered_only_for_an_exact_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("partial-ready-authority");
        drop(open_authority(&root));
        let initialization_lock = initialization_lock_path(&root).unwrap();
        std::fs::write(
            &initialization_lock,
            &INITIALIZATION_READY[..INITIALIZATION_READY.len() / 2],
        )
        .unwrap();

        let authority = GlobalProviderLeaseAuthority::open_test_root(&root).unwrap();
        assert!(authority.snapshot().unwrap().active.is_empty());
        assert_eq!(
            std::fs::read(&initialization_lock).unwrap(),
            INITIALIZATION_READY
        );
        drop(authority);

        std::fs::write(&initialization_lock, b"not-an-initialization-prefix").unwrap();
        assert!(matches!(
            GlobalProviderLeaseAuthority::open_test_root(&root),
            Err(ProviderLeaseError::UnsafeControlPath(_))
        ));
    }

    #[test]
    fn claim_rejects_request_or_namespace_mismatch_before_wal() {
        let source = TaskVersion {
            authority_scope: AuthorityScope::new("run-a", "build"),
            phase_epoch: 0,
            task_id: "task-a".to_string(),
            attempt: 0,
            revision: 1,
            kind: SourceRevisionKind::TaskAttempt,
        };
        let capacity_evidence = HostCapacityEvidence::ProbeSingleStream {
            probe_epoch: "probe-a".to_string(),
        };
        let admission = AdmissionReceipt {
            admission_id: "admission-a".to_string(),
            work_id: "work-a".to_string(),
            role: WorkRole::Build,
            priority: WorkPriority::Implementation,
            task_rank: 1,
            source,
            fleet_snapshot_id: "fleet-a".to_string(),
            logical_device_id: "device-a".to_string(),
            model_id: "model-a".to_string(),
            physical_host_id: "host-a".to_string(),
            model_instance_id: "instance-a".to_string(),
            provider_transport_id: TRANSPORT.to_string(),
            route_evidence_id: "route-a".to_string(),
            capacity_evidence: capacity_evidence.clone(),
            queue_sequence: 1,
            admission_sequence: 1,
        };
        let fleet = PhysicalFleetSnapshot::new(
            "fleet-a",
            vec![VerifiedPhysicalLane {
                logical_device_id: "device-a".to_string(),
                model_id: "model-a".to_string(),
                host_id: "host-a".to_string(),
                model_instance_id: "instance-a".to_string(),
                provider_transport_id: TRANSPORT.to_string(),
                advertised_instance_capacity: 3,
                routing_weight: 1,
                capacity_evidence,
                route_evidence_id: "route-a".to_string(),
            }],
        )
        .unwrap();
        let authority =
            SealedProviderLeaseAuthority::from_fleet_snapshot(
                &fleet,
                [VerifiedProviderProtocolRoute::new(
                    TRANSPORT,
                    ProviderHttpProtocol::OpenAiResponses,
                )
                .unwrap()],
            )
            .unwrap();
        let request = ProviderRequestReceipt {
            admission_id: "other-admission".to_string(),
            key: ProviderRequestKey {
                ordinal: 0,
                provider_request_id: "request-a".to_string(),
            },
            physical_host_id: "host-a".to_string(),
            model_instance_id: "instance-a".to_string(),
        };
        assert!(matches!(
            ProviderLeaseClaim::from_authority(&authority, &admission, &request),
            Err(ProviderLeaseError::InvalidClaim(_))
        ));

        let matching_request = ProviderRequestReceipt {
            admission_id: admission.admission_id.clone(),
            key: ProviderRequestKey {
                ordinal: 0,
                provider_request_id: "request-a".to_string(),
            },
            physical_host_id: admission.physical_host_id.clone(),
            model_instance_id: admission.model_instance_id.clone(),
        };
        let claim =
            ProviderLeaseClaim::from_authority(&authority, &admission, &matching_request).unwrap();
        assert_eq!(
            claim.provider_protocol_id(),
            ProviderHttpProtocol::OpenAiResponses.authority_id()
        );
        assert_eq!(claim.advertised_instance_capacity(), 3);

        let mut forged_admission = admission;
        forged_admission.capacity_evidence = HostCapacityEvidence::MeasuredProfile {
            profile_hash: "forged-profile".to_string(),
            profile_key: "forged-runtime".to_string(),
            max_concurrent: 99,
        };
        assert!(matches!(
            ProviderLeaseClaim::from_authority(&authority, &forged_admission, &matching_request),
            Err(ProviderLeaseError::InvalidClaim(_))
        ));
    }
}
