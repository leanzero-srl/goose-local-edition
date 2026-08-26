use crate::broker::{
    AdmissionReceipt, AuthorityScope, LocalCompletionKind, ProviderTerminalKind, WorkRole,
};
use crate::semantic_control::{
    semantic_observation_task_version, AdmittedSemanticObservationHandle,
    AdmittedSemanticObservationReceipt,
};
use crate::semantic_observation::{
    AcceptanceCriterionSnapshot, ArtifactExcerptSnapshot, ParsedSemanticObservation,
    SealedSemanticObservationSnapshot, SemanticObservationBody, SemanticObservationSnapshotDraft,
    SemanticTraceSnapshot, SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
};
use anyhow::{anyhow, bail, Result};
use fs2::FileExt;
use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex as StdMutex, OnceLock};

const REPAIR_COMPOSITION_PROTOCOL: &str = "goose-repair-composition-v3";
const SEMANTIC_REVIEW_PROTOCOL: &str = "goose-repair-semantic-review-v2";
const PROVISIONAL_RECEIPT_PROTOCOL: &str = "goose-provisional-task-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SalvageReason {
    ProgressWatchdog,
    StallExhausted,
    FinalizeSpin,
    DeterministicAccept,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredVerification {
    FullRepairRuler,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ArtifactEvidence {
    sha256: String,
    bytes: u64,
    mode: u32,
}

impl ArtifactEvidence {
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn mode(&self) -> u32 {
        self.mode
    }
}

/// Engine-minted evidence that a task released its dependents provisionally. Fields are private and
/// the type is not deserializable, so a dispatcher can request salvage but cannot forge this receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProvisionalTaskReceipt {
    receipt_id: String,
    task_id: String,
    attempt: u32,
    task_contract: String,
    reason: SalvageReason,
    artifacts: BTreeMap<String, ArtifactEvidence>,
    required_verification: RequiredVerification,
}

impl ProvisionalTaskReceipt {
    pub fn receipt_id(&self) -> &str {
        &self.receipt_id
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn task_contract(&self) -> &str {
        &self.task_contract
    }

    pub fn reason(&self) -> SalvageReason {
        self.reason
    }

    pub fn artifacts(&self) -> &BTreeMap<String, ArtifactEvidence> {
        &self.artifacts
    }

    pub fn required_verification(&self) -> RequiredVerification {
        self.required_verification
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "receipt", rename_all = "snake_case")]
pub enum TaskCompletionDisposition {
    Complete,
    Salvaged(ProvisionalTaskReceipt),
}

impl TaskCompletionDisposition {
    pub fn is_salvaged(&self) -> bool {
        matches!(self, Self::Salvaged(_))
    }

    pub fn provisional_receipt(&self) -> Option<&ProvisionalTaskReceipt> {
        match self {
            Self::Complete => None,
            Self::Salvaged(receipt) => Some(receipt),
        }
    }
}

fn looks_like_test_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(lower.as_str());
    base.starts_with("test_")
        || base.ends_with("_test.py")
        || base.ends_with("_test.rs")
        || base.ends_with("_test.go")
        || base.contains(".test.")
        || base.contains(".spec.")
        || base == "conftest.py"
        || lower.contains("/tests/")
        || lower.contains("/test/")
}

fn looks_like_manifest_file(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    matches!(
        base.as_str(),
        "go.mod"
            | "go.sum"
            | "package.json"
            | "package-lock.json"
            | "cargo.toml"
            | "cargo.lock"
            | "requirements.txt"
            | "setup.py"
            | "setup.cfg"
            | "pyproject.toml"
            | "__init__.py"
            | "tsconfig.json"
            | "gemfile"
    )
}

fn artifact_evidence_at(
    root: &Path,
    owned_files: &[String],
) -> Option<BTreeMap<String, ArtifactEvidence>> {
    if owned_files.is_empty() {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let mut artifacts = BTreeMap::new();
    for relative in owned_files {
        if !safe_relative_path(relative) || excluded_from_repair_tree(Path::new(relative)) {
            return None;
        }
        let path = root.join(relative);
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
            return None;
        }
        if !path.canonicalize().ok()?.starts_with(&root) {
            return None;
        }
        artifacts.insert(
            relative.clone(),
            ArtifactEvidence {
                sha256: format!("sha256:{}", sha256_hex(&std::fs::read(path).ok()?)),
                bytes: metadata.len(),
                mode: repair_entry_mode(&metadata),
            },
        );
    }
    (artifacts.len() == owned_files.len()).then_some(artifacts)
}

pub(crate) fn mint_provisional_task_receipt(
    root: &Path,
    task_id: &str,
    attempt: u32,
    task_contract: &str,
    reason: SalvageReason,
    owned_files: &[String],
) -> Option<ProvisionalTaskReceipt> {
    let strict_artifact_salvage = !matches!(reason, SalvageReason::DeterministicAccept);
    if task_id.trim().is_empty()
        || task_contract.trim().is_empty()
        || (strict_artifact_salvage
            && (task_id.to_lowercase().contains("test")
                || owned_files.iter().all(|path| looks_like_test_file(path))
                || owned_files
                    .iter()
                    .all(|path| looks_like_manifest_file(path))))
    {
        return None;
    }
    let artifacts = artifact_evidence_at(root, owned_files)?;
    let required_verification = RequiredVerification::FullRepairRuler;
    let identity = serde_json::to_vec(&(
        PROVISIONAL_RECEIPT_PROTOCOL,
        task_id,
        attempt,
        task_contract,
        reason,
        &artifacts,
        required_verification,
    ))
    .ok()?;
    Some(ProvisionalTaskReceipt {
        receipt_id: format!("provisional:{}", sha256_hex(&identity)),
        task_id: task_id.to_string(),
        attempt,
        task_contract: task_contract.to_string(),
        reason,
        artifacts,
        required_verification,
    })
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct DefectId(String);

impl DefectId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DefectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct RequirementId(String);

impl RequirementId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            bail!("repair requirement identity is empty");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RequirementId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct RulerLegId(String);

impl RulerLegId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            bail!("repair ruler leg identity is empty");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateId {
    Smoke,
    FailedTask,
    ProvisionalTask,
    MissingDeliverable,
    CrossModule,
    HttpTimeout,
    DomId,
    CssCoherence,
    SpecContract,
    Ruler,
}

#[allow(dead_code)]
impl GateId {
    fn invariant_name(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::FailedTask => "failed_task",
            Self::ProvisionalTask => "provisional_task",
            Self::MissingDeliverable => "missing_deliverable",
            Self::CrossModule => "cross_module",
            Self::HttpTimeout => "http_timeout",
            Self::DomId => "dom_id",
            Self::CssCoherence => "css_coherence",
            Self::SpecContract => "spec_contract",
            Self::Ruler => "ruler",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SubjectRef {
    File(String),
    Task(String),
    Interface(String),
    Requirement(RequirementId),
    Runtime(String),
}

#[allow(dead_code)]
impl SubjectRef {
    fn stable_name(&self) -> String {
        match self {
            Self::File(value) => format!("file:{value}"),
            Self::Task(value) => format!("task:{value}"),
            Self::Interface(value) => format!("interface:{value}"),
            Self::Requirement(value) => format!("requirement:{value}"),
            Self::Runtime(value) => format!("runtime:{value}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefectKind {
    RuntimeBoot,
    TestCollection,
    TestFailure,
    PlannedTaskFailure,
    ProvisionalCompletion,
    MissingArtifact,
    CrossModuleContract,
    UnsafeNetworkCall,
    MissingDomTarget,
    StyleMarkupMismatch,
    RequirementViolation,
    GateUnestablished,
    InvariantViolation,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MechanicalSeverity {
    Blocking,
    Major,
    Advisory,
    Unestablished,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactFact {
    AdvertisedEntryDidNotRun,
    TestCollectionFailed,
    NamedTestFailed,
    PlannedTaskFailed,
    ProvisionalArtifactUnverified,
    RequiredArtifactAbsent,
    CrossModuleContractDrift,
    UnboundedNetworkCall,
    ReferencedDomTargetAbsent,
    StyleMarkupContractDrift,
    AdvertisedRequirementViolated,
    GateDidNotEstablishVerdict,
    MechanicallyObservedInvariant,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ImpactEvidence {
    severity: MechanicalSeverity,
    fact: ImpactFact,
    gate: GateId,
}

impl ImpactEvidence {
    pub fn severity(&self) -> MechanicalSeverity {
        self.severity
    }

    pub fn fact(&self) -> ImpactFact {
        self.fact
    }

    pub fn blocks_promotion(&self) -> bool {
        matches!(
            self.severity,
            MechanicalSeverity::Blocking | MechanicalSeverity::Unestablished
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EvidenceRef {
    sha256: String,
    relative_path: String,
    media_type: String,
    bytes: usize,
}

impl EvidenceRef {
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FindingProvenance {
    ruler_id: String,
    ruler_authority: String,
    ruler_leg: RulerLegId,
    gate: GateId,
    first_seen_tree: String,
    last_seen_tree: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DefectObservation {
    id: DefectId,
    provenance: FindingProvenance,
    causal_key: String,
    requirement_ids: BTreeSet<RequirementId>,
    subjects: BTreeSet<SubjectRef>,
    kind: DefectKind,
    impact: ImpactEvidence,
    evidence: Vec<EvidenceRef>,
    invariant: String,
}

impl DefectObservation {
    pub fn id(&self) -> &DefectId {
        &self.id
    }

    pub fn requirement_ids(&self) -> &BTreeSet<RequirementId> {
        &self.requirement_ids
    }

    pub fn subjects(&self) -> &BTreeSet<SubjectRef> {
        &self.subjects
    }

    pub fn kind(&self) -> DefectKind {
        self.kind
    }

    pub fn impact(&self) -> &ImpactEvidence {
        &self.impact
    }

    pub fn evidence(&self) -> &[EvidenceRef] {
        &self.evidence
    }

    pub fn provenance(&self) -> &FindingProvenance {
        &self.provenance
    }
}

pub struct FindingInput<'a> {
    pub gate: GateId,
    pub causal_key: &'a str,
    pub rendered: &'a str,
    pub known_files: &'a [String],
    pub explicit_subjects: BTreeSet<SubjectRef>,
    pub requirement_ids: BTreeSet<RequirementId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RulerIdentity {
    id: String,
    required_legs: BTreeSet<RulerLegId>,
    authority_nonce: String,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct EngineRulerAuthority {
    nonce: String,
}

#[allow(dead_code)]
impl EngineRulerAuthority {
    pub(crate) fn mint() -> Self {
        Self {
            nonce: random_authority_nonce("repair-ruler"),
        }
    }
}

impl RulerIdentity {
    #[allow(dead_code)]
    pub(crate) fn new(
        authority: &EngineRulerAuthority,
        id: impl Into<String>,
        required_legs: BTreeSet<RulerLegId>,
    ) -> Result<Self> {
        let id = id.into();
        if id.trim().is_empty() || required_legs.is_empty() {
            bail!("repair ruler identity and required legs must be non-empty");
        }
        Ok(Self {
            id,
            required_legs,
            authority_nonce: authority.nonce.clone(),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn required_legs(&self) -> &BTreeSet<RulerLegId> {
        &self.required_legs
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DefectLedger {
    tree_hash: String,
    ruler: RulerIdentity,
    established_legs: BTreeSet<RulerLegId>,
    requirements: BTreeSet<RequirementId>,
    observations: BTreeMap<DefectId, DefectObservation>,
    hash: String,
}

impl DefectLedger {
    #[allow(dead_code)]
    pub(crate) fn new(
        authority: &EngineRulerAuthority,
        tree_hash: impl Into<String>,
        ruler: RulerIdentity,
        established_legs: BTreeSet<RulerLegId>,
        requirements: BTreeSet<RequirementId>,
        observations: impl IntoIterator<Item = DefectObservation>,
    ) -> Result<Self> {
        let tree_hash = tree_hash.into();
        if ruler.authority_nonce != authority.nonce {
            bail!("repair ledger ruler was not minted by this engine authority");
        }
        if tree_hash.trim().is_empty() || requirements.is_empty() {
            bail!("repair ledger requires a tree identity and authoritative requirements");
        }
        if !established_legs.is_subset(ruler.required_legs()) {
            bail!("repair ledger established an unknown ruler leg");
        }
        let mut merged = BTreeMap::<DefectId, DefectObservation>::new();
        for mut observation in observations {
            validate_observation(&observation, &tree_hash, &ruler, &requirements)?;
            observation
                .evidence
                .sort_by(|left, right| left.sha256.cmp(&right.sha256));
            observation
                .evidence
                .dedup_by(|left, right| left.sha256 == right.sha256);
            match merged.get_mut(&observation.id) {
                Some(existing) => {
                    let mut same = existing.clone();
                    same.evidence.clear();
                    same.invariant.clear();
                    let mut incoming = observation.clone();
                    incoming.evidence.clear();
                    incoming.invariant.clear();
                    if same != incoming {
                        bail!("one causal defect identity carried contradictory provenance");
                    }
                    existing.evidence.extend(observation.evidence);
                    existing
                        .evidence
                        .sort_by(|left, right| left.sha256.cmp(&right.sha256));
                    existing
                        .evidence
                        .dedup_by(|left, right| left.sha256 == right.sha256);
                    if observation.invariant < existing.invariant {
                        existing.invariant = observation.invariant;
                    }
                }
                None => {
                    merged.insert(observation.id.clone(), observation);
                }
            }
        }
        let mut ledger = Self {
            tree_hash,
            ruler,
            established_legs,
            requirements,
            observations: merged,
            hash: String::new(),
        };
        ledger.hash = ledger.content_hash();
        Ok(ledger)
    }

    pub fn tree_hash(&self) -> &str {
        &self.tree_hash
    }

    pub fn ruler(&self) -> &RulerIdentity {
        &self.ruler
    }

    pub fn requirements(&self) -> &BTreeSet<RequirementId> {
        &self.requirements
    }

    pub fn observations(&self) -> &BTreeMap<DefectId, DefectObservation> {
        &self.observations
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn is_fully_established(&self) -> bool {
        self.established_legs == self.ruler.required_legs
    }

    pub fn blocking_ids(&self) -> BTreeSet<DefectId> {
        self.observations
            .iter()
            .filter(|(_, observation)| observation.impact.blocks_promotion())
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn evidence_ids(&self) -> BTreeSet<String> {
        self.observations
            .values()
            .flat_map(|observation| observation.evidence.iter())
            .map(|evidence| evidence.sha256.clone())
            .collect()
    }

    pub fn reconcile(&mut self, previous: &Self) -> Result<()> {
        if self.ruler != previous.ruler || self.requirements != previous.requirements {
            bail!("repair ledger reconciliation crossed ruler or requirement authority");
        }
        for (id, observation) in &mut self.observations {
            if let Some(prior) = previous.observations.get(id) {
                observation.provenance.first_seen_tree = prior.provenance.first_seen_tree.clone();
                observation.evidence.extend(prior.evidence.iter().cloned());
                observation
                    .evidence
                    .sort_by(|left, right| left.sha256.cmp(&right.sha256));
                observation
                    .evidence
                    .dedup_by(|left, right| left.sha256 == right.sha256);
            }
        }
        self.hash = self.content_hash();
        Ok(())
    }

    fn content_hash(&self) -> String {
        let bytes = serde_json::to_vec(&(
            &self.tree_hash,
            &self.ruler,
            &self.established_legs,
            &self.requirements,
            &self.observations,
        ))
        .expect("repair ledger contains only serializable engine values");
        sha256_hex(&bytes)
    }
}

#[allow(dead_code)]
fn validate_observation(
    observation: &DefectObservation,
    tree_hash: &str,
    ruler: &RulerIdentity,
    requirements: &BTreeSet<RequirementId>,
) -> Result<()> {
    if observation.provenance.ruler_id != ruler.id
        || observation.provenance.ruler_authority != ruler.authority_nonce
        || !ruler
            .required_legs
            .contains(&observation.provenance.ruler_leg)
        || observation.provenance.last_seen_tree != tree_hash
        || !observation.requirement_ids.is_subset(requirements)
        || observation.requirement_ids.is_empty()
        || observation.causal_key.trim().is_empty()
        || observation.evidence.is_empty()
        || observation
            .evidence
            .iter()
            .any(|evidence| evidence.sha256.trim().is_empty())
    {
        bail!("repair observation has foreign or incomplete provenance");
    }
    let expected_impact = impact_evidence(observation.provenance.gate, observation.kind);
    if observation.impact != expected_impact {
        bail!("repair observation severity is not supported by its mechanical fact");
    }
    let expected_id = defect_id(
        observation.provenance.gate,
        &observation.causal_key,
        &observation.subjects,
        &observation.requirement_ids,
    );
    if observation.id != expected_id {
        bail!("repair observation identity does not match its causal facts");
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn observe_finding(
    authority: &EngineRulerAuthority,
    root: &Path,
    tree_hash: &str,
    ruler: &RulerIdentity,
    ruler_leg: RulerLegId,
    mut input: FindingInput<'_>,
) -> Result<DefectObservation> {
    if ruler.authority_nonce != authority.nonce
        || !ruler.required_legs.contains(&ruler_leg)
        || input.requirement_ids.is_empty()
        || input.causal_key.trim().is_empty()
    {
        bail!("repair finding is not bound to a required ruler leg and requirement");
    }
    input
        .explicit_subjects
        .extend(subjects_from_known_files(input.rendered, input.known_files));
    let kind = defect_kind(input.gate, input.rendered);
    match kind {
        DefectKind::RuntimeBoot => {
            input
                .explicit_subjects
                .insert(SubjectRef::Runtime("advertised-entry".to_string()));
        }
        DefectKind::CrossModuleContract => {
            input
                .explicit_subjects
                .insert(SubjectRef::Interface("cross-module".to_string()));
        }
        DefectKind::RequirementViolation => {
            input.explicit_subjects.extend(
                input
                    .requirement_ids
                    .iter()
                    .cloned()
                    .map(SubjectRef::Requirement),
            );
        }
        _ => {}
    }
    let invariant = normalized_invariant(input.rendered, &input.explicit_subjects);
    let id = defect_id(
        input.gate,
        input.causal_key,
        &input.explicit_subjects,
        &input.requirement_ids,
    );
    Ok(DefectObservation {
        id,
        provenance: FindingProvenance {
            ruler_id: ruler.id.clone(),
            ruler_authority: authority.nonce.clone(),
            ruler_leg,
            gate: input.gate,
            first_seen_tree: tree_hash.to_string(),
            last_seen_tree: tree_hash.to_string(),
        },
        causal_key: input.causal_key.to_string(),
        requirement_ids: input.requirement_ids,
        subjects: input.explicit_subjects,
        kind,
        impact: impact_evidence(input.gate, kind),
        evidence: vec![persist_evidence(root, input.gate, input.rendered)?],
        invariant,
    })
}

#[allow(dead_code)]
fn observe_unestablished_leg(
    authority: &EngineRulerAuthority,
    root: &Path,
    tree_hash: &str,
    ruler: &RulerIdentity,
    ruler_leg: RulerLegId,
    requirement_ids: BTreeSet<RequirementId>,
    rendered: &str,
) -> Result<DefectObservation> {
    let kind = DefectKind::GateUnestablished;
    let subjects = BTreeSet::new();
    let invariant = normalized_invariant(rendered, &subjects);
    let gate = GateId::Ruler;
    let causal_key = format!("unestablished:{}", ruler_leg.as_str());
    Ok(DefectObservation {
        id: defect_id(gate, &causal_key, &subjects, &requirement_ids),
        provenance: FindingProvenance {
            ruler_id: ruler.id.clone(),
            ruler_authority: authority.nonce.clone(),
            ruler_leg,
            gate,
            first_seen_tree: tree_hash.to_string(),
            last_seen_tree: tree_hash.to_string(),
        },
        causal_key,
        requirement_ids,
        subjects,
        kind,
        impact: impact_evidence(gate, kind),
        evidence: vec![persist_evidence(root, gate, rendered)?],
        invariant,
    })
}

pub struct FindingBatch<'a> {
    pub ruler_leg: RulerLegId,
    pub gate: GateId,
    pub findings: &'a [String],
    pub causal_keys: &'a [String],
    pub established: bool,
    pub known_files: &'a [String],
    pub explicit_subjects: BTreeSet<SubjectRef>,
    pub requirement_ids: BTreeSet<RequirementId>,
}

#[allow(dead_code)]
pub(crate) fn build_defect_ledger(
    authority: &EngineRulerAuthority,
    root: &Path,
    tree_hash: &str,
    ruler: RulerIdentity,
    requirements: BTreeSet<RequirementId>,
    batches: Vec<FindingBatch<'_>>,
) -> Result<DefectLedger> {
    let mut observations = Vec::new();
    let mut established_legs = BTreeSet::new();
    let mut seen_legs = BTreeSet::new();
    for batch in batches {
        if !ruler.required_legs.contains(&batch.ruler_leg)
            || !seen_legs.insert(batch.ruler_leg.clone())
            || batch.requirement_ids.is_empty()
            || !batch.requirement_ids.is_subset(&requirements)
            || batch.findings.len() != batch.causal_keys.len()
        {
            bail!("repair ruler batch has duplicate, foreign, or unbound authority");
        }
        for (rendered, causal_key) in batch.findings.iter().zip(batch.causal_keys) {
            observations.push(observe_finding(
                authority,
                root,
                tree_hash,
                &ruler,
                batch.ruler_leg.clone(),
                FindingInput {
                    gate: batch.gate,
                    causal_key,
                    rendered,
                    known_files: batch.known_files,
                    explicit_subjects: batch.explicit_subjects.clone(),
                    requirement_ids: batch.requirement_ids.clone(),
                },
            )?);
        }
        if batch.established {
            established_legs.insert(batch.ruler_leg);
        } else {
            let rendered = format!(
                "ruler leg `{}` did not establish a complete verdict",
                batch.ruler_leg.as_str()
            );
            observations.push(observe_unestablished_leg(
                authority,
                root,
                tree_hash,
                &ruler,
                batch.ruler_leg,
                batch.requirement_ids,
                &rendered,
            )?);
        }
    }
    for missing in ruler.required_legs.difference(&seen_legs) {
        let rendered = format!("required ruler leg `{}` did not run", missing.as_str());
        observations.push(observe_unestablished_leg(
            authority,
            root,
            tree_hash,
            &ruler,
            missing.clone(),
            requirements.clone(),
            &rendered,
        )?);
    }
    DefectLedger::new(
        authority,
        tree_hash,
        ruler,
        established_legs,
        requirements,
        observations,
    )
}

#[allow(dead_code)]
fn defect_kind(gate: GateId, rendered: &str) -> DefectKind {
    match gate {
        GateId::FailedTask => DefectKind::PlannedTaskFailure,
        GateId::ProvisionalTask => DefectKind::ProvisionalCompletion,
        GateId::MissingDeliverable => DefectKind::MissingArtifact,
        GateId::CrossModule => DefectKind::CrossModuleContract,
        GateId::HttpTimeout => DefectKind::UnsafeNetworkCall,
        GateId::DomId => DefectKind::MissingDomTarget,
        GateId::CssCoherence => DefectKind::StyleMarkupMismatch,
        GateId::SpecContract => DefectKind::RequirementViolation,
        GateId::Ruler => DefectKind::GateUnestablished,
        GateId::Smoke => {
            let lower = rendered.to_lowercase();
            if lower.contains("collect") || lower.contains("compile") || lower.contains("syntax") {
                DefectKind::TestCollection
            } else if lower.contains("failed") || lower.contains("failure") {
                DefectKind::TestFailure
            } else if lower.contains("entry")
                || lower.contains("boot")
                || lower.contains("traceback")
            {
                DefectKind::RuntimeBoot
            } else {
                DefectKind::InvariantViolation
            }
        }
    }
}

#[allow(dead_code)]
fn impact_evidence(gate: GateId, kind: DefectKind) -> ImpactEvidence {
    let (severity, fact) = match kind {
        DefectKind::RuntimeBoot => (
            MechanicalSeverity::Blocking,
            ImpactFact::AdvertisedEntryDidNotRun,
        ),
        DefectKind::TestCollection => (
            MechanicalSeverity::Blocking,
            ImpactFact::TestCollectionFailed,
        ),
        DefectKind::TestFailure => (MechanicalSeverity::Major, ImpactFact::NamedTestFailed),
        DefectKind::PlannedTaskFailure => {
            (MechanicalSeverity::Blocking, ImpactFact::PlannedTaskFailed)
        }
        DefectKind::ProvisionalCompletion => (
            MechanicalSeverity::Blocking,
            ImpactFact::ProvisionalArtifactUnverified,
        ),
        DefectKind::MissingArtifact => (
            MechanicalSeverity::Blocking,
            ImpactFact::RequiredArtifactAbsent,
        ),
        DefectKind::CrossModuleContract => (
            MechanicalSeverity::Major,
            ImpactFact::CrossModuleContractDrift,
        ),
        DefectKind::UnsafeNetworkCall => {
            (MechanicalSeverity::Major, ImpactFact::UnboundedNetworkCall)
        }
        DefectKind::MissingDomTarget => (
            MechanicalSeverity::Major,
            ImpactFact::ReferencedDomTargetAbsent,
        ),
        DefectKind::StyleMarkupMismatch => (
            MechanicalSeverity::Advisory,
            ImpactFact::StyleMarkupContractDrift,
        ),
        DefectKind::RequirementViolation => (
            MechanicalSeverity::Blocking,
            ImpactFact::AdvertisedRequirementViolated,
        ),
        DefectKind::GateUnestablished => (
            MechanicalSeverity::Unestablished,
            ImpactFact::GateDidNotEstablishVerdict,
        ),
        DefectKind::InvariantViolation => (
            MechanicalSeverity::Major,
            ImpactFact::MechanicallyObservedInvariant,
        ),
    };
    ImpactEvidence {
        severity,
        fact,
        gate,
    }
}

#[allow(dead_code)]
fn subjects_from_known_files(rendered: &str, known_files: &[String]) -> BTreeSet<SubjectRef> {
    known_files
        .iter()
        .filter(|file| rendered.contains(file.as_str()))
        .map(|file| SubjectRef::File(file.replace('\\', "/")))
        .collect()
}

#[allow(dead_code)]
fn normalized_invariant(rendered: &str, subjects: &BTreeSet<SubjectRef>) -> String {
    static NORMALIZERS: OnceLock<Vec<Regex>> = OnceLock::new();
    let mut normalized = rendered.replace('\\', "/");
    for expression in NORMALIZERS.get_or_init(|| {
        [
            r"\x1b\[[0-9;]*[A-Za-z]",
            r"(?i)(localhost|127\.0\.0\.1|\[::1\]):[0-9]+",
            r#"(?i)(/private)?/(tmp|var/folders)/[^\s'"`]+"#,
            r"(?i)\bline\s+[0-9]+\b",
            r"(?i):[0-9]+(:[0-9]+)?\b",
            r"(?i)\b[0-9]+(?:\.[0-9]+)?\s*(ms|msec|seconds?|secs?|minutes?|mins?)\b",
            r"(?i)\b[0-9]+\s+(failed|passed|errors?|failures?|findings?|tests?)\b",
        ]
        .into_iter()
        .map(|expression| Regex::new(expression).expect("repair invariant regex is static"))
        .collect()
    }) {
        normalized = expression
            .replace_all(&normalized, "<volatile>")
            .into_owned();
    }
    let mut subject_names: Vec<String> = subjects
        .iter()
        .filter_map(|subject| match subject {
            SubjectRef::File(value) | SubjectRef::Task(value) => Some(value.clone()),
            _ => None,
        })
        .collect();
    subject_names.sort_by_key(|name| std::cmp::Reverse(name.len()));
    for subject in subject_names {
        normalized = normalized.replace(&subject, "<subject>");
    }
    let mut lines: Vec<String> = normalized
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .map(|line| line.to_lowercase())
        .collect();
    lines.sort();
    lines.dedup();
    lines.join("\n")
}

#[allow(dead_code)]
fn defect_id(
    gate: GateId,
    causal_key: &str,
    subjects: &BTreeSet<SubjectRef>,
    requirements: &BTreeSet<RequirementId>,
) -> DefectId {
    let mut identity = String::new();
    identity.push_str(gate.invariant_name());
    identity.push('\0');
    identity.push_str(causal_key);
    for subject in subjects {
        identity.push('\0');
        identity.push_str(&subject.stable_name());
    }
    for requirement in requirements {
        identity.push('\0');
        identity.push_str(requirement.as_str());
    }
    DefectId(format!("defect:{}", sha256_hex(identity.as_bytes())))
}

#[allow(dead_code)]
fn persist_evidence(root: &Path, gate: GateId, rendered: &str) -> Result<EvidenceRef> {
    static EVIDENCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sha256 = sha256_hex(rendered.as_bytes());
    let relative_path = format!(".swarm/repair/evidence/{sha256}.txt");
    let path = root.join(&relative_path);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("repair evidence path has no parent"))?;
    let canonical_root = root.canonicalize()?;
    create_contained_directory(&canonical_root, parent)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if std::fs::read(&path)? != rendered.as_bytes() {
                bail!("repair evidence digest collision at {relative_path}");
            }
        }
        Ok(_) => bail!("repair evidence path is not a regular contained file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let sequence = EVIDENCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let temp = parent.join(format!(".{sha256}.{}.{}.tmp", std::process::id(), sequence));
            write_new_file(&temp, rendered.as_bytes())?;
            match std::fs::rename(&temp, &path) {
                Ok(()) => {}
                Err(error) => {
                    let destination_is_same =
                        std::fs::symlink_metadata(&path).is_ok_and(|metadata| {
                            metadata.is_file() && !metadata.file_type().is_symlink()
                        }) && std::fs::read(&path)? == rendered.as_bytes();
                    let _ = std::fs::remove_file(&temp);
                    if !destination_is_same {
                        return Err(error.into());
                    }
                }
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(EvidenceRef {
        sha256,
        relative_path,
        media_type: format!("text/plain; gate={}", gate.invariant_name()),
        bytes: rendered.len(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateDelta {
    closed: BTreeSet<DefectId>,
    persisted: BTreeSet<DefectId>,
    introduced: BTreeSet<DefectId>,
    changed_evidence: BTreeSet<DefectId>,
    blocking_regressions: BTreeSet<DefectId>,
    fully_established: bool,
}

impl CandidateDelta {
    pub fn between(base: &DefectLedger, candidate: &DefectLedger) -> Self {
        let base_ids: BTreeSet<_> = base.observations.keys().cloned().collect();
        let candidate_ids: BTreeSet<_> = candidate.observations.keys().cloned().collect();
        let closed = base_ids.difference(&candidate_ids).cloned().collect();
        let introduced = candidate_ids.difference(&base_ids).cloned().collect();
        let persisted = base_ids.intersection(&candidate_ids).cloned().collect();
        let changed_evidence = base_ids
            .intersection(&candidate_ids)
            .filter(|id| {
                let before = &base.observations[*id];
                let after = &candidate.observations[*id];
                before.kind != after.kind
                    || before.impact != after.impact
                    || before.requirement_ids != after.requirement_ids
                    || before.subjects != after.subjects
                    || before.evidence != after.evidence
            })
            .cloned()
            .collect();
        let blocking_regressions = candidate
            .observations
            .iter()
            .filter(|(id, observation)| {
                observation.impact.blocks_promotion()
                    && base
                        .observations
                        .get(*id)
                        .is_none_or(|before| !before.impact.blocks_promotion())
            })
            .map(|(id, _)| id.clone())
            .collect();
        Self {
            closed,
            persisted,
            introduced,
            changed_evidence,
            blocking_regressions,
            fully_established: candidate.is_fully_established(),
        }
    }

    pub fn closed(&self) -> &BTreeSet<DefectId> {
        &self.closed
    }

    pub fn introduced(&self) -> &BTreeSet<DefectId> {
        &self.introduced
    }

    pub fn persisted(&self) -> &BTreeSet<DefectId> {
        &self.persisted
    }

    pub fn changed_evidence(&self) -> &BTreeSet<DefectId> {
        &self.changed_evidence
    }

    pub fn blocking_regressions(&self) -> &BTreeSet<DefectId> {
        &self.blocking_regressions
    }
}

pub fn safe_relative_path(path: &str) -> bool {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return false;
    }
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        let Component::Normal(name) = component else {
            return false;
        };
        normalized.push(name);
    }
    !normalized.as_os_str().is_empty() && normalized.as_os_str() == candidate.as_os_str()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut result = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use fmt::Write;
        let _ = write!(result, "{byte:02x}");
    }
    result
}

fn random_authority_nonce(namespace: &str) -> String {
    let random: [u8; 32] = rand::random();
    format!("{namespace}:{}", sha256_hex(&random))
}

fn create_contained_directory(canonical_root: &Path, directory: &Path) -> Result<()> {
    let mut existing = directory;
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| anyhow!("directory has no existing ancestor"))?;
    }
    if !existing.canonicalize()?.starts_with(canonical_root) {
        bail!("repair directory escaped the application root");
    }
    std::fs::create_dir_all(directory)?;
    if !directory.canonicalize()?.starts_with(canonical_root) {
        bail!("repair directory escaped the application root");
    }
    Ok(())
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn repair_entry_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn repair_entry_mode(metadata: &std::fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_readonly(mode != 0);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

pub fn excluded_from_repair_tree(relative: &Path) -> bool {
    const ENGINE_EVIDENCE_SUBTREES: &[&str] = &[".swarm", ".swarm-monitor", "bench-shots"];
    const ENGINE_EVIDENCE_FILES: &[&str] =
        &["run.jsonl", "engine-console.log", "heartbeat", "graded.db"];

    ENGINE_EVIDENCE_SUBTREES
        .iter()
        .any(|entry| relative.starts_with(entry))
        || ENGINE_EVIDENCE_FILES
            .iter()
            .any(|entry| relative == Path::new(entry))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RepairTreeSnapshot {
    hash: String,
    entries: BTreeMap<String, String>,
}

impl RepairTreeSnapshot {
    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn entries(&self) -> &BTreeMap<String, String> {
        &self.entries
    }
}

pub fn repair_tree_snapshot(root: &Path) -> Result<RepairTreeSnapshot> {
    fn collect(root: &Path, dir: &Path, entries: &mut BTreeMap<String, String>) -> Result<()> {
        let mut dir_entries = std::fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
        dir_entries.sort_by_key(|entry| entry.file_name());
        for entry in dir_entries {
            let path = entry.path();
            let relative = path.strip_prefix(root)?;
            if excluded_from_repair_tree(relative) {
                continue;
            }
            let metadata = std::fs::symlink_metadata(&path)?;
            let relative_name = relative.to_string_lossy().replace('\\', "/");
            let mut hasher = Sha256::new();
            if metadata.file_type().is_symlink() {
                hasher.update(b"symlink\0");
                hasher.update(std::fs::read_link(&path)?.to_string_lossy().as_bytes());
            } else if metadata.is_dir() {
                hasher.update(b"dir\0");
                hasher.update(repair_entry_mode(&metadata).to_be_bytes());
                entries.insert(relative_name, sha256_hex(&hasher.finalize()));
                collect(root, &path, entries)?;
                continue;
            } else if metadata.is_file() {
                hasher.update(b"file\0");
                hasher.update(repair_entry_mode(&metadata).to_be_bytes());
                hasher.update(std::fs::read(&path)?);
            } else {
                bail!("repair tree contains unsupported entry `{relative_name}`");
            }
            entries.insert(relative_name, sha256_hex(&hasher.finalize()));
        }
        Ok(())
    }

    let root = root.canonicalize()?;
    if !root.is_dir() {
        bail!("repair tree root is not a directory");
    }
    let mut entries = BTreeMap::new();
    collect(&root, &root, &mut entries)?;
    let bytes = serde_json::to_vec(&("goose-repair-tree-v3", &entries))?;
    Ok(RepairTreeSnapshot {
        hash: sha256_hex(&bytes),
        entries,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RepairEpoch {
    tree_hash: String,
    ledger_hash: String,
    transaction_nonce: String,
}

impl RepairEpoch {
    pub fn tree_hash(&self) -> &str {
        &self.tree_hash
    }

    pub fn ledger_hash(&self) -> &str {
        &self.ledger_hash
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileMutation {
    Write { bytes: Vec<u8>, mode: u32 },
    Delete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RepairCandidatePatch {
    id: String,
    base: RepairEpoch,
    targets: BTreeSet<DefectId>,
    changes: BTreeMap<String, FileMutation>,
}

impl RepairCandidatePatch {
    pub fn new(
        id: impl Into<String>,
        base: RepairEpoch,
        targets: BTreeSet<DefectId>,
        changes: BTreeMap<String, FileMutation>,
    ) -> Result<Self> {
        let id = id.into();
        if id.trim().is_empty() || targets.is_empty() || changes.is_empty() {
            bail!("repair candidate requires identity, causal targets, and changes");
        }
        for path in changes.keys() {
            if !safe_relative_path(path) || excluded_from_repair_tree(Path::new(path)) {
                bail!("repair candidate contains unruly path `{path}`");
            }
        }
        Ok(Self {
            id,
            base,
            targets,
            changes,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn targets(&self) -> &BTreeSet<DefectId> {
        &self.targets
    }

    pub fn changes(&self) -> &BTreeMap<String, FileMutation> {
        &self.changes
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SemanticReviewRequest {
    identity: String,
    composition_id: String,
    candidate_ids: Vec<String>,
    base_epoch: RepairEpoch,
    preview_epoch: RepairEpoch,
    targets: BTreeSet<DefectId>,
    changes: BTreeMap<String, FileMutation>,
    delta: CandidateDelta,
    requirements: BTreeSet<RequirementId>,
    evidence_ids: BTreeSet<String>,
}

impl SemanticReviewRequest {
    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn composition_id(&self) -> &str {
        &self.composition_id
    }

    pub fn targets(&self) -> &BTreeSet<DefectId> {
        &self.targets
    }

    pub fn requirements(&self) -> &BTreeSet<RequirementId> {
        &self.requirements
    }

    pub fn evidence_ids(&self) -> &BTreeSet<String> {
        &self.evidence_ids
    }

    pub fn delta(&self) -> &CandidateDelta {
        &self.delta
    }

    /// The immutable observation the broker must admit for this exact repair preview.
    pub fn observation_snapshot(&self) -> Result<SealedSemanticObservationSnapshot> {
        semantic_review_snapshot(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SemanticAcceptanceReceipt {
    receipt_id: String,
    review_identity: String,
    composition_id: String,
    admission: AdmissionReceipt,
    provider_terminal: ProviderTerminalKind,
    reviewer_reply_hash: String,
    cited_requirements: BTreeSet<RequirementId>,
    cited_evidence: BTreeSet<String>,
}

impl SemanticAcceptanceReceipt {
    pub fn receipt_id(&self) -> &str {
        &self.receipt_id
    }

    pub fn admission(&self) -> &AdmissionReceipt {
        &self.admission
    }

    pub fn provider_terminal(&self) -> ProviderTerminalKind {
        self.provider_terminal
    }

    pub fn reviewer_reply_hash(&self) -> &str {
        &self.reviewer_reply_hash
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PromotionDecision {
    Rejected {
        reason: String,
        preview_tree_hash: Option<String>,
        preview_ledger_hash: Option<String>,
        delta: Option<CandidateDelta>,
    },
    RolledBack {
        reason: String,
        restored_tree_hash: String,
    },
    Promoted {
        epoch_before: RepairEpoch,
        epoch_after: RepairEpoch,
        candidates: Vec<String>,
        changed_files: Vec<String>,
        delta: CandidateDelta,
        semantic_acceptance: Box<SemanticAcceptanceReceipt>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LockIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    platform_lock_held: bool,
}

struct ProcessRepairAuthorityGuard {
    root: PathBuf,
}

fn active_repair_roots() -> &'static StdMutex<BTreeSet<PathBuf>> {
    static ACTIVE_ROOTS: OnceLock<StdMutex<BTreeSet<PathBuf>>> = OnceLock::new();
    ACTIVE_ROOTS.get_or_init(|| StdMutex::new(BTreeSet::new()))
}

impl ProcessRepairAuthorityGuard {
    fn acquire(root: &Path) -> Result<Self> {
        let mut active = active_repair_roots()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !active.insert(root.to_path_buf()) {
            bail!("repair mutation authority is already held in this engine");
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }
}

impl Drop for ProcessRepairAuthorityGuard {
    fn drop(&mut self) {
        let mut active = active_repair_roots()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active.remove(&self.root);
    }
}

impl LockIdentity {
    fn from_file(file: &File) -> Result<Self> {
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            bail!("repair promotion lock is not a regular file");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                platform_lock_held: true,
            })
        }
    }

    fn from_path(path: &Path) -> Result<Self> {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("repair promotion lock path is not a regular file");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                platform_lock_held: true,
            })
        }
    }
}

impl PromotionDecision {
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Rejected { reason, .. } | Self::RolledBack { reason, .. } => Some(reason),
            Self::Promoted { .. } => None,
        }
    }
}

pub struct RepairTransaction {
    root: PathBuf,
    base_snapshot: RepairTreeSnapshot,
    base_ledger: DefectLedger,
    epoch: RepairEpoch,
    candidates: Vec<RepairCandidatePatch>,
    lock_file: File,
    lock_path: PathBuf,
    lock_identity: LockIdentity,
    _process_authority: ProcessRepairAuthorityGuard,
    consumed: bool,
}

impl RepairTransaction {
    pub fn open(root: &Path, base_ledger: DefectLedger) -> Result<Self> {
        let root = root.canonicalize()?;
        let process_authority = ProcessRepairAuthorityGuard::acquire(&root)?;
        let repair_dir = root.join(".swarm/repair");
        create_contained_directory(&root, &repair_dir)?;
        let lock_path = repair_dir.join("promotion.lock");
        let lock_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        FileExt::try_lock_exclusive(&lock_file)
            .map_err(|error| anyhow!("repair mutation authority is already held: {error}"))?;
        let lock_identity = LockIdentity::from_file(&lock_file)?;
        if LockIdentity::from_path(&lock_path)? != lock_identity {
            bail!("repair lock path was replaced while authority was acquired");
        }
        let base_snapshot = repair_tree_snapshot(&root)?;
        if base_ledger.tree_hash != base_snapshot.hash {
            bail!("repair ledger does not describe the tree opening this transaction");
        }
        if !base_ledger.is_fully_established() {
            bail!("repair transaction cannot open on a partial ruler");
        }
        let epoch = RepairEpoch {
            tree_hash: base_snapshot.hash.clone(),
            ledger_hash: base_ledger.hash.clone(),
            transaction_nonce: random_authority_nonce("repair-transaction"),
        };
        Ok(Self {
            root,
            base_snapshot,
            base_ledger,
            epoch,
            candidates: Vec::new(),
            lock_file,
            lock_path,
            lock_identity,
            _process_authority: process_authority,
            consumed: false,
        })
    }

    pub fn epoch(&self) -> RepairEpoch {
        self.epoch.clone()
    }

    pub fn add_candidate(&mut self, candidate: RepairCandidatePatch) -> Result<()> {
        if self.consumed {
            bail!("repair transaction has already been ruled");
        }
        if candidate.base != self.epoch() {
            bail!("repair candidate was generated against a stale tree or ledger");
        }
        if candidate
            .targets
            .iter()
            .any(|target| !self.base_ledger.observations.contains_key(target))
        {
            bail!("repair candidate targets a defect outside the base ledger");
        }
        if self
            .candidates
            .iter()
            .any(|existing| existing.id == candidate.id)
        {
            bail!("repair candidate identity `{}` is duplicated", candidate.id);
        }
        for existing in &self.candidates {
            if let Some(path) = candidate.changes.keys().find(|path| {
                existing
                    .changes
                    .keys()
                    .any(|existing_path| mutation_paths_overlap(path, existing_path))
            }) {
                bail!("repair candidates overlap on `{path}`");
            }
            if let Some(target) = candidate
                .targets
                .iter()
                .find(|target| existing.targets.contains(*target))
            {
                bail!("repair defect `{target}` has more than one primary candidate");
            }
        }
        self.candidates.push(candidate);
        Ok(())
    }

    pub async fn preview_and_promote<R, RFut, S, SFut>(
        &mut self,
        ruler: R,
        semantic_review: S,
    ) -> Result<PromotionDecision>
    where
        R: FnMut(PathBuf) -> RFut,
        RFut: std::future::Future<Output = Result<DefectLedger>>,
        S: FnOnce(SemanticReviewRequest) -> SFut,
        SFut: std::future::Future<Output = Result<AdmittedSemanticObservationHandle>>,
    {
        self.preview_and_promote_with_apply(ruler, semantic_review, apply_mutations)
            .await
    }

    async fn preview_and_promote_with_apply<R, RFut, S, SFut, A>(
        &mut self,
        mut ruler: R,
        semantic_review: S,
        mut real_apply: A,
    ) -> Result<PromotionDecision>
    where
        R: FnMut(PathBuf) -> RFut,
        RFut: std::future::Future<Output = Result<DefectLedger>>,
        S: FnOnce(SemanticReviewRequest) -> SFut,
        SFut: std::future::Future<Output = Result<AdmittedSemanticObservationHandle>>,
        A: FnMut(&Path, &BTreeMap<String, FileMutation>) -> Result<()>,
    {
        if self.consumed {
            bail!("repair transaction is one-shot and has already been ruled");
        }
        self.consumed = true;
        if !self.lock_authority_is_current()? {
            bail!("repair promotion lock authority was replaced");
        }
        if self.candidates.is_empty() {
            return Ok(rejected("transaction contains no candidates", None, None));
        }
        let current = repair_tree_snapshot(&self.root)?;
        if current.hash != self.base_snapshot.hash {
            return Ok(rejected(
                "real tree changed after transaction opened",
                None,
                None,
            ));
        }

        let changes = self.composed_changes();
        let preview = tempfile::TempDir::new()?;
        copy_ruled_tree(&self.root, preview.path())?;
        apply_mutations(preview.path(), &changes)?;
        let preview_before_ruler = repair_tree_snapshot(preview.path())?;
        if preview_before_ruler.hash == self.base_snapshot.hash {
            return Ok(rejected(
                "composed candidate is a byte-identical no-op",
                Some(&preview_before_ruler),
                None,
            ));
        }

        let preview_ledger = match ruler(preview.path().to_path_buf()).await {
            Ok(ledger) => ledger,
            Err(error) => {
                return Ok(rejected(
                    &format!("composed ruler failed: {error}"),
                    Some(&preview_before_ruler),
                    None,
                ));
            }
        };
        let preview_after_ruler = repair_tree_snapshot(preview.path())?;
        if preview_after_ruler != preview_before_ruler {
            return Ok(rejected(
                "full ruler mutated the composed preview",
                Some(&preview_after_ruler),
                Some(&preview_ledger),
            ));
        }
        if let Err(reason) = self.validate_candidate_ledger(&preview_ledger, &preview_after_ruler) {
            return Ok(rejected(
                &reason.to_string(),
                Some(&preview_after_ruler),
                Some(&preview_ledger),
            ));
        }
        let delta = CandidateDelta::between(&self.base_ledger, &preview_ledger);
        let targets = self.assigned_targets();
        if !targets.is_subset(&delta.closed) {
            return Ok(rejected_with_delta(
                "composed preview did not close every assigned causal defect",
                &preview_after_ruler,
                &preview_ledger,
                delta,
            ));
        }
        if !delta.blocking_regressions.is_empty() {
            return Ok(rejected_with_delta(
                "composed preview introduced or escalated a mechanically blocking defect",
                &preview_after_ruler,
                &preview_ledger,
                delta,
            ));
        }

        let composition_id = composition_id(&self.candidates);
        let preview_epoch = RepairEpoch {
            tree_hash: preview_after_ruler.hash.clone(),
            ledger_hash: preview_ledger.hash.clone(),
            transaction_nonce: self.epoch.transaction_nonce.clone(),
        };
        let evidence_ids = self
            .base_ledger
            .evidence_ids()
            .into_iter()
            .chain(preview_ledger.evidence_ids())
            .collect::<BTreeSet<_>>();
        let mut request = SemanticReviewRequest {
            identity: String::new(),
            composition_id,
            candidate_ids: self
                .candidates
                .iter()
                .map(|candidate| candidate.id.clone())
                .collect(),
            base_epoch: self.epoch(),
            preview_epoch,
            targets,
            changes: changes.clone(),
            delta: delta.clone(),
            requirements: self.base_ledger.requirements.clone(),
            evidence_ids,
        };
        request.identity = semantic_review_identity(&request);
        let expected_snapshot = request.observation_snapshot()?;
        let review = match semantic_review(request.clone()).await {
            Ok(review) => review,
            Err(error) => {
                return Ok(rejected_with_delta(
                    &format!("semantic review unavailable: {error}"),
                    &preview_after_ruler,
                    &preview_ledger,
                    delta,
                ));
            }
        };
        let handle_snapshot_hash = review.snapshot_hash().to_string();
        let handle_admission = review.admission().clone();
        let admitted_review = match review.wait().await {
            Ok(review) => review,
            Err(error) => {
                return Ok(rejected_with_delta(
                    &format!("semantic review provider lifecycle unavailable: {error}"),
                    &preview_after_ruler,
                    &preview_ledger,
                    delta,
                ));
            }
        };
        let semantic_acceptance = match self.validate_semantic_review(
            &request,
            &expected_snapshot,
            &handle_snapshot_hash,
            &handle_admission,
            admitted_review,
        ) {
            Ok(receipt) => receipt,
            Err(reason) => {
                return Ok(rejected_with_delta(
                    &reason.to_string(),
                    &preview_after_ruler,
                    &preview_ledger,
                    delta,
                ));
            }
        };

        let before_land = repair_tree_snapshot(&self.root)?;
        if !self.lock_authority_is_current()? {
            return Ok(rejected_with_delta(
                "repair promotion lock authority was replaced during review",
                &preview_after_ruler,
                &preview_ledger,
                delta,
            ));
        }
        if before_land.hash != self.base_snapshot.hash {
            return Ok(rejected_with_delta(
                "real tree changed while the composed preview was being ruled",
                &preview_after_ruler,
                &preview_ledger,
                delta,
            ));
        }
        let rollback_tree = tempfile::TempDir::new()?;
        copy_ruled_tree(&self.root, rollback_tree.path())?;
        if repair_tree_snapshot(rollback_tree.path())? != self.base_snapshot
            || repair_tree_snapshot(&self.root)? != self.base_snapshot
        {
            return Ok(rejected_with_delta(
                "rollback snapshot did not freeze the exact parent epoch",
                &preview_after_ruler,
                &preview_ledger,
                delta,
            ));
        }

        if let Err(error) = real_apply(&self.root, &changes) {
            let restored =
                restore_ruled_tree(&self.root, rollback_tree.path(), &self.base_snapshot)?;
            return Ok(PromotionDecision::RolledBack {
                reason: format!("transactional land failed: {error}"),
                restored_tree_hash: restored.hash,
            });
        }
        let landed_before_ruler = repair_tree_snapshot(&self.root)?;
        if landed_before_ruler != preview_before_ruler {
            let restored =
                restore_ruled_tree(&self.root, rollback_tree.path(), &self.base_snapshot)?;
            return Ok(PromotionDecision::RolledBack {
                reason: "landed bytes differed from the ruled composition".to_string(),
                restored_tree_hash: restored.hash,
            });
        }

        let real_ledger = match ruler(self.root.clone()).await {
            Ok(ledger) => ledger,
            Err(error) => {
                let restored =
                    restore_ruled_tree(&self.root, rollback_tree.path(), &self.base_snapshot)?;
                return Ok(PromotionDecision::RolledBack {
                    reason: format!("post-land full ruler failed: {error}"),
                    restored_tree_hash: restored.hash,
                });
            }
        };
        let landed_after_ruler = repair_tree_snapshot(&self.root)?;
        let post_validation = self.validate_candidate_ledger(&real_ledger, &landed_after_ruler);
        if landed_after_ruler != landed_before_ruler
            || post_validation.is_err()
            || landed_after_ruler != preview_after_ruler
            || real_ledger.hash != preview_ledger.hash
        {
            let restored =
                restore_ruled_tree(&self.root, rollback_tree.path(), &self.base_snapshot)?;
            return Ok(PromotionDecision::RolledBack {
                reason: "post-land same-ruler tree or ledger drifted from the accepted preview"
                    .to_string(),
                restored_tree_hash: restored.hash,
            });
        }

        Ok(PromotionDecision::Promoted {
            epoch_before: self.epoch(),
            epoch_after: RepairEpoch {
                tree_hash: landed_after_ruler.hash,
                ledger_hash: real_ledger.hash,
                transaction_nonce: self.epoch.transaction_nonce.clone(),
            },
            candidates: self
                .candidates
                .iter()
                .map(|candidate| candidate.id.clone())
                .collect(),
            changed_files: changes.keys().cloned().collect(),
            delta,
            semantic_acceptance: Box::new(semantic_acceptance),
        })
    }

    fn lock_authority_is_current(&self) -> Result<bool> {
        Ok(LockIdentity::from_path(&self.lock_path)? == self.lock_identity)
    }

    fn validate_candidate_ledger(
        &self,
        ledger: &DefectLedger,
        snapshot: &RepairTreeSnapshot,
    ) -> Result<()> {
        if ledger.tree_hash != snapshot.hash {
            bail!("ruler ledger describes a different tree");
        }
        if ledger.ruler != self.base_ledger.ruler {
            bail!("candidate used a different repair ruler");
        }
        if ledger.requirements != self.base_ledger.requirements {
            bail!("candidate changed authoritative requirement identity");
        }
        if !ledger.is_fully_established() {
            bail!("candidate did not establish every required ruler leg");
        }
        Ok(())
    }

    fn validate_semantic_review(
        &self,
        request: &SemanticReviewRequest,
        expected_snapshot: &SealedSemanticObservationSnapshot,
        handle_snapshot_hash: &str,
        handle_admission: &AdmissionReceipt,
        review: AdmittedSemanticObservationReceipt,
    ) -> Result<SemanticAcceptanceReceipt> {
        let expected_source = semantic_observation_task_version(expected_snapshot);
        let admission = review.admission().clone();
        let observation = review.observation();
        if handle_snapshot_hash != expected_snapshot.snapshot_hash()
            || handle_admission != &admission
            || admission.role != WorkRole::SemanticJudgeObservation
            || admission.source != expected_source
            || review.local_completion() != LocalCompletionKind::Success
            || observation.stale
            || observation.snapshot_hash != expected_snapshot.snapshot_hash()
        {
            bail!("semantic review was not a finished broker admission for this exact preview");
        }
        let reviewer_reply_hash = observation
            .reviewer_reply_hash
            .clone()
            .ok_or_else(|| anyhow!("semantic review has no provider reply identity"))?;
        let (rationale, citations, covered_requirements) = match &observation.decision {
            ParsedSemanticObservation::Parsed { reply } => match &reply.observation {
                SemanticObservationBody::AcceptCandidate {
                    summary,
                    evidence,
                    covered_requirements,
                } => (
                    summary.clone(),
                    evidence.clone(),
                    covered_requirements.clone(),
                ),
                _ => bail!("semantic review did not accept this exact preview"),
            },
            ParsedSemanticObservation::Abstained { .. } => {
                bail!("semantic review did not accept this exact preview")
            }
        };
        let cited_requirements = covered_requirements
            .into_iter()
            .map(RequirementId::new)
            .collect::<Result<BTreeSet<_>>>()?;
        let cited_evidence = citations
            .into_iter()
            .map(|citation| citation.source_id)
            .collect::<BTreeSet<_>>();
        if rationale.trim().is_empty()
            || !cited_requirements.is_subset(&request.requirements)
            || !cited_evidence.is_subset(&request.evidence_ids)
        {
            bail!(
                "semantic review did not accept this exact preview with valid authority citations"
            );
        }
        for target in &request.targets {
            let observation = self
                .base_ledger
                .observations
                .get(target)
                .ok_or_else(|| anyhow!("semantic review target left the base ledger"))?;
            if !observation.requirement_ids.is_subset(&cited_requirements)
                || observation
                    .evidence
                    .iter()
                    .all(|evidence| !cited_evidence.contains(&evidence.sha256))
            {
                bail!("semantic review omitted requirement or evidence for an assigned target");
            }
        }
        let bytes = serde_json::to_vec(&(
            SEMANTIC_REVIEW_PROTOCOL,
            &request.identity,
            &request.composition_id,
            &admission,
            ProviderTerminalKind::Finished,
            &reviewer_reply_hash,
            &cited_requirements,
            &cited_evidence,
            &rationale,
        ))?;
        Ok(SemanticAcceptanceReceipt {
            receipt_id: format!("repair-acceptance:{}", sha256_hex(&bytes)),
            review_identity: request.identity.clone(),
            composition_id: request.composition_id.clone(),
            admission,
            provider_terminal: ProviderTerminalKind::Finished,
            reviewer_reply_hash,
            cited_requirements,
            cited_evidence,
        })
    }

    fn assigned_targets(&self) -> BTreeSet<DefectId> {
        self.candidates
            .iter()
            .flat_map(|candidate| candidate.targets.iter().cloned())
            .collect()
    }

    fn composed_changes(&self) -> BTreeMap<String, FileMutation> {
        self.candidates
            .iter()
            .flat_map(|candidate| candidate.changes.clone())
            .collect()
    }
}

fn mutation_paths_overlap(left: &str, right: &str) -> bool {
    let left = Path::new(left);
    let right = Path::new(right);
    left == right || left.starts_with(right) || right.starts_with(left)
}

impl Drop for RepairTransaction {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock_file);
    }
}

fn semantic_review_identity(request: &SemanticReviewRequest) -> String {
    let bytes = serde_json::to_vec(&(
        SEMANTIC_REVIEW_PROTOCOL,
        &request.composition_id,
        &request.candidate_ids,
        &request.base_epoch,
        &request.preview_epoch,
        &request.targets,
        &request.changes,
        &request.delta,
        &request.requirements,
        &request.evidence_ids,
    ))
    .expect("semantic repair request contains only serializable engine values");
    format!("repair-review:{}", sha256_hex(&bytes))
}

fn semantic_review_snapshot(
    request: &SemanticReviewRequest,
) -> Result<SealedSemanticObservationSnapshot> {
    let canonical_request = serde_json::to_string(request)?;
    SemanticObservationSnapshotDraft {
        schema_version: SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
        authority_scope: AuthorityScope::new(
            format!("repair:{}", request.base_epoch.ledger_hash),
            format!("repair:{}", request.composition_id),
        ),
        phase_epoch: 0,
        task_id: request.identity.clone(),
        attempt: 0,
        source_revision: 1,
        contract_version: request.base_epoch.ledger_hash.clone(),
        artifact_version: request.preview_epoch.tree_hash.clone(),
        goal: "Review the exact composed repair preview for semantic acceptance".to_string(),
        task_contract: canonical_request,
        acceptance_oracle: request
            .requirements
            .iter()
            .map(|requirement| AcceptanceCriterionSnapshot {
                id: requirement.as_str().to_string(),
                text: format!("The repair preserves requirement `{requirement}`"),
            })
            .collect(),
        dependency_contract_versions: BTreeMap::new(),
        sibling_contract_versions: BTreeMap::new(),
        allowed_finding_routes: Vec::new(),
        artifacts: request
            .evidence_ids
            .iter()
            .enumerate()
            .map(|(index, evidence_id)| ArtifactExcerptSnapshot {
                source_id: evidence_id.clone(),
                path: format!(".swarm/repair/evidence/{index}"),
                excerpt: format!("Bound repair-ledger evidence `{evidence_id}`"),
                complete: true,
            })
            .collect(),
        trace: SemanticTraceSnapshot {
            sequence: 1,
            recent_reasoning: format!(
                "Review exact repair request {} without substituting another preview",
                request.identity
            ),
            recent_actions: request.candidate_ids.clone(),
            prior_intervention: None,
            response_to_prior_intervention: None,
        },
        neutral_signals: Vec::new(),
    }
    .seal()
}

fn composition_id(candidates: &[RepairCandidatePatch]) -> String {
    let mut candidates = candidates.to_vec();
    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    let bytes = serde_json::to_vec(&(REPAIR_COMPOSITION_PROTOCOL, candidates))
        .expect("repair candidates contain only serializable engine values");
    format!("composition:{}", sha256_hex(&bytes))
}

fn rejected(
    reason: &str,
    snapshot: Option<&RepairTreeSnapshot>,
    ledger: Option<&DefectLedger>,
) -> PromotionDecision {
    PromotionDecision::Rejected {
        reason: reason.to_string(),
        preview_tree_hash: snapshot.map(|snapshot| snapshot.hash.clone()),
        preview_ledger_hash: ledger.map(|ledger| ledger.hash.clone()),
        delta: None,
    }
}

fn rejected_with_delta(
    reason: &str,
    snapshot: &RepairTreeSnapshot,
    ledger: &DefectLedger,
    delta: CandidateDelta,
) -> PromotionDecision {
    PromotionDecision::Rejected {
        reason: reason.to_string(),
        preview_tree_hash: Some(snapshot.hash.clone()),
        preview_ledger_hash: Some(ledger.hash.clone()),
        delta: Some(delta),
    }
}

fn apply_mutations(root: &Path, changes: &BTreeMap<String, FileMutation>) -> Result<()> {
    for (relative, mutation) in changes {
        match mutation {
            FileMutation::Write { bytes, mode } => {
                let path = guarded_mutation_path(root, relative, true)?
                    .ok_or_else(|| anyhow!("repair write path could not be created"))?;
                atomic_write(&path, bytes, *mode)?;
            }
            FileMutation::Delete => {
                let Some(path) = guarded_mutation_path(root, relative, false)? else {
                    continue;
                };
                match std::fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                        std::fs::remove_file(path)?;
                    }
                    Ok(_) => bail!("repair transaction refuses to delete non-regular `{relative}`"),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(())
}

fn guarded_mutation_path(
    root: &Path,
    relative: &str,
    create_parent: bool,
) -> Result<Option<PathBuf>> {
    if !safe_relative_path(relative) || excluded_from_repair_tree(Path::new(relative)) {
        bail!("unsafe or unruly repair path `{relative}`");
    }
    let root = root.canonicalize()?;
    let path = root.join(relative);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("repair path has no parent"))?;
    if !parent.exists() {
        if !create_parent {
            return Ok(None);
        }
        create_contained_directory(&root, parent)?;
    }
    let mut current = root.clone();
    for component in Path::new(relative)
        .parent()
        .into_iter()
        .flat_map(Path::components)
    {
        let Component::Normal(name) = component else {
            bail!("repair path contains a non-normal component");
        };
        current.push(name);
        let metadata = std::fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("repair path traverses a symlink or non-directory");
        }
    }
    if path.exists() {
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("repair target is not a regular file");
        }
    }
    Ok(Some(path))
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("repair write has no parent"))?;
    let sequence = WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".repair-land.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let result = (|| -> Result<()> {
        write_new_file(&temp, bytes)?;
        set_mode(&temp, mode)?;
        std::fs::rename(&temp, path)?;
        sync_directory(parent)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn sync_directory(directory: &Path) -> Result<()> {
    File::open(directory)?.sync_all()?;
    Ok(())
}

fn restore_ruled_tree(
    root: &Path,
    rollback_tree: &Path,
    base: &RepairTreeSnapshot,
) -> Result<RepairTreeSnapshot> {
    if repair_tree_snapshot(rollback_tree)? != *base {
        bail!("repair rollback source no longer matches the parent epoch");
    }
    let staged = tempfile::TempDir::new()?;
    copy_ruled_tree(rollback_tree, staged.path())?;
    if repair_tree_snapshot(staged.path())? != *base {
        bail!("staged repair rollback does not match the parent epoch");
    }
    clear_ruled_tree(root, root)?;
    if let Err(copy_error) = copy_ruled_tree(staged.path(), root) {
        let recovery = clear_ruled_tree(root, root)
            .and_then(|_| copy_ruled_tree(rollback_tree, root))
            .and_then(|_| {
                (repair_tree_snapshot(root)? == *base)
                    .then_some(())
                    .ok_or_else(|| anyhow!("rollback recovery did not restore the parent epoch"))
            });
        return match recovery {
            Ok(()) => Err(copy_error),
            Err(recovery_error) => Err(anyhow!(
                "staged rollback failed ({copy_error}); recovery also failed ({recovery_error})"
            )),
        };
    }
    let restored = repair_tree_snapshot(root)?;
    if restored != *base {
        bail!("repair rollback did not restore the exact parent epoch");
    }
    Ok(restored)
}

fn clear_ruled_tree(root: &Path, directory: &Path) -> Result<()> {
    let mut entries = std::fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        if excluded_from_repair_tree(relative) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            clear_ruled_tree(root, &path)?;
            match std::fs::remove_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => return Err(error.into()),
            }
        } else {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn copy_ruled_tree(source: &Path, destination: &Path) -> Result<()> {
    fn copy_dir(source_root: &Path, source: &Path, destination: &Path) -> Result<()> {
        std::fs::create_dir_all(destination)?;
        let source_metadata = std::fs::symlink_metadata(source)?;
        let mut entries = std::fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let relative = path.strip_prefix(source_root)?;
            if excluded_from_repair_tree(relative) {
                continue;
            }
            let target = destination.join(entry.file_name());
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                copy_symlink(&path, &target)?;
            } else if metadata.is_dir() {
                copy_dir(source_root, &path, &target)?;
            } else if metadata.is_file() {
                std::fs::copy(&path, &target)?;
                set_mode(&target, repair_entry_mode(&metadata))?;
            } else {
                bail!(
                    "repair transaction refuses special entry `{}`",
                    relative.display()
                );
            }
        }
        set_mode(destination, repair_entry_mode(&source_metadata))?;
        Ok(())
    }
    copy_dir(source, source, destination)
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(source)?, destination)?;
    Ok(())
}

#[cfg(windows)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    let target = std::fs::read_link(source)?;
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(target, destination)?;
    } else {
        std::os::windows::fs::symlink_file(target, destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::{HostCapacityEvidence, PhysicalFleetSnapshot, VerifiedPhysicalLane};
    use crate::control_plane::PhysicalAdmissionControl;
    use crate::event::{EventSink, NullSink};
    use crate::semantic_control::{
        AdmittedSemanticObservationRequest, AdmittedSemanticObservationReviewer,
        AdmittedSemanticReviewError, BrokeredSemanticObservationPlane,
        SemanticObservationAdmissionPolicy, SemanticObservationAdmissionSubmission,
    };
    use crate::semantic_observation::SEMANTIC_OBSERVATION_PROTOCOL;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::{Arc, Mutex};

    fn requirement_set() -> BTreeSet<RequirementId> {
        BTreeSet::from([RequirementId::new("requirement:app-runs").unwrap()])
    }

    fn ruler_authority() -> &'static EngineRulerAuthority {
        static AUTHORITY: OnceLock<EngineRulerAuthority> = OnceLock::new();
        AUTHORITY.get_or_init(EngineRulerAuthority::mint)
    }

    fn ruler(id: &str, legs: &[&str]) -> RulerIdentity {
        RulerIdentity::new(
            ruler_authority(),
            id,
            legs.iter()
                .map(|leg| RulerLegId::new(*leg).unwrap())
                .collect(),
        )
        .unwrap()
    }

    fn write_file(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn observation(
        root: &Path,
        snapshot: &RepairTreeSnapshot,
        ruler: &RulerIdentity,
        leg: &str,
        requirements: &BTreeSet<RequirementId>,
        gate: GateId,
        rendered: &str,
    ) -> DefectObservation {
        observation_with_key(
            root,
            snapshot,
            ruler,
            leg,
            requirements,
            gate,
            rendered,
            rendered,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn observation_with_key(
        root: &Path,
        snapshot: &RepairTreeSnapshot,
        ruler: &RulerIdentity,
        leg: &str,
        requirements: &BTreeSet<RequirementId>,
        gate: GateId,
        causal_key: &str,
        rendered: &str,
    ) -> DefectObservation {
        observe_finding(
            ruler_authority(),
            root,
            snapshot.hash(),
            ruler,
            RulerLegId::new(leg).unwrap(),
            FindingInput {
                gate,
                causal_key,
                rendered,
                known_files: &["app.txt".to_string()],
                explicit_subjects: BTreeSet::new(),
                requirement_ids: requirements.clone(),
            },
        )
        .unwrap()
    }

    fn ledger_with_findings(
        root: &Path,
        ruler: RulerIdentity,
        requirements: BTreeSet<RequirementId>,
        established_legs: BTreeSet<RulerLegId>,
        findings: &[(RulerLegId, GateId, &str)],
    ) -> DefectLedger {
        let snapshot = repair_tree_snapshot(root).unwrap();
        let observations = findings
            .iter()
            .map(|(leg, gate, rendered)| {
                observe_finding(
                    ruler_authority(),
                    root,
                    snapshot.hash(),
                    &ruler,
                    leg.clone(),
                    FindingInput {
                        gate: *gate,
                        causal_key: rendered,
                        rendered,
                        known_files: &["app.txt".to_string()],
                        explicit_subjects: BTreeSet::new(),
                        requirement_ids: requirements.clone(),
                    },
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        DefectLedger::new(
            ruler_authority(),
            snapshot.hash().to_string(),
            ruler,
            established_legs,
            requirements,
            observations,
        )
        .unwrap()
    }

    fn clean_ledger(
        root: &Path,
        ruler: RulerIdentity,
        requirements: BTreeSet<RequirementId>,
    ) -> DefectLedger {
        let snapshot = repair_tree_snapshot(root).unwrap();
        DefectLedger::new(
            ruler_authority(),
            snapshot.hash().to_string(),
            ruler.clone(),
            ruler.required_legs().clone(),
            requirements,
            Vec::new(),
        )
        .unwrap()
    }

    fn write_change(root: &Path, bytes: &[u8]) -> BTreeMap<String, FileMutation> {
        let mode = repair_entry_mode(&std::fs::metadata(root.join("app.txt")).unwrap());
        BTreeMap::from([(
            "app.txt".to_string(),
            FileMutation::Write {
                bytes: bytes.to_vec(),
                mode,
            },
        )])
    }

    #[derive(Clone, Copy)]
    enum SemanticTestVerdict {
        Accept,
        Abstain,
        TerminalFailure,
    }

    struct RepairSemanticReviewer {
        verdict: SemanticTestVerdict,
    }

    #[async_trait]
    impl AdmittedSemanticObservationReviewer for RepairSemanticReviewer {
        async fn review(
            &self,
            request: AdmittedSemanticObservationRequest,
        ) -> std::result::Result<String, AdmittedSemanticReviewError> {
            if matches!(self.verdict, SemanticTestVerdict::TerminalFailure) {
                return Err(AdmittedSemanticReviewError::terminal_failure(
                    "semantic provider rejected the repair preview",
                ));
            }
            let snapshot = &request.observation.snapshot;
            let observation = match self.verdict {
                SemanticTestVerdict::Accept => serde_json::json!({
                    "action": "ACCEPT_CANDIDATE",
                    "summary": "the exact candidate closes the cited requirement without semantic drift",
                    "evidence": snapshot
                        .payload()
                        .artifacts
                        .iter()
                        .map(|artifact| serde_json::json!({
                            "source_id": artifact.source_id,
                            "observation": "the bound repair evidence supports this acceptance"
                        }))
                        .collect::<Vec<_>>(),
                    "covered_requirements": snapshot
                        .payload()
                        .acceptance_oracle
                        .iter()
                        .map(|criterion| criterion.id.clone())
                        .collect::<Vec<_>>()
                }),
                SemanticTestVerdict::Abstain => serde_json::json!({
                    "action": "ABSTAIN",
                    "reason": "the provider cannot accept this preview"
                }),
                SemanticTestVerdict::TerminalFailure => unreachable!(),
            };
            Ok(serde_json::json!({
                "protocol": SEMANTIC_OBSERVATION_PROTOCOL,
                "snapshot_hash": snapshot.snapshot_hash(),
                "observation": observation
            })
            .to_string())
        }
    }

    fn semantic_control(scope: &str, sink: Arc<dyn EventSink>) -> PhysicalAdmissionControl {
        let snapshot = PhysicalFleetSnapshot::new(
            format!("snapshot:{scope}"),
            vec![VerifiedPhysicalLane {
                logical_device_id: "repair-review-lane".to_string(),
                model_id: "repair-review-model".to_string(),
                host_id: "repair-review-host".to_string(),
                model_instance_id: "repair-review-instance".to_string(),
                provider_transport_id:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                advertised_instance_capacity: 1,
                routing_weight: 1,
                capacity_evidence: HostCapacityEvidence::MeasuredProfile {
                    profile_hash: format!("profile:{scope}"),
                    profile_key: "repair-review:model:context".to_string(),
                    max_concurrent: 1,
                },
                route_evidence_id: format!("route:{scope}"),
            }],
        )
        .unwrap();
        PhysicalAdmissionControl::new(scope, snapshot, sink).unwrap()
    }

    async fn brokered_review(
        request: &SemanticReviewRequest,
        verdict: SemanticTestVerdict,
    ) -> Result<AdmittedSemanticObservationHandle> {
        let sink: Arc<dyn EventSink> = Arc::new(NullSink);
        let scope = format!("repair-review-test:{}", request.identity());
        let control = semantic_control(&scope, sink.clone());
        let plane = BrokeredSemanticObservationPlane::new(control, sink)?;
        let submission = plane
            .submit(
                request.observation_snapshot()?,
                SemanticObservationAdmissionPolicy::default(),
                Arc::new(RepairSemanticReviewer { verdict }),
            )
            .await?;
        match submission {
            SemanticObservationAdmissionSubmission::Started(handle) => Ok(handle),
            SemanticObservationAdmissionSubmission::Rejected(rejection) => {
                bail!("semantic review was rejected before provider admission: {rejection:?}")
            }
        }
    }

    async fn accept(request: &SemanticReviewRequest) -> Result<AdmittedSemanticObservationHandle> {
        brokered_review(request, SemanticTestVerdict::Accept).await
    }

    #[test]
    fn repair_snapshot_excludes_only_root_engine_evidence() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "src/lib.rs", b"pub fn value() -> u8 { 1 }\n");
        let before = repair_tree_snapshot(root.path()).unwrap();

        for (path, bytes) in [
            (".swarm/run.jsonl", b"swarm evidence\n".as_slice()),
            (
                ".swarm-monitor/watch.jsonl",
                b"monitor evidence\n".as_slice(),
            ),
            ("run.jsonl", b"run evidence\n".as_slice()),
            ("engine-console.log", b"console evidence\n".as_slice()),
            ("bench-shots/shot.png", b"screenshot evidence\n".as_slice()),
            ("heartbeat", b"heartbeat evidence\n".as_slice()),
            ("graded.db", b"grade evidence\n".as_slice()),
        ] {
            write_file(root.path(), path, bytes);
        }

        assert_eq!(repair_tree_snapshot(root.path()).unwrap(), before);
    }

    #[test]
    fn nested_engine_evidence_names_remain_repair_tree_bytes() {
        let root = tempfile::TempDir::new().unwrap();
        let nested_paths = [
            "app/.swarm/data.json",
            "app/.swarm-monitor/watch.jsonl",
            "app/run.jsonl",
            "app/engine-console.log",
            "app/bench-shots/shot.png",
            "src/heartbeat/state.json",
            "app/graded.db",
        ];
        for path in nested_paths {
            write_file(root.path(), path, b"v1\n");
        }
        let before = repair_tree_snapshot(root.path()).unwrap();

        for path in nested_paths {
            write_file(root.path(), path, b"v2\n");
        }
        let after = repair_tree_snapshot(root.path()).unwrap();

        assert_ne!(before.hash(), after.hash());
        for path in nested_paths {
            assert_ne!(
                before.entries().get(path),
                after.entries().get(path),
                "{path}"
            );
        }
    }

    #[test]
    fn provisional_receipts_bind_exact_bytes_and_refuse_unsound_salvage() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "src/lib.rs", b"pub fn value() -> u8 { 1 }\n");
        let owned = vec!["src/lib.rs".to_string()];
        let first = mint_provisional_task_receipt(
            root.path(),
            "module",
            2,
            "contract:v1",
            SalvageReason::ProgressWatchdog,
            &owned,
        )
        .unwrap();
        write_file(root.path(), "src/lib.rs", b"pub fn value() -> u8 { 2 }\n");
        let changed = mint_provisional_task_receipt(
            root.path(),
            "module",
            2,
            "contract:v1",
            SalvageReason::ProgressWatchdog,
            &owned,
        )
        .unwrap();
        assert_ne!(first.receipt_id(), changed.receipt_id());
        assert_ne!(
            first.artifacts()["src/lib.rs"].sha256(),
            changed.artifacts()["src/lib.rs"].sha256()
        );
        assert_eq!(
            first.required_verification(),
            RequiredVerification::FullRepairRuler
        );

        write_file(root.path(), "tests/test_lib.py", b"def test_lib(): pass\n");
        assert!(mint_provisional_task_receipt(
            root.path(),
            "test-lib",
            1,
            "contract:v1",
            SalvageReason::StallExhausted,
            &["tests/test_lib.py".to_string()],
        )
        .is_none());
        write_file(root.path(), "Cargo.toml", b"[package]\nname='fixture'\n");
        assert!(mint_provisional_task_receipt(
            root.path(),
            "manifest",
            1,
            "contract:v1",
            SalvageReason::FinalizeSpin,
            &["Cargo.toml".to_string()],
        )
        .is_none());
        assert!(mint_provisional_task_receipt(
            root.path(),
            "module",
            1,
            "contract:v1",
            SalvageReason::StallExhausted,
            &[],
        )
        .is_none());
        assert!(mint_provisional_task_receipt(
            root.path(),
            "module",
            1,
            "contract:v1",
            SalvageReason::StallExhausted,
            &["src/lib.rs".to_string(), "src/missing.rs".to_string()],
        )
        .is_none());
    }

    #[test]
    fn causal_identity_ignores_volatile_counts_order_and_ports_but_keeps_full_evidence() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let snapshot = repair_tree_snapshot(root.path()).unwrap();
        let ruler = ruler("ruler:v1", &["smoke"]);
        let requirements = requirement_set();
        let first_rendered = "localhost:4123 line 17: 2 failed tests in 440ms\nsecondary detail";
        let first = observation_with_key(
            root.path(),
            &snapshot,
            &ruler,
            "smoke",
            &requirements,
            GateId::Smoke,
            "smoke:test-failure",
            first_rendered,
        );
        let reordered = observation_with_key(
            root.path(),
            &snapshot,
            &ruler,
            "smoke",
            &requirements,
            GateId::Smoke,
            "smoke:test-failure",
            "secondary detail\nlocalhost:9999 line 801: 19 failed tests in 9 seconds",
        );
        assert_eq!(first.id(), reordered.id());
        assert_ne!(
            first.evidence()[0].sha256(),
            reordered.evidence()[0].sha256()
        );
        assert_eq!(first.evidence()[0].bytes(), first_rendered.len());
        assert!(root
            .path()
            .join(first.evidence()[0].relative_path())
            .is_file());

        let base = DefectLedger::new(
            ruler_authority(),
            snapshot.hash().to_string(),
            ruler.clone(),
            ruler.required_legs().clone(),
            requirements.clone(),
            [first],
        )
        .unwrap();
        write_file(root.path(), "app.txt", b"different tree bytes\n");
        let next_snapshot = repair_tree_snapshot(root.path()).unwrap();
        let same_evidence = observation_with_key(
            root.path(),
            &next_snapshot,
            &ruler,
            "smoke",
            &requirements,
            GateId::Smoke,
            "smoke:test-failure",
            first_rendered,
        );
        let candidate = DefectLedger::new(
            ruler_authority(),
            next_snapshot.hash().to_string(),
            ruler.clone(),
            ruler.required_legs().clone(),
            requirements,
            [same_evidence],
        )
        .unwrap();
        let delta = CandidateDelta::between(&base, &candidate);
        assert_eq!(delta.persisted().len(), 1);
        assert!(delta.changed_evidence().is_empty());
    }

    #[test]
    fn causal_identity_survives_wording_churn_and_kind_reclassification() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let snapshot = repair_tree_snapshot(root.path()).unwrap();
        let ruler = ruler("ruler:v1", &["smoke"]);
        let requirements = requirement_set();
        let first = observation_with_key(
            root.path(),
            &snapshot,
            &ruler,
            "smoke",
            &requirements,
            GateId::Smoke,
            "smoke:app-contract",
            "app.txt: one named test failed",
        );
        let reworded = observation_with_key(
            root.path(),
            &snapshot,
            &ruler,
            "smoke",
            &requirements,
            GateId::Smoke,
            "smoke:app-contract",
            "app.txt: generic invariant remains unsatisfied",
        );

        assert_ne!(first.kind(), reworded.kind());
        assert_eq!(first.id(), reworded.id());
    }

    #[test]
    fn foreign_engine_authority_cannot_mint_a_matching_ruler_ledger() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let snapshot = repair_tree_snapshot(root.path()).unwrap();
        let ruler = ruler("ruler:v1", &["smoke"]);
        let foreign = EngineRulerAuthority::mint();

        assert!(DefectLedger::new(
            &foreign,
            snapshot.hash().to_string(),
            ruler.clone(),
            ruler.required_legs().clone(),
            requirement_set(),
            Vec::new(),
        )
        .is_err());
    }

    #[test]
    fn stale_and_foreign_ledgers_fail_closed() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let ruler = ruler("ruler:v1", &["smoke"]);
        let requirements = requirement_set();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements.clone(),
            ruler.required_legs().clone(),
            &[(
                RulerLegId::new("smoke").unwrap(),
                GateId::Smoke,
                "advertised entry cannot boot",
            )],
        );

        let foreign_root = tempfile::TempDir::new().unwrap();
        write_file(foreign_root.path(), "app.txt", b"different tree\n");
        assert!(RepairTransaction::open(foreign_root.path(), base.clone()).is_err());

        let mut transaction = RepairTransaction::open(root.path(), base.clone()).unwrap();
        let stale_epoch = RepairEpoch {
            tree_hash: transaction.epoch().tree_hash,
            ledger_hash: "stale-ledger".to_string(),
            transaction_nonce: transaction.epoch().transaction_nonce,
        };
        let target = base.observations().keys().next().unwrap().clone();
        let stale = RepairCandidatePatch::new(
            "stale",
            stale_epoch,
            BTreeSet::from([target]),
            write_change(root.path(), b"fixed\n"),
        )
        .unwrap();
        assert!(transaction.add_candidate(stale).is_err());
    }

    #[test]
    fn identical_tree_and_ledger_bytes_cannot_replay_an_old_transaction_epoch() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let ruler = ruler("ruler:v1", &["smoke"]);
        let requirements = requirement_set();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements,
            ruler.required_legs().clone(),
            &[(
                RulerLegId::new("smoke").unwrap(),
                GateId::Smoke,
                "advertised entry cannot boot",
            )],
        );
        let target = base.observations().keys().next().unwrap().clone();
        let first = RepairTransaction::open(root.path(), base.clone()).unwrap();
        let stale_epoch = first.epoch();
        drop(first);
        let mut second = RepairTransaction::open(root.path(), base).unwrap();

        assert_ne!(stale_epoch, second.epoch());
        let replay = RepairCandidatePatch::new(
            "replayed",
            stale_epoch,
            BTreeSet::from([target]),
            write_change(root.path(), b"fixed\n"),
        )
        .unwrap();
        assert!(second.add_candidate(replay).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn replacing_the_lock_path_revokes_the_held_promotion_authority() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let ruler = ruler("ruler:v1", &["smoke"]);
        let requirements = requirement_set();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements,
            ruler.required_legs().clone(),
            &[(
                RulerLegId::new("smoke").unwrap(),
                GateId::Smoke,
                "advertised entry cannot boot",
            )],
        );
        let transaction = RepairTransaction::open(root.path(), base).unwrap();
        std::fs::remove_file(&transaction.lock_path).unwrap();
        File::create(&transaction.lock_path).unwrap();

        assert!(!transaction.lock_authority_is_current().unwrap());
    }

    #[tokio::test]
    async fn fewer_findings_cannot_hide_a_blocking_severity_swap() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let ruler = ruler("ruler:v1", &["mechanical"]);
        let requirements = requirement_set();
        let leg = RulerLegId::new("mechanical").unwrap();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements.clone(),
            ruler.required_legs().clone(),
            &[
                (
                    leg.clone(),
                    GateId::CssCoherence,
                    "first style contract drift",
                ),
                (
                    leg.clone(),
                    GateId::CssCoherence,
                    "second style contract drift",
                ),
            ],
        );
        assert_eq!(base.observations().len(), 2);
        let targets = base.observations().keys().cloned().collect();
        let mut transaction = RepairTransaction::open(root.path(), base.clone()).unwrap();
        transaction
            .add_candidate(
                RepairCandidatePatch::new(
                    "looks-better-by-count",
                    transaction.epoch(),
                    targets,
                    write_change(root.path(), b"apparently fixed\n"),
                )
                .unwrap(),
            )
            .unwrap();

        let semantic_called = Arc::new(AtomicBool::new(false));
        let semantic_called_in_review = semantic_called.clone();
        let ruler_for_run = ruler.clone();
        let requirements_for_run = requirements.clone();
        let decision = transaction
            .preview_and_promote(
                move |path| {
                    let ruler = ruler_for_run.clone();
                    let requirements = requirements_for_run.clone();
                    async move {
                        let snapshot = repair_tree_snapshot(&path)?;
                        let introduced = observation(
                            &path,
                            &snapshot,
                            &ruler,
                            "mechanical",
                            &requirements,
                            GateId::Smoke,
                            "advertised entry cannot boot",
                        );
                        DefectLedger::new(
                            ruler_authority(),
                            snapshot.hash().to_string(),
                            ruler.clone(),
                            ruler.required_legs().clone(),
                            requirements,
                            [introduced],
                        )
                    }
                },
                move |request| {
                    semantic_called_in_review.store(true, AtomicOrdering::SeqCst);
                    async move { accept(&request).await }
                },
            )
            .await
            .unwrap();
        assert!(matches!(decision, PromotionDecision::Rejected { .. }));
        assert!(decision.reason().unwrap().contains("blocking defect"));
        assert!(!semantic_called.load(AtomicOrdering::SeqCst));

        let snapshot = repair_tree_snapshot(root.path()).unwrap();
        let mut forged = observation(
            root.path(),
            &snapshot,
            &ruler,
            "mechanical",
            &requirements,
            GateId::Smoke,
            "advertised entry cannot boot",
        );
        forged.impact.severity = MechanicalSeverity::Advisory;
        assert!(DefectLedger::new(
            ruler_authority(),
            snapshot.hash().to_string(),
            ruler.clone(),
            ruler.required_legs().clone(),
            requirements,
            [forged],
        )
        .is_err());
    }

    #[test]
    fn overlapping_candidates_and_duplicate_targets_are_rejected_before_preview() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let ruler = ruler("ruler:v1", &["mechanical"]);
        let requirements = requirement_set();
        let leg = RulerLegId::new("mechanical").unwrap();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements,
            ruler.required_legs().clone(),
            &[
                (
                    leg.clone(),
                    GateId::CssCoherence,
                    "first style contract drift",
                ),
                (leg, GateId::CssCoherence, "second style contract drift"),
            ],
        );
        let targets = base.observations().keys().cloned().collect::<Vec<_>>();
        let mut transaction = RepairTransaction::open(root.path(), base).unwrap();
        transaction
            .add_candidate(
                RepairCandidatePatch::new(
                    "first",
                    transaction.epoch(),
                    BTreeSet::from([targets[0].clone()]),
                    write_change(root.path(), b"first\n"),
                )
                .unwrap(),
            )
            .unwrap();
        let same_path = RepairCandidatePatch::new(
            "same-path",
            transaction.epoch(),
            BTreeSet::from([targets[1].clone()]),
            write_change(root.path(), b"second\n"),
        )
        .unwrap();
        assert!(transaction.add_candidate(same_path).is_err());

        let same_target = RepairCandidatePatch::new(
            "same-target",
            transaction.epoch(),
            BTreeSet::from([targets[0].clone()]),
            BTreeMap::from([(
                "other.txt".to_string(),
                FileMutation::Write {
                    bytes: b"other\n".to_vec(),
                    mode: 0o100644,
                },
            )]),
        )
        .unwrap();
        assert!(transaction.add_candidate(same_target).is_err());

        let nested_path = RepairCandidatePatch::new(
            "nested-path",
            transaction.epoch(),
            BTreeSet::from([targets[1].clone()]),
            BTreeMap::from([(
                "app.txt/child".to_string(),
                FileMutation::Write {
                    bytes: b"child\n".to_vec(),
                    mode: 0o100644,
                },
            )]),
        )
        .unwrap();
        assert!(transaction.add_candidate(nested_path).is_err());
    }

    #[tokio::test]
    async fn semantic_receipt_replay_is_rejected_when_candidate_bytes_change() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let ruler = ruler("ruler:v1", &["smoke"]);
        let requirements = requirement_set();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements.clone(),
            ruler.required_legs().clone(),
            &[(
                RulerLegId::new("smoke").unwrap(),
                GateId::Smoke,
                "advertised entry cannot boot",
            )],
        );
        let target = base.observations().keys().next().unwrap().clone();
        let captured = Arc::new(Mutex::new(None::<SemanticReviewRequest>));

        let mut first = RepairTransaction::open(root.path(), base.clone()).unwrap();
        first
            .add_candidate(
                RepairCandidatePatch::new(
                    "first-bytes",
                    first.epoch(),
                    BTreeSet::from([target.clone()]),
                    write_change(root.path(), b"fixed-one\n"),
                )
                .unwrap(),
            )
            .unwrap();
        let ruler_first = ruler.clone();
        let requirements_first = requirements.clone();
        let captured_first = captured.clone();
        let first_decision = first
            .preview_and_promote(
                move |path| {
                    let ruler = ruler_first.clone();
                    let requirements = requirements_first.clone();
                    async move { Ok(clean_ledger(&path, ruler, requirements)) }
                },
                move |request| {
                    *captured_first.lock().unwrap() = Some(request.clone());
                    async move { brokered_review(&request, SemanticTestVerdict::Abstain).await }
                },
            )
            .await
            .unwrap();
        assert!(matches!(first_decision, PromotionDecision::Rejected { .. }));
        drop(first);

        let stale_request = captured.lock().unwrap().clone().unwrap();
        let mut second = RepairTransaction::open(root.path(), base).unwrap();
        second
            .add_candidate(
                RepairCandidatePatch::new(
                    "changed-bytes",
                    second.epoch(),
                    BTreeSet::from([target]),
                    write_change(root.path(), b"fixed-two\n"),
                )
                .unwrap(),
            )
            .unwrap();
        let ruler_second = ruler.clone();
        let requirements_second = requirements.clone();
        let second_decision = second
            .preview_and_promote(
                move |path| {
                    let ruler = ruler_second.clone();
                    let requirements = requirements_second.clone();
                    async move { Ok(clean_ledger(&path, ruler, requirements)) }
                },
                move |_request| async move {
                    brokered_review(&stale_request, SemanticTestVerdict::Accept).await
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            second_decision,
            PromotionDecision::Rejected { .. }
        ));
        assert!(second_decision.reason().unwrap().contains("exact preview"));
        assert_eq!(
            std::fs::read(root.path().join("app.txt")).unwrap(),
            b"broken\n"
        );
    }

    #[tokio::test]
    async fn provider_terminal_failure_cannot_authorize_promotion() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let ruler = ruler("ruler:v1", &["smoke"]);
        let requirements = requirement_set();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements.clone(),
            ruler.required_legs().clone(),
            &[(
                RulerLegId::new("smoke").unwrap(),
                GateId::Smoke,
                "advertised entry cannot boot",
            )],
        );
        let target = base.observations().keys().next().unwrap().clone();
        let mut transaction = RepairTransaction::open(root.path(), base).unwrap();
        transaction
            .add_candidate(
                RepairCandidatePatch::new(
                    "provider-failed-review",
                    transaction.epoch(),
                    BTreeSet::from([target]),
                    write_change(root.path(), b"fixed\n"),
                )
                .unwrap(),
            )
            .unwrap();

        let decision = transaction
            .preview_and_promote(
                move |path| {
                    let ruler = ruler.clone();
                    let requirements = requirements.clone();
                    async move { Ok(clean_ledger(&path, ruler, requirements)) }
                },
                |request| async move {
                    brokered_review(&request, SemanticTestVerdict::TerminalFailure).await
                },
            )
            .await
            .unwrap();

        assert!(matches!(decision, PromotionDecision::Rejected { .. }));
        assert!(decision
            .reason()
            .unwrap()
            .contains("finished broker admission"));
        assert_eq!(
            std::fs::read(root.path().join("app.txt")).unwrap(),
            b"broken\n"
        );
    }

    #[tokio::test]
    async fn partial_or_foreign_ruler_cannot_authorize_promotion() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let ruler = ruler("ruler:v1", &["smoke", "contract"]);
        let requirements = requirement_set();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements.clone(),
            ruler.required_legs().clone(),
            &[(
                RulerLegId::new("smoke").unwrap(),
                GateId::Smoke,
                "advertised entry cannot boot",
            )],
        );
        let target = base.observations().keys().next().unwrap().clone();
        let mut partial = RepairTransaction::open(root.path(), base.clone()).unwrap();
        partial
            .add_candidate(
                RepairCandidatePatch::new(
                    "partial-ruler",
                    partial.epoch(),
                    BTreeSet::from([target.clone()]),
                    write_change(root.path(), b"fixed\n"),
                )
                .unwrap(),
            )
            .unwrap();
        let ruler_partial = ruler.clone();
        let requirements_partial = requirements.clone();
        let decision = partial
            .preview_and_promote(
                move |path| {
                    let ruler = ruler_partial.clone();
                    let requirements = requirements_partial.clone();
                    async move {
                        let snapshot = repair_tree_snapshot(&path)?;
                        DefectLedger::new(
                            ruler_authority(),
                            snapshot.hash().to_string(),
                            ruler,
                            BTreeSet::from([RulerLegId::new("smoke").unwrap()]),
                            requirements,
                            Vec::new(),
                        )
                    }
                },
                |request| async move { accept(&request).await },
            )
            .await
            .unwrap();
        assert!(matches!(decision, PromotionDecision::Rejected { .. }));
        assert!(decision
            .reason()
            .unwrap()
            .contains("every required ruler leg"));
        drop(partial);

        let mut foreign = RepairTransaction::open(root.path(), base).unwrap();
        foreign
            .add_candidate(
                RepairCandidatePatch::new(
                    "foreign-ruler",
                    foreign.epoch(),
                    BTreeSet::from([target]),
                    write_change(root.path(), b"fixed\n"),
                )
                .unwrap(),
            )
            .unwrap();
        let foreign_ruler = RulerIdentity::new(
            ruler_authority(),
            "ruler:foreign",
            ["smoke", "contract"]
                .into_iter()
                .map(|leg| RulerLegId::new(leg).unwrap())
                .collect(),
        )
        .unwrap();
        let requirements_foreign = requirements.clone();
        let decision = foreign
            .preview_and_promote(
                move |path| {
                    let ruler = foreign_ruler.clone();
                    let requirements = requirements_foreign.clone();
                    async move { Ok(clean_ledger(&path, ruler, requirements)) }
                },
                |request| async move { accept(&request).await },
            )
            .await
            .unwrap();
        assert!(matches!(decision, PromotionDecision::Rejected { .. }));
        assert!(decision
            .reason()
            .unwrap()
            .contains("different repair ruler"));
    }

    #[tokio::test]
    async fn rollback_removes_out_of_set_writes_and_restores_exact_parent() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        write_file(root.path(), "src/normal.rs", b"pub fn base() {}\n");
        write_file(
            root.path(),
            "src/heartbeat/engine-state",
            b"must survive rollback\n",
        );
        let ruler = ruler("ruler:v1", &["smoke"]);
        let requirements = requirement_set();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements.clone(),
            ruler.required_legs().clone(),
            &[(
                RulerLegId::new("smoke").unwrap(),
                GateId::Smoke,
                "advertised entry cannot boot",
            )],
        );
        let base_snapshot = repair_tree_snapshot(root.path()).unwrap();
        let target = base.observations().keys().next().unwrap().clone();
        let mut transaction = RepairTransaction::open(root.path(), base).unwrap();
        transaction
            .add_candidate(
                RepairCandidatePatch::new(
                    "faulted-land",
                    transaction.epoch(),
                    BTreeSet::from([target]),
                    write_change(root.path(), b"fixed\n"),
                )
                .unwrap(),
            )
            .unwrap();
        let ruler_for_run = ruler.clone();
        let requirements_for_run = requirements.clone();
        let decision = transaction
            .preview_and_promote_with_apply(
                move |path| {
                    let ruler = ruler_for_run.clone();
                    let requirements = requirements_for_run.clone();
                    async move { Ok(clean_ledger(&path, ruler, requirements)) }
                },
                |request| async move { accept(&request).await },
                |root, changes| {
                    apply_mutations(root, changes)?;
                    std::fs::write(root.join("outside-candidate.txt"), b"unruled write\n")?;
                    bail!("injected failure after an out-of-set write")
                },
            )
            .await
            .unwrap();
        assert!(matches!(decision, PromotionDecision::RolledBack { .. }));
        assert_eq!(repair_tree_snapshot(root.path()).unwrap(), base_snapshot);
        assert!(!root.path().join("outside-candidate.txt").exists());
        assert_eq!(
            std::fs::read(root.path().join("src/normal.rs")).unwrap(),
            b"pub fn base() {}\n"
        );
        assert_eq!(
            std::fs::read(root.path().join("src/heartbeat/engine-state")).unwrap(),
            b"must survive rollback\n"
        );
    }

    #[tokio::test]
    async fn post_land_same_ruler_drift_rolls_back_exactly() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let ruler = ruler("ruler:v1", &["smoke"]);
        let requirements = requirement_set();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements.clone(),
            ruler.required_legs().clone(),
            &[(
                RulerLegId::new("smoke").unwrap(),
                GateId::Smoke,
                "advertised entry cannot boot",
            )],
        );
        let base_snapshot = repair_tree_snapshot(root.path()).unwrap();
        let target = base.observations().keys().next().unwrap().clone();
        let mut transaction = RepairTransaction::open(root.path(), base).unwrap();
        transaction
            .add_candidate(
                RepairCandidatePatch::new(
                    "post-land-drift",
                    transaction.epoch(),
                    BTreeSet::from([target]),
                    write_change(root.path(), b"fixed\n"),
                )
                .unwrap(),
            )
            .unwrap();
        let ruler_calls = Arc::new(AtomicUsize::new(0));
        let calls_for_ruler = ruler_calls.clone();
        let ruler_for_run = ruler.clone();
        let requirements_for_run = requirements.clone();
        let decision = transaction
            .preview_and_promote(
                move |path| {
                    let call = calls_for_ruler.fetch_add(1, AtomicOrdering::SeqCst);
                    let ruler = ruler_for_run.clone();
                    let requirements = requirements_for_run.clone();
                    async move {
                        let ledger = clean_ledger(&path, ruler, requirements);
                        if call == 1 {
                            std::fs::write(path.join("ruler-drift.txt"), b"post-land drift\n")?;
                        }
                        Ok(ledger)
                    }
                },
                |request| async move { accept(&request).await },
            )
            .await
            .unwrap();
        assert!(matches!(decision, PromotionDecision::RolledBack { .. }));
        assert_eq!(ruler_calls.load(AtomicOrdering::SeqCst), 2);
        assert_eq!(repair_tree_snapshot(root.path()).unwrap(), base_snapshot);
        assert!(!root.path().join("ruler-drift.txt").exists());
    }

    #[tokio::test]
    async fn exact_composed_preview_promotes_only_after_same_ruler_verification() {
        let root = tempfile::TempDir::new().unwrap();
        write_file(root.path(), "app.txt", b"broken\n");
        let ruler = ruler("ruler:v1", &["smoke"]);
        let requirements = requirement_set();
        let base = ledger_with_findings(
            root.path(),
            ruler.clone(),
            requirements.clone(),
            ruler.required_legs().clone(),
            &[(
                RulerLegId::new("smoke").unwrap(),
                GateId::Smoke,
                "advertised entry cannot boot",
            )],
        );
        let target = base.observations().keys().next().unwrap().clone();
        let mut transaction = RepairTransaction::open(root.path(), base).unwrap();
        transaction
            .add_candidate(
                RepairCandidatePatch::new(
                    "verified-fix",
                    transaction.epoch(),
                    BTreeSet::from([target]),
                    write_change(root.path(), b"fixed\n"),
                )
                .unwrap(),
            )
            .unwrap();
        let ruler_calls = Arc::new(AtomicUsize::new(0));
        let calls_for_ruler = ruler_calls.clone();
        let ruler_for_run = ruler.clone();
        let requirements_for_run = requirements.clone();
        let decision = transaction
            .preview_and_promote(
                move |path| {
                    calls_for_ruler.fetch_add(1, AtomicOrdering::SeqCst);
                    let ruler = ruler_for_run.clone();
                    let requirements = requirements_for_run.clone();
                    async move { Ok(clean_ledger(&path, ruler, requirements)) }
                },
                |request| async move { accept(&request).await },
            )
            .await
            .unwrap();
        let PromotionDecision::Promoted {
            semantic_acceptance,
            ..
        } = decision
        else {
            panic!("exact brokered semantic review did not promote");
        };
        assert_eq!(
            semantic_acceptance.provider_terminal(),
            ProviderTerminalKind::Finished
        );
        assert_eq!(
            semantic_acceptance.admission().role,
            WorkRole::SemanticJudgeObservation
        );
        assert!(!semantic_acceptance.reviewer_reply_hash().is_empty());
        assert_eq!(ruler_calls.load(AtomicOrdering::SeqCst), 2);
        assert_eq!(
            std::fs::read(root.path().join("app.txt")).unwrap(),
            b"fixed\n"
        );
    }
}
