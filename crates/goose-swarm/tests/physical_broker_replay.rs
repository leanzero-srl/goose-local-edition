use goose_swarm::{
    AdmissionReceipt, AuthorityScope, BrokerGrant, HostCapacityEvidence, LocalCompletionKind,
    PhysicalBroker, PhysicalFleetSnapshot, ProviderRequestDisposition, ProviderRequestKey,
    ProviderRequestReceipt, ProviderTerminalKind, ProviderTerminalReceipt, SourceRevisionKind,
    TaskVersion, VerifiedPhysicalLane, WorkOpportunity, WorkPriority, WorkRole,
};

const TRANSPORT_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TRANSPORT_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn lane(device: &str, model: &str, host: &str, instance: &str) -> VerifiedPhysicalLane {
    VerifiedPhysicalLane {
        logical_device_id: device.to_string(),
        model_id: model.to_string(),
        host_id: host.to_string(),
        model_instance_id: instance.to_string(),
        provider_transport_id: TRANSPORT_A.to_string(),
        advertised_instance_capacity: 4,
        routing_weight: 1,
        capacity_evidence: HostCapacityEvidence::MeasuredProfile {
            profile_hash: format!("fixture:{host}"),
            profile_key: "test-runtime:model:context:role".to_string(),
            max_concurrent: 1,
        },
        route_evidence_id: format!("fixture-route:{host}:{instance}"),
    }
}

fn probe_lane(device: &str, model: &str, host: &str, instance: &str) -> VerifiedPhysicalLane {
    let mut lane = lane(device, model, host, instance);
    lane.capacity_evidence = HostCapacityEvidence::ProbeSingleStream {
        probe_epoch: format!("probe:{host}"),
    };
    lane
}

fn measured_lane(
    device: &str,
    model: &str,
    host: &str,
    instance: &str,
    max_concurrent: u32,
) -> VerifiedPhysicalLane {
    let mut lane = lane(device, model, host, instance);
    lane.advertised_instance_capacity = max_concurrent;
    lane.capacity_evidence = HostCapacityEvidence::MeasuredProfile {
        profile_hash: format!("fixture:{host}:capacity:{max_concurrent}"),
        profile_key: "test-runtime:model:context:role".to_string(),
        max_concurrent,
    };
    lane
}

fn snapshot(id: &str, lanes: Vec<VerifiedPhysicalLane>) -> PhysicalFleetSnapshot {
    PhysicalFleetSnapshot::new(id, lanes).unwrap()
}

fn attempt(task: &str, attempt: u32, revision: u64) -> TaskVersion {
    TaskVersion {
        authority_scope: AuthorityScope::new("broker-replay", "main"),
        phase_epoch: 0,
        task_id: task.to_string(),
        attempt,
        revision,
        kind: SourceRevisionKind::TaskAttempt,
    }
}

fn artifact(task: &str, attempt: u32, revision: u64) -> TaskVersion {
    TaskVersion {
        authority_scope: AuthorityScope::new("broker-replay", "main"),
        phase_epoch: 0,
        task_id: task.to_string(),
        attempt,
        revision,
        kind: SourceRevisionKind::Artifact {
            snapshot_hash: format!("sha256:{task}:{attempt}:{revision}"),
        },
    }
}

fn trace(task: &str, attempt: u32, revision: u64) -> TaskVersion {
    TaskVersion {
        authority_scope: AuthorityScope::new("broker-replay", "main"),
        phase_epoch: 0,
        task_id: task.to_string(),
        attempt,
        revision,
        kind: SourceRevisionKind::Trace {
            trace_sequence: revision,
            snapshot_hash: format!("sha256:{task}:{attempt}:{revision}"),
        },
    }
}

fn work(
    work_id: &str,
    role: WorkRole,
    priority: WorkPriority,
    source: TaskVersion,
) -> WorkOpportunity {
    WorkOpportunity {
        work_id: work_id.to_string(),
        role,
        priority,
        task_rank: 0,
        source,
        eligible_logical_device_ids: Vec::new(),
        preferred_model_id: None,
        excluded_logical_device_id: None,
    }
}

fn admit_next(broker: &mut PhysicalBroker) -> AdmissionReceipt {
    match broker.grant_next().expect("expected an admission grant") {
        BrokerGrant::Admission(receipt) => receipt,
        BrokerGrant::ProviderRequest { .. } => panic!("expected task admission, got provider turn"),
    }
}

fn provider_start(
    admission: &goose_swarm::AdmissionReceipt,
    ordinal: u32,
) -> ProviderRequestReceipt {
    ProviderRequestReceipt {
        admission_id: admission.admission_id.clone(),
        key: ProviderRequestKey {
            ordinal,
            provider_request_id: format!("provider:{}:{ordinal}", admission.admission_id),
        },
        physical_host_id: admission.physical_host_id.clone(),
        model_instance_id: admission.model_instance_id.clone(),
    }
}

fn provider_terminal(
    start: &ProviderRequestReceipt,
    kind: ProviderTerminalKind,
) -> ProviderTerminalReceipt {
    ProviderTerminalReceipt {
        admission_id: start.admission_id.clone(),
        key: start.key.clone(),
        physical_host_id: start.physical_host_id.clone(),
        model_instance_id: start.model_instance_id.clone(),
        kind,
    }
}

fn finish_one_turn(broker: &mut PhysicalBroker, admission: &goose_swarm::AdmissionReceipt) {
    let start = provider_start(admission, 0);
    assert_eq!(
        broker.request_provider_turn(start.clone()).unwrap(),
        ProviderRequestDisposition::Granted(start.clone())
    );
    broker
        .close_provider_starts(&admission.admission_id)
        .unwrap();
    broker
        .observe_provider_terminal(provider_terminal(&start, ProviderTerminalKind::Finished))
        .unwrap();
    broker
        .record_local_completion(&admission.admission_id, LocalCompletionKind::Success)
        .unwrap();
    assert!(broker
        .release_if_terminal(&admission.admission_id)
        .unwrap()
        .is_some());
}

#[test]
fn one_logical_task_creates_one_admission_and_no_idle_derived_auxiliary_work() {
    let mut broker = PhysicalBroker::new(
        "one-task",
        snapshot(
            "fleet-one-task",
            vec![lane("logical-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let source = attempt("task-a", 0, 1);
    broker.set_source_revision(source.clone()).unwrap();
    broker
        .enqueue(work(
            "build:task-a:0",
            WorkRole::Build,
            WorkPriority::Implementation,
            source,
        ))
        .unwrap();

    let admission = admit_next(&mut broker);
    assert_eq!(admission.physical_host_id, "host-a");
    assert_eq!(broker.pending_len(), 0);
    assert_eq!(broker.active_len(), 1);
    assert!(broker.grant_next().is_none());
    finish_one_turn(&mut broker, &admission);
    assert_eq!(broker.active_len(), 0);
}

#[test]
fn two_configured_lanes_on_one_host_do_not_double_physical_capacity() {
    let mut broker = PhysicalBroker::new(
        "aliased-host",
        snapshot(
            "fleet-aliased-host",
            vec![
                lane("lane-a", "model-a", "same-host", "instance-a"),
                lane("lane-b", "model-b", "same-host", "instance-b"),
            ],
        ),
    )
    .unwrap();
    for task in ["task-a", "task-b"] {
        let source = attempt(task, 0, 1);
        broker.set_source_revision(source.clone()).unwrap();
        broker
            .enqueue(work(
                &format!("build:{task}:0"),
                WorkRole::Build,
                WorkPriority::Implementation,
                source,
            ))
            .unwrap();
    }

    let first = admit_next(&mut broker);
    assert!(broker.grant_next().is_none());
    finish_one_turn(&mut broker, &first);
    assert!(matches!(
        broker.grant_next(),
        Some(BrokerGrant::Admission(_))
    ));
}

#[test]
fn stale_auxiliary_work_is_removed_before_admission() {
    let mut broker = PhysicalBroker::new(
        "stale-aux",
        snapshot(
            "fleet-stale-aux",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let old = artifact("source", 0, 7);
    broker.set_source_revision(old.clone()).unwrap();
    broker
        .enqueue(work(
            "review:source:7",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            old.clone(),
        ))
        .unwrap();

    let stale = broker
        .set_source_revision(artifact("source", 1, 8))
        .unwrap();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].queued_source, old);
    assert!(broker.grant_next().is_none());
}

#[test]
fn newly_ready_critical_work_passes_only_queued_auxiliary_work() {
    let mut broker = PhysicalBroker::new(
        "critical-priority",
        snapshot(
            "fleet-critical-priority",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let review = artifact("done-task", 0, 2);
    let build = attempt("critical-task", 1, 2);
    broker.set_source_revision(review.clone()).unwrap();
    broker.set_source_revision(build.clone()).unwrap();
    broker
        .enqueue(work(
            "review:done-task:2",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            review,
        ))
        .unwrap();
    broker
        .enqueue(work(
            "build:critical-task:0",
            WorkRole::Repair,
            WorkPriority::CriticalPath,
            build,
        ))
        .unwrap();

    assert_eq!(admit_next(&mut broker).work_id, "build:critical-task:0");
    assert_eq!(broker.pending_len(), 1);
}

#[test]
fn newly_ready_critical_work_never_preempts_an_admitted_auxiliary_request() {
    let mut broker = PhysicalBroker::new(
        "admission-boundary",
        snapshot(
            "fleet-admission-boundary",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let review_source = artifact("done-task", 0, 2);
    broker.set_source_revision(review_source.clone()).unwrap();
    broker
        .enqueue(work(
            "review:done-task:2",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            review_source,
        ))
        .unwrap();
    let review = admit_next(&mut broker);

    let build_source = attempt("critical-task", 1, 2);
    broker.set_source_revision(build_source.clone()).unwrap();
    broker
        .enqueue(work(
            "build:critical-task:0",
            WorkRole::Repair,
            WorkPriority::CriticalPath,
            build_source,
        ))
        .unwrap();
    assert!(broker.grant_next().is_none());
    assert_eq!(broker.active_receipt(&review.admission_id), Some(&review));
    finish_one_turn(&mut broker, &review);
    assert_eq!(admit_next(&mut broker).work_id, "build:critical-task:0");
}

#[test]
fn terminal_not_yet_observed_blocks_replacement_and_wrong_receipts_do_not_release() {
    let mut broker = PhysicalBroker::new(
        "unwinding",
        snapshot(
            "fleet-unwinding",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    for task in ["first", "next"] {
        let source = attempt(task, 0, 1);
        broker.set_source_revision(source.clone()).unwrap();
        broker
            .enqueue(work(
                &format!("build:{task}:0"),
                WorkRole::Build,
                WorkPriority::Implementation,
                source,
            ))
            .unwrap();
    }
    let first = admit_next(&mut broker);
    let start = provider_start(&first, 0);
    assert!(matches!(
        broker.request_provider_turn(start.clone()).unwrap(),
        ProviderRequestDisposition::Granted(_)
    ));
    broker.close_provider_starts(&first.admission_id).unwrap();
    broker
        .record_local_completion(&first.admission_id, LocalCompletionKind::StreamDropped)
        .unwrap();
    assert!(broker.grant_next().is_none());
    assert!(broker
        .release_if_terminal(&first.admission_id)
        .unwrap()
        .is_none());

    let mut wrong = provider_terminal(&start, ProviderTerminalKind::Failed);
    wrong.key.provider_request_id = "wrong-provider-request".to_string();
    assert!(broker.observe_provider_terminal(wrong).is_err());
    assert_eq!(broker.active_len(), 1);
    assert!(broker.grant_next().is_none());

    broker
        .observe_provider_terminal(provider_terminal(&start, ProviderTerminalKind::Failed))
        .unwrap();
    assert!(broker
        .release_if_terminal(&first.admission_id)
        .unwrap()
        .is_some());
    assert_eq!(admit_next(&mut broker).work_id, "build:next:0");
}

#[test]
fn all_provider_turns_must_end_before_a_multi_turn_agent_releases_its_host() {
    let mut broker = PhysicalBroker::new(
        "multi-turn",
        snapshot(
            "fleet-multi-turn",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let source = attempt("task", 0, 1);
    broker.set_source_revision(source.clone()).unwrap();
    broker
        .enqueue(work(
            "build:task:0",
            WorkRole::Build,
            WorkPriority::Implementation,
            source,
        ))
        .unwrap();
    let admission = admit_next(&mut broker);
    let first = provider_start(&admission, 0);
    let second = provider_start(&admission, 1);
    assert!(matches!(
        broker.request_provider_turn(first.clone()).unwrap(),
        ProviderRequestDisposition::Granted(_)
    ));
    broker
        .observe_provider_terminal(provider_terminal(&first, ProviderTerminalKind::Finished))
        .unwrap();
    assert!(matches!(
        broker.request_provider_turn(second.clone()).unwrap(),
        ProviderRequestDisposition::Queued(_)
    ));
    assert!(matches!(
        broker.grant_next(),
        Some(BrokerGrant::ProviderRequest { receipt, .. }) if receipt == second
    ));
    broker
        .close_provider_starts(&admission.admission_id)
        .unwrap();
    broker
        .record_local_completion(&admission.admission_id, LocalCompletionKind::Success)
        .unwrap();
    assert!(broker
        .release_if_terminal(&admission.admission_id)
        .unwrap()
        .is_none());
    broker
        .observe_provider_terminal(provider_terminal(&second, ProviderTerminalKind::Finished))
        .unwrap();
    assert!(broker
        .release_if_terminal(&admission.admission_id)
        .unwrap()
        .is_some());
}

#[test]
fn generic_auxiliary_prose_cannot_enter_the_queue_without_typed_evidence() {
    let mut broker = PhysicalBroker::new(
        "generic-aux",
        snapshot(
            "fleet-generic-aux",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let generic = attempt("source", 0, 1);
    broker.set_source_revision(generic.clone()).unwrap();
    assert!(broker
        .enqueue(work(
            "review-whatever-is-useful",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            generic,
        ))
        .is_err());
    assert_eq!(broker.pending_len(), 0);
}

#[test]
fn capacity_changes_admission_timing_but_not_the_task_derived_work_graph() {
    let mut one_lane = lane("lane-a", "model-a", "host-a", "instance-a");
    let mut eight_lane = one_lane.clone();
    one_lane.capacity_evidence = HostCapacityEvidence::MeasuredProfile {
        profile_hash: "capacity-one".to_string(),
        profile_key: "test-runtime:model:context:role".to_string(),
        max_concurrent: 1,
    };
    eight_lane.capacity_evidence = HostCapacityEvidence::MeasuredProfile {
        profile_hash: "capacity-eight".to_string(),
        profile_key: "test-runtime:model:context:role".to_string(),
        max_concurrent: 8,
    };
    let mut one = PhysicalBroker::new("cap-one", snapshot("cap-one", vec![one_lane])).unwrap();
    let mut eight =
        PhysicalBroker::new("cap-eight", snapshot("cap-eight", vec![eight_lane])).unwrap();
    for task in ["a", "b", "c"] {
        let source = attempt(task, 0, 1);
        let opportunity = work(
            &format!("build:{task}:0"),
            WorkRole::Build,
            WorkPriority::Implementation,
            source.clone(),
        );
        one.set_source_revision(source.clone()).unwrap();
        eight.set_source_revision(source).unwrap();
        one.enqueue(opportunity.clone()).unwrap();
        eight.enqueue(opportunity).unwrap();
    }
    assert_eq!(one.pending_work_ids(), eight.pending_work_ids());
    assert_eq!(admit_next(&mut one).work_id, "build:a:0");
    assert_eq!(admit_next(&mut eight).work_id, "build:a:0");
    assert_eq!(one.pending_len(), 2);
    assert_eq!(eight.pending_len(), 2);
    assert!(one.grant_next().is_none());
    assert!(matches!(
        eight.grant_next(),
        Some(BrokerGrant::Admission(_))
    ));
    assert!(matches!(
        eight.grant_next(),
        Some(BrokerGrant::Admission(_))
    ));
    assert_eq!(eight.pending_len(), 0);
}

#[test]
fn one_model_identifier_on_two_hosts_is_not_a_verified_route() {
    let error = PhysicalFleetSnapshot::new(
        "ambiguous",
        vec![
            lane("lane-a", "shared-model", "host-a", "instance-a"),
            lane("lane-b", "shared-model", "host-b", "instance-b"),
        ],
    )
    .unwrap_err();
    assert!(error.to_string().contains("multiple physical routes"));
}

#[test]
fn provider_request_ids_are_nonempty_and_unique_even_within_one_admission() {
    let mut broker = PhysicalBroker::new(
        "request-identity",
        snapshot(
            "fleet-request-identity",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let source = attempt("task", 0, 1);
    broker.set_source_revision(source.clone()).unwrap();
    broker
        .enqueue(work(
            "build:task:0",
            WorkRole::Build,
            WorkPriority::Implementation,
            source,
        ))
        .unwrap();
    let admission = admit_next(&mut broker);
    let mut empty = provider_start(&admission, 0);
    empty.key.provider_request_id.clear();
    assert!(broker.request_provider_turn(empty).is_err());

    let first = provider_start(&admission, 0);
    assert!(matches!(
        broker.request_provider_turn(first.clone()).unwrap(),
        ProviderRequestDisposition::Granted(_)
    ));
    let mut reused = provider_start(&admission, 1);
    reused.key.provider_request_id = first.key.provider_request_id;
    assert!(broker.request_provider_turn(reused).is_err());
}

#[test]
fn source_revision_is_monotonic_and_an_old_removal_cannot_delete_new_authority() {
    let mut broker = PhysicalBroker::new(
        "source-cas",
        snapshot(
            "source-cas",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let old = artifact("artifact", 0, 1);
    let current = artifact("artifact", 1, 2);
    broker.set_source_revision(old.clone()).unwrap();
    broker.set_source_revision(current.clone()).unwrap();

    assert!(broker.set_source_revision(old.clone()).is_err());
    assert!(broker.remove_source_revision(&old).is_err());
    broker
        .enqueue(work(
            "review:current",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            current.clone(),
        ))
        .unwrap();
    broker.remove_source_revision(&current).unwrap();
    assert!(broker.grant_next().is_none());
}

#[test]
fn unroutable_or_priority_laundered_work_fails_before_queueing() {
    let mut broker = PhysicalBroker::new(
        "route-validation",
        snapshot(
            "route-validation",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let source = attempt("task", 0, 1);
    broker.set_source_revision(source.clone()).unwrap();

    let mut unknown = work(
        "build:unknown-route",
        WorkRole::Build,
        WorkPriority::Implementation,
        source.clone(),
    );
    unknown.eligible_logical_device_ids = vec!["missing-lane".to_string()];
    assert!(broker.enqueue(unknown).is_err());

    let mut excluded = work(
        "build:no-route",
        WorkRole::Build,
        WorkPriority::Implementation,
        source.clone(),
    );
    excluded.eligible_logical_device_ids = vec!["lane-a".to_string()];
    excluded.excluded_logical_device_id = Some("lane-a".to_string());
    assert!(broker.enqueue(excluded).is_err());

    let laundered = work(
        "build:laundered",
        WorkRole::Build,
        WorkPriority::CriticalPath,
        source,
    );
    assert!(broker.enqueue(laundered).is_err());
    assert_eq!(broker.pending_len(), 0);
}

#[test]
fn same_host_lanes_must_share_the_exact_capacity_evidence() {
    let first = lane("lane-a", "model-a", "same-host", "instance-a");
    let mut contradictory = lane("lane-b", "model-b", "same-host", "instance-b");
    contradictory.capacity_evidence = HostCapacityEvidence::MeasuredProfile {
        profile_hash: "different-measurement".to_string(),
        profile_key: "test-runtime:model:context:role".to_string(),
        max_concurrent: 1,
    };
    assert!(PhysicalFleetSnapshot::new("contradictory", vec![first, contradictory]).is_err());
}

#[test]
fn aliases_of_one_physical_instance_must_share_exact_route_evidence() {
    let first = lane("lane-a", "model-a", "same-host", "same-instance");
    let mut contradictory = first.clone();
    contradictory.logical_device_id = "lane-b".to_string();
    contradictory.route_evidence_id = "different-route-observation".to_string();
    assert!(PhysicalFleetSnapshot::new("contradictory-route", vec![first, contradictory]).is_err());
}

#[test]
fn aliases_of_one_physical_instance_must_share_provider_transport() {
    let first = lane("lane-a", "model-a", "same-host", "same-instance");
    let mut contradictory = first.clone();
    contradictory.logical_device_id = "lane-b".to_string();
    contradictory.provider_transport_id = TRANSPORT_B.to_string();
    assert!(PhysicalFleetSnapshot::new(
        "contradictory-provider-transport",
        vec![first, contradictory]
    )
    .is_err());
}

#[test]
fn raw_provider_endpoint_is_rejected_without_serializing_it() {
    let mut unsealed = lane("lane-a", "model-a", "host-a", "instance-a");
    unsealed.provider_transport_id =
        "http://operator:secret@lm-link.test/v1/chat/completions".to_string();
    let error = PhysicalFleetSnapshot::new("raw-provider-endpoint", vec![unsealed])
        .expect_err("raw provider endpoint must never enter a sealed snapshot");
    let rendered = error.to_string();
    assert!(rendered.contains("canonical sha256 digest"));
    assert!(!rendered.contains("lm-link"));
    assert!(!rendered.contains("secret"));

    let sealed = snapshot(
        "hashed-provider-endpoint",
        vec![lane("lane-a", "model-a", "host-a", "instance-a")],
    );
    let serialized = serde_json::to_string(&sealed).unwrap();
    assert!(serialized.contains(TRANSPORT_A));
    assert!(!serialized.contains("http://"));
}

#[test]
fn provider_turns_reenter_the_common_ranked_queue_after_tool_gaps() {
    let mut broker = PhysicalBroker::new(
        "turn-reacquisition",
        snapshot(
            "turn-reacquisition",
            vec![measured_lane(
                "lane-a",
                "model-a",
                "host-a",
                "instance-a",
                2,
            )],
        ),
    )
    .unwrap();
    let review_source = artifact("review", 0, 1);
    broker.set_source_revision(review_source.clone()).unwrap();
    broker
        .enqueue(work(
            "review:artifact",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            review_source,
        ))
        .unwrap();
    let review = admit_next(&mut broker);
    let first = provider_start(&review, 0);
    assert!(matches!(
        broker.request_provider_turn(first.clone()).unwrap(),
        ProviderRequestDisposition::Granted(_)
    ));
    broker
        .observe_provider_terminal(provider_terminal(&first, ProviderTerminalKind::Finished))
        .unwrap();

    let second = provider_start(&review, 1);
    assert!(matches!(
        broker.request_provider_turn(second).unwrap(),
        ProviderRequestDisposition::Queued(_)
    ));
    let repair_source = attempt("repair", 1, 2);
    broker.set_source_revision(repair_source.clone()).unwrap();
    broker
        .enqueue(work(
            "repair:critical",
            WorkRole::Repair,
            WorkPriority::CriticalPath,
            repair_source,
        ))
        .unwrap();

    assert_eq!(admit_next(&mut broker).work_id, "repair:critical");
    assert_eq!(broker.active_len(), 2);
}

#[test]
fn provider_reacquisition_uses_its_task_work_id_for_equal_rank_tiebreaking() {
    let mut broker = PhysicalBroker::new(
        "zzzz-scope",
        snapshot(
            "provider-work-rank",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let first_source = attempt("a-provider", 0, 1);
    broker.set_source_revision(first_source.clone()).unwrap();
    broker
        .enqueue(work(
            "a-provider",
            WorkRole::Build,
            WorkPriority::Implementation,
            first_source,
        ))
        .unwrap();
    let first = admit_next(&mut broker);
    let first_turn = provider_start(&first, 0);
    broker.request_provider_turn(first_turn.clone()).unwrap();
    broker
        .observe_provider_terminal(provider_terminal(
            &first_turn,
            ProviderTerminalKind::Finished,
        ))
        .unwrap();
    assert!(matches!(
        broker
            .request_provider_turn(provider_start(&first, 1))
            .unwrap(),
        ProviderRequestDisposition::Queued(_)
    ));

    let other_source = attempt("m-new", 0, 1);
    broker.set_source_revision(other_source.clone()).unwrap();
    broker
        .enqueue(work(
            "m-new",
            WorkRole::Build,
            WorkPriority::Implementation,
            other_source,
        ))
        .unwrap();

    match broker.grant_next().unwrap() {
        BrokerGrant::ProviderRequest { admission, .. } => {
            assert_eq!(admission.work_id, "a-provider");
        }
        BrokerGrant::Admission(receipt) => {
            panic!("task-id ordering was replaced by admission-id ordering: {receipt:?}")
        }
    }
}

fn assert_existing_core_session_continuation_beats_queued_build(
    role: WorkRole,
    existing_work_id: &str,
) {
    let mut broker = PhysicalBroker::new(
        "r5-continuation-inversion",
        snapshot(
            "r5-continuation-inversion",
            vec![measured_lane(
                "lane-a",
                "model-a",
                "host-a",
                "instance-a",
                2,
            )],
        ),
    )
    .unwrap();
    let existing_source = attempt(existing_work_id, 0, 1);
    broker.set_source_revision(existing_source.clone()).unwrap();
    broker
        .enqueue(work(
            existing_work_id,
            role,
            role.priority(),
            existing_source,
        ))
        .unwrap();
    let existing = admit_next(&mut broker);
    let first = provider_start(&existing, 0);
    broker.request_provider_turn(first.clone()).unwrap();
    broker
        .observe_provider_terminal(provider_terminal(&first, ProviderTerminalKind::Finished))
        .unwrap();
    let continuation = provider_start(&existing, 1);
    assert!(matches!(
        broker.request_provider_turn(continuation.clone()).unwrap(),
        ProviderRequestDisposition::Queued(_)
    ));

    let queued_source = attempt("build-01", 0, 1);
    broker.set_source_revision(queued_source.clone()).unwrap();
    broker
        .enqueue(work(
            "build-01",
            WorkRole::Build,
            WorkPriority::Implementation,
            queued_source,
        ))
        .unwrap();

    assert!(matches!(
        broker.grant_next(),
        Some(BrokerGrant::ProviderRequest { admission, receipt })
            if admission.work_id == existing_work_id && receipt == continuation
    ));
}

#[test]
fn existing_build_session_continuation_beats_r5_queued_build_inversion() {
    assert_existing_core_session_continuation_beats_queued_build(WorkRole::Build, "build-03");
}

#[test]
fn existing_research_session_continuation_beats_queued_build_inversion() {
    assert_existing_core_session_continuation_beats_queued_build(
        WorkRole::ResearchEvidence,
        "research-03",
    );
}

#[test]
fn existing_planning_session_continuation_beats_queued_build_inversion() {
    assert_existing_core_session_continuation_beats_queued_build(
        WorkRole::PlanningAuthority,
        "planning-03",
    );
}

#[test]
fn queued_core_build_beats_unstarted_repair_alternative() {
    let mut broker = PhysicalBroker::new(
        "core-before-repair",
        snapshot(
            "core-before-repair",
            vec![
                lane("lane-a", "model-a", "host-a", "instance-a"),
                lane("lane-b", "model-b", "host-b", "instance-b"),
            ],
        ),
    )
    .unwrap();
    let repair_source = attempt("repair", 1, 2);
    broker.set_source_revision(repair_source.clone()).unwrap();
    broker
        .enqueue(work(
            "repair-alternative",
            WorkRole::Repair,
            WorkPriority::CriticalPath,
            repair_source,
        ))
        .unwrap();
    let build_source = attempt("build", 0, 1);
    broker.set_source_revision(build_source.clone()).unwrap();
    broker
        .enqueue(work(
            "core-build",
            WorkRole::Build,
            WorkPriority::Implementation,
            build_source,
        ))
        .unwrap();

    assert_eq!(admit_next(&mut broker).work_id, "core-build");
}

#[test]
fn idle_judge_never_steals_a_route_from_queued_core_work() {
    let mut broker = PhysicalBroker::new(
        "core-before-judge",
        snapshot(
            "core-before-judge",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let judge_source = trace("observed", 0, 1);
    broker.set_source_revision(judge_source.clone()).unwrap();
    let mut judge = work(
        "semantic-judge",
        WorkRole::SemanticJudgeObservation,
        WorkPriority::AuxiliaryEvidence,
        judge_source,
    );
    judge.task_rank = u64::MAX;
    broker.enqueue(judge).unwrap();
    let build_source = attempt("core", 0, 1);
    broker.set_source_revision(build_source.clone()).unwrap();
    broker
        .enqueue(work(
            "core-build",
            WorkRole::Build,
            WorkPriority::Implementation,
            build_source,
        ))
        .unwrap();

    assert_eq!(admit_next(&mut broker).work_id, "core-build");
}

#[test]
fn probe_single_stream_keeps_one_parked_session_and_resumes_it_before_fresh_work() {
    let mut broker = PhysicalBroker::new(
        "probe-parked-session",
        snapshot(
            "probe-parked-session",
            vec![probe_lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let parked_source = attempt("parked", 0, 1);
    broker.set_source_revision(parked_source.clone()).unwrap();
    broker
        .enqueue(work(
            "parked",
            WorkRole::Build,
            WorkPriority::Implementation,
            parked_source,
        ))
        .unwrap();
    let parked = admit_next(&mut broker);
    let first = provider_start(&parked, 0);
    broker.request_provider_turn(first.clone()).unwrap();
    broker
        .observe_provider_terminal(provider_terminal(&first, ProviderTerminalKind::Finished))
        .unwrap();

    let fresh_source = attempt("fresh", 0, 1);
    broker.set_source_revision(fresh_source.clone()).unwrap();
    broker
        .enqueue(work(
            "fresh",
            WorkRole::Build,
            WorkPriority::Implementation,
            fresh_source,
        ))
        .unwrap();
    assert!(broker.grant_next().is_none());
    assert_eq!(broker.active_len(), 1);

    let continuation = provider_start(&parked, 1);
    assert!(matches!(
        broker.request_provider_turn(continuation.clone()).unwrap(),
        ProviderRequestDisposition::Queued(_)
    ));
    assert!(matches!(
        broker.grant_next(),
        Some(BrokerGrant::ProviderRequest { receipt, .. }) if receipt == continuation
    ));
    assert_eq!(broker.active_len(), 1);
}

#[test]
fn measured_capacity_one_keeps_one_parked_session_and_resumes_it_before_fresh_work() {
    let mut broker = PhysicalBroker::new(
        "measured-one-parked-session",
        snapshot(
            "measured-one-parked-session",
            vec![measured_lane(
                "lane-a",
                "model-a",
                "host-a",
                "instance-a",
                1,
            )],
        ),
    )
    .unwrap();
    let parked_source = attempt("parked", 0, 1);
    broker.set_source_revision(parked_source.clone()).unwrap();
    broker
        .enqueue(work(
            "parked",
            WorkRole::Build,
            WorkPriority::Implementation,
            parked_source,
        ))
        .unwrap();
    let parked = admit_next(&mut broker);
    let first = provider_start(&parked, 0);
    broker.request_provider_turn(first.clone()).unwrap();
    broker
        .observe_provider_terminal(provider_terminal(&first, ProviderTerminalKind::Finished))
        .unwrap();

    let fresh_source = attempt("fresh", 0, 1);
    broker.set_source_revision(fresh_source.clone()).unwrap();
    broker
        .enqueue(work(
            "fresh",
            WorkRole::Build,
            WorkPriority::Implementation,
            fresh_source,
        ))
        .unwrap();
    assert!(broker.grant_next().is_none());
    assert_eq!(broker.active_len(), 1);

    let continuation = provider_start(&parked, 1);
    assert!(matches!(
        broker.request_provider_turn(continuation.clone()).unwrap(),
        ProviderRequestDisposition::Queued(_)
    ));
    assert!(matches!(
        broker.grant_next(),
        Some(BrokerGrant::ProviderRequest { receipt, .. }) if receipt == continuation
    ));
    assert_eq!(broker.active_len(), 1);
}

#[test]
fn provider_failure_downgrades_a_claimed_local_success() {
    let mut broker = PhysicalBroker::new(
        "outcome-reconciliation",
        snapshot(
            "outcome-reconciliation",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let source = attempt("task", 0, 1);
    broker.set_source_revision(source.clone()).unwrap();
    broker
        .enqueue(work(
            "build:task",
            WorkRole::Build,
            WorkPriority::Implementation,
            source,
        ))
        .unwrap();
    let admission = admit_next(&mut broker);
    let start = provider_start(&admission, 0);
    broker.request_provider_turn(start.clone()).unwrap();
    broker
        .observe_provider_terminal(provider_terminal(&start, ProviderTerminalKind::Failed))
        .unwrap();
    broker
        .close_provider_starts(&admission.admission_id)
        .unwrap();
    broker
        .record_local_completion(&admission.admission_id, LocalCompletionKind::Success)
        .unwrap();
    let released = broker
        .release_if_terminal(&admission.admission_id)
        .unwrap()
        .unwrap();
    assert_eq!(released.local_completion, LocalCompletionKind::Error);
}

#[test]
fn later_finished_provider_attempt_recovers_an_earlier_failure() {
    let mut broker = PhysicalBroker::new(
        "outcome-recovery",
        snapshot(
            "outcome-recovery",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let source = attempt("task", 0, 1);
    broker.set_source_revision(source.clone()).unwrap();
    broker
        .enqueue(work(
            "build:task",
            WorkRole::Build,
            WorkPriority::Implementation,
            source,
        ))
        .unwrap();
    let admission = admit_next(&mut broker);
    let failed = provider_start(&admission, 0);
    broker.request_provider_turn(failed.clone()).unwrap();
    broker
        .observe_provider_terminal(provider_terminal(&failed, ProviderTerminalKind::Failed))
        .unwrap();
    let recovered = provider_start(&admission, 1);
    assert!(matches!(
        broker.request_provider_turn(recovered.clone()).unwrap(),
        ProviderRequestDisposition::Queued(_)
    ));
    assert!(matches!(
        broker.grant_next(),
        Some(BrokerGrant::ProviderRequest { receipt, .. }) if receipt == recovered
    ));
    broker
        .observe_provider_terminal(provider_terminal(
            &recovered,
            ProviderTerminalKind::Finished,
        ))
        .unwrap();
    broker
        .close_provider_starts(&admission.admission_id)
        .unwrap();
    broker
        .record_local_completion(&admission.admission_id, LocalCompletionKind::Success)
        .unwrap();
    let released = broker
        .release_if_terminal(&admission.admission_id)
        .unwrap()
        .unwrap();
    assert_eq!(released.local_completion, LocalCompletionKind::Success);
    assert_eq!(released.provider_terminals.len(), 2);
    assert_eq!(
        released.provider_terminals[0].kind,
        ProviderTerminalKind::Failed
    );
    assert_eq!(
        released.provider_terminals[1].kind,
        ProviderTerminalKind::Finished
    );
}

#[test]
fn final_failed_provider_attempt_downgrades_prior_success() {
    let mut broker = PhysicalBroker::new(
        "outcome-regression",
        snapshot(
            "outcome-regression",
            vec![lane("lane-a", "model-a", "host-a", "instance-a")],
        ),
    )
    .unwrap();
    let source = attempt("task", 0, 1);
    broker.set_source_revision(source.clone()).unwrap();
    broker
        .enqueue(work(
            "build:task",
            WorkRole::Build,
            WorkPriority::Implementation,
            source,
        ))
        .unwrap();
    let admission = admit_next(&mut broker);
    let finished = provider_start(&admission, 0);
    broker.request_provider_turn(finished.clone()).unwrap();
    broker
        .observe_provider_terminal(provider_terminal(&finished, ProviderTerminalKind::Finished))
        .unwrap();
    let failed = provider_start(&admission, 1);
    assert!(matches!(
        broker.request_provider_turn(failed.clone()).unwrap(),
        ProviderRequestDisposition::Queued(_)
    ));
    assert!(matches!(
        broker.grant_next(),
        Some(BrokerGrant::ProviderRequest { receipt, .. }) if receipt == failed
    ));
    broker
        .observe_provider_terminal(provider_terminal(&failed, ProviderTerminalKind::Failed))
        .unwrap();
    broker
        .close_provider_starts(&admission.admission_id)
        .unwrap();
    broker
        .record_local_completion(&admission.admission_id, LocalCompletionKind::Success)
        .unwrap();
    let released = broker
        .release_if_terminal(&admission.admission_id)
        .unwrap()
        .unwrap();
    assert_eq!(released.local_completion, LocalCompletionKind::Error);
}
