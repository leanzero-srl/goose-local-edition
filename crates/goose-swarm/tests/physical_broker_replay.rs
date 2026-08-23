use goose_swarm::{
    HostCapacityEvidence, LocalCompletionKind, PhysicalBroker, PhysicalFleetSnapshot,
    ProviderRequestKey, ProviderRequestReceipt, ProviderTerminalKind, ProviderTerminalReceipt,
    SourceRevisionKind, TaskVersion, VerifiedPhysicalLane, WorkOpportunity, WorkPriority, WorkRole,
};

fn lane(device: &str, model: &str, host: &str, instance: &str) -> VerifiedPhysicalLane {
    VerifiedPhysicalLane {
        logical_device_id: device.to_string(),
        model_id: model.to_string(),
        host_id: host.to_string(),
        model_instance_id: instance.to_string(),
        advertised_instance_capacity: 4,
        routing_weight: 1,
        capacity_evidence: HostCapacityEvidence::ReplayFixture {
            fixture_id: format!("fixture:{host}"),
            max_concurrent: 1,
        },
        route_evidence_id: format!("fixture-route:{host}:{instance}"),
    }
}

fn snapshot(id: &str, lanes: Vec<VerifiedPhysicalLane>) -> PhysicalFleetSnapshot {
    PhysicalFleetSnapshot::new(id, lanes).unwrap()
}

fn attempt(task: &str, attempt: u32, revision: u64) -> TaskVersion {
    TaskVersion {
        task_id: task.to_string(),
        attempt,
        revision,
        kind: SourceRevisionKind::TaskAttempt,
    }
}

fn artifact(task: &str, attempt: u32, revision: u64) -> TaskVersion {
    TaskVersion {
        task_id: task.to_string(),
        attempt,
        revision,
        kind: SourceRevisionKind::Artifact {
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
        source,
        eligible_logical_device_ids: Vec::new(),
        preferred_model_id: None,
        excluded_logical_device_id: None,
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
    broker.bind_provider_request(start.clone()).unwrap();
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
    broker.set_source_revision(source.clone());
    broker
        .enqueue(work(
            "build:task-a:0",
            WorkRole::Build,
            WorkPriority::CriticalPath,
            source,
        ))
        .unwrap();

    let admission = broker.admit_next().unwrap();
    assert_eq!(admission.physical_host_id, "host-a");
    assert_eq!(broker.pending_len(), 0);
    assert_eq!(broker.active_len(), 1);
    assert!(broker.admit_next().is_none());
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
        broker.set_source_revision(source.clone());
        broker
            .enqueue(work(
                &format!("build:{task}:0"),
                WorkRole::Build,
                WorkPriority::Implementation,
                source,
            ))
            .unwrap();
    }

    let first = broker.admit_next().unwrap();
    assert!(broker.admit_next().is_none());
    finish_one_turn(&mut broker, &first);
    assert!(broker.admit_next().is_some());
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
    broker.set_source_revision(old.clone());
    broker
        .enqueue(work(
            "review:source:7",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            old.clone(),
        ))
        .unwrap();

    let stale = broker.set_source_revision(artifact("source", 1, 8));
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].queued_source, old);
    assert!(broker.admit_next().is_none());
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
    let build = attempt("critical-task", 0, 1);
    broker.set_source_revision(review.clone());
    broker.set_source_revision(build.clone());
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
            WorkRole::Build,
            WorkPriority::CriticalPath,
            build,
        ))
        .unwrap();

    assert_eq!(
        broker.admit_next().unwrap().work_id,
        "build:critical-task:0"
    );
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
    broker.set_source_revision(review_source.clone());
    broker
        .enqueue(work(
            "review:done-task:2",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            review_source,
        ))
        .unwrap();
    let review = broker.admit_next().unwrap();

    let build_source = attempt("critical-task", 0, 1);
    broker.set_source_revision(build_source.clone());
    broker
        .enqueue(work(
            "build:critical-task:0",
            WorkRole::Build,
            WorkPriority::CriticalPath,
            build_source,
        ))
        .unwrap();
    assert!(broker.admit_next().is_none());
    assert_eq!(broker.active_receipt(&review.admission_id), Some(&review));
    finish_one_turn(&mut broker, &review);
    assert_eq!(
        broker.admit_next().unwrap().work_id,
        "build:critical-task:0"
    );
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
        broker.set_source_revision(source.clone());
        broker
            .enqueue(work(
                &format!("build:{task}:0"),
                WorkRole::Build,
                WorkPriority::Implementation,
                source,
            ))
            .unwrap();
    }
    let first = broker.admit_next().unwrap();
    let start = provider_start(&first, 0);
    broker.bind_provider_request(start.clone()).unwrap();
    broker
        .record_local_completion(&first.admission_id, LocalCompletionKind::StreamDropped)
        .unwrap();
    assert!(broker.admit_next().is_none());
    assert!(broker
        .release_if_terminal(&first.admission_id)
        .unwrap()
        .is_none());

    let mut wrong = provider_terminal(&start, ProviderTerminalKind::Failed);
    wrong.key.provider_request_id = "wrong-provider-request".to_string();
    assert!(broker.observe_provider_terminal(wrong).is_err());
    assert_eq!(broker.active_len(), 1);
    assert!(broker.admit_next().is_none());

    broker
        .observe_provider_terminal(provider_terminal(&start, ProviderTerminalKind::Failed))
        .unwrap();
    assert!(broker
        .release_if_terminal(&first.admission_id)
        .unwrap()
        .is_some());
    assert_eq!(broker.admit_next().unwrap().work_id, "build:next:0");
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
    broker.set_source_revision(source.clone());
    broker
        .enqueue(work(
            "build:task:0",
            WorkRole::Build,
            WorkPriority::Implementation,
            source,
        ))
        .unwrap();
    let admission = broker.admit_next().unwrap();
    let first = provider_start(&admission, 0);
    let second = provider_start(&admission, 1);
    broker.bind_provider_request(first.clone()).unwrap();
    broker.bind_provider_request(second.clone()).unwrap();
    broker
        .record_local_completion(&admission.admission_id, LocalCompletionKind::Success)
        .unwrap();
    broker
        .observe_provider_terminal(provider_terminal(&first, ProviderTerminalKind::Finished))
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
    broker.set_source_revision(generic.clone());
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
    one_lane.capacity_evidence = HostCapacityEvidence::ReplayFixture {
        fixture_id: "capacity-one".to_string(),
        max_concurrent: 1,
    };
    eight_lane.capacity_evidence = HostCapacityEvidence::ReplayFixture {
        fixture_id: "capacity-eight".to_string(),
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
        one.set_source_revision(source.clone());
        eight.set_source_revision(source);
        one.enqueue(opportunity.clone()).unwrap();
        eight.enqueue(opportunity).unwrap();
    }
    assert_eq!(one.pending_work_ids(), eight.pending_work_ids());
    assert_eq!(one.admit_next().unwrap().work_id, "build:a:0");
    assert_eq!(eight.admit_next().unwrap().work_id, "build:a:0");
    assert_eq!(one.pending_work_ids(), eight.pending_work_ids());
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
