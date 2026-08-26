use crate::pillar::{
    validate_pillar_opening, CompiledPillarReport, Confidence, EvidenceClass, IntegrationContract,
    ProvenanceMatch, ResearchPillar, ResearchPillarOpening,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

const SCHEMA_VERSION: u32 = 3;
const CHECKPOINT_DIRECTORY: &str = "pillar-checkpoint-v3";
const STATE_FILE: &str = "state.json";
const LOCK_FILE: &str = "control.lock";
const OPENING_STAGE_FILE: &str = "opening-stage.json";
const OPENING_STAGE_LOCK_FILE: &str = "opening-stage.lock";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PillarAttemptCheckpoint {
    pub pillar_id: String,
    pub attempt_ordinal: u8,
    pub model_id: String,
    pub physical_host: String,
    pub status: PillarAttemptStatus,
    pub report: CompiledPillarReport,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PillarAttemptStatus {
    ModelReport,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PillarResumeDecision {
    RunPrimary,
    ReusePrimaryHigh,
    RunFocusedRetry,
    ReuseFocusedRetry,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PillarCheckpointReceipt {
    pub pillar_id: String,
    pub attempt_ordinal: u8,
    pub revision: u64,
    pub reused: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PillarOpeningCheckpointReceipt {
    pub response_schema_digest: String,
    pub opener_contract_digest: String,
    pub integration_owner: String,
    pub minimum_research_slices: usize,
    pub accepted_model_id: String,
    pub accepted_physical_host: String,
    pub accepted_attempt: u32,
    pub raw_output_digests: Vec<String>,
    pub semantic_topology_digest: String,
    pub compiler_receipt_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PillarOpeningContractBinding {
    pub response_schema_digest: String,
    pub opener_contract_digest: String,
    pub integration_owner: String,
    pub minimum_research_slices: usize,
}

impl PillarOpeningContractBinding {
    pub fn validate(&self) -> Result<(), PillarCheckpointError> {
        if !canonical_digest(&self.response_schema_digest)
            || !canonical_digest(&self.opener_contract_digest)
            || self.integration_owner.trim().is_empty()
            || self.minimum_research_slices == 0
        {
            return Err(PillarCheckpointError::new(
                "pillar opening contract binding is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PillarOpeningRawOutputCheckpoint {
    pub model_id: String,
    pub physical_host: String,
    pub attempt: u32,
    pub raw_output: String,
    pub raw_output_digest: String,
}

impl PillarOpeningRawOutputCheckpoint {
    pub fn new(
        model_id: impl Into<String>,
        physical_host: impl Into<String>,
        attempt: u32,
        raw_output: impl Into<String>,
    ) -> Result<Self, PillarCheckpointError> {
        let raw_output = raw_output.into();
        let checkpoint = Self {
            model_id: model_id.into(),
            physical_host: physical_host.into(),
            attempt,
            raw_output_digest: sha256_digest(raw_output.as_bytes()),
            raw_output,
        };
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    fn validate(&self) -> Result<(), PillarCheckpointError> {
        if self.model_id.trim().is_empty()
            || self.physical_host.trim().is_empty()
            || self.attempt == 0
            || self.raw_output.trim().is_empty()
            || !canonical_digest(&self.raw_output_digest)
            || sha256_digest(self.raw_output.as_bytes()) != self.raw_output_digest
        {
            return Err(PillarCheckpointError::new(
                "pillar opening raw-output checkpoint is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PillarOpeningPartialCheckpoint {
    pub binding: PillarOpeningContractBinding,
    pub full_candidate: PillarOpeningRawOutputCheckpoint,
    pub semantic_domains: serde_json::Value,
    pub research_slices: serde_json::Value,
    pub integration_contract: IntegrationContract,
    pub valid_domain_assignment_by_requirement: BTreeMap<String, String>,
    pub valid_slice_assignment_by_requirement: BTreeMap<String, String>,
    pub unresolved_domain_requirement_ids: Vec<String>,
    pub unresolved_slice_requirement_ids: Vec<String>,
    pub correction_fingerprint: String,
}

pub struct PillarOpeningPartialSemanticState {
    pub semantic_domains: serde_json::Value,
    pub research_slices: serde_json::Value,
    pub integration_contract: IntegrationContract,
    pub valid_domain_assignment_by_requirement: BTreeMap<String, String>,
    pub valid_slice_assignment_by_requirement: BTreeMap<String, String>,
    pub unresolved_domain_requirement_ids: Vec<String>,
    pub unresolved_slice_requirement_ids: Vec<String>,
}

#[derive(Serialize)]
struct PillarOpeningCorrectionFingerprintMaterial<'a> {
    semantic_domains: &'a serde_json::Value,
    research_slices: &'a serde_json::Value,
    integration_contract: &'a IntegrationContract,
    valid_domain_assignment_by_requirement: &'a BTreeMap<String, String>,
    valid_slice_assignment_by_requirement: &'a BTreeMap<String, String>,
    unresolved_domain_requirement_ids: &'a [String],
    unresolved_slice_requirement_ids: &'a [String],
}

impl PillarOpeningPartialCheckpoint {
    pub fn new(
        binding: PillarOpeningContractBinding,
        full_candidate: PillarOpeningRawOutputCheckpoint,
        semantic_state: PillarOpeningPartialSemanticState,
    ) -> Result<Self, PillarCheckpointError> {
        let PillarOpeningPartialSemanticState {
            semantic_domains,
            research_slices,
            integration_contract,
            valid_domain_assignment_by_requirement,
            valid_slice_assignment_by_requirement,
            unresolved_domain_requirement_ids,
            unresolved_slice_requirement_ids,
        } = semantic_state;
        let mut checkpoint = Self {
            binding,
            full_candidate,
            semantic_domains,
            research_slices,
            integration_contract,
            valid_domain_assignment_by_requirement,
            valid_slice_assignment_by_requirement,
            unresolved_domain_requirement_ids,
            unresolved_slice_requirement_ids,
            correction_fingerprint: String::new(),
        };
        checkpoint.correction_fingerprint = checkpoint.expected_correction_fingerprint()?;
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    fn expected_correction_fingerprint(&self) -> Result<String, PillarCheckpointError> {
        hash_serializable(&PillarOpeningCorrectionFingerprintMaterial {
            semantic_domains: &self.semantic_domains,
            research_slices: &self.research_slices,
            integration_contract: &self.integration_contract,
            valid_domain_assignment_by_requirement: &self.valid_domain_assignment_by_requirement,
            valid_slice_assignment_by_requirement: &self.valid_slice_assignment_by_requirement,
            unresolved_domain_requirement_ids: &self.unresolved_domain_requirement_ids,
            unresolved_slice_requirement_ids: &self.unresolved_slice_requirement_ids,
        })
    }

    fn validate(&self) -> Result<(), PillarCheckpointError> {
        self.binding.validate()?;
        self.full_candidate.validate()?;
        if !self.semantic_domains.is_array()
            || !self.research_slices.is_array()
            || self.integration_contract.owner != self.binding.integration_owner
            || !canonical_digest(&self.correction_fingerprint)
            || self.expected_correction_fingerprint()? != self.correction_fingerprint
        {
            return Err(PillarCheckpointError::new(
                "pillar opening partial checkpoint is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum PillarOpeningCheckpointStage {
    FullCandidate {
        candidate: PillarOpeningPartialCheckpoint,
    },
    FocusedRepair {
        candidate: PillarOpeningPartialCheckpoint,
        repair_attempts: Vec<PillarOpeningRawOutputCheckpoint>,
        attempts: u32,
    },
    Accepted {
        candidate: Box<PillarOpeningPartialCheckpoint>,
        repair_attempts: Vec<PillarOpeningRawOutputCheckpoint>,
        opening: ResearchPillarOpening,
        receipt: PillarOpeningCheckpointReceipt,
    },
    Unavailable {
        binding: PillarOpeningContractBinding,
        candidate: Option<PillarOpeningPartialCheckpoint>,
        full_attempts: Vec<PillarOpeningRawOutputCheckpoint>,
        repair_attempts: Vec<PillarOpeningRawOutputCheckpoint>,
        attempts: u32,
        reason: String,
    },
}

#[derive(Serialize)]
struct PillarOpeningCompilerReceiptMaterial<'a> {
    response_schema_digest: &'a str,
    opener_contract_digest: &'a str,
    integration_owner: &'a str,
    minimum_research_slices: usize,
    accepted_model_id: &'a str,
    accepted_physical_host: &'a str,
    accepted_attempt: u32,
    raw_output_digests: &'a [String],
    semantic_topology_digest: &'a str,
    opening: &'a ResearchPillarOpening,
}

impl PillarOpeningCheckpointReceipt {
    pub fn new(
        binding: &PillarOpeningContractBinding,
        accepted_model_id: impl Into<String>,
        accepted_physical_host: impl Into<String>,
        accepted_attempt: u32,
        raw_outputs: &[String],
        semantic_topology_digest: impl Into<String>,
        opening: &ResearchPillarOpening,
    ) -> Result<Self, PillarCheckpointError> {
        let mut receipt = Self {
            response_schema_digest: binding.response_schema_digest.clone(),
            opener_contract_digest: binding.opener_contract_digest.clone(),
            integration_owner: binding.integration_owner.clone(),
            minimum_research_slices: binding.minimum_research_slices,
            accepted_model_id: accepted_model_id.into(),
            accepted_physical_host: accepted_physical_host.into(),
            accepted_attempt,
            raw_output_digests: raw_outputs
                .iter()
                .map(|output| sha256_digest(output.as_bytes()))
                .collect(),
            semantic_topology_digest: semantic_topology_digest.into(),
            compiler_receipt_digest: String::new(),
        };
        receipt.compiler_receipt_digest = receipt.expected_compiler_receipt_digest(opening)?;
        receipt.validate(opening)?;
        Ok(receipt)
    }

    fn expected_compiler_receipt_digest(
        &self,
        opening: &ResearchPillarOpening,
    ) -> Result<String, PillarCheckpointError> {
        hash_serializable(&PillarOpeningCompilerReceiptMaterial {
            response_schema_digest: &self.response_schema_digest,
            opener_contract_digest: &self.opener_contract_digest,
            integration_owner: &self.integration_owner,
            minimum_research_slices: self.minimum_research_slices,
            accepted_model_id: &self.accepted_model_id,
            accepted_physical_host: &self.accepted_physical_host,
            accepted_attempt: self.accepted_attempt,
            raw_output_digests: &self.raw_output_digests,
            semantic_topology_digest: &self.semantic_topology_digest,
            opening,
        })
    }

    fn validate(&self, opening: &ResearchPillarOpening) -> Result<(), PillarCheckpointError> {
        if !canonical_digest(&self.response_schema_digest)
            || !canonical_digest(&self.opener_contract_digest)
            || self.integration_owner.trim().is_empty()
            || self.minimum_research_slices == 0
            || opening.pillars.len() < self.minimum_research_slices
            || self.accepted_model_id.trim().is_empty()
            || self.accepted_physical_host.trim().is_empty()
            || self.accepted_attempt == 0
            || self.raw_output_digests.is_empty()
            || self
                .raw_output_digests
                .iter()
                .any(|digest| !canonical_digest(digest))
            || !canonical_digest(&self.semantic_topology_digest)
            || !canonical_digest(&self.compiler_receipt_digest)
            || self.expected_compiler_receipt_digest(opening)? != self.compiler_receipt_digest
        {
            return Err(PillarCheckpointError::new(
                "pillar opening checkpoint receipt binding is invalid",
            ));
        }
        Ok(())
    }
}

impl PillarOpeningCheckpointStage {
    pub fn binding(&self) -> &PillarOpeningContractBinding {
        match self {
            Self::FullCandidate { candidate } | Self::FocusedRepair { candidate, .. } => {
                &candidate.binding
            }
            Self::Accepted { candidate, .. } => &candidate.binding,
            Self::Unavailable { binding, .. } => binding,
        }
    }

    pub fn candidate(&self) -> Option<&PillarOpeningPartialCheckpoint> {
        match self {
            Self::FullCandidate { candidate } | Self::FocusedRepair { candidate, .. } => {
                Some(candidate)
            }
            Self::Accepted { candidate, .. } => Some(candidate),
            Self::Unavailable { candidate, .. } => candidate.as_ref(),
        }
    }

    fn validate(&self) -> Result<(), PillarCheckpointError> {
        self.binding().validate()?;
        match self {
            Self::FullCandidate { candidate } => candidate.validate(),
            Self::FocusedRepair {
                candidate,
                repair_attempts,
                attempts,
            } => {
                candidate.validate()?;
                for attempt in repair_attempts {
                    attempt.validate()?;
                }
                if *attempts < candidate.full_candidate.attempt
                    || repair_attempts
                        .iter()
                        .any(|attempt| attempt.attempt > *attempts)
                {
                    return Err(PillarCheckpointError::new(
                        "focused pillar opening attempt ledger is invalid",
                    ));
                }
                Ok(())
            }
            Self::Accepted {
                candidate,
                repair_attempts,
                opening,
                receipt,
            } => {
                candidate.validate()?;
                for attempt in repair_attempts {
                    attempt.validate()?;
                }
                validate_pillar_opening(opening).map_err(|error| {
                    PillarCheckpointError::new(format!(
                        "accepted opening stage is invalid: {error}"
                    ))
                })?;
                receipt.validate(opening)?;
                let mut expected_raw_digests =
                    vec![candidate.full_candidate.raw_output_digest.clone()];
                expected_raw_digests.extend(
                    repair_attempts
                        .iter()
                        .map(|attempt| attempt.raw_output_digest.clone()),
                );
                let accepted_attempt = repair_attempts.last().unwrap_or(&candidate.full_candidate);
                if receipt.response_schema_digest != candidate.binding.response_schema_digest
                    || receipt.opener_contract_digest != candidate.binding.opener_contract_digest
                    || receipt.integration_owner != candidate.binding.integration_owner
                    || receipt.minimum_research_slices != candidate.binding.minimum_research_slices
                    || receipt.semantic_topology_digest != candidate.correction_fingerprint
                    || receipt.raw_output_digests != expected_raw_digests
                    || receipt.accepted_model_id != accepted_attempt.model_id
                    || receipt.accepted_physical_host != accepted_attempt.physical_host
                    || receipt.accepted_attempt != accepted_attempt.attempt
                    || opening.integration_contract.owner != candidate.binding.integration_owner
                {
                    return Err(PillarCheckpointError::new(
                        "accepted pillar opening stage is not bound to its candidate, repair, and strongest owner",
                    ));
                }
                Ok(())
            }
            Self::Unavailable {
                binding,
                candidate,
                full_attempts,
                repair_attempts,
                attempts,
                reason,
            } => {
                binding.validate()?;
                for attempt in full_attempts {
                    attempt.validate()?;
                }
                for attempt in repair_attempts {
                    attempt.validate()?;
                }
                if *attempts == 0 || reason.trim().is_empty() {
                    return Err(PillarCheckpointError::new(
                        "unavailable pillar opening stage is incomplete",
                    ));
                }
                if let Some(candidate) = candidate {
                    candidate.validate()?;
                    if &candidate.binding != binding {
                        return Err(PillarCheckpointError::new(
                            "unavailable pillar opening changed its preserved contract binding",
                        ));
                    }
                    if !full_attempts.is_empty() {
                        return Err(PillarCheckpointError::new(
                            "unavailable focused-opening stage duplicated pre-candidate raw outputs",
                        ));
                    }
                } else if !repair_attempts.is_empty() {
                    return Err(PillarCheckpointError::new(
                        "unavailable full-opening stage cannot carry focused repair outputs",
                    ));
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
pub struct PillarCheckpointError {
    detail: String,
}

impl PillarCheckpointError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    fn io(action: &str, error: std::io::Error) -> Self {
        Self::new(format!("{action}: {error}"))
    }
}

impl std::fmt::Display for PillarCheckpointError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for PillarCheckpointError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CheckpointMaterial {
    schema_version: u32,
    revision: u64,
    working_root: PathBuf,
    frozen_spec_digest: String,
    requirement_digest: String,
    opening: ResearchPillarOpening,
    opening_receipt: PillarOpeningCheckpointReceipt,
    attempts: BTreeMap<String, Vec<PillarAttemptCheckpoint>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CheckpointRecord {
    material: CheckpointMaterial,
    checkpoint_hash: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OpeningStageMaterial {
    schema_version: u32,
    revision: u64,
    working_root: PathBuf,
    frozen_spec_digest: String,
    requirement_digest: String,
    authored_requirements: Vec<crate::pillar::AuthoredRequirement>,
    stage: PillarOpeningCheckpointStage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OpeningStageRecord {
    material: OpeningStageMaterial,
    checkpoint_hash: String,
}

pub struct PillarOpeningStageStore;

struct StoreState {
    record: CheckpointRecord,
    poisoned: Option<String>,
}

pub struct PillarCheckpointStore {
    state_path: PathBuf,
    _lock: File,
    state: Mutex<StoreState>,
}

impl PillarOpeningStageStore {
    pub fn load(
        working_root: impl AsRef<Path>,
        frozen_spec_digest: impl Into<String>,
        authored_requirements: &[crate::pillar::AuthoredRequirement],
        binding: &PillarOpeningContractBinding,
        accepted_lanes: &[(String, String)],
    ) -> Result<Option<PillarOpeningCheckpointStage>, PillarCheckpointError> {
        binding.validate()?;
        let working_root = std::fs::canonicalize(working_root.as_ref()).map_err(|error| {
            PillarCheckpointError::io("cannot canonicalize pillar opening stage root", error)
        })?;
        let frozen_spec_digest = frozen_spec_digest.into();
        if !canonical_digest(&frozen_spec_digest) {
            return Err(PillarCheckpointError::new(
                "frozen specification digest is not a canonical sha256 digest",
            ));
        }
        let requirement_digest = pillar_requirement_digest(authored_requirements)?;
        let generation_directory =
            checkpoint_generation_directory(&working_root, &frozen_spec_digest);
        reject_symlink_if_present(
            &generation_directory,
            "pillar opening stage generation directory",
        )?;
        let stage_path = generation_directory.join(OPENING_STAGE_FILE);
        reject_symlink_if_present(&stage_path, "pillar opening stage")?;
        let bytes = match std::fs::read(&stage_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(PillarCheckpointError::io(
                    "cannot read pillar opening stage",
                    error,
                ));
            }
        };
        let record = decode_opening_stage_record(&bytes)?;
        validate_opening_stage_record(&record)?;
        if record.material.working_root != working_root
            || record.material.frozen_spec_digest != frozen_spec_digest
            || record.material.requirement_digest != requirement_digest
            || record.material.authored_requirements != authored_requirements
            || record.material.stage.binding() != binding
            || record.material.stage.candidate().is_some_and(|candidate| {
                !accepted_lanes.iter().any(|(model_id, physical_host)| {
                    model_id == &candidate.full_candidate.model_id
                        && physical_host == &candidate.full_candidate.physical_host
                })
            })
            || matches!(
                &record.material.stage,
                PillarOpeningCheckpointStage::Accepted { receipt, .. }
                    if !accepted_lanes.iter().any(|(model_id, physical_host)| {
                        model_id == &receipt.accepted_model_id
                            && physical_host == &receipt.accepted_physical_host
                    })
            )
        {
            return Err(PillarCheckpointError::new(
                "pillar opening stage is incompatible with this root, specification, contract, requirements, or authenticated lanes",
            ));
        }
        Ok(Some(record.material.stage))
    }

    pub fn persist(
        working_root: impl AsRef<Path>,
        frozen_spec_digest: impl Into<String>,
        authored_requirements: &[crate::pillar::AuthoredRequirement],
        stage: PillarOpeningCheckpointStage,
    ) -> Result<(), PillarCheckpointError> {
        stage.validate()?;
        let working_root = std::fs::canonicalize(working_root.as_ref()).map_err(|error| {
            PillarCheckpointError::io("cannot canonicalize pillar opening stage root", error)
        })?;
        if !working_root.is_dir() {
            return Err(PillarCheckpointError::new(format!(
                "pillar opening stage root is not a directory: {}",
                working_root.display()
            )));
        }
        let frozen_spec_digest = frozen_spec_digest.into();
        if !canonical_digest(&frozen_spec_digest) {
            return Err(PillarCheckpointError::new(
                "frozen specification digest is not a canonical sha256 digest",
            ));
        }
        let requirement_digest = pillar_requirement_digest(authored_requirements)?;
        let swarm_directory = working_root.join(".swarm");
        let directory = swarm_directory.join(CHECKPOINT_DIRECTORY);
        let generation_directory =
            checkpoint_generation_directory(&working_root, &frozen_spec_digest);
        ensure_control_directory(&working_root, &swarm_directory, "swarm state directory")?;
        ensure_control_directory(&swarm_directory, &directory, "pillar checkpoint directory")?;
        ensure_control_directory(
            &directory,
            &generation_directory,
            "pillar checkpoint generation directory",
        )?;
        let lock_path = generation_directory.join(OPENING_STAGE_LOCK_FILE);
        reject_symlink_if_present(&lock_path, "pillar opening stage lock")?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                PillarCheckpointError::io("cannot open pillar opening stage lock", error)
            })?;
        FileExt::lock_exclusive(&lock).map_err(|error| {
            PillarCheckpointError::io("cannot lock pillar opening stage", error)
        })?;
        verify_linked_file(&lock_path, &lock, "pillar opening stage lock")?;
        let stage_path = generation_directory.join(OPENING_STAGE_FILE);
        reject_symlink_if_present(&stage_path, "pillar opening stage")?;
        let previous = match std::fs::read(&stage_path) {
            Ok(bytes) => {
                let record = decode_opening_stage_record(&bytes)?;
                validate_opening_stage_record(&record)?;
                if record.material.working_root != working_root
                    || record.material.frozen_spec_digest != frozen_spec_digest
                    || record.material.requirement_digest != requirement_digest
                    || record.material.authored_requirements != authored_requirements
                    || record.material.stage.binding() != stage.binding()
                {
                    return Err(PillarCheckpointError::new(
                        "pillar opening stage conflicts with its durable generation binding",
                    ));
                }
                Some(record)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(PillarCheckpointError::io(
                    "cannot read pillar opening stage before update",
                    error,
                ));
            }
        };
        if let Some(previous) = &previous {
            validate_opening_stage_transition(&previous.material.stage, &stage)?;
            if previous.material.stage == stage {
                return Ok(());
            }
        }
        let material = OpeningStageMaterial {
            schema_version: SCHEMA_VERSION,
            revision: previous
                .as_ref()
                .map_or(0, |record| record.material.revision.saturating_add(1)),
            working_root,
            frozen_spec_digest,
            requirement_digest,
            authored_requirements: authored_requirements.to_vec(),
            stage,
        };
        let record = seal_opening_stage_record(material)?;
        write_opening_stage_record_atomic(&stage_path, &record)
    }
}

impl PillarCheckpointStore {
    pub fn load_opening(
        working_root: impl AsRef<Path>,
        frozen_spec_digest: impl Into<String>,
        authored_requirements: &[crate::pillar::AuthoredRequirement],
        binding: &PillarOpeningContractBinding,
        accepted_lanes: &[(String, String)],
    ) -> Result<
        Option<(ResearchPillarOpening, PillarOpeningCheckpointReceipt)>,
        PillarCheckpointError,
    > {
        binding.validate()?;
        let working_root = std::fs::canonicalize(working_root.as_ref()).map_err(|error| {
            PillarCheckpointError::io("cannot canonicalize pillar checkpoint root", error)
        })?;
        let frozen_spec_digest = frozen_spec_digest.into();
        if !canonical_digest(&frozen_spec_digest) {
            return Err(PillarCheckpointError::new(
                "frozen specification digest is not a canonical sha256 digest",
            ));
        }
        let requirement_digest = pillar_requirement_digest(authored_requirements)?;
        let generation_directory =
            checkpoint_generation_directory(&working_root, &frozen_spec_digest);
        reject_symlink_if_present(
            &generation_directory,
            "pillar checkpoint generation directory",
        )?;
        let state_path = generation_directory.join(STATE_FILE);
        reject_symlink_if_present(&state_path, "pillar checkpoint state")?;
        let bytes = match std::fs::read(&state_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(PillarCheckpointError::io(
                    "cannot read pillar checkpoint state",
                    error,
                ));
            }
        };
        let record = decode_record(&bytes)?;
        validate_record(&record)?;
        if record.material.working_root != working_root
            || record.material.frozen_spec_digest != frozen_spec_digest
            || record.material.requirement_digest != requirement_digest
            || record.material.opening.requirements != authored_requirements
            || record.material.opening_receipt.response_schema_digest
                != binding.response_schema_digest
            || record.material.opening_receipt.opener_contract_digest
                != binding.opener_contract_digest
            || record.material.opening_receipt.integration_owner != binding.integration_owner
            || record.material.opening_receipt.minimum_research_slices
                != binding.minimum_research_slices
            || !accepted_lanes.iter().any(|(model_id, physical_host)| {
                model_id == &record.material.opening_receipt.accepted_model_id
                    && physical_host == &record.material.opening_receipt.accepted_physical_host
            })
        {
            return Err(PillarCheckpointError::new(
                "pillar checkpoint is incompatible with this root, frozen specification, opener contract, strongest owner, slice floor, or accepted lane",
            ));
        }
        Ok(Some((
            record.material.opening,
            record.material.opening_receipt,
        )))
    }

    pub fn open(
        working_root: impl AsRef<Path>,
        frozen_spec_digest: impl Into<String>,
        opening: &ResearchPillarOpening,
        opening_receipt: &PillarOpeningCheckpointReceipt,
    ) -> Result<Self, PillarCheckpointError> {
        validate_pillar_opening(opening).map_err(|error| {
            PillarCheckpointError::new(format!("cannot checkpoint invalid pillar opening: {error}"))
        })?;
        opening_receipt.validate(opening)?;
        let working_root = std::fs::canonicalize(working_root.as_ref()).map_err(|error| {
            PillarCheckpointError::io("cannot canonicalize pillar checkpoint root", error)
        })?;
        if !working_root.is_dir() {
            return Err(PillarCheckpointError::new(format!(
                "pillar checkpoint root is not a directory: {}",
                working_root.display()
            )));
        }
        let frozen_spec_digest = frozen_spec_digest.into();
        if !canonical_digest(&frozen_spec_digest) {
            return Err(PillarCheckpointError::new(
                "frozen specification digest is not a canonical sha256 digest",
            ));
        }
        let requirement_digest = pillar_requirement_digest(&opening.requirements)?;

        let swarm_directory = working_root.join(".swarm");
        let directory = swarm_directory.join(CHECKPOINT_DIRECTORY);
        let generation_directory =
            checkpoint_generation_directory(&working_root, &frozen_spec_digest);
        ensure_control_directory(&working_root, &swarm_directory, "swarm state directory")?;
        ensure_control_directory(&swarm_directory, &directory, "pillar checkpoint directory")?;
        ensure_control_directory(
            &directory,
            &generation_directory,
            "pillar checkpoint generation directory",
        )?;

        let lock_path = generation_directory.join(LOCK_FILE);
        reject_symlink_if_present(&lock_path, "pillar checkpoint lock")?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                PillarCheckpointError::io("cannot open pillar checkpoint lock", error)
            })?;
        FileExt::try_lock_exclusive(&lock).map_err(|error| {
            PillarCheckpointError::io("pillar checkpoint is already open", error)
        })?;
        verify_linked_file(&lock_path, &lock, "pillar checkpoint lock")?;

        let state_path = generation_directory.join(STATE_FILE);
        reject_symlink_if_present(&state_path, "pillar checkpoint state")?;
        let record = match std::fs::read(&state_path) {
            Ok(bytes) => {
                let record = decode_record(&bytes)?;
                validate_record(&record)?;
                if record.material.working_root != working_root
                    || record.material.frozen_spec_digest != frozen_spec_digest
                    || record.material.requirement_digest != requirement_digest
                    || record.material.opening != *opening
                    || record.material.opening_receipt != *opening_receipt
                {
                    return Err(PillarCheckpointError::new(
                        "pillar checkpoint is incompatible with this root, frozen specification, opening, or opening receipt",
                    ));
                }
                record
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let material = CheckpointMaterial {
                    schema_version: SCHEMA_VERSION,
                    revision: 0,
                    working_root: working_root.clone(),
                    frozen_spec_digest,
                    requirement_digest,
                    opening: opening.clone(),
                    opening_receipt: opening_receipt.clone(),
                    attempts: BTreeMap::new(),
                };
                let record = seal_record(material)?;
                write_record_atomic(&state_path, &record)?;
                record
            }
            Err(error) => {
                return Err(PillarCheckpointError::io(
                    "cannot read pillar checkpoint state",
                    error,
                ));
            }
        };

        Ok(Self {
            state_path,
            _lock: lock,
            state: Mutex::new(StoreState {
                record,
                poisoned: None,
            }),
        })
    }

    pub fn opening(&self) -> ResearchPillarOpening {
        lock(&self.state).record.material.opening.clone()
    }

    pub fn completed_attempts(
        &self,
        pillar_id: &str,
    ) -> Result<Vec<PillarAttemptCheckpoint>, PillarCheckpointError> {
        let state = lock(&self.state);
        ensure_usable(&state)?;
        verify_current_record(&self.state_path, &state.record)?;
        known_pillar(&state.record.material.opening, pillar_id)?;
        Ok(state
            .record
            .material
            .attempts
            .get(pillar_id)
            .cloned()
            .unwrap_or_default())
    }

    pub fn resume_decision(
        &self,
        pillar_id: &str,
    ) -> Result<PillarResumeDecision, PillarCheckpointError> {
        let attempts = self.completed_attempts(pillar_id)?;
        match attempts.as_slice() {
            [] => Ok(PillarResumeDecision::RunPrimary),
            [primary] if primary.report.effective_confidence == Confidence::High => {
                Ok(PillarResumeDecision::ReusePrimaryHigh)
            }
            [primary] if primary.report.effective_confidence == Confidence::Low => {
                Ok(PillarResumeDecision::RunFocusedRetry)
            }
            [primary, _retry] if primary.report.effective_confidence == Confidence::Low => {
                Ok(PillarResumeDecision::ReuseFocusedRetry)
            }
            _ => Err(PillarCheckpointError::new(
                "pillar checkpoint contains an impossible attempt sequence",
            )),
        }
    }

    pub fn next_attempt_ordinal(
        &self,
        pillar_id: &str,
    ) -> Result<Option<u8>, PillarCheckpointError> {
        Ok(match self.resume_decision(pillar_id)? {
            PillarResumeDecision::RunPrimary => Some(1),
            PillarResumeDecision::RunFocusedRetry => Some(2),
            PillarResumeDecision::ReusePrimaryHigh | PillarResumeDecision::ReuseFocusedRetry => {
                None
            }
        })
    }

    pub fn persist_attempt(
        &self,
        attempt: PillarAttemptCheckpoint,
    ) -> Result<PillarCheckpointReceipt, PillarCheckpointError> {
        let mut state = lock(&self.state);
        ensure_usable(&state)?;
        verify_current_record(&self.state_path, &state.record)?;
        let pillar = known_pillar(&state.record.material.opening, &attempt.pillar_id)?;
        validate_attempt(pillar, &attempt)?;

        let existing = state
            .record
            .material
            .attempts
            .get(&attempt.pillar_id)
            .cloned()
            .unwrap_or_default();
        if let Some(saved) = existing
            .iter()
            .find(|saved| saved.attempt_ordinal == attempt.attempt_ordinal)
        {
            if saved == &attempt {
                return Ok(PillarCheckpointReceipt {
                    pillar_id: attempt.pillar_id,
                    attempt_ordinal: attempt.attempt_ordinal,
                    revision: state.record.material.revision,
                    reused: true,
                });
            }
            return Err(PillarCheckpointError::new(format!(
                "pillar {:?} attempt {} conflicts with its durable checkpoint",
                attempt.pillar_id, attempt.attempt_ordinal
            )));
        }
        match (attempt.attempt_ordinal, existing.as_slice()) {
            (1, []) => {}
            (2, [primary]) if primary.report.effective_confidence == Confidence::Low => {}
            (2, []) => {
                return Err(PillarCheckpointError::new(
                    "cannot checkpoint a focused retry before its primary attempt",
                ));
            }
            (2, [primary]) if primary.report.effective_confidence == Confidence::High => {
                return Err(PillarCheckpointError::new(
                    "cannot checkpoint a focused retry after a high-confidence primary attempt",
                ));
            }
            _ => {
                return Err(PillarCheckpointError::new(
                    "pillar checkpoint accepts only one primary and one focused retry",
                ));
            }
        }

        let mut material = state.record.material.clone();
        material.revision = material
            .revision
            .checked_add(1)
            .ok_or_else(|| PillarCheckpointError::new("pillar checkpoint revision overflowed"))?;
        material
            .attempts
            .entry(attempt.pillar_id.clone())
            .or_default()
            .push(attempt.clone());
        let next_record = seal_record(material)?;
        if let Err(error) = write_record_atomic(&self.state_path, &next_record) {
            state.poisoned = Some(error.to_string());
            return Err(error);
        }
        state.record = next_record;
        Ok(PillarCheckpointReceipt {
            pillar_id: attempt.pillar_id,
            attempt_ordinal: attempt.attempt_ordinal,
            revision: state.record.material.revision,
            reused: false,
        })
    }
}

pub fn pillar_frozen_spec_digest(spec: &str) -> String {
    sha256_digest(spec.as_bytes())
}

pub fn pillar_requirement_digest(
    requirements: &[crate::pillar::AuthoredRequirement],
) -> Result<String, PillarCheckpointError> {
    hash_serializable(requirements)
}

fn validate_record(record: &CheckpointRecord) -> Result<(), PillarCheckpointError> {
    if record.material.schema_version != SCHEMA_VERSION
        || !canonical_digest(&record.material.frozen_spec_digest)
        || !canonical_digest(&record.material.requirement_digest)
        || hash_serializable(&record.material)? != record.checkpoint_hash
    {
        return Err(PillarCheckpointError::new(
            "pillar checkpoint hash, schema, or digest binding is invalid",
        ));
    }
    validate_pillar_opening(&record.material.opening).map_err(|error| {
        PillarCheckpointError::new(format!("pillar checkpoint opening is invalid: {error}"))
    })?;
    record
        .material
        .opening_receipt
        .validate(&record.material.opening)?;
    if pillar_requirement_digest(&record.material.opening.requirements)?
        != record.material.requirement_digest
    {
        return Err(PillarCheckpointError::new(
            "pillar checkpoint requirement digest does not match its opening",
        ));
    }
    let known = record
        .material
        .opening
        .pillars
        .iter()
        .map(|pillar| (pillar.id.as_str(), pillar))
        .collect::<BTreeMap<_, _>>();
    for (pillar_id, attempts) in &record.material.attempts {
        let pillar = known.get(pillar_id.as_str()).ok_or_else(|| {
            PillarCheckpointError::new(format!(
                "pillar checkpoint contains unknown pillar {pillar_id:?}"
            ))
        })?;
        for attempt in attempts {
            validate_attempt(pillar, attempt)?;
        }
        match attempts.as_slice() {
            [primary] if primary.attempt_ordinal == 1 => {}
            [primary, retry]
                if primary.attempt_ordinal == 1
                    && retry.attempt_ordinal == 2
                    && primary.report.effective_confidence == Confidence::Low => {}
            _ => {
                return Err(PillarCheckpointError::new(format!(
                    "pillar {pillar_id:?} has an invalid checkpoint attempt sequence"
                )));
            }
        }
    }
    Ok(())
}

fn validate_opening_stage_record(record: &OpeningStageRecord) -> Result<(), PillarCheckpointError> {
    if record.material.schema_version != SCHEMA_VERSION
        || !canonical_digest(&record.material.frozen_spec_digest)
        || !canonical_digest(&record.material.requirement_digest)
        || hash_serializable(&record.material)? != record.checkpoint_hash
        || pillar_requirement_digest(&record.material.authored_requirements)?
            != record.material.requirement_digest
    {
        return Err(PillarCheckpointError::new(
            "pillar opening stage hash, schema, or requirement binding is invalid",
        ));
    }
    record.material.stage.validate()?;
    if let Some(candidate) = record.material.stage.candidate() {
        let expected = record
            .material
            .authored_requirements
            .iter()
            .map(|requirement| requirement.id.as_str())
            .collect::<BTreeSet<_>>();
        validate_partial_key_cover(
            &expected,
            &candidate.valid_domain_assignment_by_requirement,
            &candidate.unresolved_domain_requirement_ids,
            "domain",
        )?;
        validate_partial_key_cover(
            &expected,
            &candidate.valid_slice_assignment_by_requirement,
            &candidate.unresolved_slice_requirement_ids,
            "slice",
        )?;
    }
    if let PillarOpeningCheckpointStage::Accepted { opening, .. } = &record.material.stage {
        if opening.requirements != record.material.authored_requirements {
            return Err(PillarCheckpointError::new(
                "accepted opening stage changed the frozen authored requirements",
            ));
        }
    }
    Ok(())
}

fn validate_partial_key_cover(
    expected: &BTreeSet<&str>,
    valid_assignments: &BTreeMap<String, String>,
    unresolved_requirement_ids: &[String],
    label: &str,
) -> Result<(), PillarCheckpointError> {
    let valid = valid_assignments
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let unresolved = unresolved_requirement_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if valid.len() != valid_assignments.len()
        || unresolved.len() != unresolved_requirement_ids.len()
        || !valid.is_disjoint(&unresolved)
        || valid.union(&unresolved).copied().collect::<BTreeSet<_>>() != *expected
        || valid_assignments
            .values()
            .any(|owner| owner.trim().is_empty())
    {
        return Err(PillarCheckpointError::new(format!(
            "pillar opening partial {label} assignment cover is invalid"
        )));
    }
    Ok(())
}

fn validate_opening_stage_transition(
    previous: &PillarOpeningCheckpointStage,
    next: &PillarOpeningCheckpointStage,
) -> Result<(), PillarCheckpointError> {
    let same_candidate = match (previous.candidate(), next.candidate()) {
        (Some(previous), Some(next)) => previous == next,
        (None, None) => true,
        _ => false,
    };
    let valid = match (previous, next) {
        (
            PillarOpeningCheckpointStage::FullCandidate { .. },
            PillarOpeningCheckpointStage::FocusedRepair { .. }
            | PillarOpeningCheckpointStage::Accepted { .. }
            | PillarOpeningCheckpointStage::Unavailable { .. },
        ) => same_candidate,
        (
            PillarOpeningCheckpointStage::FocusedRepair {
                repair_attempts: previous_attempts,
                attempts: previous_attempt_count,
                ..
            },
            PillarOpeningCheckpointStage::FocusedRepair {
                repair_attempts: next_attempts,
                attempts: next_attempt_count,
                ..
            },
        ) => {
            same_candidate
                && next_attempts.starts_with(previous_attempts)
                && next_attempt_count >= previous_attempt_count
        }
        (
            PillarOpeningCheckpointStage::FocusedRepair {
                repair_attempts: previous_attempts,
                attempts: previous_attempt_count,
                ..
            },
            PillarOpeningCheckpointStage::Accepted {
                repair_attempts: next_attempts,
                receipt,
                ..
            },
        ) => {
            same_candidate
                && next_attempts.starts_with(previous_attempts)
                && receipt.accepted_attempt >= *previous_attempt_count
        }
        (
            PillarOpeningCheckpointStage::FocusedRepair {
                repair_attempts: previous_attempts,
                attempts: previous_attempt_count,
                ..
            },
            PillarOpeningCheckpointStage::Unavailable {
                repair_attempts: next_attempts,
                attempts: next_attempt_count,
                ..
            },
        ) => {
            same_candidate
                && next_attempts == previous_attempts
                && next_attempt_count >= previous_attempt_count
        }
        (
            PillarOpeningCheckpointStage::Unavailable {
                candidate: Some(_),
                full_attempts,
                repair_attempts: previous_attempts,
                attempts: previous_attempt_count,
                ..
            },
            PillarOpeningCheckpointStage::FocusedRepair {
                repair_attempts: next_attempts,
                attempts: next_attempt_count,
                ..
            },
        ) => {
            same_candidate
                && full_attempts.is_empty()
                && next_attempts == previous_attempts
                && next_attempt_count == previous_attempt_count
        }
        (
            PillarOpeningCheckpointStage::Unavailable {
                candidate: None, ..
            },
            PillarOpeningCheckpointStage::FullCandidate { .. },
        ) => true,
        (
            PillarOpeningCheckpointStage::Unavailable {
                full_attempts: previous_full_attempts,
                repair_attempts: previous_attempts,
                attempts: previous_attempt_count,
                ..
            },
            PillarOpeningCheckpointStage::Unavailable {
                full_attempts: next_full_attempts,
                repair_attempts: next_attempts,
                attempts: next_attempt_count,
                ..
            },
        ) => {
            same_candidate
                && next_full_attempts.starts_with(previous_full_attempts)
                && next_attempts.starts_with(previous_attempts)
                && next_attempt_count >= previous_attempt_count
        }
        (PillarOpeningCheckpointStage::Accepted { .. }, _) => false,
        _ => false,
    };
    if !valid {
        return Err(PillarCheckpointError::new(
            "pillar opening stage transition changed first-authenticated semantic authority or regressed its stage",
        ));
    }
    Ok(())
}

fn validate_attempt(
    pillar: &ResearchPillar,
    attempt: &PillarAttemptCheckpoint,
) -> Result<(), PillarCheckpointError> {
    if attempt.pillar_id != pillar.id
        || !matches!(attempt.attempt_ordinal, 1 | 2)
        || attempt.model_id.trim().is_empty()
        || attempt.physical_host.trim().is_empty()
        || attempt.report.pillar_id != pillar.id
    {
        return Err(PillarCheckpointError::new(
            "pillar attempt identity, ordinal, model, host, or report binding is invalid",
        ));
    }
    let owned = pillar
        .requirement_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut evidenced = BTreeSet::new();
    let mut forces_low = attempt.report.reported_confidence == Confidence::Low
        || !attempt.report.unresolved_uncertainties.is_empty();
    for claim in &attempt.report.claims {
        if claim.statement.trim().is_empty() || !owned.contains(claim.requirement_id.as_str()) {
            return Err(PillarCheckpointError::new(
                "pillar report contains an empty statement or a claim outside pillar ownership",
            ));
        }
        evidenced.insert(claim.requirement_id.as_str());
        let provenance_valid = claim.provenance.is_valid()
            && (claim.reported_class != EvidenceClass::Proven
                || matches!(claim.provenance, ProvenanceMatch::Unique { .. }));
        let class_valid = match claim.reported_class {
            EvidenceClass::Proven if provenance_valid => {
                claim.effective_class == EvidenceClass::Proven
            }
            EvidenceClass::Proven => claim.effective_class == EvidenceClass::Supported,
            class => claim.effective_class == class,
        };
        if !class_valid {
            return Err(PillarCheckpointError::new(
                "pillar report effective evidence class is inconsistent with provenance",
            ));
        }
        forces_low |= !provenance_valid || claim.effective_class == EvidenceClass::Unresolved;
    }
    let missing = pillar
        .requirement_ids
        .iter()
        .filter(|requirement_id| !evidenced.contains(requirement_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if attempt.report.missing_requirement_ids != missing
        || attempt.report.acceptance_tests.is_empty()
        || attempt
            .report
            .acceptance_tests
            .iter()
            .any(|value| value.trim().is_empty())
        || attempt.report.exclusions.is_empty()
        || attempt
            .report
            .exclusions
            .iter()
            .any(|value| value.trim().is_empty())
    {
        return Err(PillarCheckpointError::new(
            "pillar report requirement coverage, acceptance tests, or exclusions are invalid",
        ));
    }
    forces_low |= !missing.is_empty();
    let expected_confidence = if forces_low {
        Confidence::Low
    } else {
        Confidence::High
    };
    if attempt.report.effective_confidence != expected_confidence {
        return Err(PillarCheckpointError::new(
            "pillar report effective confidence is inconsistent with its evidence",
        ));
    }
    if attempt.status == PillarAttemptStatus::Unavailable
        && attempt.report.effective_confidence != Confidence::Low
    {
        return Err(PillarCheckpointError::new(
            "an unavailable pillar attempt cannot checkpoint a high-confidence report",
        ));
    }
    Ok(())
}

fn known_pillar<'a>(
    opening: &'a ResearchPillarOpening,
    pillar_id: &str,
) -> Result<&'a ResearchPillar, PillarCheckpointError> {
    opening
        .pillars
        .iter()
        .find(|pillar| pillar.id == pillar_id)
        .ok_or_else(|| {
            PillarCheckpointError::new(format!("unknown checkpoint pillar {pillar_id:?}"))
        })
}

fn ensure_usable(state: &StoreState) -> Result<(), PillarCheckpointError> {
    if let Some(error) = &state.poisoned {
        Err(PillarCheckpointError::new(format!(
            "pillar checkpoint is latched after a durability failure: {error}"
        )))
    } else {
        Ok(())
    }
}

fn verify_current_record(
    path: &Path,
    expected: &CheckpointRecord,
) -> Result<(), PillarCheckpointError> {
    reject_symlink_if_present(path, "pillar checkpoint state")?;
    let bytes = std::fs::read(path).map_err(|error| {
        PillarCheckpointError::io("cannot reread pillar checkpoint state", error)
    })?;
    let current = decode_record(&bytes)?;
    validate_record(&current)?;
    if &current != expected {
        return Err(PillarCheckpointError::new(
            "pillar checkpoint changed outside its active writer",
        ));
    }
    Ok(())
}

fn seal_record(material: CheckpointMaterial) -> Result<CheckpointRecord, PillarCheckpointError> {
    let checkpoint_hash = hash_serializable(&material)?;
    let record = CheckpointRecord {
        material,
        checkpoint_hash,
    };
    validate_record(&record)?;
    Ok(record)
}

fn seal_opening_stage_record(
    material: OpeningStageMaterial,
) -> Result<OpeningStageRecord, PillarCheckpointError> {
    let checkpoint_hash = hash_serializable(&material)?;
    let record = OpeningStageRecord {
        material,
        checkpoint_hash,
    };
    validate_opening_stage_record(&record)?;
    Ok(record)
}

fn decode_record(bytes: &[u8]) -> Result<CheckpointRecord, PillarCheckpointError> {
    if !bytes.ends_with(b"\n") || bytes[..bytes.len().saturating_sub(1)].contains(&b'\n') {
        return Err(PillarCheckpointError::new(
            "pillar checkpoint is torn or contains multiple records",
        ));
    }
    serde_json::from_slice(&bytes[..bytes.len() - 1]).map_err(|error| {
        PillarCheckpointError::new(format!("pillar checkpoint JSON is invalid: {error}"))
    })
}

fn decode_opening_stage_record(bytes: &[u8]) -> Result<OpeningStageRecord, PillarCheckpointError> {
    if !bytes.ends_with(b"\n") || bytes[..bytes.len().saturating_sub(1)].contains(&b'\n') {
        return Err(PillarCheckpointError::new(
            "pillar opening stage is torn or contains multiple records",
        ));
    }
    serde_json::from_slice(&bytes[..bytes.len() - 1]).map_err(|error| {
        PillarCheckpointError::new(format!("pillar opening stage JSON is invalid: {error}"))
    })
}

fn write_record_atomic(
    path: &Path,
    record: &CheckpointRecord,
) -> Result<(), PillarCheckpointError> {
    let mut bytes = serde_json::to_vec(record).map_err(|error| {
        PillarCheckpointError::new(format!("cannot encode pillar checkpoint: {error}"))
    })?;
    bytes.push(b'\n');
    let parent = path
        .parent()
        .ok_or_else(|| PillarCheckpointError::new("pillar checkpoint has no parent directory"))?;
    let (temporary, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{STATE_FILE}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(PillarCheckpointError::io(
                    "cannot create pillar checkpoint temp",
                    error,
                ));
            }
        }
    };
    let result = (|| {
        file.write_all(&bytes).map_err(|error| {
            PillarCheckpointError::io("cannot write pillar checkpoint temp", error)
        })?;
        file.sync_all().map_err(|error| {
            PillarCheckpointError::io("cannot sync pillar checkpoint temp", error)
        })?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|error| {
            PillarCheckpointError::io("cannot atomically replace pillar checkpoint", error)
        })?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn write_opening_stage_record_atomic(
    path: &Path,
    record: &OpeningStageRecord,
) -> Result<(), PillarCheckpointError> {
    let mut bytes = serde_json::to_vec(record).map_err(|error| {
        PillarCheckpointError::new(format!("cannot encode pillar opening stage: {error}"))
    })?;
    bytes.push(b'\n');
    let parent = path.parent().ok_or_else(|| {
        PillarCheckpointError::new("pillar opening stage has no parent directory")
    })?;
    let (temporary, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{OPENING_STAGE_FILE}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(PillarCheckpointError::io(
                    "cannot create pillar opening stage temp",
                    error,
                ));
            }
        }
    };
    let result = (|| {
        file.write_all(&bytes).map_err(|error| {
            PillarCheckpointError::io("cannot write pillar opening stage temp", error)
        })?;
        file.sync_all().map_err(|error| {
            PillarCheckpointError::io("cannot sync pillar opening stage temp", error)
        })?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|error| {
            PillarCheckpointError::io("cannot atomically replace pillar opening stage", error)
        })?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn ensure_control_directory(
    parent: &Path,
    path: &Path,
    label: &str,
) -> Result<(), PillarCheckpointError> {
    let created = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(PillarCheckpointError::new(format!(
                "{label} is not a real directory: {}",
                path.display()
            )));
        }
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path).map_err(|error| {
                PillarCheckpointError::io(&format!("cannot create {label}"), error)
            })?;
            true
        }
        Err(error) => {
            return Err(PillarCheckpointError::io(
                &format!("cannot inspect {label}"),
                error,
            ));
        }
    };
    if created {
        sync_directory(parent)?;
    }
    Ok(())
}

fn reject_symlink_if_present(path: &Path, label: &str) -> Result<(), PillarCheckpointError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(PillarCheckpointError::new(
            format!("{label} must not be a symlink: {}", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PillarCheckpointError::io(
            &format!("cannot inspect {label}"),
            error,
        )),
    }
}

fn verify_linked_file(path: &Path, file: &File, label: &str) -> Result<(), PillarCheckpointError> {
    let open = file.metadata().map_err(|error| {
        PillarCheckpointError::io(&format!("cannot inspect open {label}"), error)
    })?;
    let linked = std::fs::symlink_metadata(path).map_err(|error| {
        PillarCheckpointError::io(&format!("cannot inspect {label} path"), error)
    })?;
    if linked.file_type().is_symlink() || !linked.is_file() {
        return Err(PillarCheckpointError::new(format!(
            "{label} path is not a regular file"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if open.dev() != linked.dev() || open.ino() != linked.ino() {
            return Err(PillarCheckpointError::new(format!(
                "{label} path was replaced"
            )));
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), PillarCheckpointError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            PillarCheckpointError::io("cannot sync pillar checkpoint directory", error)
        })
}

fn hash_serializable<T: Serialize + ?Sized>(value: &T) -> Result<String, PillarCheckpointError> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        PillarCheckpointError::new(format!(
            "cannot encode pillar checkpoint hash material: {error}"
        ))
    })?;
    Ok(sha256_digest(&bytes))
}

fn sha256_digest(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn canonical_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn checkpoint_generation_directory(working_root: &Path, frozen_spec_digest: &str) -> PathBuf {
    working_root
        .join(".swarm")
        .join(CHECKPOINT_DIRECTORY)
        .join(&frozen_spec_digest["sha256:".len()..])
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pillar::{
        AuthoredRequirement, CompiledResearchClaim, IntegrationContract, ResearchPillar,
    };
    use std::sync::Arc;

    fn opening() -> ResearchPillarOpening {
        ResearchPillarOpening {
            requirements: vec![
                AuthoredRequirement {
                    id: "req-ui".to_string(),
                    text: "Render status".to_string(),
                    critical: false,
                },
                AuthoredRequirement {
                    id: "req-api".to_string(),
                    text: "Persist status".to_string(),
                    critical: true,
                },
            ],
            pillars: vec![pillar("ui", "req-ui"), pillar("api", "req-api")],
            integration_contract: IntegrationContract {
                owner: "planner".to_string(),
                integration_required: true,
                objective: "Compose the application".to_string(),
                interface_invariants: vec!["One status contract".to_string()],
                acceptance_criteria: vec!["Runnable result".to_string()],
            },
        }
    }

    fn opening_receipt(opening: &ResearchPillarOpening) -> PillarOpeningCheckpointReceipt {
        PillarOpeningCheckpointReceipt::new(
            &opening_binding(),
            "opening-model",
            "opening-host",
            1,
            &["model-authored opening".to_string()],
            pillar_frozen_spec_digest("semantic topology"),
            opening,
        )
        .unwrap()
    }

    fn opening_binding() -> PillarOpeningContractBinding {
        PillarOpeningContractBinding {
            response_schema_digest: pillar_frozen_spec_digest("opening schema"),
            opener_contract_digest: pillar_frozen_spec_digest("opening contract"),
            integration_owner: "planner".to_string(),
            minimum_research_slices: 2,
        }
    }

    fn partial_opening_checkpoint(
        binding: &PillarOpeningContractBinding,
    ) -> PillarOpeningPartialCheckpoint {
        PillarOpeningPartialCheckpoint::new(
            binding.clone(),
            PillarOpeningRawOutputCheckpoint::new(
                "opening-model",
                "opening-host",
                1,
                r#"{"semantic_domains":[{"id":"domain-ui"}]}"#,
            )
            .unwrap(),
            PillarOpeningPartialSemanticState {
                semantic_domains: serde_json::json!([{
                    "id": "domain-ui",
                    "title": "Interface status",
                    "objective": "Research the user-visible status contract"
                }]),
                research_slices: serde_json::json!([]),
                integration_contract: opening().integration_contract,
                valid_domain_assignment_by_requirement: BTreeMap::from([(
                    "req-ui".to_string(),
                    "domain-ui".to_string(),
                )]),
                valid_slice_assignment_by_requirement: BTreeMap::new(),
                unresolved_domain_requirement_ids: vec!["req-api".to_string()],
                unresolved_slice_requirement_ids: vec!["req-api".to_string(), "req-ui".to_string()],
            },
        )
        .unwrap()
    }

    fn open_store(
        root: &Path,
        digest: &str,
        opening: &ResearchPillarOpening,
    ) -> Result<PillarCheckpointStore, PillarCheckpointError> {
        PillarCheckpointStore::open(root, digest, opening, &opening_receipt(opening))
    }

    fn load_test_opening(
        root: &Path,
        digest: &str,
        opening: &ResearchPillarOpening,
    ) -> Result<
        Option<(ResearchPillarOpening, PillarOpeningCheckpointReceipt)>,
        PillarCheckpointError,
    > {
        PillarCheckpointStore::load_opening(
            root,
            digest,
            &opening.requirements,
            &opening_binding(),
            &[("opening-model".to_string(), "opening-host".to_string())],
        )
    }

    fn pillar(id: &str, requirement_id: &str) -> ResearchPillar {
        ResearchPillar {
            id: id.to_string(),
            title: format!("{id} pillar"),
            objective: format!("Complete {id}"),
            requirement_ids: vec![requirement_id.to_string()],
            dependencies: Vec::new(),
            research_questions: vec![format!("What does {id} require?")],
            acceptance_criteria: vec![format!("{id} is demonstrable")],
            exclusions: vec!["Other pillars".to_string()],
        }
    }

    fn attempt(
        pillar_id: &str,
        requirement_id: &str,
        ordinal: u8,
        confidence: Confidence,
    ) -> PillarAttemptCheckpoint {
        let claim = CompiledResearchClaim {
            requirement_id: requirement_id.to_string(),
            statement: format!("Finding for {requirement_id}"),
            reported_class: if confidence == Confidence::High {
                EvidenceClass::Supported
            } else {
                EvidenceClass::Unresolved
            },
            effective_class: if confidence == Confidence::High {
                EvidenceClass::Supported
            } else {
                EvidenceClass::Unresolved
            },
            provenance: ProvenanceMatch::NotClaimed,
        };
        PillarAttemptCheckpoint {
            pillar_id: pillar_id.to_string(),
            attempt_ordinal: ordinal,
            model_id: format!("model-{pillar_id}"),
            physical_host: format!("host-{pillar_id}"),
            status: PillarAttemptStatus::ModelReport,
            report: CompiledPillarReport {
                pillar_id: pillar_id.to_string(),
                reported_confidence: confidence,
                effective_confidence: confidence,
                claims: vec![claim],
                missing_requirement_ids: Vec::new(),
                unresolved_uncertainties: Vec::new(),
                acceptance_tests: vec!["Run focused test".to_string()],
                interfaces: vec!["StatusContract".to_string()],
                exclusions: vec!["Other pillar implementation".to_string()],
            },
        }
    }

    #[test]
    fn writes_reopens_and_reuses_a_completed_attempt() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let digest = pillar_frozen_spec_digest("frozen spec");
        let store = open_store(root.path(), &digest, &opening).unwrap();
        let saved = attempt("ui", "req-ui", 1, Confidence::High);
        let receipt = store.persist_attempt(saved.clone()).unwrap();
        assert_eq!(receipt.revision, 1);
        assert!(!receipt.reused);
        let persisted = std::fs::read_to_string(&store.state_path).unwrap();
        assert!(store
            .state_path
            .to_string_lossy()
            .contains("pillar-checkpoint-v3"));
        assert!(persisted.contains("\"schema_version\":3"));
        assert!(persisted.contains("\"status\":\"model_report\""));
        drop(store);

        let reopened = open_store(root.path(), &digest, &opening).unwrap();
        assert_eq!(reopened.completed_attempts("ui").unwrap(), vec![saved]);
        assert_eq!(
            reopened.resume_decision("ui").unwrap(),
            PillarResumeDecision::ReusePrimaryHigh
        );
        assert_eq!(reopened.next_attempt_ordinal("ui").unwrap(), None);
    }

    #[test]
    fn rejects_tampered_and_mismatched_state() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let digest = pillar_frozen_spec_digest("frozen spec");
        let store = open_store(root.path(), &digest, &opening).unwrap();
        let state_path = store.state_path.clone();
        drop(store);

        let different_root = tempfile::tempdir().unwrap();
        let copied_directory = different_root
            .path()
            .join(".swarm")
            .join(CHECKPOINT_DIRECTORY)
            .join(&digest["sha256:".len()..]);
        std::fs::create_dir_all(&copied_directory).unwrap();
        std::fs::copy(&state_path, copied_directory.join(STATE_FILE)).unwrap();
        assert!(open_store(different_root.path(), &digest, &opening).is_err());

        let mut changed_opening = opening.clone();
        changed_opening.requirements[0]
            .text
            .push_str(" with animation");
        assert!(open_store(root.path(), &digest, &changed_opening).is_err());

        let mut bytes = std::fs::read(&state_path).unwrap();
        let revision = bytes
            .windows(b"\"revision\":0".len())
            .position(|window| window == b"\"revision\":0")
            .unwrap();
        bytes[revision + b"\"revision\":".len()] = b'9';
        std::fs::write(&state_path, bytes).unwrap();
        assert!(open_store(root.path(), &digest, &opening).is_err());
    }

    #[test]
    fn frozen_specs_share_a_root_without_overwriting_each_others_attempts() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let digest_a = pillar_frozen_spec_digest("frozen spec A");
        let digest_b = pillar_frozen_spec_digest("frozen spec B");
        let attempt_a = attempt("ui", "req-ui", 1, Confidence::Low);
        let mut attempt_b = attempt("ui", "req-ui", 1, Confidence::High);
        attempt_b.report.claims[0].statement = "Spec B finding".to_string();

        let store_a = open_store(root.path(), &digest_a, &opening).unwrap();
        store_a.persist_attempt(attempt_a.clone()).unwrap();
        let state_a = store_a.state_path.clone();
        drop(store_a);

        let store_b = open_store(root.path(), &digest_b, &opening).unwrap();
        store_b.persist_attempt(attempt_b.clone()).unwrap();
        let state_b = store_b.state_path.clone();
        assert_ne!(state_a, state_b);
        assert_eq!(store_b.completed_attempts("ui").unwrap(), vec![attempt_b]);
        drop(store_b);

        assert_eq!(
            load_test_opening(root.path(), &digest_a, &opening)
                .unwrap()
                .map(|(opening, _)| opening),
            Some(opening.clone()),
        );
        let resumed_a = open_store(root.path(), &digest_a, &opening).unwrap();
        assert_eq!(resumed_a.completed_attempts("ui").unwrap(), vec![attempt_a]);
        assert_eq!(
            resumed_a.resume_decision("ui").unwrap(),
            PillarResumeDecision::RunFocusedRetry
        );
    }

    #[test]
    fn primary_high_is_complete_and_never_requests_a_retry() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let digest = pillar_frozen_spec_digest("spec");
        let store = open_store(root.path(), &digest, &opening).unwrap();
        store
            .persist_attempt(attempt("ui", "req-ui", 1, Confidence::High))
            .unwrap();
        assert_eq!(store.next_attempt_ordinal("ui").unwrap(), None);
        assert!(store
            .persist_attempt(attempt("ui", "req-ui", 2, Confidence::High))
            .is_err());
    }

    #[test]
    fn primary_low_reopens_with_exactly_one_missing_retry() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let digest = pillar_frozen_spec_digest("spec");
        let store = open_store(root.path(), &digest, &opening).unwrap();
        store
            .persist_attempt(attempt("ui", "req-ui", 1, Confidence::Low))
            .unwrap();
        drop(store);

        let reopened = open_store(root.path(), &digest, &opening).unwrap();
        assert_eq!(reopened.next_attempt_ordinal("ui").unwrap(), Some(2));
        reopened
            .persist_attempt(attempt("ui", "req-ui", 2, Confidence::High))
            .unwrap();
        assert_eq!(reopened.next_attempt_ordinal("ui").unwrap(), None);
        assert_eq!(
            reopened.resume_decision("ui").unwrap(),
            PillarResumeDecision::ReuseFocusedRetry
        );
    }

    #[test]
    fn unavailable_owner_attempt_is_durable_and_still_requests_one_focused_retry() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let digest = pillar_frozen_spec_digest("unavailable owner spec");
        let store = open_store(root.path(), &digest, &opening).unwrap();
        let mut unavailable = attempt("ui", "req-ui", 1, Confidence::Low);
        unavailable.status = PillarAttemptStatus::Unavailable;
        store.persist_attempt(unavailable.clone()).unwrap();
        let state_path = store.state_path.clone();
        drop(store);

        let persisted = std::fs::read_to_string(state_path).unwrap();
        assert!(persisted.contains("\"status\":\"unavailable\""));
        let reopened = open_store(root.path(), &digest, &opening).unwrap();
        assert_eq!(
            reopened.completed_attempts("ui").unwrap(),
            vec![unavailable]
        );
        assert_eq!(
            reopened.resume_decision("ui").unwrap(),
            PillarResumeDecision::RunFocusedRetry
        );
    }

    #[test]
    fn concurrent_pillars_share_one_mutex_safe_store() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let digest = pillar_frozen_spec_digest("spec");
        let store = Arc::new(open_store(root.path(), &digest, &opening).unwrap());
        let ui = {
            let store = store.clone();
            std::thread::spawn(move || {
                store
                    .persist_attempt(attempt("ui", "req-ui", 1, Confidence::High))
                    .unwrap();
            })
        };
        let api = {
            let store = store.clone();
            std::thread::spawn(move || {
                store
                    .persist_attempt(attempt("api", "req-api", 1, Confidence::High))
                    .unwrap();
            })
        };
        ui.join().unwrap();
        api.join().unwrap();
        assert_eq!(store.completed_attempts("ui").unwrap().len(), 1);
        assert_eq!(store.completed_attempts("api").unwrap().len(), 1);
    }

    #[test]
    fn opening_can_be_restored_without_repeating_the_model_call() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let digest = pillar_frozen_spec_digest("frozen spec");
        assert!(load_test_opening(root.path(), &digest, &opening)
            .unwrap()
            .is_none());
        drop(open_store(root.path(), &digest, &opening).unwrap());
        assert_eq!(
            load_test_opening(root.path(), &digest, &opening).unwrap(),
            Some((opening.clone(), opening_receipt(&opening)))
        );
    }

    #[test]
    fn opening_restore_requires_the_bound_schema_floor_lane_and_compiler_receipt() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let digest = pillar_frozen_spec_digest("frozen spec");
        drop(open_store(root.path(), &digest, &opening).unwrap());

        let (_, receipt) = load_test_opening(root.path(), &digest, &opening)
            .unwrap()
            .unwrap();
        assert_eq!(receipt.minimum_research_slices, 2);
        assert_eq!(receipt.accepted_model_id, "opening-model");
        assert_eq!(receipt.accepted_physical_host, "opening-host");
        assert_eq!(receipt.accepted_attempt, 1);
        assert_eq!(receipt.raw_output_digests.len(), 1);
        assert!(canonical_digest(&receipt.compiler_receipt_digest));

        let mut schema_binding = opening_binding();
        schema_binding.response_schema_digest = pillar_frozen_spec_digest("different schema");
        let schema_mismatch = PillarCheckpointStore::load_opening(
            root.path(),
            &digest,
            &opening.requirements,
            &schema_binding,
            &[("opening-model".to_string(), "opening-host".to_string())],
        );
        assert!(schema_mismatch.is_err());
        let mut floor_binding = opening_binding();
        floor_binding.minimum_research_slices = 1;
        let floor_mismatch = PillarCheckpointStore::load_opening(
            root.path(),
            &digest,
            &opening.requirements,
            &floor_binding,
            &[("opening-model".to_string(), "opening-host".to_string())],
        );
        assert!(floor_mismatch.is_err());
        let lane_mismatch = PillarCheckpointStore::load_opening(
            root.path(),
            &digest,
            &opening.requirements,
            &opening_binding(),
            &[("other-model".to_string(), "other-host".to_string())],
        );
        assert!(lane_mismatch.is_err());

        let mut tampered = receipt;
        tampered.accepted_attempt = 2;
        assert!(tampered.validate(&opening).is_err());
    }

    #[test]
    fn staged_opening_restores_exact_repair_state_and_rejects_raw_output_tampering() {
        let root = tempfile::tempdir().unwrap();
        let authored_requirements = opening().requirements;
        let digest = pillar_frozen_spec_digest("staged frozen spec");
        let binding = opening_binding();
        let candidate = partial_opening_checkpoint(&binding);
        let lanes = [("opening-model".to_string(), "opening-host".to_string())];

        PillarOpeningStageStore::persist(
            root.path(),
            &digest,
            &authored_requirements,
            PillarOpeningCheckpointStage::FullCandidate {
                candidate: candidate.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            PillarOpeningStageStore::load(
                root.path(),
                &digest,
                &authored_requirements,
                &binding,
                &lanes,
            )
            .unwrap(),
            Some(PillarOpeningCheckpointStage::FullCandidate {
                candidate: candidate.clone(),
            })
        );

        let mut different_binding = binding.clone();
        different_binding.minimum_research_slices += 1;
        assert!(PillarOpeningStageStore::load(
            root.path(),
            &digest,
            &authored_requirements,
            &different_binding,
            &lanes,
        )
        .is_err());

        PillarOpeningStageStore::persist(
            root.path(),
            &digest,
            &authored_requirements,
            PillarOpeningCheckpointStage::FocusedRepair {
                candidate: candidate.clone(),
                repair_attempts: Vec::new(),
                attempts: 1,
            },
        )
        .unwrap();
        let repair = PillarOpeningRawOutputCheckpoint::new(
            "opening-model",
            "opening-host",
            2,
            r#"{"domain_assignment_by_requirement":{"req-api":"domain-ui"}}"#,
        )
        .unwrap();
        let focused = PillarOpeningCheckpointStage::FocusedRepair {
            candidate: candidate.clone(),
            repair_attempts: vec![repair.clone()],
            attempts: 2,
        };
        PillarOpeningStageStore::persist(
            root.path(),
            &digest,
            &authored_requirements,
            focused.clone(),
        )
        .unwrap();
        let unavailable = PillarOpeningCheckpointStage::Unavailable {
            binding: binding.clone(),
            candidate: Some(candidate.clone()),
            full_attempts: Vec::new(),
            repair_attempts: vec![repair.clone()],
            attempts: 2,
            reason: "authenticated lanes exhausted during focused repair".to_string(),
        };
        PillarOpeningStageStore::persist(
            root.path(),
            &digest,
            &authored_requirements,
            unavailable.clone(),
        )
        .unwrap();
        assert_eq!(
            PillarOpeningStageStore::load(
                root.path(),
                &digest,
                &authored_requirements,
                &binding,
                &lanes,
            )
            .unwrap(),
            Some(unavailable)
        );
        PillarOpeningStageStore::persist(root.path(), &digest, &authored_requirements, focused)
            .unwrap();

        let canonical_root = std::fs::canonicalize(root.path()).unwrap();
        let stage_path =
            checkpoint_generation_directory(&canonical_root, &digest).join(OPENING_STAGE_FILE);
        let mut record = decode_opening_stage_record(&std::fs::read(&stage_path).unwrap()).unwrap();
        let PillarOpeningCheckpointStage::FocusedRepair { candidate, .. } =
            &mut record.material.stage
        else {
            panic!("focused repair stage must remain durable");
        };
        candidate.full_candidate.raw_output.push_str(" tampered");
        record.checkpoint_hash = hash_serializable(&record.material).unwrap();
        let mut tampered_bytes = serde_json::to_vec(&record).unwrap();
        tampered_bytes.push(b'\n');
        std::fs::write(stage_path, tampered_bytes).unwrap();

        assert!(PillarOpeningStageStore::load(
            root.path(),
            &digest,
            &authored_requirements,
            &binding,
            &lanes,
        )
        .is_err());
    }

    #[test]
    fn legacy_v1_opening_namespace_is_not_reused() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let digest = pillar_frozen_spec_digest("frozen spec");
        let legacy_generation = root
            .path()
            .join(".swarm")
            .join("pillar-checkpoint-v1")
            .join(&digest["sha256:".len()..]);
        std::fs::create_dir_all(&legacy_generation).unwrap();
        std::fs::write(
            legacy_generation.join(STATE_FILE),
            b"legacy opening must never be decoded",
        )
        .unwrap();

        assert!(load_test_opening(root.path(), &digest, &opening)
            .unwrap()
            .is_none());
    }
}
