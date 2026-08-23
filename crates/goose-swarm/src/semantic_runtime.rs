//! Typed production boundary between measured worker state and semantic observation.
//!
//! A summons is evidence that a trace changed, not a verdict about that change. This module has no
//! action-delivery API and cannot mutate scheduler state.

use crate::semantic_observation::{
    AcceptanceCriterionSnapshot, NeutralJudgeSignal, SealedSemanticObservationSnapshot,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
