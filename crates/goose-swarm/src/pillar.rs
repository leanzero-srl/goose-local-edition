//! Deterministic domain model for the pillar-oriented swarm opening and research flow.
//!
//! The model-facing CLI may produce drafts of these values, but this module owns the invariants
//! which make those drafts safe to checkpoint and hand to a planner. No provider or scheduler
//! state is involved here.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthoredRequirement {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub critical: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResearchPillar {
    pub id: String,
    pub title: String,
    pub objective: String,
    pub requirement_ids: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub research_questions: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub exclusions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IntegrationContract {
    pub owner: String,
    #[serde(default = "integration_required_default")]
    pub integration_required: bool,
    pub objective: String,
    pub interface_invariants: Vec<String>,
    pub acceptance_criteria: Vec<String>,
}

fn integration_required_default() -> bool {
    true
}

pub fn authored_requirements_require_integration(requirements: &[AuthoredRequirement]) -> bool {
    let authored = requirements
        .iter()
        .map(|requirement| requirement.text.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let explicitly_integrated = authored.split(['\n', '.', ';']).any(|clause| {
        let action = clause
            .split(|character: char| !character.is_ascii_alphabetic())
            .any(|word| {
                word.starts_with("compos")
                    || word.starts_with("integrat")
                    || word.starts_with("hook")
            });
        let one_product = [
            "into one app",
            "into one application",
            "into one product",
            "into a single app",
            "into a single application",
            "into a single product",
            "as one app",
            "as one application",
            "as one product",
            "as a single app",
            "as a single application",
            "as a single product",
        ]
        .iter()
        .any(|signal| clause.contains(signal));
        let negated = [
            "do not compose",
            "must not compose",
            "do not integrate",
            "must not integrate",
            "do not hook",
            "must not hook",
            "no integration required",
            "without integration",
            "without composing",
            "without integrating",
            "without hooking",
        ]
        .iter()
        .any(|signal| clause.contains(signal));
        action && one_product && !negated
    });
    let explicitly_independent = [
        "independent deliverables",
        "independent outputs",
        "standalone deliverables",
        "do not integrate",
        "must not integrate",
        "no integration required",
        "without integration",
    ]
    .iter()
    .any(|signal| authored.contains(signal));
    explicitly_integrated || !explicitly_independent
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResearchPillarOpening {
    pub requirements: Vec<AuthoredRequirement>,
    pub pillars: Vec<ResearchPillar>,
    pub integration_contract: IntegrationContract,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PillarImplementationTaskCoverage {
    pub task_id: String,
    pub requirement_ids: Vec<String>,
    pub is_validated_strongest_terminal: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    Proven,
    Supported,
    Unresolved,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Low,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResearchClaimDraft {
    pub requirement_id: String,
    pub statement: String,
    pub reported_class: EvidenceClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_section_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_quote: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceSourceSection {
    pub id: String,
    pub requirement_id: String,
    pub text: String,
    pub content_sha256: String,
    pub authority: EvidenceSourceAuthority,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSourceAuthority {
    EngineReceipt,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PillarReportDraft {
    pub pillar_id: String,
    pub reported_confidence: Confidence,
    pub claims: Vec<ResearchClaimDraft>,
    #[serde(default)]
    pub unresolved_uncertainties: Vec<String>,
    pub acceptance_tests: Vec<String>,
    #[serde(default)]
    pub interfaces: Vec<String>,
    pub exclusions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProvenanceMatch {
    NotClaimed,
    Unique {
        section_id: String,
        requirement_id: String,
        content_sha256: String,
        authority: EvidenceSourceAuthority,
    },
    MissingQuote,
    NotFound,
    Ambiguous,
    RequirementMismatch {
        matched_requirement_id: String,
    },
    SectionMismatch {
        matched_requirement_id: String,
    },
}

impl ProvenanceMatch {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::NotClaimed | Self::Unique { .. })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CompiledResearchClaim {
    pub requirement_id: String,
    pub statement: String,
    pub reported_class: EvidenceClass,
    pub effective_class: EvidenceClass,
    pub provenance: ProvenanceMatch,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CompiledPillarReport {
    pub pillar_id: String,
    pub reported_confidence: Confidence,
    pub effective_confidence: Confidence,
    pub claims: Vec<CompiledResearchClaim>,
    pub missing_requirement_ids: Vec<String>,
    pub unresolved_uncertainties: Vec<String>,
    pub acceptance_tests: Vec<String>,
    pub interfaces: Vec<String>,
    pub exclusions: Vec<String>,
}

impl CompiledPillarReport {
    pub fn needs_focused_retry(&self) -> bool {
        self.effective_confidence == Confidence::Low
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PillarDomainError {
    pub code: &'static str,
    pub message: String,
}

impl PillarDomainError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for PillarDomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PillarDomainError {}

pub fn validate_pillar_opening(opening: &ResearchPillarOpening) -> Result<(), PillarDomainError> {
    if opening.requirements.is_empty() {
        return Err(PillarDomainError::new(
            "requirements_empty",
            "the opening must contain at least one authored requirement",
        ));
    }
    if opening.pillars.is_empty() {
        return Err(PillarDomainError::new(
            "pillars_empty",
            "the opening must contain at least one research pillar",
        ));
    }

    let mut requirement_ids = BTreeSet::new();
    for requirement in &opening.requirements {
        require_text("requirement id", &requirement.id)?;
        require_text("requirement text", &requirement.text)?;
        if !requirement_ids.insert(requirement.id.clone()) {
            return Err(PillarDomainError::new(
                "requirement_id_duplicate",
                format!("requirement id {:?} is duplicated", requirement.id),
            ));
        }
    }

    let mut pillar_ids = BTreeSet::new();
    for pillar in &opening.pillars {
        require_text("pillar id", &pillar.id)?;
        if !pillar_ids.insert(pillar.id.clone()) {
            return Err(PillarDomainError::new(
                "pillar_id_duplicate",
                format!("pillar id {:?} is duplicated", pillar.id),
            ));
        }
        require_text("pillar title", &pillar.title)?;
        require_text("pillar objective", &pillar.objective)?;
        require_nonempty_texts("pillar research questions", &pillar.research_questions)?;
        require_nonempty_texts("pillar acceptance criteria", &pillar.acceptance_criteria)?;
        require_nonempty_texts("pillar exclusions", &pillar.exclusions)?;
        if pillar.requirement_ids.is_empty() {
            return Err(PillarDomainError::new(
                "pillar_requirements_empty",
                format!("pillar {:?} owns no requirements", pillar.id),
            ));
        }
    }

    let mut ownership = BTreeMap::<&str, Vec<&str>>::new();
    for pillar in &opening.pillars {
        let mut local = BTreeSet::new();
        for requirement_id in &pillar.requirement_ids {
            require_text("pillar requirement id", requirement_id)?;
            if !requirement_ids.contains(requirement_id) {
                return Err(PillarDomainError::new(
                    "pillar_requirement_unknown",
                    format!(
                        "pillar {:?} refers to unknown requirement {:?}",
                        pillar.id, requirement_id
                    ),
                ));
            }
            if !local.insert(requirement_id.as_str()) {
                return Err(PillarDomainError::new(
                    "pillar_requirement_duplicate",
                    format!(
                        "pillar {:?} repeats requirement {:?}",
                        pillar.id, requirement_id
                    ),
                ));
            }
            ownership
                .entry(requirement_id)
                .or_default()
                .push(&pillar.id);
        }

        for dependency in &pillar.dependencies {
            require_text("pillar dependency", dependency)?;
            if dependency == &pillar.id {
                return Err(PillarDomainError::new(
                    "pillar_dependency_self",
                    format!("pillar {:?} depends on itself", pillar.id),
                ));
            }
            if !pillar_ids.contains(dependency) {
                return Err(PillarDomainError::new(
                    "pillar_dependency_unknown",
                    format!(
                        "pillar {:?} depends on unknown pillar {:?}",
                        pillar.id, dependency
                    ),
                ));
            }
        }
    }

    for requirement_id in &requirement_ids {
        match ownership.get(requirement_id.as_str()) {
            None => {
                return Err(PillarDomainError::new(
                    "requirement_unowned",
                    format!("requirement {requirement_id:?} has no pillar owner"),
                ));
            }
            Some(owners) if owners.len() > 1 => {
                return Err(PillarDomainError::new(
                    "requirement_overlap",
                    format!(
                        "requirement {requirement_id:?} is owned by pillars {}",
                        owners
                            .iter()
                            .map(|owner| format!("{owner:?}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
            Some(_) => {}
        }
    }

    validate_acyclic_dependencies(&opening.pillars)?;
    require_text(
        "integration contract owner",
        &opening.integration_contract.owner,
    )?;
    require_text(
        "integration contract objective",
        &opening.integration_contract.objective,
    )?;
    require_nonempty_texts(
        "integration contract interface invariants",
        &opening.integration_contract.interface_invariants,
    )?;
    require_nonempty_texts(
        "integration contract acceptance criteria",
        &opening.integration_contract.acceptance_criteria,
    )?;
    Ok(())
}

pub fn validate_pillar_opening_against(
    opening: &ResearchPillarOpening,
    authored_requirements: &[AuthoredRequirement],
) -> Result<(), PillarDomainError> {
    validate_pillar_opening(opening)?;
    if opening.requirements != authored_requirements {
        return Err(PillarDomainError::new(
            "authored_requirement_binding_mismatch",
            "the opening requirement ledger is not the frozen authored requirement ledger",
        ));
    }
    Ok(())
}

pub fn validate_pillar_integration_task_ownership(
    opening: &ResearchPillarOpening,
    implementation_tasks: &[PillarImplementationTaskCoverage],
) -> Result<(), PillarDomainError> {
    validate_pillar_opening(opening)?;
    if !opening.integration_contract.integration_required {
        return Ok(());
    }

    let requirement_pillars = opening
        .pillars
        .iter()
        .flat_map(|pillar| {
            pillar
                .requirement_ids
                .iter()
                .map(move |requirement_id| (requirement_id.as_str(), pillar.id.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut integration_tasks = Vec::new();

    for task in implementation_tasks {
        require_text("pillar implementation task id", &task.task_id)?;
        let mut covered_pillars = BTreeSet::new();
        for requirement_id in &task.requirement_ids {
            let pillar_id = requirement_pillars.get(requirement_id.as_str()).ok_or_else(|| {
                PillarDomainError::new(
                    "pillar_task_requirement_unknown",
                    format!(
                        "implementation task {:?} references requirement {:?} outside the pillar opening",
                        task.task_id, requirement_id
                    ),
                )
            })?;
            covered_pillars.insert(*pillar_id);
        }

        match covered_pillars.len() {
            0 => {
                return Err(PillarDomainError::new(
                    "pillar_task_coverage_empty",
                    format!(
                        "implementation task {:?} must cover exactly one pillar or be the single integration task",
                        task.task_id
                    ),
                ));
            }
            1 => {
                if task.is_validated_strongest_terminal {
                    return Err(PillarDomainError::new(
                        "pillar_integration_terminal_not_cross_pillar",
                        format!(
                            "validated strongest terminal {:?} does not integrate multiple pillars",
                            task.task_id
                        ),
                    ));
                }
            }
            _ => integration_tasks.push(task),
        }
    }

    if integration_tasks.len() != 1 {
        return Err(PillarDomainError::new(
            "pillar_integration_task_count",
            format!(
                "pillar integration requires exactly one multi-pillar implementation task, found {}",
                integration_tasks.len()
            ),
        ));
    }
    if !integration_tasks[0].is_validated_strongest_terminal {
        return Err(PillarDomainError::new(
            "pillar_integration_task_not_strongest_terminal",
            format!(
                "multi-pillar implementation task {:?} is not the validated strongest-node terminal",
                integration_tasks[0].task_id
            ),
        ));
    }
    Ok(())
}

pub fn evidence_source_digest(text: &str) -> String {
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in Sha256::digest(text.as_bytes()) {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

pub fn authored_section_fallback(
    authored_spec: &str,
    integration_owner: &str,
) -> Result<ResearchPillarOpening, PillarDomainError> {
    require_text("authored specification", authored_spec)?;
    require_text("integration contract owner", integration_owner)?;

    let sections = authored_sections(authored_spec);
    if sections.is_empty() {
        return Err(PillarDomainError::new(
            "authored_sections_empty",
            "the authored specification contains no meaningful sections",
        ));
    }

    let requirements = sections
        .iter()
        .enumerate()
        .map(|(index, section)| AuthoredRequirement {
            id: format!("requirement-{:03}", index + 1),
            text: section.clone(),
            critical: false,
        })
        .collect::<Vec<_>>();
    let pillars = requirements
        .iter()
        .enumerate()
        .map(|(index, requirement)| ResearchPillar {
            id: format!("pillar-{:03}", index + 1),
            title: first_line(&requirement.text),
            objective: format!("Specify and implement {}", requirement.text),
            requirement_ids: vec![requirement.id.clone()],
            dependencies: Vec::new(),
            research_questions: vec![format!(
                "What exact behavior and constraints satisfy {}?",
                requirement.id
            )],
            acceptance_criteria: vec![format!(
                "The delivered behavior demonstrably satisfies {}",
                requirement.id
            )],
            exclusions: vec!["Work owned by every other pillar".to_string()],
        })
        .collect::<Vec<_>>();
    let integration_required =
        pillars.len() > 1 && authored_requirements_require_integration(&requirements);
    let opening = ResearchPillarOpening {
        requirements,
        pillars,
        integration_contract: IntegrationContract {
            owner: integration_owner.trim().to_string(),
            integration_required,
            objective: "Integrate pillar outputs without transferring their exclusive ownership"
                .to_string(),
            interface_invariants: vec![
                "Shared interfaces have one integration owner".to_string(),
                "Pillar-owned implementation remains disjoint".to_string(),
            ],
            acceptance_criteria: vec![
                "All pillar outputs compose into one runnable result".to_string()
            ],
        },
    };
    validate_pillar_opening(&opening)?;
    Ok(opening)
}

pub fn compile_pillar_report(
    opening: &ResearchPillarOpening,
    draft: PillarReportDraft,
) -> Result<CompiledPillarReport, PillarDomainError> {
    compile_pillar_report_with_sources(opening, &[], draft)
}

pub fn compile_pillar_report_with_sources(
    opening: &ResearchPillarOpening,
    source_sections: &[EvidenceSourceSection],
    draft: PillarReportDraft,
) -> Result<CompiledPillarReport, PillarDomainError> {
    validate_pillar_opening(opening)?;
    let pillar = opening
        .pillars
        .iter()
        .find(|pillar| pillar.id == draft.pillar_id)
        .ok_or_else(|| {
            PillarDomainError::new(
                "report_pillar_unknown",
                format!("report refers to unknown pillar {:?}", draft.pillar_id),
            )
        })?;

    require_nonempty_texts("report acceptance tests", &draft.acceptance_tests)?;
    require_nonempty_texts("report exclusions", &draft.exclusions)?;
    for uncertainty in &draft.unresolved_uncertainties {
        require_text("report unresolved uncertainty", uncertainty)?;
    }
    for interface in &draft.interfaces {
        require_text("report interface", interface)?;
    }

    let owned_requirements = pillar
        .requirement_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let evidence_corpus = evidence_corpus(opening, source_sections)?;
    let mut evidenced_requirements = BTreeSet::new();
    let mut invalid_provenance = false;
    let mut has_unresolved_claim = false;
    let mut claims = Vec::with_capacity(draft.claims.len());

    for claim in draft.claims {
        require_text("research claim statement", &claim.statement)?;
        if !owned_requirements.contains(claim.requirement_id.as_str()) {
            return Err(PillarDomainError::new(
                "claim_requirement_not_owned",
                format!(
                    "pillar {:?} does not own claim requirement {:?}",
                    pillar.id, claim.requirement_id
                ),
            ));
        }
        evidenced_requirements.insert(claim.requirement_id.clone());
        let provenance = match_provenance(&evidence_corpus, &claim);
        let provenance_valid = provenance.is_valid()
            && (!matches!(claim.reported_class, EvidenceClass::Proven)
                || matches!(provenance, ProvenanceMatch::Unique { .. }));
        invalid_provenance |= !provenance_valid;
        let effective_class = match claim.reported_class {
            EvidenceClass::Proven if provenance_valid => EvidenceClass::Proven,
            EvidenceClass::Proven => EvidenceClass::Supported,
            class => class,
        };
        has_unresolved_claim |= effective_class == EvidenceClass::Unresolved;
        claims.push(CompiledResearchClaim {
            requirement_id: claim.requirement_id,
            statement: claim.statement,
            reported_class: claim.reported_class,
            effective_class,
            provenance,
        });
    }

    if !draft.unresolved_uncertainties.is_empty() && !has_unresolved_claim {
        return Err(PillarDomainError::new(
            "unresolved_uncertainty_without_unresolved_claim",
            format!(
                "pillar {:?} reports unresolved uncertainty without binding it to an effectively Unresolved owned claim",
                pillar.id
            ),
        ));
    }

    let missing_requirement_ids = pillar
        .requirement_ids
        .iter()
        .filter(|id| !evidenced_requirements.contains(id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let effective_confidence = if draft.reported_confidence == Confidence::Low
        || !draft.unresolved_uncertainties.is_empty()
        || !missing_requirement_ids.is_empty()
        || invalid_provenance
        || has_unresolved_claim
    {
        Confidence::Low
    } else {
        Confidence::High
    };

    Ok(CompiledPillarReport {
        pillar_id: draft.pillar_id,
        reported_confidence: draft.reported_confidence,
        effective_confidence,
        claims,
        missing_requirement_ids,
        unresolved_uncertainties: draft.unresolved_uncertainties,
        acceptance_tests: draft.acceptance_tests,
        interfaces: draft.interfaces,
        exclusions: draft.exclusions,
    })
}

pub fn render_synthesis_input(reports: &[CompiledPillarReport], max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut reports = reports.iter().collect::<Vec<_>>();
    reports.sort_by_key(|report| report.pillar_id.as_str());

    const HEADER: &str = "PILLAR RESEARCH SYNTHESIS";
    const OWNER_SPEC_HEADER: &str = "[OWNER SPECS: INTERFACES AND ACCEPTANCE]";
    let essential_line_count = reports.len().saturating_mul(2);
    let essential_overhead = HEADER
        .len()
        .saturating_add(OWNER_SPEC_HEADER.len())
        .saturating_add(essential_line_count.saturating_add(1));
    let essential_line_budget = if essential_line_count == 0 {
        0
    } else {
        max_chars
            .saturating_sub(essential_overhead)
            .checked_div(essential_line_count)
            .unwrap_or(0)
            .min(4_096)
    };

    let mut lines = vec![HEADER.to_string(), OWNER_SPEC_HEADER.to_string()];
    for report in &reports {
        lines.push(bounded_synthesis_list_line(
            &format!("- {}/interfaces: ", report.pillar_id),
            &report.interfaces,
            essential_line_budget,
        ));
        lines.push(bounded_synthesis_list_line(
            &format!("- {}/acceptance-tests: ", report.pillar_id),
            &report.acceptance_tests,
            essential_line_budget,
        ));
    }

    let low_confidence = reports
        .iter()
        .copied()
        .filter(|report| report.effective_confidence == Confidence::Low)
        .collect::<Vec<_>>();
    if !low_confidence.is_empty() {
        lines.push("[LOW CONFIDENCE]".to_string());
        for report in low_confidence {
            let mut reasons = Vec::new();
            if report.reported_confidence == Confidence::Low {
                reasons.push("reported low".to_string());
            }
            if !report.missing_requirement_ids.is_empty() {
                reasons.push(format!(
                    "missing {}",
                    report.missing_requirement_ids.join(",")
                ));
            }
            if !report.unresolved_uncertainties.is_empty() {
                reasons.push(format!(
                    "{} unresolved item(s)",
                    report.unresolved_uncertainties.len()
                ));
            }
            if reasons.is_empty() {
                reasons.push("evidence provenance not verified".to_string());
            }
            lines.push(bounded_synthesis_line(
                &format!("- {}: {}", report.pillar_id, reasons.join("; ")),
                2_048,
            ));
        }
    }

    lines.push("[OWNER CONSTRAINTS AND UNRESOLVED RATIONALES]".to_string());
    for report in &reports {
        for exclusion in &report.exclusions {
            lines.push(bounded_synthesis_line(
                &format!(
                    "- {}/exclusion: {}",
                    report.pillar_id,
                    canonical_whitespace(exclusion)
                ),
                2_048,
            ));
        }
        for uncertainty in &report.unresolved_uncertainties {
            lines.push(bounded_synthesis_line(
                &format!(
                    "- {}/unresolved-rationale: {}",
                    report.pillar_id,
                    canonical_whitespace(uncertainty)
                ),
                2_048,
            ));
        }
        if !report.missing_requirement_ids.is_empty() {
            lines.push(bounded_synthesis_line(
                &format!(
                    "- {}/missing-requirements: {}",
                    report.pillar_id,
                    report.missing_requirement_ids.join(", ")
                ),
                2_048,
            ));
        }
    }

    let mut grouped = BTreeMap::<EvidenceClass, Vec<(&str, &CompiledResearchClaim)>>::new();
    for report in &reports {
        for claim in &report.claims {
            grouped
                .entry(claim.effective_class)
                .or_default()
                .push((&report.pillar_id, claim));
        }
    }
    for claims in grouped.values_mut() {
        claims.sort_by(|(left_pillar, left), (right_pillar, right)| {
            (
                left_pillar,
                left.requirement_id.as_str(),
                left.statement.as_str(),
            )
                .cmp(&(
                    right_pillar,
                    right.requirement_id.as_str(),
                    right.statement.as_str(),
                ))
        });
    }

    for class in [
        EvidenceClass::Unresolved,
        EvidenceClass::Proven,
        EvidenceClass::Supported,
    ] {
        let Some(class_claims) = grouped.get(&class) else {
            continue;
        };
        lines.push(format!("[{}]", evidence_label(class)));
        for (pillar_id, claim) in class_claims {
            lines.push(bounded_synthesis_line(
                &format!(
                    "- {pillar_id}/{}: {}",
                    claim.requirement_id,
                    canonical_whitespace(&claim.statement)
                ),
                2_048,
            ));
        }
    }

    bound_lines(lines, max_chars)
}

fn match_provenance(
    evidence_corpus: &[EvidenceCorpusSection<'_>],
    claim: &ResearchClaimDraft,
) -> ProvenanceMatch {
    let Some(raw_quote) = claim.source_quote.as_deref() else {
        return if claim.reported_class == EvidenceClass::Proven {
            ProvenanceMatch::MissingQuote
        } else {
            ProvenanceMatch::NotClaimed
        };
    };
    let quote = canonical_whitespace(raw_quote);
    if quote.is_empty() {
        return ProvenanceMatch::MissingQuote;
    }

    let mut matches = Vec::new();
    for section in evidence_corpus {
        if claim
            .source_section_id
            .as_deref()
            .is_some_and(|section_id| section_id != section.id)
        {
            continue;
        }
        let source = canonical_whitespace(section.text);
        for _ in source.match_indices(&quote) {
            matches.push((section.id.to_string(), section.requirement_id.to_string()));
        }
    }
    if matches.is_empty() {
        return ProvenanceMatch::NotFound;
    }
    if matches.len() > 1 {
        return ProvenanceMatch::Ambiguous;
    }

    let (matched_section_id, matched_requirement_id) =
        matches.pop().expect("one match was established");
    if matched_requirement_id != claim.requirement_id {
        return ProvenanceMatch::RequirementMismatch {
            matched_requirement_id,
        };
    }
    if claim
        .source_section_id
        .as_deref()
        .is_some_and(|section_id| section_id != matched_section_id)
    {
        return ProvenanceMatch::SectionMismatch {
            matched_requirement_id,
        };
    }
    let matched = evidence_corpus
        .iter()
        .find(|section| section.id == matched_section_id)
        .expect("matched evidence section remains in the corpus");
    ProvenanceMatch::Unique {
        section_id: matched_section_id,
        requirement_id: matched_requirement_id,
        content_sha256: matched.content_sha256.to_string(),
        authority: matched.authority,
    }
}

struct EvidenceCorpusSection<'a> {
    id: &'a str,
    requirement_id: &'a str,
    text: &'a str,
    content_sha256: &'a str,
    authority: EvidenceSourceAuthority,
}

fn evidence_corpus<'a>(
    opening: &'a ResearchPillarOpening,
    source_sections: &'a [EvidenceSourceSection],
) -> Result<Vec<EvidenceCorpusSection<'a>>, PillarDomainError> {
    let requirement_ids = opening
        .requirements
        .iter()
        .map(|requirement| requirement.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut section_ids = BTreeSet::new();
    let mut corpus = Vec::with_capacity(source_sections.len());
    for requirement in &opening.requirements {
        section_ids.insert(requirement.id.as_str());
    }
    for section in source_sections {
        require_text("evidence source section id", &section.id)?;
        require_text("evidence source requirement id", &section.requirement_id)?;
        require_text("evidence source text", &section.text)?;
        require_text("evidence source digest", &section.content_sha256)?;
        if !section_ids.insert(&section.id) {
            return Err(PillarDomainError::new(
                "evidence_section_id_duplicate",
                format!("evidence section id {:?} is duplicated", section.id),
            ));
        }
        if !requirement_ids.contains(section.requirement_id.as_str()) {
            return Err(PillarDomainError::new(
                "evidence_requirement_unknown",
                format!(
                    "evidence section {:?} maps to unknown requirement {:?}",
                    section.id, section.requirement_id
                ),
            ));
        }
        if evidence_source_digest(&section.text) != section.content_sha256 {
            return Err(PillarDomainError::new(
                "evidence_source_digest_mismatch",
                format!(
                    "evidence section {:?} content does not match its engine receipt digest",
                    section.id
                ),
            ));
        }
        corpus.push(EvidenceCorpusSection {
            id: &section.id,
            requirement_id: &section.requirement_id,
            text: &section.text,
            content_sha256: &section.content_sha256,
            authority: section.authority,
        });
    }
    Ok(corpus)
}

fn validate_acyclic_dependencies(pillars: &[ResearchPillar]) -> Result<(), PillarDomainError> {
    fn visit<'a>(
        id: &'a str,
        by_id: &BTreeMap<&'a str, &'a ResearchPillar>,
        visiting: &mut BTreeSet<&'a str>,
        visited: &mut BTreeSet<&'a str>,
    ) -> Result<(), PillarDomainError> {
        if visited.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            return Err(PillarDomainError::new(
                "pillar_dependency_cycle",
                format!("pillar dependency cycle reaches {id:?}"),
            ));
        }
        for dependency in &by_id[id].dependencies {
            visit(dependency, by_id, visiting, visited)?;
        }
        visiting.remove(id);
        visited.insert(id);
        Ok(())
    }

    let by_id = pillars
        .iter()
        .map(|pillar| (pillar.id.as_str(), pillar))
        .collect::<BTreeMap<_, _>>();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for id in by_id.keys() {
        visit(id, &by_id, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn authored_sections(spec: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut paragraph = Vec::new();
    let flush = |paragraph: &mut Vec<String>, sections: &mut Vec<String>| {
        if !paragraph.is_empty() {
            sections.push(canonical_whitespace(&paragraph.join(" ")));
            paragraph.clear();
        }
    };

    for raw_line in spec.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            flush(&mut paragraph, &mut sections);
            continue;
        }
        if is_list_item(line) {
            flush(&mut paragraph, &mut sections);
            sections.push(canonical_whitespace(strip_list_marker(line)));
        } else {
            paragraph.push(line.to_string());
        }
    }
    flush(&mut paragraph, &mut sections);
    sections.retain(|section| !section.is_empty());
    sections
}

fn is_list_item(line: &str) -> bool {
    if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
        return true;
    }
    let digit_count = line.chars().take_while(|ch| ch.is_ascii_digit()).count();
    digit_count > 0 && line[digit_count..].starts_with(". ")
}

fn strip_list_marker(line: &str) -> &str {
    if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
        return &line[2..];
    }
    let digit_count = line.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count > 0 && line[digit_count..].starts_with(". ") {
        &line[digit_count + 2..]
    } else {
        line
    }
}

fn first_line(text: &str) -> String {
    const MAX_TITLE_CHARS: usize = 80;
    let canonical = canonical_whitespace(text);
    let mut chars = canonical.chars();
    let title = chars.by_ref().take(MAX_TITLE_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}

fn canonical_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn require_text(label: &str, text: &str) -> Result<(), PillarDomainError> {
    if text.trim().is_empty() {
        Err(PillarDomainError::new(
            "required_text_empty",
            format!("{label} must not be empty"),
        ))
    } else {
        Ok(())
    }
}

fn require_nonempty_texts(label: &str, values: &[String]) -> Result<(), PillarDomainError> {
    if values.is_empty() {
        return Err(PillarDomainError::new(
            "required_list_empty",
            format!("{label} must contain at least one item"),
        ));
    }
    for value in values {
        require_text(label, value)?;
    }
    Ok(())
}

fn evidence_label(class: EvidenceClass) -> &'static str {
    match class {
        EvidenceClass::Proven => "PROVEN",
        EvidenceClass::Supported => "SUPPORTED",
        EvidenceClass::Unresolved => "UNRESOLVED",
    }
}

fn bounded_synthesis_list_line(prefix: &str, values: &[String], max_bytes: usize) -> String {
    let body = if values.is_empty() {
        "(none declared)".to_string()
    } else {
        values
            .iter()
            .map(|value| canonical_whitespace(value))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    bounded_synthesis_line(&format!("{prefix}{body}"), max_bytes)
}

fn bounded_synthesis_line(line: &str, max_bytes: usize) -> String {
    if line.len() <= max_bytes {
        return line.to_string();
    }
    const MARKER: &str = "…";
    if max_bytes < MARKER.len() {
        return String::new();
    }
    let mut end = max_bytes - MARKER.len();
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &line[..end], MARKER)
}

fn bound_lines(lines: Vec<String>, max_chars: usize) -> String {
    let mut rendered = String::new();
    for line in lines {
        let separator = usize::from(!rendered.is_empty());
        if rendered.len() + separator + line.len() > max_chars {
            let marker = "[truncated]";
            let separator = usize::from(!rendered.is_empty());
            if rendered.len() + separator + marker.len() <= max_chars {
                if !rendered.is_empty() {
                    rendered.push('\n');
                }
                rendered.push_str(marker);
            }
            break;
        }
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        rendered.push_str(&line);
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirement(id: &str, text: &str) -> AuthoredRequirement {
        AuthoredRequirement {
            id: id.to_string(),
            text: text.to_string(),
            critical: false,
        }
    }

    fn pillar(id: &str, requirement_ids: &[&str]) -> ResearchPillar {
        ResearchPillar {
            id: id.to_string(),
            title: format!("Title {id}"),
            objective: format!("Objective {id}"),
            requirement_ids: requirement_ids
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            dependencies: Vec::new(),
            research_questions: vec![format!("Question {id}")],
            acceptance_criteria: vec![format!("Acceptance {id}")],
            exclusions: vec![format!("Exclusion {id}")],
        }
    }

    fn opening() -> ResearchPillarOpening {
        ResearchPillarOpening {
            requirements: vec![
                requirement(
                    "req-ui",
                    "The dashboard must render a saturated status card.",
                ),
                requirement(
                    "req-api",
                    "The service must persist each status transition exactly once.",
                ),
            ],
            pillars: vec![pillar("ui", &["req-ui"]), pillar("api", &["req-api"])],
            integration_contract: IntegrationContract {
                owner: "planner".to_string(),
                integration_required: true,
                objective: "Compose UI and service".to_string(),
                interface_invariants: vec!["One status schema".to_string()],
                acceptance_criteria: vec!["Runnable end to end".to_string()],
            },
        }
    }

    fn report(claims: Vec<ResearchClaimDraft>) -> PillarReportDraft {
        PillarReportDraft {
            pillar_id: "ui".to_string(),
            reported_confidence: Confidence::High,
            claims,
            unresolved_uncertainties: Vec::new(),
            acceptance_tests: vec!["Render the dashboard".to_string()],
            interfaces: vec!["StatusView".to_string()],
            exclusions: vec!["Persistence implementation".to_string()],
        }
    }

    #[test]
    fn exact_requirement_cover_rejects_overlap_and_omission() {
        let mut overlapping = opening();
        overlapping.pillars[1]
            .requirement_ids
            .push("req-ui".to_string());
        assert_eq!(
            validate_pillar_opening(&overlapping).unwrap_err().code,
            "requirement_overlap"
        );

        let mut omitted = opening();
        omitted.pillars[1].requirement_ids = vec!["req-ui".to_string()];
        assert_eq!(
            validate_pillar_opening(&omitted).unwrap_err().code,
            "requirement_unowned"
        );
        omitted.pillars[1].requirement_ids.clear();
        assert_eq!(
            validate_pillar_opening(&omitted).unwrap_err().code,
            "pillar_requirements_empty"
        );

        let mut strictly_omitted = opening();
        strictly_omitted
            .requirements
            .push(requirement("req-extra", "Extra behavior"));
        assert_eq!(
            validate_pillar_opening(&strictly_omitted).unwrap_err().code,
            "requirement_unowned"
        );
    }

    #[test]
    fn authored_section_fallback_preserves_paragraphs_and_list_items_once() {
        let fallback = authored_section_fallback(
            "Build a runnable service.\n\n- Persist transitions\n- Render status\n\nKeep errors visible.",
            "integrator",
        )
        .unwrap();

        assert_eq!(fallback.requirements.len(), 4);
        assert_eq!(fallback.pillars.len(), 4);
        assert_eq!(fallback.integration_contract.owner, "integrator");
        assert_eq!(
            fallback
                .pillars
                .iter()
                .flat_map(|pillar| pillar.requirement_ids.iter())
                .collect::<Vec<_>>(),
            fallback
                .requirements
                .iter()
                .map(|requirement| &requirement.id)
                .collect::<Vec<_>>()
        );
        validate_pillar_opening(&fallback).unwrap();
    }

    #[test]
    fn authored_text_cannot_be_promoted_to_proven() {
        let compiled = compile_pillar_report(
            &opening(),
            report(vec![ResearchClaimDraft {
                requirement_id: "req-ui".to_string(),
                statement: "The dashboard renders the required card".to_string(),
                reported_class: EvidenceClass::Proven,
                source_section_id: Some("req-ui".to_string()),
                source_quote: Some("dashboard   must\nrender a saturated status card".to_string()),
            }]),
        )
        .unwrap();

        assert_eq!(compiled.effective_confidence, Confidence::Low);
        assert_eq!(compiled.claims[0].effective_class, EvidenceClass::Supported);
        assert_eq!(compiled.claims[0].provenance, ProvenanceMatch::NotFound);
    }

    #[test]
    fn uniquely_mapped_source_section_can_prove_a_claim_without_rendering_its_body() {
        let source_body = "The implementation contract requires one atomic status transition.";
        let compiled = compile_pillar_report_with_sources(
            &opening(),
            &[EvidenceSourceSection {
                id: "source-api-contract".to_string(),
                requirement_id: "req-ui".to_string(),
                text: source_body.to_string(),
                content_sha256: evidence_source_digest(source_body),
                authority: EvidenceSourceAuthority::EngineReceipt,
            }],
            report(vec![ResearchClaimDraft {
                requirement_id: "req-ui".to_string(),
                statement: "The UI observes an atomic transition".to_string(),
                reported_class: EvidenceClass::Proven,
                source_section_id: Some("source-api-contract".to_string()),
                source_quote: Some("requires one\natomic status transition".to_string()),
            }]),
        )
        .unwrap();

        assert_eq!(compiled.effective_confidence, Confidence::High);
        assert!(matches!(
            compiled.claims[0].provenance,
            ProvenanceMatch::Unique { ref section_id, .. }
                if section_id == "source-api-contract"
        ));
        assert!(!render_synthesis_input(&[compiled], 1_000).contains(source_body));
    }

    #[test]
    fn quote_repeated_in_one_engine_receipt_is_ambiguous() {
        let source_body = "render status, then render status again.";
        let compiled = compile_pillar_report_with_sources(
            &opening(),
            &[EvidenceSourceSection {
                id: "source-repeated".to_string(),
                requirement_id: "req-ui".to_string(),
                text: source_body.to_string(),
                content_sha256: evidence_source_digest(source_body),
                authority: EvidenceSourceAuthority::EngineReceipt,
            }],
            report(vec![ResearchClaimDraft {
                requirement_id: "req-ui".to_string(),
                statement: "Status is rendered".to_string(),
                reported_class: EvidenceClass::Proven,
                source_section_id: Some("source-repeated".to_string()),
                source_quote: Some("render status".to_string()),
            }]),
        )
        .unwrap();

        assert_eq!(compiled.effective_confidence, Confidence::Low);
        assert_eq!(compiled.claims[0].effective_class, EvidenceClass::Supported);
        assert_eq!(compiled.claims[0].provenance, ProvenanceMatch::Ambiguous);
    }

    #[test]
    fn reported_low_missing_evidence_and_invalid_provenance_trigger_retry() {
        let mut draft = report(vec![ResearchClaimDraft {
            requirement_id: "req-ui".to_string(),
            statement: "Unsupported certainty".to_string(),
            reported_class: EvidenceClass::Proven,
            source_section_id: Some("req-ui".to_string()),
            source_quote: Some("words absent from the authored requirement".to_string()),
        }]);
        draft.reported_confidence = Confidence::Low;
        let compiled = compile_pillar_report(&opening(), draft).unwrap();
        assert!(compiled.needs_focused_retry());

        let mut missing = opening();
        missing.pillars[0]
            .requirement_ids
            .push("req-api".to_string());
        missing.pillars.pop();
        let compiled = compile_pillar_report(
            &missing,
            report(vec![ResearchClaimDraft {
                requirement_id: "req-ui".to_string(),
                statement: "UI evidence only".to_string(),
                reported_class: EvidenceClass::Supported,
                source_section_id: None,
                source_quote: None,
            }]),
        )
        .unwrap();
        assert_eq!(compiled.missing_requirement_ids, vec!["req-api"]);
        assert!(compiled.needs_focused_retry());
    }

    #[test]
    fn synthesis_render_is_bounded_grouped_and_omits_quotes() {
        let secret_receipt_body = "PRIVATE RECEIPT BODY";
        let compiled = compile_pillar_report(
            &opening(),
            report(vec![ResearchClaimDraft {
                requirement_id: "req-ui".to_string(),
                statement: "Visible status card".to_string(),
                reported_class: EvidenceClass::Proven,
                source_section_id: Some("req-ui".to_string()),
                source_quote: Some(secret_receipt_body.to_string()),
            }]),
        )
        .unwrap();
        let rendered = render_synthesis_input(&[compiled], 1_000);
        assert!(rendered.len() <= 1_000);
        assert!(rendered.contains("ui/interfaces: StatusView"));
        assert!(rendered.contains("ui/acceptance-tests: Render the dashboard"));
        assert!(rendered.contains("ui/exclusion: Persistence implementation"));
        assert!(rendered.contains("[SUPPORTED]"));
        assert!(!rendered.contains(secret_receipt_body));
    }

    #[test]
    fn tight_synthesis_bound_prioritizes_every_owner_interface_and_acceptance_test() {
        let source_body = "PRIVATE SOURCE BODY: the API status transition is atomic.";
        let api_report = compile_pillar_report_with_sources(
            &opening(),
            &[EvidenceSourceSection {
                id: "source-api".to_string(),
                requirement_id: "req-api".to_string(),
                text: source_body.to_string(),
                content_sha256: evidence_source_digest(source_body),
                authority: EvidenceSourceAuthority::EngineReceipt,
            }],
            PillarReportDraft {
                pillar_id: "api".to_string(),
                reported_confidence: Confidence::High,
                claims: vec![ResearchClaimDraft {
                    requirement_id: "req-api".to_string(),
                    statement: format!("Atomic API contract {}", "bulk-claim ".repeat(100)),
                    reported_class: EvidenceClass::Proven,
                    source_section_id: Some("source-api".to_string()),
                    source_quote: Some("API status transition is atomic".to_string()),
                }],
                unresolved_uncertainties: Vec::new(),
                acceptance_tests: vec!["run api smoke".to_string()],
                interfaces: vec!["StatusApi".to_string()],
                exclusions: vec!["UI rendering".to_string()],
            },
        )
        .unwrap();
        let compiled = [
            api_report,
            CompiledPillarReport {
                pillar_id: "ui".to_string(),
                reported_confidence: Confidence::High,
                effective_confidence: Confidence::High,
                claims: vec![CompiledResearchClaim {
                    requirement_id: "req-ui".to_string(),
                    statement: "A bulk UI claim that should lose to the owner specification"
                        .to_string(),
                    reported_class: EvidenceClass::Supported,
                    effective_class: EvidenceClass::Supported,
                    provenance: ProvenanceMatch::NotClaimed,
                }],
                missing_requirement_ids: Vec::new(),
                unresolved_uncertainties: Vec::new(),
                acceptance_tests: vec!["run ui smoke".to_string()],
                interfaces: vec!["StatusView".to_string()],
                exclusions: vec!["Persistence".to_string()],
            },
        ];

        let rendered = render_synthesis_input(&compiled, 250);
        assert!(rendered.len() <= 250);
        for essential in [
            "api/interfaces: StatusApi",
            "api/acceptance-tests: run api smoke",
            "ui/interfaces: StatusView",
            "ui/acceptance-tests: run ui smoke",
        ] {
            assert!(
                rendered.contains(essential),
                "missing {essential}: {rendered}"
            );
        }
        assert!(!rendered.contains(source_body));
        assert!(!rendered.contains("bulk UI claim"));
    }

    #[test]
    fn bounded_synthesis_keeps_uncertainty_before_positive_claims() {
        let mut draft = report(vec![ResearchClaimDraft {
            requirement_id: "req-ui".to_string(),
            statement: "Unresolved rendering contract".to_string(),
            reported_class: EvidenceClass::Unresolved,
            source_section_id: None,
            source_quote: None,
        }]);
        draft.reported_confidence = Confidence::Low;
        draft.unresolved_uncertainties = vec!["Renderer availability is unknown".to_string()];
        let compiled = compile_pillar_report(&opening(), draft).unwrap();
        let rendered = render_synthesis_input(&[compiled], 700);
        assert!(rendered.contains("[LOW CONFIDENCE]"));
        assert!(rendered.contains("[UNRESOLVED]"));
        assert!(!rendered.contains("[SUPPORTED]"));
    }

    #[test]
    fn unresolved_uncertainty_requires_an_owned_unresolved_claim() {
        let mut draft = report(vec![ResearchClaimDraft {
            requirement_id: "req-ui".to_string(),
            statement: "The renderer interface is supported".to_string(),
            reported_class: EvidenceClass::Supported,
            source_section_id: None,
            source_quote: None,
        }]);
        draft.reported_confidence = Confidence::Low;
        draft.unresolved_uncertainties = vec!["Renderer availability is unknown".to_string()];

        let error = compile_pillar_report(&opening(), draft).unwrap_err();
        assert_eq!(
            error.code,
            "unresolved_uncertainty_without_unresolved_claim"
        );
    }

    #[test]
    fn engine_receipt_digest_must_match_the_verified_body() {
        let error = compile_pillar_report_with_sources(
            &opening(),
            &[EvidenceSourceSection {
                id: "source-tampered".to_string(),
                requirement_id: "req-ui".to_string(),
                text: "different bytes".to_string(),
                content_sha256: evidence_source_digest("trusted bytes"),
                authority: EvidenceSourceAuthority::EngineReceipt,
            }],
            report(vec![ResearchClaimDraft {
                requirement_id: "req-ui".to_string(),
                statement: "Claim".to_string(),
                reported_class: EvidenceClass::Proven,
                source_section_id: Some("source-tampered".to_string()),
                source_quote: Some("different bytes".to_string()),
            }]),
        )
        .unwrap_err();
        assert_eq!(error.code, "evidence_source_digest_mismatch");
    }

    #[test]
    fn dependency_cycles_are_rejected() {
        let mut opening = opening();
        opening.pillars[0].dependencies = vec!["api".to_string()];
        opening.pillars[1].dependencies = vec!["ui".to_string()];
        assert_eq!(
            validate_pillar_opening(&opening).unwrap_err().code,
            "pillar_dependency_cycle"
        );
    }

    #[test]
    fn integration_requires_one_strongest_terminal_cross_pillar_implementation() {
        let opening = opening();
        let tasks = vec![
            PillarImplementationTaskCoverage {
                task_id: "build-ui".to_string(),
                requirement_ids: vec!["req-ui".to_string()],
                is_validated_strongest_terminal: false,
            },
            PillarImplementationTaskCoverage {
                task_id: "build-api".to_string(),
                requirement_ids: vec!["req-api".to_string()],
                is_validated_strongest_terminal: false,
            },
            PillarImplementationTaskCoverage {
                task_id: "integrate".to_string(),
                requirement_ids: vec!["req-ui".to_string(), "req-api".to_string()],
                is_validated_strongest_terminal: true,
            },
        ];

        validate_pillar_integration_task_ownership(&opening, &tasks).unwrap();
    }

    #[test]
    fn integration_rejects_two_cross_pillar_implementation_tasks() {
        let opening = opening();
        let tasks = vec![
            PillarImplementationTaskCoverage {
                task_id: "integrate-modules".to_string(),
                requirement_ids: vec!["req-ui".to_string(), "req-api".to_string()],
                is_validated_strongest_terminal: false,
            },
            PillarImplementationTaskCoverage {
                task_id: "integrate-entry".to_string(),
                requirement_ids: vec!["req-ui".to_string(), "req-api".to_string()],
                is_validated_strongest_terminal: true,
            },
        ];

        assert_eq!(
            validate_pillar_integration_task_ownership(&opening, &tasks)
                .unwrap_err()
                .code,
            "pillar_integration_task_count"
        );
    }

    #[test]
    fn cross_pillar_implementation_must_be_the_validated_strongest_terminal() {
        let opening = opening();
        let tasks = vec![PillarImplementationTaskCoverage {
            task_id: "integrate".to_string(),
            requirement_ids: vec!["req-ui".to_string(), "req-api".to_string()],
            is_validated_strongest_terminal: false,
        }];

        assert_eq!(
            validate_pillar_integration_task_ownership(&opening, &tasks)
                .unwrap_err()
                .code,
            "pillar_integration_task_not_strongest_terminal"
        );
    }

    #[test]
    fn fallback_integration_is_disabled_only_by_explicit_independence() {
        let ordinary_product = vec![AuthoredRequirement {
            id: "req-cli".to_string(),
            text: "Build a CLI with import and export commands".to_string(),
            critical: false,
        }];
        assert!(authored_requirements_require_integration(&ordinary_product));

        let independent = vec![AuthoredRequirement {
            id: "req-assets".to_string(),
            text: "Produce separate deliverables with no integration required".to_string(),
            critical: false,
        }];
        assert!(!authored_requirements_require_integration(&independent));

        for text in [
            "Produce independent deliverables, then hook them into one application",
            "Keep standalone deliverables while composing them into one product",
            "Create independent outputs and integrate them into a single app",
        ] {
            assert!(
                authored_requirements_require_integration(&[AuthoredRequirement {
                    id: "req-contradictory".to_string(),
                    text: text.to_string(),
                    critical: false,
                }]),
                "explicit one-product integration must override independence wording: {text}"
            );
        }

        assert!(!authored_requirements_require_integration(&[
            AuthoredRequirement {
                id: "req-negative".to_string(),
                text: "Do not integrate these independent deliverables into one application"
                    .to_string(),
                critical: false,
            },
        ]));

        for text in [
            "Generate standalone outputs for every supported format",
            "Write separate artifacts for the client and server packages",
            "Create separate deliverables, then hook them into one application",
        ] {
            assert!(
                authored_requirements_require_integration(&[AuthoredRequirement {
                    id: "req-ambiguous".to_string(),
                    text: text.to_string(),
                    critical: false,
                }]),
                "ambiguous wording must not veto integration: {text}"
            );
        }
    }
}
