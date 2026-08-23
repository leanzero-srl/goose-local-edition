//! Physical-host admission for model work.
//!
//! A logical scheduler lane is not a physical decoder. This opt-in broker admits model work only
//! through a same-run physical fleet snapshot, keeps the physical claim across local completion,
//! and releases it only after every provider turn has an exact terminal receipt. It deliberately
//! has no API for cancelling admitted work.

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
    /// Deterministic tests only.
    ReplayFixture {
        fixture_id: String,
        max_concurrent: u32,
    },
}

impl HostCapacityEvidence {
    pub fn max_concurrent(&self) -> u32 {
        match self {
            Self::ProbeSingleStream { .. } => 1,
            Self::MeasuredProfile { max_concurrent, .. }
            | Self::ReplayFixture { max_concurrent, .. } => *max_concurrent,
        }
    }

    fn identity(&self) -> &str {
        match self {
            Self::ProbeSingleStream { probe_epoch } => probe_epoch,
            Self::MeasuredProfile { profile_hash, .. } => profile_hash,
            Self::ReplayFixture { fixture_id, .. } => fixture_id,
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
        let mut host_capacities = HashMap::new();
        let mut instance_capacities = HashMap::new();
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
            let host_capacity = lane.capacity_evidence.max_concurrent();
            if let Some(first) = host_capacities.insert(&lane.host_id, host_capacity) {
                if first != host_capacity {
                    return Err(BrokerError::ConflictingHostCapacity {
                        host_id: lane.host_id.clone(),
                        first,
                        second: host_capacity,
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
        ("route evidence id", lane.route_evidence_id.trim()),
    ]
    .into_iter()
    .find(|(_, value)| value.is_empty())
    .map(|(name, _)| format!("{name} is empty"))
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
            SourceRevisionKind::TaskAttempt => Ok(()),
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

/// One task-derived, version-current candidate for the common queue.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkOpportunity {
    pub work_id: String,
    pub role: WorkRole,
    pub priority: WorkPriority,
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
    pub source: TaskVersion,
    pub fleet_snapshot_id: String,
    pub logical_device_id: String,
    pub model_id: String,
    pub physical_host_id: String,
    pub model_instance_id: String,
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
    pub previous_capacity: u32,
    pub new_capacity: u32,
    pub capacity_evidence: HostCapacityEvidence,
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
    InvalidProviderNotStarted {
        admission_id: String,
        reason: String,
    },
    UnknownPhysicalHost(String),
    InvalidCapacityEvidence {
        host_id: String,
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
struct ProviderTurn {
    start: ProviderRequestReceipt,
    terminal: Option<ProviderTerminalReceipt>,
}

#[derive(Clone, Debug)]
struct ActiveAdmission {
    receipt: AdmissionReceipt,
    provider_requests: BTreeMap<u32, ProviderTurn>,
    local_completion: Option<LocalCompletionKind>,
    provider_not_started: bool,
}

/// Stateful admission controller. There is intentionally no admitted-request cancellation API.
pub struct PhysicalBroker {
    correlation_scope: String,
    snapshot_id: String,
    lanes: Vec<VerifiedPhysicalLane>,
    host_capacities: HashMap<String, u32>,
    instance_capacities: HashMap<(String, String), u32>,
    current_versions: HashMap<String, TaskVersion>,
    pending: BTreeMap<String, QueuedWork>,
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
            snapshot_id: snapshot.snapshot_id,
            lanes: snapshot.lanes,
            host_capacities,
            instance_capacities,
            current_versions: HashMap::new(),
            pending: BTreeMap::new(),
            active: BTreeMap::new(),
            queue_sequence: 0,
            admission_sequence: 0,
        })
    }

    pub fn set_source_revision(&mut self, source: TaskVersion) -> Vec<StaleWorkReceipt> {
        self.current_versions.insert(source.authority_key(), source);
        self.prune_stale()
    }

    pub fn remove_source_revision(&mut self, source: &TaskVersion) -> Vec<StaleWorkReceipt> {
        self.current_versions.remove(&source.authority_key());
        self.prune_stale()
    }

    pub fn enqueue(&mut self, opportunity: WorkOpportunity) -> Result<QueueReceipt, BrokerError> {
        validate_opportunity(&opportunity)?;
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

    pub fn admit_next(&mut self) -> Option<AdmissionReceipt> {
        self.prune_stale();
        let host_occupancy = self.host_occupancy();
        let instance_occupancy = self.instance_occupancy();
        let selected = self
            .pending
            .iter()
            .filter_map(|(work_id, queued)| {
                let lane =
                    self.select_lane(&queued.opportunity, &host_occupancy, &instance_occupancy)?;
                Some((work_id.clone(), queued.clone(), lane))
            })
            .max_by(|(_, left, _), (_, right, _)| compare_queued(left, right))?;

        let (work_id, queued, lane_index) = selected;
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
            source: queued.opportunity.source,
            fleet_snapshot_id: self.snapshot_id.clone(),
            logical_device_id: lane.logical_device_id.clone(),
            model_id: lane.model_id.clone(),
            physical_host_id: lane.host_id.clone(),
            model_instance_id: lane.model_instance_id.clone(),
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
                local_completion: None,
                provider_not_started: false,
            },
        );
        Some(receipt)
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

    pub fn bind_provider_request(
        &mut self,
        receipt: ProviderRequestReceipt,
    ) -> Result<(), BrokerError> {
        if self.active.values().any(|active| {
            active.provider_requests.values().any(|turn| {
                turn.start.key.provider_request_id == receipt.key.provider_request_id
                    && active.receipt.admission_id != receipt.admission_id
            })
        }) {
            return Err(BrokerError::DuplicateProviderRequest(receipt.key));
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
        if let Some(existing) = active.provider_requests.get(&receipt.key.ordinal) {
            if existing.start == receipt {
                return Ok(());
            }
            return Err(BrokerError::DuplicateProviderRequest(receipt.key));
        }
        active.provider_requests.insert(
            receipt.key.ordinal,
            ProviderTurn {
                start: receipt,
                terminal: None,
            },
        );
        Ok(())
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
        if let Some(first) = active.local_completion {
            if first != kind {
                return Err(BrokerError::ConflictingLocalCompletion {
                    admission_id: admission_id.to_string(),
                    first,
                    second: kind,
                });
            }
        } else {
            active.local_completion = Some(kind);
        }
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
        if !active.provider_requests.is_empty() {
            return Err(BrokerError::InvalidProviderNotStarted {
                admission_id: receipt.admission_id,
                reason: "one or more provider requests already started".to_string(),
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
        let releasable = active.provider_not_started
            || (!active.provider_requests.is_empty()
                && active
                    .provider_requests
                    .values()
                    .all(|turn| turn.terminal.is_some()));
        if !releasable {
            return Ok(None);
        }
        let active = self
            .active
            .remove(admission_id)
            .expect("active admission was checked above");
        Ok(Some(ReleasedAdmissionReceipt {
            admission: active.receipt,
            local_completion,
            provider_terminals: active
                .provider_requests
                .into_values()
                .filter_map(|turn| turn.terminal)
                .collect(),
            provider_not_started: active.provider_not_started,
        }))
    }

    pub fn update_host_capacity(
        &mut self,
        host_id: &str,
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
        let new_capacity = evidence.max_concurrent();
        self.host_capacities
            .insert(host_id.to_string(), new_capacity);
        for lane in self.lanes.iter_mut().filter(|lane| lane.host_id == host_id) {
            lane.capacity_evidence = evidence.clone();
        }
        Ok(CapacityUpdateReceipt {
            physical_host_id: host_id.to_string(),
            previous_capacity,
            new_capacity,
            capacity_evidence: evidence,
        })
    }

    pub fn active_receipt(&self, admission_id: &str) -> Option<&AdmissionReceipt> {
        self.active.get(admission_id).map(|active| &active.receipt)
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn active_len(&self) -> usize {
        self.active.len()
    }

    pub fn active_on_host(&self, host_id: &str) -> usize {
        self.active
            .values()
            .filter(|active| active.receipt.physical_host_id == host_id)
            .count()
    }

    pub fn pending_work_ids(&self) -> Vec<String> {
        self.pending.keys().cloned().collect()
    }

    fn host_occupancy(&self) -> HashMap<String, u32> {
        let mut occupancy = HashMap::new();
        for active in self.active.values() {
            *occupancy
                .entry(active.receipt.physical_host_id.clone())
                .or_default() += 1;
        }
        occupancy
    }

    fn instance_occupancy(&self) -> HashMap<(String, String), u32> {
        let mut occupancy = HashMap::new();
        for active in self.active.values() {
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
    let valid_authority = match opportunity.role {
        WorkRole::Build | WorkRole::Repair => {
            matches!(opportunity.source.kind, SourceRevisionKind::TaskAttempt)
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
    left.opportunity
        .priority
        .cmp(&right.opportunity.priority)
        .then_with(|| right.sequence.cmp(&left.sequence))
        .then_with(|| right.opportunity.work_id.cmp(&left.opportunity.work_id))
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
