use anyhow::{bail, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct DefectId(pub(crate) String);

impl fmt::Display for DefectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GateId {
    Smoke,
    FailedTask,
    ProvisionalTask,
    MissingDeliverable,
    CrossModule,
    HttpTimeout,
    DomId,
    CssCoherence,
    SpecContract,
}

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
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct RequirementId(pub(crate) String);

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(crate) enum SubjectRef {
    File(String),
    Task(String),
    Interface(String),
    Requirement(RequirementId),
    Runtime(String),
}

impl SubjectRef {
    fn stable_name(&self) -> String {
        match self {
            Self::File(value) => format!("file:{value}"),
            Self::Task(value) => format!("task:{value}"),
            Self::Interface(value) => format!("interface:{value}"),
            Self::Requirement(value) => format!("requirement:{}", value.0),
            Self::Runtime(value) => format!("runtime:{value}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DefectKind {
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

impl DefectKind {
    fn invariant_name(self) -> &'static str {
        match self {
            Self::RuntimeBoot => "runtime_boot",
            Self::TestCollection => "test_collection",
            Self::TestFailure => "test_failure",
            Self::PlannedTaskFailure => "planned_task_failure",
            Self::ProvisionalCompletion => "provisional_completion",
            Self::MissingArtifact => "missing_artifact",
            Self::CrossModuleContract => "cross_module_contract",
            Self::UnsafeNetworkCall => "unsafe_network_call",
            Self::MissingDomTarget => "missing_dom_target",
            Self::StyleMarkupMismatch => "style_markup_mismatch",
            Self::RequirementViolation => "requirement_violation",
            Self::GateUnestablished => "gate_unestablished",
            Self::InvariantViolation => "invariant_violation",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MechanicalSeverity {
    Blocking,
    Major,
    Advisory,
    Unestablished,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImpactFact {
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ImpactEvidence {
    pub(crate) severity: MechanicalSeverity,
    pub(crate) fact: ImpactFact,
    pub(crate) gate: GateId,
}

impl ImpactEvidence {
    pub(crate) fn blocks_promotion(&self) -> bool {
        matches!(
            self.severity,
            MechanicalSeverity::Blocking | MechanicalSeverity::Unestablished
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct EvidenceRef {
    pub(crate) sha256: String,
    pub(crate) relative_path: String,
    pub(crate) media_type: String,
    pub(crate) bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DefectObservation {
    pub(crate) id: DefectId,
    pub(crate) gate: GateId,
    pub(crate) requirement_ids: BTreeSet<RequirementId>,
    pub(crate) subjects: BTreeSet<SubjectRef>,
    pub(crate) kind: DefectKind,
    pub(crate) impact: ImpactEvidence,
    pub(crate) evidence: Vec<EvidenceRef>,
    pub(crate) first_seen_tree: String,
    pub(crate) last_seen_tree: String,
    pub(crate) invariant: String,
}

#[derive(Clone, Debug)]
pub(crate) struct FindingInput<'a> {
    pub(crate) gate: GateId,
    pub(crate) rendered: &'a str,
    pub(crate) established: bool,
    pub(crate) known_files: &'a [String],
    pub(crate) explicit_subjects: BTreeSet<SubjectRef>,
    pub(crate) requirement_ids: BTreeSet<RequirementId>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FindingBatch<'a> {
    pub(crate) gate: GateId,
    pub(crate) findings: &'a [String],
    pub(crate) established: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DefectLedger {
    pub(crate) observations: BTreeMap<DefectId, DefectObservation>,
    pub(crate) established: bool,
    pub(crate) hash: String,
}

impl DefectLedger {
    pub(crate) fn from_observations(
        observations: impl IntoIterator<Item = DefectObservation>,
        established: bool,
    ) -> Self {
        let observations = observations
            .into_iter()
            .map(|observation| (observation.id.clone(), observation))
            .collect();
        let mut ledger = Self {
            observations,
            established,
            hash: String::new(),
        };
        ledger.hash = ledger.content_hash();
        ledger
    }

    pub(crate) fn reconcile(&mut self, previous: &Self) {
        for (id, observation) in &mut self.observations {
            if let Some(prior) = previous.observations.get(id) {
                observation.first_seen_tree = prior.first_seen_tree.clone();
            }
        }
        self.hash = self.content_hash();
    }

    pub(crate) fn blocking_ids(&self) -> BTreeSet<DefectId> {
        self.observations
            .iter()
            .filter(|(_, observation)| observation.impact.blocks_promotion())
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(crate) fn evidence_ids(&self) -> BTreeSet<String> {
        self.observations
            .values()
            .flat_map(|observation| observation.evidence.iter())
            .map(|evidence| evidence.sha256.clone())
            .collect()
    }

    fn content_hash(&self) -> String {
        let bytes = serde_json::to_vec(&(self.established, &self.observations))
            .expect("repair ledger contains only serializable engine values");
        sha256_hex(&bytes)
    }
}

pub(crate) fn build_defect_ledger(
    root: &Path,
    tree_hash: &str,
    known_files: &[String],
    batches: &[FindingBatch<'_>],
    mut established: bool,
) -> DefectLedger {
    let mut observations = Vec::new();
    for batch in batches {
        let synthetic;
        let findings = if !batch.established && batch.findings.is_empty() {
            synthetic = vec![format!(
                "{} gate did not establish a verdict",
                batch.gate.invariant_name()
            )];
            synthetic.as_slice()
        } else {
            batch.findings
        };
        for rendered in findings {
            let mut explicit_subjects = BTreeSet::new();
            if matches!(batch.gate, GateId::FailedTask | GateId::ProvisionalTask) {
                if let Some(task_id) = first_backtick_value(rendered) {
                    explicit_subjects.insert(SubjectRef::Task(task_id));
                }
            }
            let requirement_ids = BTreeSet::from([RequirementId(format!(
                "engine:{}",
                batch.gate.invariant_name()
            ))]);
            match observe_finding(
                root,
                tree_hash,
                FindingInput {
                    gate: batch.gate,
                    rendered,
                    established: batch.established,
                    known_files,
                    explicit_subjects,
                    requirement_ids,
                },
            ) {
                Ok(observation) => observations.push(observation),
                Err(error) => {
                    established = false;
                    observations.push(evidence_failure_observation(
                        tree_hash,
                        batch.gate,
                        &error.to_string(),
                    ));
                }
            }
        }
        established &= batch.established;
    }
    DefectLedger::from_observations(observations, established)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct CandidateDelta {
    pub(crate) closed: BTreeSet<DefectId>,
    pub(crate) persisted: BTreeSet<DefectId>,
    pub(crate) introduced: BTreeSet<DefectId>,
    pub(crate) changed_evidence: BTreeSet<DefectId>,
    pub(crate) established: bool,
}

impl CandidateDelta {
    pub(crate) fn between(base: &DefectLedger, candidate: &DefectLedger) -> Self {
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
        Self {
            closed,
            persisted,
            introduced,
            changed_evidence,
            established: candidate.established,
        }
    }

    pub(crate) fn introduced_blockers(&self, ledger: &DefectLedger) -> BTreeSet<DefectId> {
        self.introduced
            .iter()
            .filter(|id| {
                ledger
                    .observations
                    .get(*id)
                    .is_some_and(|observation| observation.impact.blocks_promotion())
            })
            .cloned()
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticReviewVerdict {
    Accept,
    Revise,
    Abstain,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SemanticRepairReview {
    pub(crate) candidate: String,
    pub(crate) verdict: SemanticReviewVerdict,
    pub(crate) cited_requirements: BTreeSet<RequirementId>,
    pub(crate) cited_evidence: BTreeSet<String>,
    pub(crate) rationale: String,
}

impl SemanticRepairReview {
    pub(crate) fn validate(
        &self,
        known_requirements: &BTreeSet<RequirementId>,
        known_evidence: &BTreeSet<String>,
    ) -> Result<()> {
        if self.candidate.trim().is_empty() || self.rationale.trim().is_empty() {
            bail!("semantic repair review omitted candidate identity or rationale");
        }
        if !self.cited_requirements.is_subset(known_requirements) {
            bail!("semantic repair review cited an unknown requirement");
        }
        if !self.cited_evidence.is_subset(known_evidence) {
            bail!("semantic repair review cited unknown evidence");
        }
        if !matches!(self.verdict, SemanticReviewVerdict::Abstain)
            && (self.cited_requirements.is_empty() || self.cited_evidence.is_empty())
        {
            bail!("semantic repair verdict requires requirement and evidence citations");
        }
        Ok(())
    }
}

pub(crate) fn observe_finding(
    root: &Path,
    tree_hash: &str,
    mut input: FindingInput<'_>,
) -> Result<DefectObservation> {
    input
        .explicit_subjects
        .extend(subjects_from_known_files(input.rendered, input.known_files));
    let kind = defect_kind(input.gate, input.rendered, input.established);
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
    let impact = impact_evidence(input.gate, kind, input.established);
    let invariant = normalized_invariant(input.rendered, &input.explicit_subjects);
    let id = defect_id(
        input.gate,
        kind,
        &invariant,
        &input.explicit_subjects,
        &input.requirement_ids,
    );
    let evidence = vec![persist_evidence(root, input.gate, input.rendered)?];
    Ok(DefectObservation {
        id,
        gate: input.gate,
        requirement_ids: input.requirement_ids,
        subjects: input.explicit_subjects,
        kind,
        impact,
        evidence,
        first_seen_tree: tree_hash.to_string(),
        last_seen_tree: tree_hash.to_string(),
        invariant,
    })
}

fn defect_kind(gate: GateId, rendered: &str, established: bool) -> DefectKind {
    if !established {
        return DefectKind::GateUnestablished;
    }
    match gate {
        GateId::FailedTask => DefectKind::PlannedTaskFailure,
        GateId::ProvisionalTask => DefectKind::ProvisionalCompletion,
        GateId::MissingDeliverable => DefectKind::MissingArtifact,
        GateId::CrossModule => DefectKind::CrossModuleContract,
        GateId::HttpTimeout => DefectKind::UnsafeNetworkCall,
        GateId::DomId => DefectKind::MissingDomTarget,
        GateId::CssCoherence => DefectKind::StyleMarkupMismatch,
        GateId::SpecContract => DefectKind::RequirementViolation,
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

fn impact_evidence(gate: GateId, kind: DefectKind, established: bool) -> ImpactEvidence {
    if !established {
        return ImpactEvidence {
            severity: MechanicalSeverity::Unestablished,
            fact: ImpactFact::GateDidNotEstablishVerdict,
            gate,
        };
    }
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

fn first_backtick_value(rendered: &str) -> Option<String> {
    let (_, remainder) = rendered.split_once('`')?;
    let (value, _) = remainder.split_once('`')?;
    (!value.trim().is_empty()).then(|| value.trim().to_string())
}

fn evidence_failure_observation(tree_hash: &str, gate: GateId, error: &str) -> DefectObservation {
    let invariant = "repair evidence could not be persisted".to_string();
    let requirements = BTreeSet::from([RequirementId("engine:evidence-integrity".to_string())]);
    let id = defect_id(
        gate,
        DefectKind::GateUnestablished,
        &invariant,
        &BTreeSet::new(),
        &requirements,
    );
    DefectObservation {
        id,
        gate,
        requirement_ids: requirements,
        subjects: BTreeSet::new(),
        kind: DefectKind::GateUnestablished,
        impact: ImpactEvidence {
            severity: MechanicalSeverity::Unestablished,
            fact: ImpactFact::GateDidNotEstablishVerdict,
            gate,
        },
        evidence: vec![EvidenceRef {
            sha256: sha256_hex(error.as_bytes()),
            relative_path: String::new(),
            media_type: "text/plain; unavailable=true".to_string(),
            bytes: error.len(),
        }],
        first_seen_tree: tree_hash.to_string(),
        last_seen_tree: tree_hash.to_string(),
        invariant,
    }
}

fn subjects_from_known_files(rendered: &str, known_files: &[String]) -> BTreeSet<SubjectRef> {
    known_files
        .iter()
        .filter(|file| rendered.contains(file.as_str()))
        .map(|file| SubjectRef::File(file.replace('\\', "/")))
        .collect()
}

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

fn defect_id(
    gate: GateId,
    kind: DefectKind,
    invariant: &str,
    subjects: &BTreeSet<SubjectRef>,
    requirements: &BTreeSet<RequirementId>,
) -> DefectId {
    let mut identity = String::new();
    identity.push_str(gate.invariant_name());
    identity.push('\0');
    identity.push_str(kind.invariant_name());
    identity.push('\0');
    identity.push_str(invariant);
    for subject in subjects {
        identity.push('\0');
        identity.push_str(&subject.stable_name());
    }
    for requirement in requirements {
        identity.push('\0');
        identity.push_str(&requirement.0);
    }
    DefectId(format!("defect:{}", sha256_hex(identity.as_bytes())))
}

fn persist_evidence(root: &Path, gate: GateId, rendered: &str) -> Result<EvidenceRef> {
    static EVIDENCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sha256 = sha256_hex(rendered.as_bytes());
    let relative_path = format!(".swarm/repair/evidence/{sha256}.txt");
    let path = root.join(&relative_path);
    if path.exists() {
        if std::fs::read(&path)? != rendered.as_bytes() {
            bail!("repair evidence digest collision at {relative_path}");
        }
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("repair evidence path has no parent"))?;
        std::fs::create_dir_all(parent)?;
        let canonical_root = root.canonicalize()?;
        if !parent.canonicalize()?.starts_with(canonical_root) {
            bail!("repair evidence path escaped the application root");
        }
        let sequence = EVIDENCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".{sha256}.{}.{}.tmp", std::process::id(), sequence));
        std::fs::write(&temp, rendered)?;
        match std::fs::rename(&temp, &path) {
            Ok(()) => {}
            Err(_error) if path.exists() && std::fs::read(&path)? == rendered.as_bytes() => {
                let _ = std::fs::remove_file(temp);
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(EvidenceRef {
        sha256,
        relative_path,
        media_type: format!("text/plain; gate={}", gate.invariant_name()),
        bytes: rendered.len(),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(digest.len() * 2);
    for byte in digest {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    result
}

pub(crate) fn safe_relative_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RepairEpoch {
    pub(crate) tree_hash: String,
    pub(crate) ledger_hash: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileMutation {
    Write { bytes: Vec<u8>, mode: u32 },
    Delete,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct RepairCandidatePatch {
    pub(crate) id: String,
    pub(crate) base: RepairEpoch,
    pub(crate) targets: BTreeSet<DefectId>,
    pub(crate) changes: BTreeMap<String, FileMutation>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum PromotionDecision {
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
    },
}

#[allow(dead_code)]
pub(crate) struct RepairTransaction {
    root: std::path::PathBuf,
    base_snapshot: super::RepairTreeSnapshot,
    base_ledger: DefectLedger,
    candidates: Vec<RepairCandidatePatch>,
}

#[derive(Clone)]
struct OriginalFile {
    path: String,
    bytes: Option<Vec<u8>>,
    mode: Option<u32>,
}

#[allow(dead_code)]
impl RepairTransaction {
    pub(crate) fn open(root: &Path, base_ledger: DefectLedger) -> Result<Self> {
        let base_snapshot = super::repair_tree_snapshot(root)?;
        Ok(Self {
            root: root.to_path_buf(),
            base_snapshot,
            base_ledger,
            candidates: Vec::new(),
        })
    }

    pub(crate) fn epoch(&self) -> RepairEpoch {
        RepairEpoch {
            tree_hash: self.base_snapshot.sha256.clone(),
            ledger_hash: self.base_ledger.hash.clone(),
        }
    }

    pub(crate) fn add_candidate(&mut self, candidate: RepairCandidatePatch) -> Result<()> {
        if candidate.id.trim().is_empty() {
            bail!("repair candidate omitted its identity");
        }
        if candidate.base != self.epoch() {
            bail!("repair candidate was generated against a stale tree or ledger");
        }
        if candidate.targets.is_empty() {
            bail!("repair candidate has no causal defect target");
        }
        if candidate.changes.is_empty() {
            bail!("repair candidate is a byte-identical no-op");
        }
        for path in candidate.changes.keys() {
            if !safe_relative_path(path) {
                bail!("repair candidate contains unsafe path `{path}`");
            }
            if self
                .candidates
                .iter()
                .any(|existing| existing.changes.contains_key(path))
            {
                bail!("repair candidates overlap on `{path}`");
            }
        }
        self.candidates.push(candidate);
        Ok(())
    }

    pub(crate) fn preview_and_promote<F>(
        &self,
        review: &SemanticRepairReview,
        known_requirements: &BTreeSet<RequirementId>,
        ruler: F,
    ) -> Result<PromotionDecision>
    where
        F: FnMut(&Path) -> Result<DefectLedger>,
    {
        self.preview_and_promote_with_apply(review, known_requirements, ruler, apply_mutations)
    }

    fn preview_and_promote_with_apply<F, A>(
        &self,
        review: &SemanticRepairReview,
        known_requirements: &BTreeSet<RequirementId>,
        mut ruler: F,
        mut real_apply: A,
    ) -> Result<PromotionDecision>
    where
        F: FnMut(&Path) -> Result<DefectLedger>,
        A: FnMut(&Path, &BTreeMap<String, FileMutation>) -> Result<()>,
    {
        if self.candidates.is_empty() {
            return Ok(PromotionDecision::Rejected {
                reason: "transaction contains no candidates".to_string(),
                preview_tree_hash: None,
                preview_ledger_hash: None,
                delta: None,
            });
        }
        let current = super::repair_tree_snapshot(&self.root)?;
        if current.sha256 != self.base_snapshot.sha256 {
            return Ok(PromotionDecision::Rejected {
                reason: "real tree changed after transaction opened".to_string(),
                preview_tree_hash: None,
                preview_ledger_hash: None,
                delta: None,
            });
        }

        let changes = self.composed_changes();
        let preview = tempfile::TempDir::new()?;
        copy_ruled_tree(&self.root, preview.path())?;
        apply_mutations(preview.path(), &changes)?;
        let preview_snapshot = super::repair_tree_snapshot(preview.path())?;
        if preview_snapshot.sha256 == self.base_snapshot.sha256 {
            return Ok(PromotionDecision::Rejected {
                reason: "composed candidate is a byte-identical no-op".to_string(),
                preview_tree_hash: Some(preview_snapshot.sha256),
                preview_ledger_hash: None,
                delta: None,
            });
        }

        // This closure is the one hermetic ruler. It is called once on the exact composed preview and,
        // only after every promotion predicate passes, once on the exact landed real tree.
        let preview_ledger = ruler(preview.path())?;
        let delta = CandidateDelta::between(&self.base_ledger, &preview_ledger);
        let targets: BTreeSet<DefectId> = self
            .candidates
            .iter()
            .flat_map(|candidate| candidate.targets.iter().cloned())
            .collect();
        if !preview_ledger.established {
            return Ok(rejected_preview(
                "composed ruler did not establish every required leg",
                &preview_snapshot,
                &preview_ledger,
                delta,
            ));
        }
        if !targets.is_subset(&delta.closed) {
            return Ok(rejected_preview(
                "composed preview did not close every targeted causal defect",
                &preview_snapshot,
                &preview_ledger,
                delta,
            ));
        }
        if !delta.introduced_blockers(&preview_ledger).is_empty() {
            return Ok(rejected_preview(
                "composed preview introduced a mechanically blocking defect",
                &preview_snapshot,
                &preview_ledger,
                delta,
            ));
        }

        let known_evidence: BTreeSet<String> = self
            .base_ledger
            .evidence_ids()
            .into_iter()
            .chain(preview_ledger.evidence_ids())
            .collect();
        review.validate(known_requirements, &known_evidence)?;
        let composition_id = composition_id(&self.candidates);
        if review.candidate != composition_id {
            return Ok(rejected_preview(
                "semantic review does not identify this exact candidate composition",
                &preview_snapshot,
                &preview_ledger,
                delta,
            ));
        }
        if !matches!(review.verdict, SemanticReviewVerdict::Accept) {
            return Ok(rejected_preview(
                "semantic judge did not accept the requirement-level delta",
                &preview_snapshot,
                &preview_ledger,
                delta,
            ));
        }

        // Recheck the base immediately before the first real write. A candidate may take hours to
        // generate and grade; a current-looking shadow is not authority over a tree that moved meanwhile.
        let before_land = super::repair_tree_snapshot(&self.root)?;
        if before_land.sha256 != self.base_snapshot.sha256 {
            return Ok(rejected_preview(
                "real tree changed while the composed preview was being ruled",
                &preview_snapshot,
                &preview_ledger,
                delta,
            ));
        }
        let originals = capture_originals(&self.root, changes.keys())?;
        if let Err(error) = real_apply(&self.root, &changes) {
            restore_originals(&self.root, &self.base_snapshot, &originals)?;
            return Ok(PromotionDecision::RolledBack {
                reason: format!("atomic land failed: {error}"),
                restored_tree_hash: super::repair_tree_snapshot(&self.root)?.sha256,
            });
        }

        let real_ledger = match ruler(&self.root) {
            Ok(ledger) => ledger,
            Err(error) => {
                restore_originals(&self.root, &self.base_snapshot, &originals)?;
                return Ok(PromotionDecision::RolledBack {
                    reason: format!("post-promotion ruler failed: {error}"),
                    restored_tree_hash: super::repair_tree_snapshot(&self.root)?.sha256,
                });
            }
        };
        let landed_snapshot = super::repair_tree_snapshot(&self.root)?;
        if landed_snapshot.sha256 != preview_snapshot.sha256
            || real_ledger.hash != preview_ledger.hash
        {
            restore_originals(&self.root, &self.base_snapshot, &originals)?;
            return Ok(PromotionDecision::RolledBack {
                reason: format!(
                    "post-promotion tree/ledger differs from preview (preview tree {}, real tree {}, preview ledger {}, real ledger {})",
                    preview_snapshot.sha256,
                    landed_snapshot.sha256,
                    preview_ledger.hash,
                    real_ledger.hash,
                ),
                restored_tree_hash: super::repair_tree_snapshot(&self.root)?.sha256,
            });
        }

        Ok(PromotionDecision::Promoted {
            epoch_before: self.epoch(),
            epoch_after: RepairEpoch {
                tree_hash: landed_snapshot.sha256,
                ledger_hash: real_ledger.hash,
            },
            candidates: self
                .candidates
                .iter()
                .map(|candidate| candidate.id.clone())
                .collect(),
            changed_files: changes.keys().cloned().collect(),
            delta,
        })
    }

    fn composed_changes(&self) -> BTreeMap<String, FileMutation> {
        self.candidates
            .iter()
            .flat_map(|candidate| candidate.changes.clone())
            .collect()
    }
}

fn rejected_preview(
    reason: &str,
    snapshot: &super::RepairTreeSnapshot,
    ledger: &DefectLedger,
    delta: CandidateDelta,
) -> PromotionDecision {
    PromotionDecision::Rejected {
        reason: reason.to_string(),
        preview_tree_hash: Some(snapshot.sha256.clone()),
        preview_ledger_hash: Some(ledger.hash.clone()),
        delta: Some(delta),
    }
}

fn composition_id(candidates: &[RepairCandidatePatch]) -> String {
    let mut ids: Vec<&str> = candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect();
    ids.sort_unstable();
    format!("composition:{}", sha256_hex(ids.join("\0").as_bytes()))
}

fn capture_originals<'a>(
    root: &Path,
    paths: impl Iterator<Item = &'a String>,
) -> Result<Vec<OriginalFile>> {
    paths
        .map(|path| {
            let absolute = root.join(path);
            match std::fs::symlink_metadata(&absolute) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    Ok(OriginalFile {
                        path: path.clone(),
                        bytes: Some(std::fs::read(absolute)?),
                        mode: Some(super::repair_entry_mode(&metadata)),
                    })
                }
                Ok(_) => bail!("repair transaction refuses non-regular target `{path}`"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(OriginalFile {
                    path: path.clone(),
                    bytes: None,
                    mode: None,
                }),
                Err(error) => Err(error.into()),
            }
        })
        .collect()
}

fn apply_mutations(root: &Path, changes: &BTreeMap<String, FileMutation>) -> Result<()> {
    for (path, mutation) in changes {
        match mutation {
            FileMutation::Write { bytes, mode } => {
                let absolute = guarded_path(root, path)?;
                atomic_write(&absolute, bytes, *mode)?;
            }
            FileMutation::Delete => {
                let Some(absolute) = guarded_existing_path(root, path)? else {
                    continue;
                };
                match std::fs::symlink_metadata(&absolute) {
                    Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                        std::fs::remove_file(absolute)?;
                    }
                    Ok(_) => bail!("repair transaction refuses to delete non-regular `{path}`"),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(())
}

fn guarded_existing_path(root: &Path, relative: &str) -> Result<Option<std::path::PathBuf>> {
    if !safe_relative_path(relative) {
        bail!("unsafe repair path `{relative}`");
    }
    let root = root.canonicalize()?;
    let path = root.join(relative);
    let Some(parent) = path.parent() else {
        bail!("repair path has no parent");
    };
    if !parent.exists() {
        return Ok(None);
    }
    if !parent.canonicalize()?.starts_with(&root) {
        bail!("repair path escaped the application root");
    }
    Ok(Some(path))
}

fn guarded_path(root: &Path, relative: &str) -> Result<std::path::PathBuf> {
    if !safe_relative_path(relative) {
        bail!("unsafe repair path `{relative}`");
    }
    let root = root.canonicalize()?;
    let path = root.join(relative);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("repair path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(&root) {
        bail!("repair path escaped the application root");
    }
    Ok(path)
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("repair write has no parent"))?;
    let sequence = WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".repair-land.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    std::fs::write(&temp, bytes)?;
    set_mode(&temp, mode)?;
    std::fs::rename(temp, path)?;
    Ok(())
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

fn restore_originals(
    root: &Path,
    base: &super::RepairTreeSnapshot,
    originals: &[OriginalFile],
) -> Result<()> {
    for original in originals.iter().rev() {
        let absolute = guarded_path(root, &original.path)?;
        match (&original.bytes, original.mode) {
            (Some(bytes), Some(mode)) => atomic_write(&absolute, bytes, mode)?,
            (None, None) => match std::fs::symlink_metadata(&absolute) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    std::fs::remove_file(&absolute)?;
                }
                Ok(_) => bail!("rollback found non-regular creation `{}`", original.path),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            },
            _ => bail!("invalid repair rollback snapshot"),
        }
    }
    remove_created_directories(root, base)?;
    let restored = super::repair_tree_snapshot(root)?;
    if restored.sha256 != base.sha256 {
        bail!(
            "repair rollback did not restore the parent epoch (expected {}, got {})",
            base.sha256,
            restored.sha256
        );
    }
    Ok(())
}

fn remove_created_directories(root: &Path, base: &super::RepairTreeSnapshot) -> Result<()> {
    fn collect(root: &Path, dir: &Path, paths: &mut Vec<std::path::PathBuf>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                let relative = path.strip_prefix(root)?;
                if super::excluded_from_repair_tree(relative) {
                    continue;
                }
                collect(root, &path, paths)?;
                paths.push(path);
            }
        }
        Ok(())
    }
    let mut directories = Vec::new();
    collect(root, root, &mut directories)?;
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        let relative = directory
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if !base.entries.contains_key(&relative) {
            match std::fs::remove_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

fn copy_ruled_tree(source: &Path, destination: &Path) -> Result<()> {
    fn copy_dir(source_root: &Path, source: &Path, destination: &Path) -> Result<()> {
        std::fs::create_dir_all(destination)?;
        let metadata = std::fs::symlink_metadata(source)?;
        set_mode(destination, super::repair_entry_mode(&metadata))?;
        let mut entries = std::fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let relative = path.strip_prefix(source_root)?;
            if super::excluded_from_repair_tree(relative) {
                continue;
            }
            let target = destination.join(entry.file_name());
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.is_dir() {
                copy_dir(source_root, &path, &target)?;
            } else if metadata.is_file() && !metadata.file_type().is_symlink() {
                std::fs::copy(&path, &target)?;
                set_mode(&target, super::repair_entry_mode(&metadata))?;
            } else {
                bail!(
                    "repair transaction refuses symlink or special entry `{}`",
                    relative.display()
                );
            }
        }
        Ok(())
    }
    copy_dir(source, source, destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn finding(root: &Path, gate: GateId, rendered: &str, files: &[String]) -> DefectObservation {
        observe_finding(
            root,
            "tree-a",
            FindingInput {
                gate,
                rendered,
                established: true,
                known_files: files,
                explicit_subjects: BTreeSet::new(),
                requirement_ids: BTreeSet::new(),
            },
        )
        .unwrap()
    }

    #[test]
    fn causal_id_ignores_ports_temp_roots_trace_lines_and_finding_order() {
        let root = tempfile::TempDir::new().unwrap();
        let files = vec!["app/api.py".to_string(), "app/store.py".to_string()];
        let first = finding(
            root.path(),
            GateId::CrossModule,
            "app/api.py:91 reads app/store.py\n127.0.0.1:53129\n/private/tmp/a9/x.py, line 44\n2 failures",
            &files,
        );
        let second = finding(
            root.path(),
            GateId::CrossModule,
            "19 failures\napp/api.py:7 reads app/store.py\n/var/folders/zz/T/run/x.py, line 803\nlocalhost:61200",
            &files,
        );
        assert_eq!(first.id, second.id);
        assert_eq!(
            first
                .subjects
                .iter()
                .filter(|subject| matches!(subject, SubjectRef::File(_)))
                .count(),
            2
        );
    }

    #[test]
    fn equal_counts_do_not_make_different_defects_equal() {
        let root = tempfile::TempDir::new().unwrap();
        let first = finding(
            root.path(),
            GateId::Smoke,
            "1 failed: payments reject idempotency replay",
            &[],
        );
        let second = finding(
            root.path(),
            GateId::Smoke,
            "1 failed: webhook accepts invalid signature",
            &[],
        );
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn cross_module_observation_keeps_producer_and_consumer() {
        let root = tempfile::TempDir::new().unwrap();
        let files = vec!["app/producer.py".to_string(), "app/consumer.py".to_string()];
        let observation = finding(
            root.path(),
            GateId::CrossModule,
            "app/consumer.py reads Payment.parent but app/producer.py defines parent_hash",
            &files,
        );
        assert_eq!(
            observation.subjects,
            BTreeSet::from([
                SubjectRef::File("app/consumer.py".to_string()),
                SubjectRef::File("app/producer.py".to_string()),
                SubjectRef::Interface("cross-module".to_string()),
            ])
        );
    }

    #[test]
    fn severity_is_mechanical_evidence_not_finding_arithmetic() {
        let root = tempfile::TempDir::new().unwrap();
        let missing = finding(
            root.path(),
            GateId::MissingDeliverable,
            "required app/main.py is missing",
            &["app/main.py".to_string()],
        );
        let style = finding(
            root.path(),
            GateId::CssCoherence,
            "web/styles.css has no matching class in web/index.html",
            &["web/styles.css".to_string(), "web/index.html".to_string()],
        );
        assert!(missing.impact.blocks_promotion());
        assert_eq!(missing.impact.fact, ImpactFact::RequiredArtifactAbsent);
        assert!(!style.impact.blocks_promotion());
        assert_eq!(style.impact.severity, MechanicalSeverity::Advisory);
    }

    #[test]
    fn semantic_review_rejects_unknown_or_missing_citations() {
        let requirements = BTreeSet::from([RequirementId("spec:boot".to_string())]);
        let evidence = BTreeSet::from(["evidence-a".to_string()]);
        let valid = SemanticRepairReview {
            candidate: "candidate-a".to_string(),
            verdict: SemanticReviewVerdict::Accept,
            cited_requirements: requirements.clone(),
            cited_evidence: evidence.clone(),
            rationale: "restores the advertised entry".to_string(),
        };
        assert!(valid.validate(&requirements, &evidence).is_ok());

        let malformed = SemanticRepairReview {
            cited_evidence: BTreeSet::from(["made-up".to_string()]),
            ..valid
        };
        assert!(malformed.validate(&requirements, &evidence).is_err());
    }

    #[test]
    fn delta_tracks_identity_and_rejectable_blocker_introduction() {
        let root = tempfile::TempDir::new().unwrap();
        let minor_a = finding(root.path(), GateId::DomId, "missing #one", &[]);
        let minor_b = finding(root.path(), GateId::CssCoherence, "missing .two", &[]);
        let boot = finding(
            root.path(),
            GateId::Smoke,
            "advertised entry failed to boot",
            &[],
        );
        let base = DefectLedger::from_observations([minor_a, minor_b], true);
        let candidate = DefectLedger::from_observations([boot.clone()], true);
        let delta = CandidateDelta::between(&base, &candidate);
        assert_eq!(delta.closed.len(), 2);
        assert_eq!(
            delta.introduced_blockers(&candidate),
            BTreeSet::from([boot.id])
        );
    }

    fn transaction_fixture() -> (tempfile::TempDir, DefectLedger, DefectObservation) {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("a.txt"), "old-a\n").unwrap();
        std::fs::write(root.path().join("b.txt"), "old-b\n").unwrap();
        let target = finding(root.path(), GateId::DomId, "missing #target", &[]);
        let ledger = DefectLedger::from_observations([target.clone()], true);
        (root, ledger, target)
    }

    fn patch(
        id: &str,
        base: RepairEpoch,
        target: DefectId,
        path: &str,
        bytes: &str,
    ) -> RepairCandidatePatch {
        RepairCandidatePatch {
            id: id.to_string(),
            base,
            targets: BTreeSet::from([target]),
            changes: BTreeMap::from([(
                path.to_string(),
                FileMutation::Write {
                    bytes: bytes.as_bytes().to_vec(),
                    mode: 0o644,
                },
            )]),
        }
    }

    fn review_for(transaction: &RepairTransaction, evidence: &EvidenceRef) -> SemanticRepairReview {
        SemanticRepairReview {
            candidate: composition_id(&transaction.candidates),
            verdict: SemanticReviewVerdict::Accept,
            cited_requirements: BTreeSet::from([RequirementId("engine:test".to_string())]),
            cited_evidence: BTreeSet::from([evidence.sha256.clone()]),
            rationale: "the exact composed diff closes the cited target".to_string(),
        }
    }

    fn requirements() -> BTreeSet<RequirementId> {
        BTreeSet::from([RequirementId("engine:test".to_string())])
    }

    #[test]
    fn independently_good_candidates_with_bad_composition_never_touch_real_tree() {
        let (root, ledger, target) = transaction_fixture();
        let original = super::super::repair_tree_snapshot(root.path()).unwrap();
        let mut transaction = RepairTransaction::open(root.path(), ledger).unwrap();
        transaction
            .add_candidate(patch(
                "a",
                transaction.epoch(),
                target.id.clone(),
                "a.txt",
                "good-a\n",
            ))
            .unwrap();
        transaction
            .add_candidate(patch(
                "b",
                transaction.epoch(),
                target.id.clone(),
                "b.txt",
                "good-b\n",
            ))
            .unwrap();

        for candidate in &transaction.candidates {
            let preview = tempfile::TempDir::new().unwrap();
            copy_ruled_tree(root.path(), preview.path()).unwrap();
            apply_mutations(preview.path(), &candidate.changes).unwrap();
            let changed_a =
                std::fs::read_to_string(preview.path().join("a.txt")).unwrap() == "good-a\n";
            let changed_b =
                std::fs::read_to_string(preview.path().join("b.txt")).unwrap() == "good-b\n";
            assert_ne!(
                changed_a, changed_b,
                "each isolated candidate is good alone"
            );
        }

        let review = review_for(&transaction, &target.evidence[0]);
        let decision = transaction
            .preview_and_promote(&review, &requirements(), |candidate_root| {
                let a = std::fs::read_to_string(candidate_root.join("a.txt"))?;
                let b = std::fs::read_to_string(candidate_root.join("b.txt"))?;
                if a == "good-a\n" && b == "good-b\n" {
                    let blocker = finding(
                        candidate_root,
                        GateId::Smoke,
                        "advertised entry failed to boot after composition",
                        &[],
                    );
                    Ok(DefectLedger::from_observations([blocker], true))
                } else {
                    Ok(DefectLedger::from_observations(Vec::new(), true))
                }
            })
            .unwrap();
        assert!(matches!(
            decision,
            PromotionDecision::Rejected { reason, .. }
                if reason.contains("introduced a mechanically blocking defect")
        ));
        assert_eq!(
            super::super::repair_tree_snapshot(root.path()).unwrap(),
            original
        );
    }

    #[test]
    fn stale_base_and_no_op_are_refused_before_real_mutation() {
        let (root, ledger, target) = transaction_fixture();
        let mut stale = RepairTransaction::open(root.path(), ledger.clone()).unwrap();
        stale
            .add_candidate(patch(
                "stale",
                stale.epoch(),
                target.id.clone(),
                "a.txt",
                "new\n",
            ))
            .unwrap();
        std::fs::write(root.path().join("b.txt"), "outside-change\n").unwrap();
        let review = review_for(&stale, &target.evidence[0]);
        let decision = stale
            .preview_and_promote(&review, &requirements(), |_| {
                Ok(DefectLedger::from_observations(Vec::new(), true))
            })
            .unwrap();
        assert!(matches!(
            decision,
            PromotionDecision::Rejected { reason, .. }
                if reason.contains("changed after transaction opened")
        ));
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "old-a\n"
        );

        let current_ledger = ledger;
        let mut no_op = RepairTransaction::open(root.path(), current_ledger).unwrap();
        no_op
            .add_candidate(patch(
                "no-op",
                no_op.epoch(),
                target.id.clone(),
                "a.txt",
                "old-a\n",
            ))
            .unwrap();
        let review = review_for(&no_op, &target.evidence[0]);
        let decision = no_op
            .preview_and_promote(&review, &requirements(), |_| {
                Ok(DefectLedger::from_observations(Vec::new(), true))
            })
            .unwrap();
        assert!(matches!(
            decision,
            PromotionDecision::Rejected { reason, .. }
                if reason.contains("byte-identical no-op")
        ));
    }

    #[test]
    fn post_land_ledger_mismatch_restores_parent_epoch() {
        let (root, ledger, target) = transaction_fixture();
        let original = super::super::repair_tree_snapshot(root.path()).unwrap();
        let mut transaction = RepairTransaction::open(root.path(), ledger).unwrap();
        transaction
            .add_candidate(patch(
                "candidate",
                transaction.epoch(),
                target.id.clone(),
                "a.txt",
                "fixed\n",
            ))
            .unwrap();
        let review = review_for(&transaction, &target.evidence[0]);
        let calls = Cell::new(0);
        let decision = transaction
            .preview_and_promote(&review, &requirements(), |candidate_root| {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    Ok(DefectLedger::from_observations(Vec::new(), true))
                } else {
                    let mismatch = finding(
                        candidate_root,
                        GateId::Smoke,
                        "advertised entry failed to boot only after landing",
                        &[],
                    );
                    Ok(DefectLedger::from_observations([mismatch], true))
                }
            })
            .unwrap();
        assert!(matches!(decision, PromotionDecision::RolledBack { .. }));
        assert_eq!(
            super::super::repair_tree_snapshot(root.path()).unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "old-a\n"
        );
    }

    #[test]
    fn partial_real_write_failure_rolls_back_every_file() {
        let (root, ledger, target) = transaction_fixture();
        let original = super::super::repair_tree_snapshot(root.path()).unwrap();
        let mut transaction = RepairTransaction::open(root.path(), ledger).unwrap();
        transaction
            .add_candidate(patch(
                "a",
                transaction.epoch(),
                target.id.clone(),
                "a.txt",
                "fixed-a\n",
            ))
            .unwrap();
        transaction
            .add_candidate(patch(
                "b",
                transaction.epoch(),
                target.id.clone(),
                "b.txt",
                "fixed-b\n",
            ))
            .unwrap();
        let review = review_for(&transaction, &target.evidence[0]);
        let decision = transaction
            .preview_and_promote_with_apply(
                &review,
                &requirements(),
                |_| Ok(DefectLedger::from_observations(Vec::new(), true)),
                |real_root, changes| {
                    let first = changes.iter().next().unwrap();
                    apply_mutations(
                        real_root,
                        &BTreeMap::from([(first.0.clone(), first.1.clone())]),
                    )?;
                    bail!("injected second-file write failure")
                },
            )
            .unwrap();
        assert!(matches!(decision, PromotionDecision::RolledBack { .. }));
        assert_eq!(
            super::super::repair_tree_snapshot(root.path()).unwrap(),
            original
        );
    }

    #[test]
    fn accepted_composition_lands_once_and_matches_preview_epoch() {
        let (root, ledger, target) = transaction_fixture();
        let mut transaction = RepairTransaction::open(root.path(), ledger).unwrap();
        transaction
            .add_candidate(patch(
                "candidate",
                transaction.epoch(),
                target.id.clone(),
                "a.txt",
                "fixed\n",
            ))
            .unwrap();
        let review = review_for(&transaction, &target.evidence[0]);
        let calls = Cell::new(0);
        let decision = transaction
            .preview_and_promote(&review, &requirements(), |_| {
                calls.set(calls.get() + 1);
                Ok(DefectLedger::from_observations(Vec::new(), true))
            })
            .unwrap();
        let PromotionDecision::Promoted {
            epoch_after,
            changed_files,
            ..
        } = decision
        else {
            panic!("accepted transaction did not promote")
        };
        assert_eq!(calls.get(), 2, "same ruler runs once on preview and real");
        assert_eq!(changed_files, vec!["a.txt"]);
        assert_eq!(
            epoch_after.tree_hash,
            super::super::repair_tree_snapshot(root.path())
                .unwrap()
                .sha256
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "fixed\n"
        );
    }
}
