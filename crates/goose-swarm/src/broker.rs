//! Physical-host admission for model requests.
//!
//! The scheduler's historical `DeviceCfg::weight` is a logical-lane limit. This broker is a
//! separate, opt-in correctness boundary: it admits work only against a verified physical host
//! and loaded model instance, and it keeps that occupancy until the provider emits a matching
//! terminal receipt. A local future ending is recorded but never interpreted as physical idleness.

use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalEvidenceKind {
    LmStudioProcessTable,
    MeasuredProfile,
    ReplayFixture,
}

/// A logical route whose physical identity and admission ceilings came from explicit evidence.
///
/// `host_capacity` is shared by every lane with the same `host_id`; it is deliberately not summed.
/// `instance_capacity` is shared by aliases with the same `(host_id, model_instance_id)` pair.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VerifiedPhysicalLane {
    pub logical_device_id: String,
    pub model_id: String,
    pub host_id: String,
    pub model_instance_id: String,
    pub host_capacity: u32,
    pub instance_capacity: u32,
    pub supervision_only: bool,
    pub routing_weight: u32,
    pub evidence_kind: PhysicalEvidenceKind,
    pub evidence_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
pub struct TaskVersion {
    pub task_id: String,
    pub attempt: u32,
    /// Monotonic scheduler-owned revision. It changes whenever the authoritative task state or
    /// artifact snapshot changes; it is not a model-authored timestamp.
    pub revision: u64,
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
    fn requires_task_version(self) -> bool {
        !matches!(self, Self::Build | Self::Repair)
    }

    fn requires_build_lane(self) -> bool {
        matches!(self, Self::Build | Self::Repair)
    }
}

/// One version-bound candidate for the common admission queue.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkOpportunity {
    pub work_id: String,
    pub role: WorkRole,
    pub priority: WorkPriority,
    pub source: TaskVersion,
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
    pub request_id: String,
    pub work_id: String,
    pub role: WorkRole,
    pub priority: WorkPriority,
    pub source: TaskVersion,
    pub logical_device_id: String,
    pub model_id: String,
    pub physical_host_id: String,
    pub model_instance_id: String,
    pub identity_evidence_id: String,
    pub queue_sequence: u64,
    pub admission_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderRequestReceipt {
    pub request_id: String,
    pub provider_request_id: String,
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
    pub request_id: String,
    pub work_id: String,
    pub physical_host_id: String,
    pub model_instance_id: String,
    pub kind: LocalCompletionKind,
    pub provider_terminal_observed: bool,
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
    pub request_id: String,
    pub provider_request_id: String,
    pub physical_host_id: String,
    pub model_instance_id: String,
    pub kind: ProviderTerminalKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReleasedAdmissionReceipt {
    pub admission: AdmissionReceipt,
    pub terminal: ProviderTerminalReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrokerError {
    EmptyCorrelationScope,
    NoVerifiedLanes,
    InvalidLane {
        device_id: String,
        reason: String,
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
        queued: TaskVersion,
        current: Option<TaskVersion>,
    },
    DuplicateWork(String),
    UnknownRequest(String),
    DuplicateProviderRequest(String),
    ProviderRequestMismatch {
        request_id: String,
        expected: Option<String>,
        received: String,
    },
    PhysicalReceiptMismatch {
        request_id: String,
        expected_host: String,
        received_host: String,
        expected_instance: String,
        received_instance: String,
    },
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyCorrelationScope => write!(f, "physical broker correlation scope is empty"),
            Self::NoVerifiedLanes => write!(f, "physical broker has no verified lanes"),
            Self::InvalidLane { device_id, reason } => {
                write!(f, "invalid verified lane `{device_id}`: {reason}")
            }
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
            Self::UnknownRequest(request_id) => {
                write!(f, "unknown broker request `{request_id}`")
            }
            Self::DuplicateProviderRequest(provider_request_id) => write!(
                f,
                "provider request `{provider_request_id}` is already bound to another admission"
            ),
            Self::ProviderRequestMismatch {
                request_id,
                expected,
                received,
            } => write!(
                f,
                "provider request mismatch for `{request_id}`: expected {expected:?}, received `{received}`"
            ),
            Self::PhysicalReceiptMismatch {
                request_id,
                expected_host,
                received_host,
                expected_instance,
                received_instance,
            } => write!(
                f,
                "physical receipt mismatch for `{request_id}`: expected `{expected_host}`/`{expected_instance}`, received `{received_host}`/`{received_instance}`"
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
struct ActiveAdmission {
    receipt: AdmissionReceipt,
    provider_request_id: Option<String>,
    local_completion: Option<LocalCompletionKind>,
}

/// Stateful admission controller. It intentionally has no API for cancelling an admitted request.
pub struct PhysicalBroker {
    correlation_scope: String,
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
        lanes: Vec<VerifiedPhysicalLane>,
    ) -> Result<Self, BrokerError> {
        let correlation_scope = correlation_scope.into();
        if correlation_scope.trim().is_empty() {
            return Err(BrokerError::EmptyCorrelationScope);
        }
        if lanes.is_empty() {
            return Err(BrokerError::NoVerifiedLanes);
        }

        let mut host_capacities = HashMap::new();
        let mut instance_capacities = HashMap::new();
        for lane in &lanes {
            Self::validate_lane(lane)?;
            if let Some(first) = host_capacities.insert(lane.host_id.clone(), lane.host_capacity) {
                if first != lane.host_capacity {
                    return Err(BrokerError::ConflictingHostCapacity {
                        host_id: lane.host_id.clone(),
                        first,
                        second: lane.host_capacity,
                    });
                }
            }
            let instance_key = (lane.host_id.clone(), lane.model_instance_id.clone());
            if let Some(first) = instance_capacities.insert(instance_key, lane.instance_capacity) {
                if first != lane.instance_capacity {
                    return Err(BrokerError::ConflictingInstanceCapacity {
                        host_id: lane.host_id.clone(),
                        model_instance_id: lane.model_instance_id.clone(),
                        first,
                        second: lane.instance_capacity,
                    });
                }
            }
        }

        Ok(Self {
            correlation_scope,
            lanes,
            host_capacities,
            instance_capacities,
            current_versions: HashMap::new(),
            pending: BTreeMap::new(),
            active: BTreeMap::new(),
            queue_sequence: 0,
            admission_sequence: 0,
        })
    }

    fn validate_lane(lane: &VerifiedPhysicalLane) -> Result<(), BrokerError> {
        let invalid = [
            ("logical device id", lane.logical_device_id.trim()),
            ("model id", lane.model_id.trim()),
            ("physical host id", lane.host_id.trim()),
            ("model instance id", lane.model_instance_id.trim()),
            ("identity evidence id", lane.evidence_id.trim()),
        ]
        .into_iter()
        .find(|(_, value)| value.is_empty())
        .map(|(name, _)| format!("{name} is empty"))
        .or_else(|| (lane.host_capacity == 0).then(|| "host capacity is zero".to_string()))
        .or_else(|| (lane.instance_capacity == 0).then(|| "instance capacity is zero".to_string()));
        if let Some(reason) = invalid {
            return Err(BrokerError::InvalidLane {
                device_id: lane.logical_device_id.clone(),
                reason,
            });
        }
        Ok(())
    }

    pub fn set_task_version(&mut self, version: TaskVersion) -> Vec<StaleWorkReceipt> {
        self.current_versions
            .insert(version.task_id.clone(), version);
        self.prune_stale()
    }

    pub fn remove_task_version(&mut self, task_id: &str) -> Vec<StaleWorkReceipt> {
        self.current_versions.remove(task_id);
        self.prune_stale()
    }

    pub fn enqueue(&mut self, opportunity: WorkOpportunity) -> Result<QueueReceipt, BrokerError> {
        self.validate_opportunity(&opportunity)?;
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
                queued: opportunity.source.clone(),
                current: self
                    .current_versions
                    .get(&opportunity.source.task_id)
                    .cloned(),
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

    fn validate_opportunity(&self, opportunity: &WorkOpportunity) -> Result<(), BrokerError> {
        if opportunity.work_id.trim().is_empty() {
            return Err(BrokerError::InvalidOpportunity {
                work_id: opportunity.work_id.clone(),
                reason: "work id is empty".to_string(),
            });
        }
        if opportunity.source.task_id.trim().is_empty() {
            return Err(BrokerError::InvalidOpportunity {
                work_id: opportunity.work_id.clone(),
                reason: "source task id is empty".to_string(),
            });
        }
        if opportunity.role.requires_task_version() && opportunity.source.revision == 0 {
            return Err(BrokerError::InvalidOpportunity {
                work_id: opportunity.work_id.clone(),
                reason: "auxiliary work requires a non-zero authoritative source revision"
                    .to_string(),
            });
        }
        Ok(())
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
                let source = queued.opportunity.source;
                Some(StaleWorkReceipt {
                    work_id,
                    role: queued.opportunity.role,
                    current_source: self.current_versions.get(&source.task_id).cloned(),
                    queued_source: source,
                })
            })
            .collect()
    }

    fn is_current(&self, source: &TaskVersion) -> bool {
        self.current_versions.get(&source.task_id) == Some(source)
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
            .max_by(|(_, left, _), (_, right, _)| Self::compare_queued(left, right))?;

        let (work_id, queued, lane_index) = selected;
        self.pending.remove(&work_id);
        self.admission_sequence += 1;
        let lane = &self.lanes[lane_index];
        let request_id = format!(
            "{}:request:{:08}",
            self.correlation_scope, self.admission_sequence
        );
        let receipt = AdmissionReceipt {
            request_id: request_id.clone(),
            work_id,
            role: queued.opportunity.role,
            priority: queued.opportunity.priority,
            source: queued.opportunity.source,
            logical_device_id: lane.logical_device_id.clone(),
            model_id: lane.model_id.clone(),
            physical_host_id: lane.host_id.clone(),
            model_instance_id: lane.model_instance_id.clone(),
            identity_evidence_id: lane.evidence_id.clone(),
            queue_sequence: queued.sequence,
            admission_sequence: self.admission_sequence,
        };
        self.active.insert(
            request_id,
            ActiveAdmission {
                receipt: receipt.clone(),
                provider_request_id: None,
                local_completion: None,
            },
        );
        Some(receipt)
    }

    fn compare_queued(left: &QueuedWork, right: &QueuedWork) -> Ordering {
        left.opportunity
            .priority
            .cmp(&right.opportunity.priority)
            // FIFO within a priority class: the smaller sequence wins `max_by`.
            .then_with(|| right.sequence.cmp(&left.sequence))
            .then_with(|| right.opportunity.work_id.cmp(&left.opportunity.work_id))
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
                if opportunity.role.requires_build_lane() && lane.supervision_only {
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
                let supervision_rank = if opportunity.role.requires_build_lane() {
                    0
                } else {
                    u8::from(!lane.supervision_only)
                };
                (
                    host_used,
                    instance_used,
                    supervision_rank,
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
            active.provider_request_id.as_deref() == Some(receipt.provider_request_id.as_str())
                && active.receipt.request_id != receipt.request_id
        }) {
            return Err(BrokerError::DuplicateProviderRequest(
                receipt.provider_request_id,
            ));
        }
        let active = self
            .active
            .get_mut(&receipt.request_id)
            .ok_or_else(|| BrokerError::UnknownRequest(receipt.request_id.clone()))?;
        Self::validate_physical_receipt(
            &active.receipt,
            &receipt.request_id,
            &receipt.physical_host_id,
            &receipt.model_instance_id,
        )?;
        if let Some(expected) = &active.provider_request_id {
            if expected != &receipt.provider_request_id {
                return Err(BrokerError::ProviderRequestMismatch {
                    request_id: receipt.request_id,
                    expected: Some(expected.clone()),
                    received: receipt.provider_request_id,
                });
            }
            return Ok(());
        }
        active.provider_request_id = Some(receipt.provider_request_id);
        Ok(())
    }

    pub fn record_local_completion(
        &mut self,
        request_id: &str,
        kind: LocalCompletionKind,
    ) -> Result<LocalCompletionReceipt, BrokerError> {
        let active = self
            .active
            .get_mut(request_id)
            .ok_or_else(|| BrokerError::UnknownRequest(request_id.to_string()))?;
        active.local_completion = Some(kind);
        Ok(LocalCompletionReceipt {
            request_id: request_id.to_string(),
            work_id: active.receipt.work_id.clone(),
            physical_host_id: active.receipt.physical_host_id.clone(),
            model_instance_id: active.receipt.model_instance_id.clone(),
            kind,
            provider_terminal_observed: false,
        })
    }

    pub fn observe_provider_terminal(
        &mut self,
        terminal: ProviderTerminalReceipt,
    ) -> Result<ReleasedAdmissionReceipt, BrokerError> {
        let active = self
            .active
            .get(&terminal.request_id)
            .ok_or_else(|| BrokerError::UnknownRequest(terminal.request_id.clone()))?;
        Self::validate_physical_receipt(
            &active.receipt,
            &terminal.request_id,
            &terminal.physical_host_id,
            &terminal.model_instance_id,
        )?;
        if active.provider_request_id.as_deref() != Some(terminal.provider_request_id.as_str()) {
            return Err(BrokerError::ProviderRequestMismatch {
                request_id: terminal.request_id,
                expected: active.provider_request_id.clone(),
                received: terminal.provider_request_id,
            });
        }
        let active = self
            .active
            .remove(&terminal.request_id)
            .expect("active admission was checked above");
        Ok(ReleasedAdmissionReceipt {
            admission: active.receipt,
            terminal,
        })
    }

    fn validate_physical_receipt(
        admission: &AdmissionReceipt,
        request_id: &str,
        host_id: &str,
        model_instance_id: &str,
    ) -> Result<(), BrokerError> {
        if admission.physical_host_id != host_id || admission.model_instance_id != model_instance_id
        {
            return Err(BrokerError::PhysicalReceiptMismatch {
                request_id: request_id.to_string(),
                expected_host: admission.physical_host_id.clone(),
                received_host: host_id.to_string(),
                expected_instance: admission.model_instance_id.clone(),
                received_instance: model_instance_id.to_string(),
            });
        }
        Ok(())
    }

    pub fn active_receipt(&self, request_id: &str) -> Option<&AdmissionReceipt> {
        self.active.get(request_id).map(|active| &active.receipt)
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
