use goose_swarm::{
    parse_semantic_observation_reply, AcceptanceCriterionSnapshot, ArtifactExcerptSnapshot,
    AuthorityScope, NeutralJudgeSignal, SemanticJudgeAction, SemanticObservationSnapshotDraft,
    SemanticTraceSnapshot, SEMANTIC_OBSERVATION_PROTOCOL, SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    schema_version: u16,
    cases: Vec<CorpusCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusCase {
    id: String,
    source_fixture: Option<String>,
    expected_action: SemanticJudgeAction,
    task_id: String,
    source_revision: u64,
    goal: String,
    task_contract: String,
    acceptance: Vec<AcceptanceCriterionSnapshot>,
    allowed_finding_routes: Vec<String>,
    artifacts: Vec<ArtifactExcerptSnapshot>,
    trace: CorpusTrace,
    neutral_signals: Vec<NeutralJudgeSignal>,
    observation: Option<serde_json::Value>,
    raw_reply: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusTrace {
    recent_reasoning: String,
    recent_actions: Vec<String>,
    prior_intervention: Option<String>,
    response_to_prior_intervention: Option<String>,
}

fn load_corpus() -> Corpus {
    serde_json::from_str(include_str!(
        "fixtures/semantic-judge-observation-corpus.json"
    ))
    .expect("semantic observation corpus must remain valid JSON")
}

fn snapshot(case: &CorpusCase) -> goose_swarm::SealedSemanticObservationSnapshot {
    SemanticObservationSnapshotDraft {
        schema_version: SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
        authority_scope: AuthorityScope::new("semantic-observation-corpus", "judge"),
        phase_epoch: 0,
        task_id: case.task_id.clone(),
        attempt: 0,
        source_revision: case.source_revision,
        contract_version: "contract-v1".into(),
        artifact_version: "artifact-v1".into(),
        goal: case.goal.clone(),
        task_contract: case.task_contract.clone(),
        acceptance_oracle: case.acceptance.clone(),
        dependency_contract_versions: BTreeMap::new(),
        sibling_contract_versions: BTreeMap::new(),
        allowed_finding_routes: case.allowed_finding_routes.clone(),
        artifacts: case.artifacts.clone(),
        trace: SemanticTraceSnapshot {
            sequence: case.source_revision,
            recent_reasoning: case.trace.recent_reasoning.clone(),
            recent_actions: case.trace.recent_actions.clone(),
            prior_intervention: case.trace.prior_intervention.clone(),
            response_to_prior_intervention: case.trace.response_to_prior_intervention.clone(),
        },
        neutral_signals: case.neutral_signals.clone(),
    }
    .seal()
    .expect("corpus snapshot must seal")
}

#[test]
fn corpus_covers_every_registered_engine_four_case_with_exact_actions() {
    let corpus = load_corpus();
    assert_eq!(corpus.schema_version, 1);
    let required: BTreeSet<&str> = BTreeSet::from([
        "f924-long-period-recurrence",
        "slow-healthy-reasoning",
        "tool-payload-silence",
        "advancing-but-wrong-work",
        "genuinely-complete-work",
        "uncertain-missing-evidence",
        "historical-fable-task-position-correction",
        "f163-flat-counters-before-write",
        "r1-parser-negative-keyword-false-positive",
    ]);
    let actual: BTreeSet<&str> = corpus.cases.iter().map(|case| case.id.as_str()).collect();
    assert_eq!(actual, required);

    for case in &corpus.cases {
        let snapshot = snapshot(case);
        let raw = match (&case.raw_reply, &case.observation) {
            (Some(raw), None) => raw.clone(),
            (None, Some(observation)) => serde_json::json!({
                "protocol": SEMANTIC_OBSERVATION_PROTOCOL,
                "snapshot_hash": snapshot.snapshot_hash(),
                "observation": observation,
            })
            .to_string(),
            _ => panic!("{} must define exactly one reply form", case.id),
        };
        let parsed = parse_semantic_observation_reply(&snapshot, &raw);
        assert_eq!(
            parsed.action(),
            case.expected_action,
            "corpus case {} parsed unexpectedly: {parsed:?}",
            case.id
        );
    }
}

#[test]
fn f924_case_is_bound_to_the_corrected_incident_shape_not_the_old_percentage() {
    let corpus = load_corpus();
    let case = corpus
        .cases
        .iter()
        .find(|case| case.id == "f924-long-period-recurrence")
        .unwrap();
    assert_eq!(
        case.source_fixture.as_deref(),
        Some("evals/swarm-bench/fixtures/f924-detail-tail-shape.json")
    );
    let shape: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evals/swarm-bench/fixtures/f924-detail-tail-shape.json"
    ))
    .unwrap();
    assert_eq!(shape["total_detail_calls"], 27);
    assert_eq!(shape["completed_before_tail"], 26);
    assert_eq!(shape["tail_reasoning_chars_at_interruption"], 203447);
    assert_eq!(shape["physical_idle_interval_proven"], false);
    let signal = case
        .neutral_signals
        .iter()
        .find(|signal| signal.source_id == "signal:f924-recurrence")
        .unwrap();
    assert_eq!(signal.value["repeat_share"], 0.4033);
    assert_ne!(signal.value["repeat_share"], 0.6758);
}

#[test]
fn adversarial_protocol_mutations_all_abstain() {
    let corpus = load_corpus();
    let case = corpus
        .cases
        .iter()
        .find(|case| case.id == "historical-fable-task-position-correction")
        .unwrap();
    let snapshot = snapshot(case);
    let valid = serde_json::json!({
        "protocol": SEMANTIC_OBSERVATION_PROTOCOL,
        "snapshot_hash": snapshot.snapshot_hash(),
        "observation": case.observation.clone().unwrap(),
    });
    let mut unknown_action = valid.clone();
    unknown_action["observation"]["action"] = serde_json::json!("KILL");
    let mut stale = valid.clone();
    stale["snapshot_hash"] = serde_json::json!("sha256:stale");
    let mut unknown_field = valid.clone();
    unknown_field["observation"]["confidence"] = serde_json::json!("HIGH");
    let mut uncited = valid.clone();
    uncited["observation"]["evidence"][0]["source_id"] =
        serde_json::json!("signal:not-in-snapshot");
    for raw in [
        "VERDICT|HIGH|STOP and kill this worker".to_string(),
        format!("```json\n{}\n```", valid),
        unknown_action.to_string(),
        stale.to_string(),
        unknown_field.to_string(),
        uncited.to_string(),
    ] {
        assert_eq!(
            parse_semantic_observation_reply(&snapshot, &raw).action(),
            SemanticJudgeAction::Abstain,
            "mutation must fail closed: {raw}"
        );
    }
}
