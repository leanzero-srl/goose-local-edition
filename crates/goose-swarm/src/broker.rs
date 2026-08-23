//! Physical-host admission for model work.
//!
//! A logical scheduler lane is not a physical decoder. This opt-in broker admits model work only
//! through a same-run physical fleet snapshot. A task admission is a correlation envelope; each
//! provider turn separately owns physical capacity until its exact terminal receipt arrives. It
//! deliberately has no API for cancelling admitted work.

use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostCapacityEvidence {
    /// A same-run probe proved the host and loaded instance exist. It does not prove that two
    /// concurrent Apple decodes improve throughput, so its host-wide capacity is exactly one.
    ProbeSingleStream { probe_epoch: String },
    /// Controlled one-versus-N measurements for one exact runtime/model/context/role profile.
    MeasuredProfile {
        profile_hash: String,
        profile_key: String,
        max_concurrent: u32,
    },
}

impl HostCapacityEvidence {
    pub fn max_concurrent(&self) -> u32 {
        match self {
            Self::ProbeSingleStream { .. } => 1,
            Self::MeasuredProfile { max_concurrent, .. } => *max_concurrent,
        }
    }

    fn identity(&self) -> &str {
        match self {
            Self::ProbeSingleStream { probe_epoch } => probe_epoch,
            Self::MeasuredProfile { profile_hash, .. } => profile_hash,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.identity().trim().is_empty() {
            return Err("capacity evidence id is empty".to_string());
        }
        if self.max_concurrent() == 0 {
            return Err("host capacity is zero".to_string());
        }
        if let Self::MeasuredProfile { profile_key, .. } = self {
            if profile_key.trim().is_empty() {
                return Err("measured profile key is empty".to_string());
            }
        }
        Ok(())
    }
}

/// Same-run physical identity carried outside `DeviceCfg`; a configured host string never becomes
/// runtime evidence merely because it was deserialized.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VerifiedPhysicalIdentity {
    pub host_id: String,
    pub model_instance_id: String,
    pub provider_transport_id: String,
    /// LM Studio's instance `PARALLEL` ceiling. It never becomes host capacity and is never summed
    /// across model rows on one host.
    pub advertised_instance_capacity: u32,
    pub capacity_evidence: HostCapacityEvidence,
    pub route_evidence_id: String,
}

impl VerifiedPhysicalIdentity {
    pub fn into_lane(
        self,
        logical_device_id: String,
        model_id: String,
        routing_weight: u32,
    ) -> VerifiedPhysicalLane {
        VerifiedPhysicalLane {
            logical_device_id,
            model_id,
            host_id: self.host_id,
            model_instance_id: self.model_instance_id,
            provider_transport_id: self.provider_transport_id,
            advertised_instance_capacity: self.advertised_instance_capacity,
            routing_weight,
            capacity_evidence: self.capacity_evidence,
            route_evidence_id: self.route_evidence_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VerifiedPhysicalLane {
    pub logical_device_id: String,
    pub model_id: String,
    pub host_id: String,
    pub model_instance_id: String,
    pub provider_transport_id: String,
    pub advertised_instance_capacity: u32,
    pub routing_weight: u32,
    pub capacity_evidence: HostCapacityEvidence,
    pub route_evidence_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PhysicalFleetSnapshot {
    pub snapshot_id: String,
    pub lanes: Vec<VerifiedPhysicalLane>,
}

impl PhysicalFleetSnapshot {
    pub fn new(
        snapshot_id: impl Into<String>,
        lanes: Vec<VerifiedPhysicalLane>,
    ) -> Result<Self, BrokerError> {
        let snapshot = Self {
            snapshot_id: snapshot_id.into(),
            lanes,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn validate(&self) -> Result<(), BrokerError> {
        if self.snapshot_id.trim().is_empty() {
            return Err(BrokerError::InvalidSnapshot(
                "snapshot id is empty".to_string(),
            ));
        }
        if self.lanes.is_empty() {
            return Err(BrokerError::NoVerifiedLanes);
        }
        let mut logical_devices = HashMap::new();
        let mut routes_by_model = HashMap::new();
        let mut host_capacities: HashMap<&str, &HostCapacityEvidence> = HashMap::new();
        let mut instance_capacities = HashMap::new();
        let mut route_evidence: HashMap<(&str, &str), &str> = HashMap::new();
        let mut provider_transports: HashMap<(&str, &str), &str> = HashMap::new();
        for lane in &self.lanes {
            validate_lane(lane)?;
            if logical_devices
                .insert(&lane.logical_device_id, &lane.model_id)
                .is_some()
            {
                return Err(BrokerError::InvalidLane {
                    device_id: lane.logical_device_id.clone(),
                    reason: "logical device id is duplicated".to_string(),
                });
            }
            let route = (&lane.host_id, &lane.model_instance_id);
            if let Some(first) = routes_by_model.insert(&lane.model_id, route) {
                if first != route {
                    return Err(BrokerError::AmbiguousModelRoute {
                        model_id: lane.model_id.clone(),
                    });
                }
            }
            if let Some(first) = host_capacities.insert(&lane.host_id, &lane.capacity_evidence) {
                if first != &lane.capacity_evidence {
                    return Err(BrokerError::ConflictingHostCapacity {
                        host_id: lane.host_id.clone(),
                        first: first.max_concurrent(),
                        second: lane.capacity_evidence.max_concurrent(),
                    });
                }
            }
            let instance_key = (&lane.host_id, &lane.model_instance_id);
            if let Some(first) =
                instance_capacities.insert(instance_key, lane.advertised_instance_capacity)
            {
                if first != lane.advertised_instance_capacity {
                    return Err(BrokerError::ConflictingInstanceCapacity {
                        host_id: lane.host_id.clone(),
                        model_instance_id: lane.model_instance_id.clone(),
                        first,
                        second: lane.advertised_instance_capacity,
                    });
                }
            }
            let route_key = (lane.host_id.as_str(), lane.model_instance_id.as_str());
            if let Some(first) = route_evidence.insert(route_key, lane.route_evidence_id.as_str()) {
                if first != lane.route_evidence_id {
                    return Err(BrokerError::ConflictingRouteEvidence {
                        host_id: lane.host_id.clone(),
                        model_instance_id: lane.model_instance_id.clone(),
                        first: first.to_string(),
                        second: lane.route_evidence_id.clone(),
                    });
                }
            }
            if let Some(first) =
                provider_transports.insert(route_key, lane.provider_transport_id.as_str())
            {
                if first != lane.provider_transport_id {
                    return Err(BrokerError::ConflictingProviderTransport {
                        host_id: lane.host_id.clone(),
                        model_instance_id: lane.model_instance_id.clone(),
                        first: first.to_string(),
                        second: lane.provider_transport_id.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

fn validate_lane(lane: &VerifiedPhysicalLane) -> Result<(), BrokerError> {
    let invalid = [
        ("logical device id", lane.logical_device_id.trim()),
        ("model id", lane.model_id.trim()),
        ("physical host id", lane.host_id.trim()),
        ("model instance id", lane.model_instance_id.trim()),
        ("provider transport id", lane.provider_transport_id.trim()),
        ("route evidence id", lane.route_evidence_id.trim()),
    ]
    .into_iter()
    .find(|(_, value)| value.is_empty())
    .map(|(name, _)| format!("{name} is empty"))
    .or_else(|| {
        (!is_canonical_transport_identity(&lane.provider_transport_id))
            .then(|| "provider transport identity is not a canonical sha256 digest".to_string())
    })
    .or_else(|| {
        (lane.advertised_instance_capacity == 0)
            .then(|| "advertised instance capacity is zero".to_string())
    })
    .or_else(|| lane.capacity_evidence.validate().err());
    if let Some(reason) = invalid {
        return Err(BrokerError::InvalidLane {
            device_id: lane.logical_device_id.clone(),
            reason,
        });
    }
    Ok(())
}

fn is_canonical_transport_identity(identity: &str) -> bool {
    let Some(digest) = identity.strip_prefix("sha256:") else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceRevisionKind {
    TaskAttempt,
    Artifact {
        snapshot_hash: String,
    },
    Trace {
        trace_sequence: u64,
        snapshot_hash: String,
    },
    Contract {
        binding_task_id: String,
        slice_id: String,
        snapshot_hash: String,
    },
}

/// Immutable authority for one queued opportunity.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
pub struct TaskVersion {
    pub task_id: String,
    pub attempt: u32,
    pub revision: u64,
    pub kind: SourceRevisionKind,
}

impl TaskVersion {
    fn authority_key(&self) -> String {
        match &self.kind {
            SourceRevisionKind::TaskAttempt => format!("{}:attempt", self.task_id),
            SourceRevisionKind::Artifact { .. } => format!("{}:artifact", self.task_id),
            SourceRevisionKind::Trace { .. } => format!("{}:trace", self.task_id),
            SourceRevisionKind::Contract {
                binding_task_id,
                slice_id,
                ..
            } => format!("{}:contract:{binding_task_id}:{slice_id}", self.task_id),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.task_id.trim().is_empty() {
            return Err("source task id is empty".to_string());
        }
        match &self.kind {
            SourceRevisionKind::TaskAttempt => {
                if self.revision != u64::from(self.attempt) + 1 {
                    Err("task-attempt revision must equal attempt + 1".to_string())
                } else {
                    Ok(())
                }
            }
            SourceRevisionKind::Artifact { snapshot_hash }
            | SourceRevisionKind::Trace { snapshot_hash, .. } => {
                if self.revision == 0 {
                    Err("artifact/trace source revision is zero".to_string())
                } else if snapshot_hash.trim().is_empty() {
                    Err("snapshot hash is empty".to_string())
                } else {
                    Ok(())
                }
            }
            SourceRevisionKind::Contract {
                binding_task_id,
                slice_id,
                snapshot_hash,
            } => {
                if self.revision == 0 {
                    return Err("contract source revision is zero".to_string());
                }
                if binding_task_id.trim().is_empty()
                    || slice_id.trim().is_empty()
                    || snapshot_hash.trim().is_empty()
                {
                    return Err(
                        "contract source requires binding task, slice, and snapshot hash"
                            .to_string(),
                    );
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPriority {
    AuxiliaryEvidence,
    Implementation,
    CriticalPath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkRole {
    Build,
    Repair,
    RuntimeAcceptanceReview,
    CompletedArtifactReview,
    SemanticJudgeObservation,
    ContractReview,
    AcceptanceOracle,
}

impl WorkRole {
    pub fn priority(self) -> WorkPriority {
        match self {
            Self::Repair | Self::AcceptanceOracle => WorkPriority::CriticalPath,
            Self::Build => WorkPriority::Implementation,
            Self::RuntimeAcceptanceReview
            | Self::CompletedArtifactReview
            | Self::SemanticJudgeObservation
            | Self::ContractReview => WorkPriority::AuxiliaryEvidence,
        }
    }
}

/// One task-derived, version-current candidate for the common queue.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkOpportunity {
    pub work_id: String,
    pub role: WorkRole,
    pub priority: WorkPriority,
    /// Task-derived scheduler rank. A larger value wins among work of the same semantic priority.
    /// Physical capacity never participates in its construction.
    pub task_rank: u64,
    pub source: TaskVersion,
    /// Empty means any verified route. This is a contract constraint, not a roster-shaped task fan.
    pub eligible_logical_device_ids: Vec<String>,
    pub preferred_model_id: Option<String>,
    pub excluded_logical_device_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QueueReceipt {
    pub work_id: String,
    pub role: WorkRole,
    pub priority: WorkPriority,
    pub task_rank: u64,
    pub source: TaskVersion,
    pub queue_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StaleWorkReceipt {
    pub work_id: String,
    pub role: WorkRole,
    pub queued_source: TaskVersion,
    pub current_source: Option<TaskVersion>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AdmissionReceipt {
    pub admission_id: String,
    pub work_id: String,
    pub role: WorkRole,
    pub priority: WorkPriority,
    pub task_rank: u64,
    pub source: TaskVersion,
    pub fleet_snapshot_id: String,
    pub logical_device_id: String,
    pub model_id: String,
    pub physical_host_id: String,
    pub model_instance_id: String,
    pub provider_transport_id: String,
    pub route_evidence_id: String,
    pub capacity_evidence: HostCapacityEvidence,
    pub queue_sequence: u64,
    pub admission_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
pub struct ProviderRequestKey {
    pub ordinal: u32,
    pub provider_request_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderRequestReceipt {
    pub admission_id: String,
    pub key: ProviderRequestKey,
    pub physical_host_id: String,
    pub model_instance_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderRequestQueueReceipt {
    pub request: ProviderRequestReceipt,
    pub priority: WorkPriority,
    pub task_rank: u64,
    pub queue_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalCompletionKind {
    Success,
    Error,
    StreamDropped,
    CancellationRequested,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LocalCompletionReceipt {
    pub admission_id: String,
    pub work_id: String,
    pub physical_host_id: String,
    pub model_instance_id: String,
    pub kind: LocalCompletionKind,
    pub provider_requests_started: usize,
    pub provider_requests_terminal: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTerminalKind {
    Finished,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderTerminalReceipt {
    pub admission_id: String,
    pub key: ProviderRequestKey,
    pub physical_host_id: String,
    pub model_instance_id: String,
    pub kind: ProviderTerminalKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderNotStartedReceipt {
    pub admission_id: String,
    pub physical_host_id: String,
    pub model_instance_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderStartsClosure {
    pub admission: Option<AdmissionReceipt>,
    pub provider_not_started: Option<ProviderNotStartedReceipt>,
    pub pending_provider_request: Option<ProviderRequestReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReleasedAdmissionReceipt {
    pub admission: AdmissionReceipt,
    pub local_completion: LocalCompletionKind,
    pub provider_terminals: Vec<ProviderTerminalReceipt>,
    pub provider_not_started: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CapacityUpdateReceipt {
    pub physical_host_id: String,
    pub previous_fleet_snapshot_id: String,
    pub new_fleet_snapshot_id: String,
    pub previous_capacity: u32,
    pub new_capacity: u32,
    pub capacity_evidence: HostCapacityEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PhysicalHostOccupancy {
    pub physical_host_id: String,
    pub provider_turn_permits_held: u32,
    pub capacity: u32,
    pub capacity_evidence: HostCapacityEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WithdrawnWorkReceipt {
    pub work_id: String,
    pub role: WorkRole,
    pub source: TaskVersion,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RevokedAdmissionReceipt {
    pub admission: AdmissionReceipt,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnresolvedAdmissionReceipt {
    pub admission: AdmissionReceipt,
    pub provider_requests_started: usize,
    pub provider_requests_terminal: usize,
    pub provider_request_pending: bool,
    pub provider_turn_permit_held: bool,
    pub provider_starts_closed: bool,
    pub local_completion: Option<LocalCompletionKind>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderRequestDisposition {
    Granted(ProviderRequestReceipt),
    Queued(ProviderRequestQueueReceipt),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrokerGrant {
    Admission(AdmissionReceipt),
    ProviderRequest {
        admission: AdmissionReceipt,
        receipt: ProviderRequestReceipt,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrokerError {
    EmptyCorrelationScope,
    NoVerifiedLanes,
    InvalidSnapshot(String),
    InvalidLane {
        device_id: String,
        reason: String,
    },
    AmbiguousModelRoute {
        model_id: String,
    },
    ConflictingHostCapacity {
        host_id: String,
        first: u32,
        second: u32,
    },
    ConflictingInstanceCapacity {
        host_id: String,
        model_instance_id: String,
        first: u32,
        second: u32,
    },
    ConflictingRouteEvidence {
        host_id: String,
        model_instance_id: String,
        first: String,
        second: String,
    },
    ConflictingProviderTransport {
        host_id: String,
        model_instance_id: String,
        first: String,
        second: String,
    },
    InvalidOpportunity {
        work_id: String,
        reason: String,
    },
    StaleOpportunity {
        work_id: String,
        queued: Box<TaskVersion>,
        current: Option<Box<TaskVersion>>,
    },
    DuplicateWork(String),
    UnknownAdmission(String),
    DuplicateProviderRequest(ProviderRequestKey),
    InvalidProviderRequest {
        admission_id: String,
        reason: String,
    },
    DuplicateProviderTerminal(ProviderRequestKey),
    ProviderRequestMismatch {
        admission_id: String,
        received: ProviderRequestKey,
    },
    PhysicalReceiptMismatch {
        admission_id: String,
        expected_host: String,
        received_host: String,
        expected_instance: String,
        received_instance: String,
    },
    ConflictingLocalCompletion {
        admission_id: String,
        first: LocalCompletionKind,
        second: LocalCompletionKind,
    },
    DuplicateLocalCompletion(String),
    InvalidProviderNotStarted {
        admission_id: String,
        reason: String,
    },
    UnknownPhysicalHost(String),
    InvalidCapacityEvidence {
        host_id: String,
        reason: String,
    },
    FleetSnapshotMismatch {
        expected: String,
        current: String,
    },
    AdmissionWaiterClosed(String),
    SourceRevisionRollback {
        authority: String,
        current: Box<TaskVersion>,
        proposed: Box<TaskVersion>,
    },
    ConflictingSourceRevision {
        authority: String,
        current: Box<TaskVersion>,
        proposed: Box<TaskVersion>,
    },
    SourceRevisionMismatch {
        authority: String,
        current: Option<Box<TaskVersion>>,
        requested: Box<TaskVersion>,
    },
    ProviderStartsClosed(String),
    ConcurrentProviderRequest(String),
    OutcomeConflict {
        admission_id: String,
        reason: String,
    },
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyCorrelationScope => write!(f, "physical broker correlation scope is empty"),
            Self::NoVerifiedLanes => write!(f, "physical broker has no verified lanes"),
            Self::InvalidSnapshot(reason) => write!(f, "invalid physical fleet snapshot: {reason}"),
            Self::InvalidLane { device_id, reason } => {
                write!(f, "invalid verified lane `{device_id}`: {reason}")
            }
            Self::AmbiguousModelRoute { model_id } => write!(
                f,
                "model id `{model_id}` resolves to multiple physical routes"
            ),
            Self::ConflictingHostCapacity {
                host_id,
                first,
                second,
            } => write!(
                f,
                "physical host `{host_id}` has conflicting capacities {first} and {second}"
            ),
            Self::ConflictingInstanceCapacity {
                host_id,
                model_instance_id,
                first,
                second,
            } => write!(
                f,
                "model instance `{model_instance_id}` on `{host_id}` has conflicting capacities {first} and {second}"
            ),
            Self::ConflictingRouteEvidence {
                host_id,
                model_instance_id,
                first,
                second,
            } => write!(
                f,
                "model instance `{model_instance_id}` on `{host_id}` has conflicting route evidence `{first}` and `{second}`"
            ),
            Self::ConflictingProviderTransport {
                host_id,
                model_instance_id,
                ..
            } => write!(
                f,
                "model instance `{model_instance_id}` on `{host_id}` has conflicting provider transport identities"
            ),
            Self::InvalidOpportunity { work_id, reason } => {
                write!(f, "invalid broker opportunity `{work_id}`: {reason}")
            }
            Self::StaleOpportunity {
                work_id,
                queued,
                current,
            } => write!(
                f,
                "broker opportunity `{work_id}` is stale: queued {queued:?}, current {current:?}"
            ),
            Self::DuplicateWork(work_id) => {
                write!(f, "broker work `{work_id}` is already queued or admitted")
            }
            Self::UnknownAdmission(admission_id) => {
                write!(f, "unknown broker admission `{admission_id}`")
            }
            Self::DuplicateProviderRequest(key) => {
                write!(f, "provider request {key:?} is already bound")
            }
            Self::InvalidProviderRequest {
                admission_id,
                reason,
            } => write!(
                f,
                "invalid provider request for admission `{admission_id}`: {reason}"
            ),
            Self::DuplicateProviderTerminal(key) => {
                write!(f, "provider request {key:?} already has a terminal receipt")
            }
            Self::ProviderRequestMismatch {
                admission_id,
                received,
            } => write!(
                f,
                "provider request {received:?} is not bound to admission `{admission_id}`"
            ),
            Self::PhysicalReceiptMismatch {
                admission_id,
                expected_host,
                received_host,
                expected_instance,
                received_instance,
            } => write!(
                f,
                "physical receipt mismatch for `{admission_id}`: expected `{expected_host}`/`{expected_instance}`, received `{received_host}`/`{received_instance}`"
            ),
            Self::ConflictingLocalCompletion {
                admission_id,
                first,
                second,
            } => write!(
                f,
                "admission `{admission_id}` has conflicting local completions {first:?} and {second:?}"
            ),
            Self::DuplicateLocalCompletion(admission_id) => write!(
                f,
                "admission `{admission_id}` already has a local completion receipt"
            ),
            Self::InvalidProviderNotStarted {
                admission_id,
                reason,
            } => write!(
                f,
                "invalid provider-not-started receipt for `{admission_id}`: {reason}"
            ),
            Self::UnknownPhysicalHost(host_id) => {
                write!(f, "unknown physical host `{host_id}`")
            }
            Self::InvalidCapacityEvidence { host_id, reason } => {
                write!(f, "invalid capacity evidence for `{host_id}`: {reason}")
            }
            Self::FleetSnapshotMismatch { expected, current } => write!(
                f,
                "capacity update expected fleet snapshot `{expected}`, but current is `{current}`"
            ),
            Self::AdmissionWaiterClosed(work_id) => write!(
                f,
                "broker admission waiter for `{work_id}` closed before a receipt arrived"
            ),
            Self::SourceRevisionRollback {
                authority,
                current,
                proposed,
            } => write!(
                f,
                "source authority `{authority}` cannot roll back from {current:?} to {proposed:?}"
            ),
            Self::ConflictingSourceRevision {
                authority,
                current,
                proposed,
            } => write!(
                f,
                "source authority `{authority}` has conflicting equal revision values {current:?} and {proposed:?}"
            ),
            Self::SourceRevisionMismatch {
                authority,
                current,
                requested,
            } => write!(
                f,
                "source authority `{authority}` cannot remove {requested:?}; current is {current:?}"
            ),
            Self::ProviderStartsClosed(admission_id) => write!(
                f,
                "admission `{admission_id}` is closed to new provider requests"
            ),
            Self::ConcurrentProviderRequest(admission_id) => write!(
                f,
                "admission `{admission_id}` already has a live or queued provider request"
            ),
            Self::OutcomeConflict {
                admission_id,
                reason,
            } => write!(
                f,
                "admission `{admission_id}` has contradictory local/provider outcomes: {reason}"
            ),
        }
    }
}

impl std::error::Error for BrokerError {}

#[derive(Clone, Debug)]
struct QueuedWork {
    opportunity: WorkOpportunity,
    sequence: u64,
}

#[derive(Clone, Debug)]
struct QueuedProviderRequest {
    receipt: ProviderRequestReceipt,
    work_id: String,
    priority: WorkPriority,
    task_rank: u64,
    sequence: u64,
}

#[derive(Clone, Debug)]
struct ProviderTurn {
    start: ProviderRequestReceipt,
    terminal: Option<ProviderTerminalReceipt>,
}

#[derive(Clone, Debug)]
struct ActiveAdmission {
    receipt: AdmissionReceipt,
    provider_requests: BTreeMap<u32, ProviderTurn>,
    pending_provider_request: Option<ProviderRequestReceipt>,
    provider_turn_permit_reserved: bool,
    live_provider_ordinal: Option<u32>,
    provider_starts_closed: bool,
    local_completion: Option<LocalCompletionKind>,
    provider_not_started: bool,
}

/// Stateful admission controller. There is intentionally no admitted-request cancellation API.
pub struct PhysicalBroker {
    correlation_scope: String,
    base_snapshot_id: String,
    snapshot_id: String,
    snapshot_revision: u64,
    lanes: Vec<VerifiedPhysicalLane>,
    host_capacities: HashMap<String, u32>,
    instance_capacities: HashMap<(String, String), u32>,
    current_versions: HashMap<String, TaskVersion>,
    pending: BTreeMap<String, QueuedWork>,
    pending_provider_requests: BTreeMap<String, QueuedProviderRequest>,
    active: BTreeMap<String, ActiveAdmission>,
    queue_sequence: u64,
    admission_sequence: u64,
}

impl PhysicalBroker {
    pub fn new(
        correlation_scope: impl Into<String>,
        snapshot: PhysicalFleetSnapshot,
    ) -> Result<Self, BrokerError> {
        let correlation_scope = correlation_scope.into();
        if correlation_scope.trim().is_empty() {
            return Err(BrokerError::EmptyCorrelationScope);
        }
        snapshot.validate()?;
        let mut host_capacities = HashMap::new();
        let mut instance_capacities = HashMap::new();
        for lane in &snapshot.lanes {
            host_capacities.insert(
                lane.host_id.clone(),
                lane.capacity_evidence.max_concurrent(),
            );
            instance_capacities.insert(
                (lane.host_id.clone(), lane.model_instance_id.clone()),
                lane.advertised_instance_capacity,
            );
        }
        Ok(Self {
            correlation_scope,
            base_snapshot_id: snapshot.snapshot_id.clone(),
            snapshot_id: snapshot.snapshot_id,
            snapshot_revision: 0,
            lanes: snapshot.lanes,
            host_capacities,
            instance_capacities,
            current_versions: HashMap::new(),
            pending: BTreeMap::new(),
            pending_provider_requests: BTreeMap::new(),
            active: BTreeMap::new(),
            queue_sequence: 0,
            admission_sequence: 0,
        })
    }

    pub fn set_source_revision(
        &mut self,
        source: TaskVersion,
    ) -> Result<Vec<StaleWorkReceipt>, BrokerError> {
        source
            .validate()
            .map_err(|reason| BrokerError::InvalidOpportunity {
                work_id: format!("source:{}", source.task_id),
                reason,
            })?;
        let authority = source.authority_key();
        if let Some(current) = self.current_versions.get(&authority) {
            if current == &source {
                return Ok(Vec::new());
            }
            if source.revision < current.revision
                || (source.revision == current.revision && source.attempt < current.attempt)
            {
                return Err(BrokerError::SourceRevisionRollback {
                    authority,
                    current: Box::new(current.clone()),
                    proposed: Box::new(source),
                });
            }
            if source.revision == current.revision {
                return Err(BrokerError::ConflictingSourceRevision {
                    authority,
                    current: Box::new(current.clone()),
                    proposed: Box::new(source),
                });
            }
        }
        self.current_versions.insert(authority, source);
        Ok(self.prune_stale())
    }

    pub fn remove_source_revision(
        &mut self,
        source: &TaskVersion,
    ) -> Result<Vec<StaleWorkReceipt>, BrokerError> {
        let authority = source.authority_key();
        let current = self.current_versions.get(&authority).cloned();
        if current.as_ref() != Some(source) {
            return Err(BrokerError::SourceRevisionMismatch {
                authority,
                current: current.map(Box::new),
                requested: Box::new(source.clone()),
            });
        }
        self.current_versions.remove(&authority);
        Ok(self.prune_stale())
    }

    pub fn enqueue(&mut self, opportunity: WorkOpportunity) -> Result<QueueReceipt, BrokerError> {
        validate_opportunity(&opportunity)?;
        self.validate_routes(&opportunity)?;
        if self.pending.contains_key(&opportunity.work_id)
            || self
                .active
                .values()
                .any(|active| active.receipt.work_id == opportunity.work_id)
        {
            return Err(BrokerError::DuplicateWork(opportunity.work_id));
        }
        if !self.is_current(&opportunity.source) {
            return Err(BrokerError::StaleOpportunity {
                work_id: opportunity.work_id,
                queued: Box::new(opportunity.source.clone()),
                current: self.current_source(&opportunity.source).map(Box::new),
            });
        }
        self.queue_sequence += 1;
        let receipt = QueueReceipt {
            work_id: opportunity.work_id.clone(),
            role: opportunity.role,
            priority: opportunity.priority,
            task_rank: opportunity.task_rank,
            source: opportunity.source.clone(),
            queue_sequence: self.queue_sequence,
        };
        self.pending.insert(
            opportunity.work_id.clone(),
            QueuedWork {
                opportunity,
                sequence: self.queue_sequence,
            },
        );
        Ok(receipt)
    }

    pub fn prune_stale(&mut self) -> Vec<StaleWorkReceipt> {
        let stale_ids: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, queued)| !self.is_current(&queued.opportunity.source))
            .map(|(work_id, _)| work_id.clone())
            .collect();
        stale_ids
            .into_iter()
            .filter_map(|work_id| {
                let queued = self.pending.remove(&work_id)?;
                Some(StaleWorkReceipt {
                    current_source: self.current_source(&queued.opportunity.source),
                    queued_source: queued.opportunity.source,
                    role: queued.opportunity.role,
                    work_id,
                })
            })
            .collect()
    }

    fn current_source(&self, source: &TaskVersion) -> Option<TaskVersion> {
        self.current_versions.get(&source.authority_key()).cloned()
    }

    fn is_current(&self, source: &TaskVersion) -> bool {
        self.current_source(source).as_ref() == Some(source)
    }

    fn validate_routes(&self, opportunity: &WorkOpportunity) -> Result<(), BrokerError> {
        let known_ids: std::collections::HashSet<&str> = self
            .lanes
            .iter()
            .map(|lane| lane.logical_device_id.as_str())
            .collect();
        if let Some(unknown) = opportunity
            .eligible_logical_device_ids
            .iter()
            .find(|id| !known_ids.contains(id.as_str()))
        {
            return Err(BrokerError::InvalidOpportunity {
                work_id: opportunity.work_id.clone(),
                reason: format!("eligible route `{unknown}` is not in the verified snapshot"),
            });
        }
        if let Some(excluded) = opportunity.excluded_logical_device_id.as_deref() {
            if !known_ids.contains(excluded) {
                return Err(BrokerError::InvalidOpportunity {
                    work_id: opportunity.work_id.clone(),
                    reason: format!("excluded route `{excluded}` is not in the verified snapshot"),
                });
            }
        }
        let has_route = self.lanes.iter().any(|lane| {
            (opportunity.eligible_logical_device_ids.is_empty()
                || opportunity
                    .eligible_logical_device_ids
                    .contains(&lane.logical_device_id))
                && opportunity.excluded_logical_device_id.as_deref()
                    != Some(lane.logical_device_id.as_str())
        });
        if !has_route {
            return Err(BrokerError::InvalidOpportunity {
                work_id: opportunity.work_id.clone(),
                reason: "route constraints exclude every verified lane".to_string(),
            });
        }
        Ok(())
    }

    pub fn grant_next(&mut self) -> Option<BrokerGrant> {
        self.prune_stale();
        let host_occupancy = self.host_occupancy();
        let instance_occupancy = self.instance_occupancy();
        let selected_work = self
            .pending
            .iter()
            .filter_map(|(work_id, queued)| {
                let lane =
                    self.select_lane(&queued.opportunity, &host_occupancy, &instance_occupancy)?;
                Some((work_id.clone(), queued.clone(), lane))
            })
            .max_by(|(_, left, _), (_, right, _)| compare_queued(left, right));
        let selected_provider = self
            .pending_provider_requests
            .iter()
            .filter(|(_, queued)| {
                self.route_has_capacity(
                    &queued.receipt.physical_host_id,
                    &queued.receipt.model_instance_id,
                    &host_occupancy,
                    &instance_occupancy,
                )
            })
            .max_by(|(_, left), (_, right)| compare_provider_queued(left, right))
            .map(|(admission_id, queued)| (admission_id.clone(), queued.clone()));

        let grant_work = match (&selected_work, &selected_provider) {
            (Some((work_id, queued, _)), Some((_admission_id, provider))) => {
                compare_queue_values(
                    queued.opportunity.priority,
                    queued.opportunity.task_rank,
                    queued.sequence,
                    work_id,
                    provider.priority,
                    provider.task_rank,
                    provider.sequence,
                    &provider.work_id,
                ) != Ordering::Less
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => return None,
        };

        if !grant_work {
            let (admission_id, queued) = selected_provider.expect("provider grant was selected");
            self.pending_provider_requests.remove(&admission_id);
            let active = self
                .active
                .get_mut(&admission_id)
                .expect("queued provider request belongs to an active admission");
            active.pending_provider_request = None;
            active.live_provider_ordinal = Some(queued.receipt.key.ordinal);
            active.provider_requests.insert(
                queued.receipt.key.ordinal,
                ProviderTurn {
                    start: queued.receipt.clone(),
                    terminal: None,
                },
            );
            return Some(BrokerGrant::ProviderRequest {
                admission: active.receipt.clone(),
                receipt: queued.receipt,
            });
        }

        let (work_id, queued, lane_index) = selected_work.expect("work grant was selected");
        self.pending.remove(&work_id);
        self.admission_sequence += 1;
        let lane = &self.lanes[lane_index];
        let admission_id = format!(
            "{}:admission:{:08}",
            self.correlation_scope, self.admission_sequence
        );
        let receipt = AdmissionReceipt {
            admission_id: admission_id.clone(),
            work_id,
            role: queued.opportunity.role,
            priority: queued.opportunity.priority,
            task_rank: queued.opportunity.task_rank,
            source: queued.opportunity.source,
            fleet_snapshot_id: self.snapshot_id.clone(),
            logical_device_id: lane.logical_device_id.clone(),
            model_id: lane.model_id.clone(),
            physical_host_id: lane.host_id.clone(),
            model_instance_id: lane.model_instance_id.clone(),
            provider_transport_id: lane.provider_transport_id.clone(),
            route_evidence_id: lane.route_evidence_id.clone(),
            capacity_evidence: lane.capacity_evidence.clone(),
            queue_sequence: queued.sequence,
            admission_sequence: self.admission_sequence,
        };
        self.active.insert(
            admission_id,
            ActiveAdmission {
                receipt: receipt.clone(),
                provider_requests: BTreeMap::new(),
                pending_provider_request: None,
                provider_turn_permit_reserved: true,
                live_provider_ordinal: None,
                provider_starts_closed: false,
                local_completion: None,
                provider_not_started: false,
            },
        );
        Some(BrokerGrant::Admission(receipt))
    }

    fn route_has_capacity(
        &self,
        host_id: &str,
        instance_id: &str,
        host_occupancy: &HashMap<String, u32>,
        instance_occupancy: &HashMap<(String, String), u32>,
    ) -> bool {
        let instance_key = (host_id.to_string(), instance_id.to_string());
        host_occupancy.get(host_id).copied().unwrap_or(0) < self.host_capacities[host_id]
            && instance_occupancy.get(&instance_key).copied().unwrap_or(0)
                < self.instance_capacities[&instance_key]
    }

    fn select_lane(
        &self,
        opportunity: &WorkOpportunity,
        host_occupancy: &HashMap<String, u32>,
        instance_occupancy: &HashMap<(String, String), u32>,
    ) -> Option<usize> {
        self.lanes
            .iter()
            .enumerate()
            .filter(|(_, lane)| {
                if !opportunity.eligible_logical_device_ids.is_empty()
                    && !opportunity
                        .eligible_logical_device_ids
                        .contains(&lane.logical_device_id)
                {
                    return false;
                }
                if opportunity.excluded_logical_device_id.as_deref()
                    == Some(lane.logical_device_id.as_str())
                {
                    return false;
                }
                let host_used = host_occupancy.get(&lane.host_id).copied().unwrap_or(0);
                let instance_key = (lane.host_id.clone(), lane.model_instance_id.clone());
                let instance_used = instance_occupancy.get(&instance_key).copied().unwrap_or(0);
                host_used < self.host_capacities[&lane.host_id]
                    && instance_used < self.instance_capacities[&instance_key]
            })
            .min_by_key(|(index, lane)| {
                let host_used = host_occupancy.get(&lane.host_id).copied().unwrap_or(0);
                let instance_used = instance_occupancy
                    .get(&(lane.host_id.clone(), lane.model_instance_id.clone()))
                    .copied()
                    .unwrap_or(0);
                let preferred_rank = match opportunity.preferred_model_id.as_deref() {
                    Some(preferred) if preferred == lane.model_id => 0,
                    _ => 1,
                };
                (
                    host_used,
                    instance_used,
                    preferred_rank,
                    u32::MAX - lane.routing_weight.max(1),
                    *index,
                )
            })
            .map(|(index, _)| index)
    }

    pub fn request_provider_turn(
        &mut self,
        receipt: ProviderRequestReceipt,
    ) -> Result<ProviderRequestDisposition, BrokerError> {
        if receipt.key.provider_request_id.trim().is_empty() {
            return Err(BrokerError::InvalidProviderRequest {
                admission_id: receipt.admission_id,
                reason: "provider request id is empty".to_string(),
            });
        }
        for active in self.active.values() {
            for turn in active.provider_requests.values() {
                if turn.start.key.provider_request_id == receipt.key.provider_request_id {
                    return Err(BrokerError::DuplicateProviderRequest(receipt.key));
                }
            }
            if active
                .pending_provider_request
                .as_ref()
                .is_some_and(|pending| {
                    pending.key.provider_request_id == receipt.key.provider_request_id
                })
            {
                return Err(BrokerError::DuplicateProviderRequest(receipt.key));
            }
        }
        let active = self
            .active
            .get_mut(&receipt.admission_id)
            .ok_or_else(|| BrokerError::UnknownAdmission(receipt.admission_id.clone()))?;
        validate_physical_receipt(
            &active.receipt,
            &receipt.admission_id,
            &receipt.physical_host_id,
            &receipt.model_instance_id,
        )?;
        if active.provider_not_started {
            return Err(BrokerError::InvalidProviderNotStarted {
                admission_id: receipt.admission_id,
                reason: "a provider request arrived after provider-not-started was certified"
                    .to_string(),
            });
        }
        if active.provider_starts_closed {
            return Err(BrokerError::ProviderStartsClosed(receipt.admission_id));
        }
        if active.live_provider_ordinal.is_some() || active.pending_provider_request.is_some() {
            return Err(BrokerError::ConcurrentProviderRequest(receipt.admission_id));
        }
        if active.provider_requests.contains_key(&receipt.key.ordinal) {
            return Err(BrokerError::DuplicateProviderRequest(receipt.key));
        }
        if active.provider_turn_permit_reserved {
            active.provider_turn_permit_reserved = false;
            active.live_provider_ordinal = Some(receipt.key.ordinal);
            active.provider_requests.insert(
                receipt.key.ordinal,
                ProviderTurn {
                    start: receipt.clone(),
                    terminal: None,
                },
            );
            return Ok(ProviderRequestDisposition::Granted(receipt));
        }
        self.queue_sequence += 1;
        active.pending_provider_request = Some(receipt.clone());
        let queued = ProviderRequestQueueReceipt {
            request: receipt.clone(),
            priority: active.receipt.priority,
            task_rank: active.receipt.task_rank,
            queue_sequence: self.queue_sequence,
        };
        self.pending_provider_requests.insert(
            receipt.admission_id.clone(),
            QueuedProviderRequest {
                receipt,
                work_id: active.receipt.work_id.clone(),
                priority: queued.priority,
                task_rank: queued.task_rank,
                sequence: self.queue_sequence,
            },
        );
        Ok(ProviderRequestDisposition::Queued(queued))
    }

    pub fn close_provider_starts(
        &mut self,
        admission_id: &str,
    ) -> Result<ProviderStartsClosure, BrokerError> {
        let active = self
            .active
            .get_mut(admission_id)
            .ok_or_else(|| BrokerError::UnknownAdmission(admission_id.to_string()))?;
        if active.provider_starts_closed {
            return Ok(ProviderStartsClosure {
                admission: None,
                provider_not_started: None,
                pending_provider_request: None,
            });
        }
        active.provider_starts_closed = true;
        let pending_provider_request = active.pending_provider_request.take();
        if pending_provider_request.is_some() {
            self.pending_provider_requests.remove(admission_id);
        }
        let provider_not_started = if active.provider_turn_permit_reserved
            && active.provider_requests.is_empty()
            && pending_provider_request.is_none()
        {
            active.provider_turn_permit_reserved = false;
            active.provider_not_started = true;
            Some(ProviderNotStartedReceipt {
                admission_id: admission_id.to_string(),
                physical_host_id: active.receipt.physical_host_id.clone(),
                model_instance_id: active.receipt.model_instance_id.clone(),
                reason: "provider lifecycle closed with no provider request".to_string(),
            })
        } else {
            None
        };
        Ok(ProviderStartsClosure {
            admission: Some(active.receipt.clone()),
            provider_not_started,
            pending_provider_request,
        })
    }

    pub fn record_local_completion(
        &mut self,
        admission_id: &str,
        kind: LocalCompletionKind,
    ) -> Result<LocalCompletionReceipt, BrokerError> {
        let active = self
            .active
            .get_mut(admission_id)
            .ok_or_else(|| BrokerError::UnknownAdmission(admission_id.to_string()))?;
        if !active.provider_starts_closed {
            return Err(BrokerError::InvalidProviderRequest {
                admission_id: admission_id.to_string(),
                reason: "provider starts must close before local completion".to_string(),
            });
        }
        if let Some(first) = active.local_completion {
            if first != kind {
                return Err(BrokerError::ConflictingLocalCompletion {
                    admission_id: admission_id.to_string(),
                    first,
                    second: kind,
                });
            }
            return Err(BrokerError::DuplicateLocalCompletion(
                admission_id.to_string(),
            ));
        }
        active.local_completion = Some(kind);
        Ok(LocalCompletionReceipt {
            admission_id: admission_id.to_string(),
            work_id: active.receipt.work_id.clone(),
            physical_host_id: active.receipt.physical_host_id.clone(),
            model_instance_id: active.receipt.model_instance_id.clone(),
            kind,
            provider_requests_started: active.provider_requests.len(),
            provider_requests_terminal: active
                .provider_requests
                .values()
                .filter(|turn| turn.terminal.is_some())
                .count(),
        })
    }

    pub fn observe_provider_terminal(
        &mut self,
        terminal: ProviderTerminalReceipt,
    ) -> Result<(), BrokerError> {
        let active = self
            .active
            .get_mut(&terminal.admission_id)
            .ok_or_else(|| BrokerError::UnknownAdmission(terminal.admission_id.clone()))?;
        validate_physical_receipt(
            &active.receipt,
            &terminal.admission_id,
            &terminal.physical_host_id,
            &terminal.model_instance_id,
        )?;
        let Some(turn) = active.provider_requests.get_mut(&terminal.key.ordinal) else {
            return Err(BrokerError::ProviderRequestMismatch {
                admission_id: terminal.admission_id,
                received: terminal.key,
            });
        };
        if turn.start.key != terminal.key {
            return Err(BrokerError::ProviderRequestMismatch {
                admission_id: terminal.admission_id,
                received: terminal.key,
            });
        }
        if turn.terminal.is_some() {
            return Err(BrokerError::DuplicateProviderTerminal(terminal.key));
        }
        turn.terminal = Some(terminal);
        active.live_provider_ordinal = None;
        Ok(())
    }

    pub fn record_provider_not_started(
        &mut self,
        receipt: ProviderNotStartedReceipt,
    ) -> Result<(), BrokerError> {
        let active = self
            .active
            .get_mut(&receipt.admission_id)
            .ok_or_else(|| BrokerError::UnknownAdmission(receipt.admission_id.clone()))?;
        validate_physical_receipt(
            &active.receipt,
            &receipt.admission_id,
            &receipt.physical_host_id,
            &receipt.model_instance_id,
        )?;
        if receipt.reason.trim().is_empty() {
            return Err(BrokerError::InvalidProviderNotStarted {
                admission_id: receipt.admission_id,
                reason: "reason is empty".to_string(),
            });
        }
        if !active.provider_requests.is_empty() || active.pending_provider_request.is_some() {
            return Err(BrokerError::InvalidProviderNotStarted {
                admission_id: receipt.admission_id,
                reason: "one or more provider requests already started".to_string(),
            });
        }
        if active.provider_starts_closed {
            return Err(BrokerError::InvalidProviderNotStarted {
                admission_id: receipt.admission_id,
                reason: "provider starts were already closed".to_string(),
            });
        }
        active.provider_turn_permit_reserved = false;
        active.provider_starts_closed = true;
        if active.provider_not_started {
            return Err(BrokerError::InvalidProviderNotStarted {
                admission_id: receipt.admission_id,
                reason: "provider-not-started was already certified".to_string(),
            });
        }
        active.provider_not_started = true;
        Ok(())
    }

    /// Release only after local work ended and either no provider call started (explicit receipt) or
    /// every provider turn has an exact terminal receipt.
    pub fn release_if_terminal(
        &mut self,
        admission_id: &str,
    ) -> Result<Option<ReleasedAdmissionReceipt>, BrokerError> {
        let Some(active) = self.active.get(admission_id) else {
            return Err(BrokerError::UnknownAdmission(admission_id.to_string()));
        };
        let Some(local_completion) = active.local_completion else {
            return Ok(None);
        };
        let releasable = active.provider_starts_closed
            && !active.provider_turn_permit_reserved
            && active.live_provider_ordinal.is_none()
            && active.pending_provider_request.is_none()
            && (active.provider_not_started
                || (!active.provider_requests.is_empty()
                    && active
                        .provider_requests
                        .values()
                        .all(|turn| turn.terminal.is_some())));
        if !releasable {
            return Ok(None);
        }
        let active = self
            .active
            .remove(admission_id)
            .expect("active admission was checked above");
        let provider_terminals: Vec<ProviderTerminalReceipt> = active
            .provider_requests
            .into_values()
            .filter_map(|turn| turn.terminal)
            .collect();
        // Provider calls are ordered attempts within one admitted task. Goose can recover from a
        // failed call by compacting or by running a configured retry, so an earlier failure is not
        // a contradiction when a later correlated call finishes. Keep every terminal receipt for
        // audit, but reconcile the local outcome against the final provider attempt.
        let final_provider_succeeded = provider_terminals
            .last()
            .is_some_and(|terminal| terminal.kind == ProviderTerminalKind::Finished);
        let effective_completion = if local_completion == LocalCompletionKind::Success
            && (active.provider_not_started || !final_provider_succeeded)
        {
            LocalCompletionKind::Error
        } else {
            local_completion
        };
        Ok(Some(ReleasedAdmissionReceipt {
            admission: active.receipt,
            local_completion: effective_completion,
            provider_terminals,
            provider_not_started: active.provider_not_started,
        }))
    }

    pub fn update_host_capacity(
        &mut self,
        host_id: &str,
        expected_fleet_snapshot_id: &str,
        evidence: HostCapacityEvidence,
    ) -> Result<CapacityUpdateReceipt, BrokerError> {
        evidence
            .validate()
            .map_err(|reason| BrokerError::InvalidCapacityEvidence {
                host_id: host_id.to_string(),
                reason,
            })?;
        let previous_capacity = self
            .host_capacities
            .get(host_id)
            .copied()
            .ok_or_else(|| BrokerError::UnknownPhysicalHost(host_id.to_string()))?;
        if expected_fleet_snapshot_id != self.snapshot_id {
            return Err(BrokerError::FleetSnapshotMismatch {
                expected: expected_fleet_snapshot_id.to_string(),
                current: self.snapshot_id.clone(),
            });
        }
        let new_capacity = evidence.max_concurrent();
        let previous_fleet_snapshot_id = self.snapshot_id.clone();
        self.snapshot_revision += 1;
        self.snapshot_id = format!(
            "{}:capacity:{:08}",
            self.base_snapshot_id, self.snapshot_revision
        );
        self.host_capacities
            .insert(host_id.to_string(), new_capacity);
        for lane in self.lanes.iter_mut().filter(|lane| lane.host_id == host_id) {
            lane.capacity_evidence = evidence.clone();
        }
        Ok(CapacityUpdateReceipt {
            physical_host_id: host_id.to_string(),
            previous_fleet_snapshot_id,
            new_fleet_snapshot_id: self.snapshot_id.clone(),
            previous_capacity,
            new_capacity,
            capacity_evidence: evidence,
        })
    }

    pub(crate) fn withdraw_pending_work(&mut self, work_id: &str) -> Option<WithdrawnWorkReceipt> {
        self.pending
            .remove(work_id)
            .map(|queued| WithdrawnWorkReceipt {
                work_id: queued.opportunity.work_id,
                role: queued.opportunity.role,
                source: queued.opportunity.source,
            })
    }

    pub(crate) fn revoke_undelivered_admission(
        &mut self,
        admission_id: &str,
        reason: impl Into<String>,
    ) -> Result<RevokedAdmissionReceipt, BrokerError> {
        let active = self
            .active
            .get(admission_id)
            .ok_or_else(|| BrokerError::UnknownAdmission(admission_id.to_string()))?;
        let untouched = active.provider_turn_permit_reserved
            && active.provider_requests.is_empty()
            && active.pending_provider_request.is_none()
            && active.live_provider_ordinal.is_none()
            && !active.provider_starts_closed
            && active.local_completion.is_none()
            && !active.provider_not_started;
        if !untouched {
            return Err(BrokerError::InvalidProviderRequest {
                admission_id: admission_id.to_string(),
                reason: "only an undelivered, unstarted admission grant can be revoked".to_string(),
            });
        }
        let active = self
            .active
            .remove(admission_id)
            .expect("untouched admission was checked above");
        Ok(RevokedAdmissionReceipt {
            admission: active.receipt,
            reason: reason.into(),
        })
    }

    pub(crate) fn withdraw_pending_provider_request(
        &mut self,
        admission_id: &str,
        key: &ProviderRequestKey,
    ) -> Result<ProviderRequestReceipt, BrokerError> {
        let queued = self
            .pending_provider_requests
            .get(admission_id)
            .ok_or_else(|| BrokerError::ProviderRequestMismatch {
                admission_id: admission_id.to_string(),
                received: key.clone(),
            })?;
        if queued.receipt.key != *key {
            return Err(BrokerError::ProviderRequestMismatch {
                admission_id: admission_id.to_string(),
                received: key.clone(),
            });
        }
        let queued = self
            .pending_provider_requests
            .remove(admission_id)
            .expect("queued provider request was checked above");
        let active = self
            .active
            .get_mut(admission_id)
            .expect("queued provider request belongs to active admission");
        active.pending_provider_request = None;
        Ok(queued.receipt)
    }

    pub(crate) fn revoke_undelivered_provider_request(
        &mut self,
        admission_id: &str,
        key: &ProviderRequestKey,
    ) -> Result<ProviderRequestReceipt, BrokerError> {
        let active = self
            .active
            .get_mut(admission_id)
            .ok_or_else(|| BrokerError::UnknownAdmission(admission_id.to_string()))?;
        if active.live_provider_ordinal != Some(key.ordinal) {
            return Err(BrokerError::ProviderRequestMismatch {
                admission_id: admission_id.to_string(),
                received: key.clone(),
            });
        }
        let turn = active.provider_requests.get(&key.ordinal).ok_or_else(|| {
            BrokerError::ProviderRequestMismatch {
                admission_id: admission_id.to_string(),
                received: key.clone(),
            }
        })?;
        if turn.start.key != *key || turn.terminal.is_some() {
            return Err(BrokerError::ProviderRequestMismatch {
                admission_id: admission_id.to_string(),
                received: key.clone(),
            });
        }
        let turn = active
            .provider_requests
            .remove(&key.ordinal)
            .expect("undelivered provider grant was checked above");
        active.live_provider_ordinal = None;
        Ok(turn.start)
    }

    pub fn unresolved_admissions(&self) -> Vec<UnresolvedAdmissionReceipt> {
        self.active
            .values()
            .map(|active| UnresolvedAdmissionReceipt {
                admission: active.receipt.clone(),
                provider_requests_started: active.provider_requests.len(),
                provider_requests_terminal: active
                    .provider_requests
                    .values()
                    .filter(|turn| turn.terminal.is_some())
                    .count(),
                provider_request_pending: active.pending_provider_request.is_some(),
                provider_turn_permit_held: active.provider_turn_permit_reserved
                    || active.live_provider_ordinal.is_some(),
                provider_starts_closed: active.provider_starts_closed,
                local_completion: active.local_completion,
            })
            .collect()
    }

    pub fn snapshot(&self) -> PhysicalFleetSnapshot {
        PhysicalFleetSnapshot {
            snapshot_id: self.snapshot_id.clone(),
            lanes: self.lanes.clone(),
        }
    }

    pub fn active_receipt(&self, admission_id: &str) -> Option<&AdmissionReceipt> {
        self.active.get(admission_id).map(|active| &active.receipt)
    }

    pub(crate) fn active_receipt_for_work(&self, work_id: &str) -> Option<&AdmissionReceipt> {
        self.active
            .values()
            .find(|active| active.receipt.work_id == work_id)
            .map(|active| &active.receipt)
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len() + self.pending_provider_requests.len()
    }

    pub fn active_len(&self) -> usize {
        self.active.len()
    }

    pub fn active_on_host(&self, host_id: &str) -> usize {
        self.host_occupancy().get(host_id).copied().unwrap_or(0) as usize
    }

    pub fn physical_occupancy(&self) -> Vec<PhysicalHostOccupancy> {
        let occupancy = self.host_occupancy();
        let mut hosts: BTreeMap<&str, &HostCapacityEvidence> = BTreeMap::new();
        for lane in &self.lanes {
            hosts
                .entry(lane.host_id.as_str())
                .or_insert(&lane.capacity_evidence);
        }
        hosts
            .into_iter()
            .map(|(host_id, evidence)| PhysicalHostOccupancy {
                physical_host_id: host_id.to_string(),
                provider_turn_permits_held: occupancy.get(host_id).copied().unwrap_or(0),
                capacity: self.host_capacities[host_id],
                capacity_evidence: evidence.clone(),
            })
            .collect()
    }

    pub fn pending_work_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.pending.keys().cloned().collect();
        ids.extend(self.pending_provider_requests.values().map(|queued| {
            format!(
                "{}:provider:{}",
                queued.receipt.admission_id, queued.receipt.key.ordinal
            )
        }));
        ids.sort();
        ids
    }

    fn host_occupancy(&self) -> HashMap<String, u32> {
        let mut occupancy = HashMap::new();
        for active in self.active.values().filter(|active| {
            active.provider_turn_permit_reserved || active.live_provider_ordinal.is_some()
        }) {
            *occupancy
                .entry(active.receipt.physical_host_id.clone())
                .or_default() += 1;
        }
        occupancy
    }

    fn instance_occupancy(&self) -> HashMap<(String, String), u32> {
        let mut occupancy = HashMap::new();
        for active in self.active.values().filter(|active| {
            active.provider_turn_permit_reserved || active.live_provider_ordinal.is_some()
        }) {
            *occupancy
                .entry((
                    active.receipt.physical_host_id.clone(),
                    active.receipt.model_instance_id.clone(),
                ))
                .or_default() += 1;
        }
        occupancy
    }
}

fn validate_opportunity(opportunity: &WorkOpportunity) -> Result<(), BrokerError> {
    if opportunity.work_id.trim().is_empty() {
        return Err(BrokerError::InvalidOpportunity {
            work_id: opportunity.work_id.clone(),
            reason: "work id is empty".to_string(),
        });
    }
    opportunity
        .source
        .validate()
        .map_err(|reason| BrokerError::InvalidOpportunity {
            work_id: opportunity.work_id.clone(),
            reason,
        })?;
    if opportunity.priority != opportunity.role.priority() {
        return Err(BrokerError::InvalidOpportunity {
            work_id: opportunity.work_id.clone(),
            reason: format!(
                "role {:?} requires priority {:?}, received {:?}",
                opportunity.role,
                opportunity.role.priority(),
                opportunity.priority
            ),
        });
    }
    let mut eligible = std::collections::HashSet::new();
    if let Some(duplicate) = opportunity
        .eligible_logical_device_ids
        .iter()
        .find(|id| id.trim().is_empty() || !eligible.insert(id.as_str()))
    {
        return Err(BrokerError::InvalidOpportunity {
            work_id: opportunity.work_id.clone(),
            reason: format!("eligible route `{duplicate}` is empty or duplicated"),
        });
    }
    let valid_authority = match opportunity.role {
        WorkRole::Build => {
            opportunity.source.attempt == 0
                && matches!(opportunity.source.kind, SourceRevisionKind::TaskAttempt)
        }
        WorkRole::Repair => {
            opportunity.source.attempt > 0
                && matches!(opportunity.source.kind, SourceRevisionKind::TaskAttempt)
        }
        WorkRole::RuntimeAcceptanceReview | WorkRole::ContractReview => {
            matches!(opportunity.source.kind, SourceRevisionKind::Contract { .. })
        }
        WorkRole::CompletedArtifactReview => {
            matches!(opportunity.source.kind, SourceRevisionKind::Artifact { .. })
        }
        WorkRole::SemanticJudgeObservation => {
            matches!(opportunity.source.kind, SourceRevisionKind::Trace { .. })
        }
        WorkRole::AcceptanceOracle => matches!(
            opportunity.source.kind,
            SourceRevisionKind::Artifact { .. } | SourceRevisionKind::Contract { .. }
        ),
    };
    if !valid_authority {
        return Err(BrokerError::InvalidOpportunity {
            work_id: opportunity.work_id.clone(),
            reason: format!(
                "role {:?} cannot use source authority {:?}",
                opportunity.role, opportunity.source.kind
            ),
        });
    }
    Ok(())
}

fn compare_queued(left: &QueuedWork, right: &QueuedWork) -> Ordering {
    compare_queue_values(
        left.opportunity.priority,
        left.opportunity.task_rank,
        left.sequence,
        &left.opportunity.work_id,
        right.opportunity.priority,
        right.opportunity.task_rank,
        right.sequence,
        &right.opportunity.work_id,
    )
}

fn compare_provider_queued(
    left: &QueuedProviderRequest,
    right: &QueuedProviderRequest,
) -> Ordering {
    compare_queue_values(
        left.priority,
        left.task_rank,
        left.sequence,
        &left.work_id,
        right.priority,
        right.task_rank,
        right.sequence,
        &right.work_id,
    )
}

#[allow(clippy::too_many_arguments)]
fn compare_queue_values(
    left_priority: WorkPriority,
    left_rank: u64,
    left_sequence: u64,
    left_id: &str,
    right_priority: WorkPriority,
    right_rank: u64,
    right_sequence: u64,
    right_id: &str,
) -> Ordering {
    left_priority
        .cmp(&right_priority)
        .then_with(|| left_rank.cmp(&right_rank))
        .then_with(|| right_id.cmp(left_id))
        .then_with(|| right_sequence.cmp(&left_sequence))
}

fn validate_physical_receipt(
    admission: &AdmissionReceipt,
    admission_id: &str,
    host_id: &str,
    model_instance_id: &str,
) -> Result<(), BrokerError> {
    if admission.physical_host_id != host_id || admission.model_instance_id != model_instance_id {
        return Err(BrokerError::PhysicalReceiptMismatch {
            admission_id: admission_id.to_string(),
            expected_host: admission.physical_host_id.clone(),
            received_host: host_id.to_string(),
            expected_instance: admission.model_instance_id.clone(),
            received_instance: model_instance_id.to_string(),
        });
    }
    Ok(())
}
