use goose_swarm::{
    LocalCompletionKind, PhysicalBroker, PhysicalEvidenceKind, ProviderRequestReceipt,
    ProviderTerminalKind, ProviderTerminalReceipt, TaskVersion, VerifiedPhysicalLane,
    WorkOpportunity, WorkPriority, WorkRole,
};

fn lane(device: &str, model: &str, host: &str, instance: &str) -> VerifiedPhysicalLane {
    VerifiedPhysicalLane {
        logical_device_id: device.to_string(),
        model_id: model.to_string(),
        host_id: host.to_string(),
        model_instance_id: instance.to_string(),
        host_capacity: 1,
        instance_capacity: 1,
        supervision_only: false,
        routing_weight: 1,
        evidence_kind: PhysicalEvidenceKind::ReplayFixture,
        evidence_id: format!("fixture:{host}:{instance}"),
    }
}

fn version(task: &str, attempt: u32, revision: u64) -> TaskVersion {
    TaskVersion {
        task_id: task.to_string(),
        attempt,
        revision,
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
        preferred_model_id: None,
        excluded_logical_device_id: None,
    }
}

fn terminal_for(admission: &goose_swarm::AdmissionReceipt) -> ProviderTerminalReceipt {
    ProviderTerminalReceipt {
        request_id: admission.request_id.clone(),
        provider_request_id: format!("provider: {}", admission.request_id),
        physical_host_id: admission.physical_host_id.clone(),
        model_instance_id: admission.model_instance_id.clone(),
        kind: ProviderTerminalKind::Finished,
    }
}

fn bind_and_finish(broker: &mut PhysicalBroker, admission: &goose_swarm::AdmissionReceipt) {
    let terminal = terminal_for(admission);
    broker
        .bind_provider_request(ProviderRequestReceipt {
            request_id: admission.request_id.clone(),
            provider_request_id: terminal.provider_request_id.clone(),
            physical_host_id: admission.physical_host_id.clone(),
            model_instance_id: admission.model_instance_id.clone(),
        })
        .unwrap();
    broker.observe_provider_terminal(terminal).unwrap();
}

#[test]
fn one_logical_task_holds_one_physical_host_until_correlated_terminal() {
    let mut broker = PhysicalBroker::new(
        "one-task",
        vec![lane("logical-a", "model-a", "host-a", "instance-a")],
    )
    .unwrap();
    let v = version("task-a", 0, 1);
    broker.set_task_version(v.clone());
    broker
        .enqueue(work(
            "build:task-a:0",
            WorkRole::Build,
            WorkPriority::CriticalPath,
            v,
        ))
        .unwrap();

    let admission = broker.admit_next().unwrap();
    assert_eq!(admission.physical_host_id, "host-a");
    assert_eq!(broker.active_on_host("host-a"), 1);
    broker
        .record_local_completion(&admission.request_id, LocalCompletionKind::Success)
        .unwrap();
    assert_eq!(
        broker.active_on_host("host-a"),
        1,
        "local completion is not provider-terminal evidence"
    );
    bind_and_finish(&mut broker, &admission);
    assert_eq!(broker.active_on_host("host-a"), 0);
}

#[test]
fn two_configured_lanes_on_one_host_do_not_double_physical_capacity() {
    let mut broker = PhysicalBroker::new(
        "aliased-host",
        vec![
            lane("lane-a", "model-a", "same-host", "instance-a"),
            lane("lane-b", "model-b", "same-host", "instance-b"),
        ],
    )
    .unwrap();
    let va = version("task-a", 0, 1);
    let vb = version("task-b", 0, 1);
    broker.set_task_version(va.clone());
    broker.set_task_version(vb.clone());
    broker
        .enqueue(work(
            "build:task-a:0",
            WorkRole::Build,
            WorkPriority::Implementation,
            va,
        ))
        .unwrap();
    broker
        .enqueue(work(
            "build:task-b:0",
            WorkRole::Build,
            WorkPriority::Implementation,
            vb,
        ))
        .unwrap();

    let first = broker.admit_next().unwrap();
    assert!(
        broker.admit_next().is_none(),
        "a second logical lane must not manufacture another physical slot"
    );
    bind_and_finish(&mut broker, &first);
    assert!(broker.admit_next().is_some());
}

#[test]
fn stale_auxiliary_work_is_removed_before_admission() {
    let mut broker = PhysicalBroker::new(
        "stale-aux",
        vec![lane("lane-a", "model-a", "host-a", "instance-a")],
    )
    .unwrap();
    let old = version("source", 0, 7);
    broker.set_task_version(old.clone());
    broker
        .enqueue(work(
            "review:source:7",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            old.clone(),
        ))
        .unwrap();

    let stale = broker.set_task_version(version("source", 1, 8));
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].queued_source, old);
    assert_eq!(broker.pending_len(), 0);
    assert!(broker.admit_next().is_none());
}

#[test]
fn newly_ready_critical_work_preempts_only_queued_auxiliary_work() {
    let mut broker = PhysicalBroker::new(
        "critical-priority",
        vec![lane("lane-a", "model-a", "host-a", "instance-a")],
    )
    .unwrap();
    let review_v = version("done-task", 0, 2);
    let build_v = version("critical-task", 0, 1);
    broker.set_task_version(review_v.clone());
    broker.set_task_version(build_v.clone());
    broker
        .enqueue(work(
            "review:done-task:2",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            review_v,
        ))
        .unwrap();
    broker
        .enqueue(work(
            "build:critical-task:0",
            WorkRole::Build,
            WorkPriority::CriticalPath,
            build_v,
        ))
        .unwrap();

    let admitted = broker.admit_next().unwrap();
    assert_eq!(admitted.work_id, "build:critical-task:0");
    assert_eq!(broker.pending_len(), 1);
}

#[test]
fn newly_ready_critical_work_never_preempts_an_admitted_auxiliary_request() {
    let mut broker = PhysicalBroker::new(
        "admission-boundary",
        vec![lane("lane-a", "model-a", "host-a", "instance-a")],
    )
    .unwrap();
    let review_v = version("done-task", 0, 2);
    let build_v = version("critical-task", 0, 1);
    broker.set_task_version(review_v.clone());
    broker.set_task_version(build_v.clone());
    broker
        .enqueue(work(
            "review:done-task:2",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            review_v,
        ))
        .unwrap();
    let review = broker.admit_next().unwrap();
    broker
        .enqueue(work(
            "build:critical-task:0",
            WorkRole::Build,
            WorkPriority::CriticalPath,
            build_v,
        ))
        .unwrap();

    assert!(
        broker.admit_next().is_none(),
        "the admission boundary is final even when critical work becomes ready"
    );
    assert_eq!(broker.active_receipt(&review.request_id), Some(&review));
    bind_and_finish(&mut broker, &review);
    assert_eq!(
        broker.admit_next().unwrap().work_id,
        "build:critical-task:0"
    );
}

#[test]
fn terminal_not_yet_observed_blocks_a_replacement_after_local_stream_drop() {
    let mut broker = PhysicalBroker::new(
        "unwinding",
        vec![lane("lane-a", "model-a", "host-a", "instance-a")],
    )
    .unwrap();
    let first_v = version("first", 0, 1);
    let next_v = version("next", 0, 1);
    broker.set_task_version(first_v.clone());
    broker.set_task_version(next_v.clone());
    broker
        .enqueue(work(
            "build:first:0",
            WorkRole::Build,
            WorkPriority::Implementation,
            first_v,
        ))
        .unwrap();
    broker
        .enqueue(work(
            "build:next:0",
            WorkRole::Build,
            WorkPriority::Implementation,
            next_v,
        ))
        .unwrap();

    let first = broker.admit_next().unwrap();
    broker
        .record_local_completion(&first.request_id, LocalCompletionKind::StreamDropped)
        .unwrap();
    assert!(
        broker.admit_next().is_none(),
        "local stream loss must not create phantom-free capacity"
    );

    let wrong_terminal = ProviderTerminalReceipt {
        request_id: first.request_id.clone(),
        provider_request_id: "wrong-provider-request".to_string(),
        physical_host_id: first.physical_host_id.clone(),
        model_instance_id: first.model_instance_id.clone(),
        kind: ProviderTerminalKind::Failed,
    };
    assert!(broker.observe_provider_terminal(wrong_terminal).is_err());
    assert_eq!(broker.active_len(), 1);
    assert!(broker.admit_next().is_none());
}
