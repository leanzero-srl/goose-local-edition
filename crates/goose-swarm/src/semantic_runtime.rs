//! Typed production boundary between measured worker state and semantic observation.
//!
//! A summons is evidence that a trace changed, not a verdict about that change. This module has no
//! action-delivery API and cannot mutate scheduler state.

use crate::semantic_observation::{
    AcceptanceCriterionSnapshot, NeutralJudgeSignal, SealedSemanticObservationSnapshot,
};
use crate::AdmissionReceipt;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticActivityPublisher {
    pub publisher_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub admission_id: String,
    pub work_role: String,
    pub source_id: String,
    pub fleet_snapshot_id: String,
    pub logical_device_id: String,
    pub model_id: String,
    pub physical_host_id: String,
    pub model_instance_id: String,
    pub provider_transport_id: String,
    pub capacity_evidence_id: String,
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
        if self.publisher_id != expected {
            return Err("semantic activity publisher id does not match its sealed identity".into());
        }
        Ok(())
    }

    fn identity_fields(&self) -> SemanticActivityPublisherIdentity<'_> {
        SemanticActivityPublisherIdentity {
            task_id: &self.task_id,
            attempt: self.attempt,
            admission_id: &self.admission_id,
            work_role: &self.work_role,
            source_id: &self.source_id,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticObservationCaptureRequest {
    pub task_id: String,
    pub attempt: u32,
    pub task_rank: u64,
    pub goal: String,
    pub task_contract: String,
    pub owned_files: Vec<String>,
    pub contract_version: String,
    pub acceptance_oracle: Vec<AcceptanceCriterionSnapshot>,
    pub dependency_contract_versions: BTreeMap<String, String>,
    pub sibling_contract_versions: BTreeMap<String, String>,
    pub allowed_finding_routes: Vec<String>,
    pub running_logical_device_id: String,
    pub running_model_id: String,
    pub activity_publisher: SemanticActivityPublisher,
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
    pub task_id: String,
    pub attempt: u32,
    pub source_revision: u64,
    pub snapshot_hash: String,
}

#[derive(Clone, Debug)]
pub struct SemanticObservationCapture {
    snapshot: SealedSemanticObservationSnapshot,
    summons: SemanticObservationSummonsSignal,
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

    pub fn revision(&self) -> SemanticTraceRevision {
        SemanticTraceRevision {
            task_id: self.snapshot.task_id().to_string(),
            attempt: self.snapshot.attempt(),
            source_revision: self.snapshot.source_revision(),
            snapshot_hash: self.snapshot.snapshot_hash().to_string(),
        }
    }

    pub fn into_snapshot(self) -> SealedSemanticObservationSnapshot {
        self.snapshot
    }
}

#[async_trait]
pub trait SemanticObservationSnapshotProducer: Send + Sync {
    async fn capture(
        &self,
        request: SemanticObservationCaptureRequest,
    ) -> Result<Option<SemanticObservationCapture>, String>;
}
