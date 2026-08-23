use goose_swarm::{
    AuthorityScope, BrokerError, HostCapacityEvidence, NullSink, PhysicalAdmissionControl,
    PhysicalFleetSnapshot, ProviderLifecycleJournal, ProviderRequestReceipt, ProviderTerminalKind,
    ProviderTerminalReceipt, SourceRevisionKind, TaskVersion, VerifiedPhysicalLane,
    WorkOpportunity, WorkPriority, WorkRole,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const TRANSPORT: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct FailingJournal {
    fail_start: bool,
    starts: AtomicUsize,
    terminals: AtomicUsize,
}

impl ProviderLifecycleJournal for FailingJournal {
    fn provider_request_started(&self, _receipt: &ProviderRequestReceipt) -> Result<(), String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        if self.fail_start {
            Err("start fsync failed".to_string())
        } else {
            Ok(())
        }
    }

    fn provider_terminal(&self, _receipt: &ProviderTerminalReceipt) -> Result<(), String> {
        self.terminals.fetch_add(1, Ordering::SeqCst);
        Err("terminal fsync failed".to_string())
    }
}

fn control(journal: Arc<dyn ProviderLifecycleJournal>) -> PhysicalAdmissionControl {
    let snapshot = PhysicalFleetSnapshot::new(
        "journal-replay-fleet",
        vec![VerifiedPhysicalLane {
            logical_device_id: "device-a".to_string(),
            model_id: "model-a".to_string(),
            host_id: "host-a".to_string(),
            model_instance_id: "instance-a".to_string(),
            provider_transport_id: TRANSPORT.to_string(),
            advertised_instance_capacity: 1,
            routing_weight: 1,
            capacity_evidence: HostCapacityEvidence::MeasuredProfile {
                profile_hash: "fixture:journal-replay".to_string(),
                profile_key: "test-runtime:model:context:journal".to_string(),
                max_concurrent: 1,
            },
            route_evidence_id: "fixture-route:journal-replay".to_string(),
        }],
    )
    .unwrap();
    PhysicalAdmissionControl::new_with_journal(
        "journal-replay",
        snapshot,
        Arc::new(NullSink),
        journal,
    )
    .unwrap()
}

fn source(task_id: &str) -> TaskVersion {
    TaskVersion {
        authority_scope: AuthorityScope::new("journal-replay", "main"),
        phase_epoch: 0,
        task_id: task_id.to_string(),
        attempt: 0,
        revision: 1,
        kind: SourceRevisionKind::TaskAttempt,
    }
}

fn work(task_id: &str) -> WorkOpportunity {
    WorkOpportunity {
        work_id: format!("build:{task_id}:0"),
        role: WorkRole::Build,
        priority: WorkPriority::Implementation,
        task_rank: 0,
        source: source(task_id),
        eligible_logical_device_ids: Vec::new(),
        preferred_model_id: None,
        excluded_logical_device_id: None,
    }
}

async fn wait_for_queued_and_active(control: &PhysicalAdmissionControl) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if control.occupancy().await == (1, 1) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("second admission must queue behind the first");
}

#[tokio::test]
async fn failed_start_fsync_latches_control_before_http_and_rejects_queued_work() {
    let journal = Arc::new(FailingJournal {
        fail_start: true,
        starts: AtomicUsize::new(0),
        terminals: AtomicUsize::new(0),
    });
    let control = control(journal.clone());
    control.set_source_revision(source("first")).await.unwrap();
    control.set_source_revision(source("second")).await.unwrap();
    let first = control.admit(work("first")).await.unwrap();
    let queued = tokio::spawn({
        let control = control.clone();
        async move { control.admit(work("second")).await }
    });
    wait_for_queued_and_active(&control).await;

    let error = first
        .lifecycle()
        .provider_request_started("request-first")
        .await
        .unwrap_err();
    assert!(matches!(error, BrokerError::ProviderLifecycleJournal(_)));
    assert_eq!(journal.starts.load(Ordering::SeqCst), 1);

    let queued_result = tokio::time::timeout(Duration::from_secs(2), queued)
        .await
        .expect("queued admission must be rejected when the journal degrades")
        .unwrap();
    let queued_error = match queued_result {
        Ok(_) => panic!("queued admission was granted after journal failure"),
        Err(error) => error,
    };
    assert!(matches!(
        queued_error,
        BrokerError::ProviderLifecycleJournal(_)
    ));
    assert!(matches!(
        control.set_source_revision(source("third")).await,
        Err(BrokerError::ProviderLifecycleJournal(_))
    ));
    assert_eq!(journal.starts.load(Ordering::SeqCst), 1);
    assert!(matches!(
        control.wait_until_drained().await,
        Err(BrokerError::ProviderLifecycleJournal(_))
    ));
}

#[tokio::test]
async fn failed_terminal_fsync_never_releases_or_allows_later_journal_transitions() {
    let journal = Arc::new(FailingJournal {
        fail_start: false,
        starts: AtomicUsize::new(0),
        terminals: AtomicUsize::new(0),
    });
    let control = control(journal.clone());
    control.set_source_revision(source("first")).await.unwrap();
    let admitted = control.admit(work("first")).await.unwrap();
    let lifecycle = admitted.lifecycle();
    let key = lifecycle
        .provider_request_started("request-first")
        .await
        .unwrap();

    let error = lifecycle
        .provider_terminal(key, ProviderTerminalKind::Finished)
        .await
        .unwrap_err();
    assert!(matches!(error, BrokerError::ProviderLifecycleJournal(_)));
    assert_eq!(journal.starts.load(Ordering::SeqCst), 1);
    assert_eq!(journal.terminals.load(Ordering::SeqCst), 1);
    assert!(matches!(
        lifecycle.provider_request_started("request-second").await,
        Err(BrokerError::ProviderLifecycleJournal(_))
    ));
    assert_eq!(journal.starts.load(Ordering::SeqCst), 1);
    assert!(matches!(
        control.wait_until_drained().await,
        Err(BrokerError::ProviderLifecycleJournal(_))
    ));
}
