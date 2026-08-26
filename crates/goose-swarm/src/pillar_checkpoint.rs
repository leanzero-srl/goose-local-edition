use crate::pillar::{
    validate_pillar_opening, CompiledPillarReport, Confidence, EvidenceClass, ProvenanceMatch,
    ResearchPillar, ResearchPillarOpening,
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

const SCHEMA_VERSION: u32 = 2;
const CHECKPOINT_DIRECTORY: &str = "pillar-checkpoint-v2";
const STATE_FILE: &str = "state.json";
const LOCK_FILE: &str = "control.lock";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PillarAttemptCheckpoint {
    pub pillar_id: String,
    pub attempt_ordinal: u8,
    pub model_id: String,
    pub physical_host: String,
    pub report: CompiledPillarReport,
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
    attempts: BTreeMap<String, Vec<PillarAttemptCheckpoint>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CheckpointRecord {
    material: CheckpointMaterial,
    checkpoint_hash: String,
}

struct StoreState {
    record: CheckpointRecord,
    poisoned: Option<String>,
}

pub struct PillarCheckpointStore {
    state_path: PathBuf,
    _lock: File,
    state: Mutex<StoreState>,
}

impl PillarCheckpointStore {
    pub fn load_opening(
        working_root: impl AsRef<Path>,
        frozen_spec_digest: impl Into<String>,
        authored_requirements: &[crate::pillar::AuthoredRequirement],
    ) -> Result<Option<ResearchPillarOpening>, PillarCheckpointError> {
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
        {
            return Err(PillarCheckpointError::new(
                "pillar checkpoint is incompatible with this root or frozen specification",
            ));
        }
        Ok(Some(record.material.opening))
    }

    pub fn open(
        working_root: impl AsRef<Path>,
        frozen_spec_digest: impl Into<String>,
        opening: &ResearchPillarOpening,
    ) -> Result<Self, PillarCheckpointError> {
        validate_pillar_opening(opening).map_err(|error| {
            PillarCheckpointError::new(format!("cannot checkpoint invalid pillar opening: {error}"))
        })?;
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
                {
                    return Err(PillarCheckpointError::new(
                        "pillar checkpoint is incompatible with this root, frozen specification, or opening",
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
        let store = PillarCheckpointStore::open(root.path(), &digest, &opening).unwrap();
        let saved = attempt("ui", "req-ui", 1, Confidence::High);
        let receipt = store.persist_attempt(saved.clone()).unwrap();
        assert_eq!(receipt.revision, 1);
        assert!(!receipt.reused);
        drop(store);

        let reopened = PillarCheckpointStore::open(root.path(), &digest, &opening).unwrap();
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
        let store = PillarCheckpointStore::open(root.path(), &digest, &opening).unwrap();
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
        assert!(PillarCheckpointStore::open(different_root.path(), &digest, &opening).is_err());

        let mut changed_opening = opening.clone();
        changed_opening.requirements[0]
            .text
            .push_str(" with animation");
        assert!(PillarCheckpointStore::open(root.path(), &digest, &changed_opening).is_err());

        let mut bytes = std::fs::read(&state_path).unwrap();
        let revision = bytes
            .windows(b"\"revision\":0".len())
            .position(|window| window == b"\"revision\":0")
            .unwrap();
        bytes[revision + b"\"revision\":".len()] = b'9';
        std::fs::write(&state_path, bytes).unwrap();
        assert!(PillarCheckpointStore::open(root.path(), &digest, &opening).is_err());
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

        let store_a = PillarCheckpointStore::open(root.path(), &digest_a, &opening).unwrap();
        store_a.persist_attempt(attempt_a.clone()).unwrap();
        let state_a = store_a.state_path.clone();
        drop(store_a);

        let store_b = PillarCheckpointStore::open(root.path(), &digest_b, &opening).unwrap();
        store_b.persist_attempt(attempt_b.clone()).unwrap();
        let state_b = store_b.state_path.clone();
        assert_ne!(state_a, state_b);
        assert_eq!(store_b.completed_attempts("ui").unwrap(), vec![attempt_b]);
        drop(store_b);

        assert_eq!(
            PillarCheckpointStore::load_opening(
                root.path(),
                digest_a.clone(),
                &opening.requirements,
            )
            .unwrap(),
            Some(opening.clone())
        );
        let resumed_a = PillarCheckpointStore::open(root.path(), &digest_a, &opening).unwrap();
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
        let store =
            PillarCheckpointStore::open(root.path(), pillar_frozen_spec_digest("spec"), &opening)
                .unwrap();
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
        let store = PillarCheckpointStore::open(root.path(), &digest, &opening).unwrap();
        store
            .persist_attempt(attempt("ui", "req-ui", 1, Confidence::Low))
            .unwrap();
        drop(store);

        let reopened = PillarCheckpointStore::open(root.path(), &digest, &opening).unwrap();
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
    fn concurrent_pillars_share_one_mutex_safe_store() {
        let root = tempfile::tempdir().unwrap();
        let opening = opening();
        let store = Arc::new(
            PillarCheckpointStore::open(root.path(), pillar_frozen_spec_digest("spec"), &opening)
                .unwrap(),
        );
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
        assert!(PillarCheckpointStore::load_opening(
            root.path(),
            digest.clone(),
            &opening.requirements,
        )
        .unwrap()
        .is_none());
        drop(PillarCheckpointStore::open(root.path(), digest.clone(), &opening).unwrap());
        assert_eq!(
            PillarCheckpointStore::load_opening(root.path(), digest, &opening.requirements,)
                .unwrap(),
            Some(opening)
        );
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

        assert!(
            PillarCheckpointStore::load_opening(root.path(), digest, &opening.requirements,)
                .unwrap()
                .is_none()
        );
    }
}
