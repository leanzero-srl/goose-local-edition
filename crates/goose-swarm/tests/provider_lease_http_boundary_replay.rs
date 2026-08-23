use goose_provider_types::base::{expose_current_provider_http_request, ProviderHttpProtocol};
use goose_swarm::{
    AuthorityScope, ExposedProviderLease, GlobalProviderLeaseAuthority, HostCapacityEvidence,
    LocalCompletionKind, NullSink, PhysicalAdmissionControl, PhysicalFleetSnapshot,
    PhysicalProviderLeaseAuthority, ProviderLeaseError, ProviderLeaseTransitionError,
    ProviderLeaseTry, ProviderLeaseWaitPolicy, ProviderLifecycleJournal,
    ProviderLifecycleOperationError, ProviderLifecycleStartError, ProviderRequestReceipt,
    ProviderTerminalKind, ProviderTerminalReceipt, ReservedProviderLease,
    RunScopedProviderLeaseAuthority, SealedProviderLeaseAuthority, SourceRevisionKind, TaskVersion,
    VerifiedPhysicalLane, VerifiedProviderProtocolRoute, WorkOpportunity, WorkPriority, WorkRole,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const TRANSPORT: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_TRANSPORT: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct NullJournal;

impl ProviderLifecycleJournal for NullJournal {
    fn provider_request_started(&self, _receipt: &ProviderRequestReceipt) -> Result<(), String> {
        Ok(())
    }

    fn provider_terminal(&self, _receipt: &ProviderTerminalReceipt) -> Result<(), String> {
        Ok(())
    }
}

struct StartSignalJournal {
    started: AtomicBool,
}

impl ProviderLifecycleJournal for StartSignalJournal {
    fn provider_request_started(&self, _receipt: &ProviderRequestReceipt) -> Result<(), String> {
        self.started.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn provider_terminal(&self, _receipt: &ProviderTerminalReceipt) -> Result<(), String> {
        Ok(())
    }
}

struct RetryTerminalOnce {
    inner: Arc<GlobalProviderLeaseAuthority>,
    attempts: AtomicUsize,
}

impl PhysicalProviderLeaseAuthority for RetryTerminalOnce {
    fn try_reserve(
        &self,
        claim: goose_swarm::ProviderLeaseClaim,
    ) -> Result<ProviderLeaseTry, ProviderLeaseError> {
        self.inner.try_reserve(claim)
    }

    fn expose(
        &self,
        reserved: ReservedProviderLease,
    ) -> Result<ExposedProviderLease, ProviderLeaseTransitionError<ReservedProviderLease>> {
        self.inner.expose(reserved)
    }

    fn abandon_reserved(
        &self,
        reserved: ReservedProviderLease,
        reason: &str,
    ) -> Result<
        goose_swarm::ProviderLeaseReleaseReceipt,
        ProviderLeaseTransitionError<ReservedProviderLease>,
    > {
        self.inner.abandon_reserved(reserved, reason)
    }

    fn provider_terminal(
        &self,
        exposed: ExposedProviderLease,
        terminal: &ProviderTerminalReceipt,
    ) -> Result<
        goose_swarm::ProviderLeaseReleaseReceipt,
        ProviderLeaseTransitionError<ExposedProviderLease>,
    > {
        if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(ProviderLeaseTransitionError::Retryable {
                error: ProviderLeaseError::AuthorityContended,
                handle: Box::new(exposed),
            });
        }
        self.inner.provider_terminal(exposed, terminal)
    }
}

fn fleet() -> PhysicalFleetSnapshot {
    PhysicalFleetSnapshot::new(
        "provider-http-boundary-fleet",
        vec![VerifiedPhysicalLane {
            logical_device_id: "device-a".to_string(),
            model_id: "model-a".to_string(),
            host_id: "host-a".to_string(),
            model_instance_id: "instance-a".to_string(),
            provider_transport_id: TRANSPORT.to_string(),
            advertised_instance_capacity: 1,
            routing_weight: 1,
            capacity_evidence: HostCapacityEvidence::MeasuredProfile {
                profile_hash: "profile-http-boundary".to_string(),
                profile_key: "runtime:model:context:http-boundary".to_string(),
                max_concurrent: 1,
            },
            route_evidence_id: "route-http-boundary".to_string(),
        }],
    )
    .unwrap()
}

fn runtime(
    physical: Arc<dyn PhysicalProviderLeaseAuthority>,
    snapshot: &PhysicalFleetSnapshot,
) -> RunScopedProviderLeaseAuthority {
    let sealed = SealedProviderLeaseAuthority::from_fleet_snapshot(
        snapshot,
        [VerifiedProviderProtocolRoute::new(
            TRANSPORT,
            ProviderHttpProtocol::OpenAiChatCompletions,
        )
        .unwrap()],
    )
    .unwrap();
    RunScopedProviderLeaseAuthority::new_with_wait_policy(
        physical,
        sealed,
        ProviderLeaseWaitPolicy::new(Duration::from_millis(1)),
    )
}

fn control(
    scope: &str,
    snapshot: PhysicalFleetSnapshot,
    runtime: RunScopedProviderLeaseAuthority,
    journal: Arc<dyn ProviderLifecycleJournal>,
) -> PhysicalAdmissionControl {
    PhysicalAdmissionControl::new_with_journal_and_provider_leases(
        scope,
        snapshot,
        Arc::new(NullSink),
        journal,
        Some(runtime),
    )
    .unwrap()
}

fn source(scope: &str) -> TaskVersion {
    TaskVersion {
        authority_scope: AuthorityScope::new(scope, "build"),
        phase_epoch: 0,
        task_id: format!("task-{scope}"),
        attempt: 0,
        revision: 1,
        kind: SourceRevisionKind::TaskAttempt,
    }
}

fn work(scope: &str) -> WorkOpportunity {
    WorkOpportunity {
        work_id: format!("work-{scope}"),
        role: WorkRole::Build,
        priority: WorkPriority::Implementation,
        task_rank: 1,
        source: source(scope),
        eligible_logical_device_ids: Vec::new(),
        preferred_model_id: Some("model-a".to_string()),
        excluded_logical_device_id: None,
    }
}

async fn admitted(control: &PhysicalAdmissionControl, scope: &str) -> goose_swarm::AdmittedWork {
    control.set_source_revision(source(scope)).await.unwrap();
    control.admit(work(scope)).await.unwrap()
}

async fn expose(started: &goose_swarm::StartedProviderRequest) -> Result<(), String> {
    started
        .scope_http(async {
            expose_current_provider_http_request(
                ProviderHttpProtocol::OpenAiChatCompletions,
                TRANSPORT,
            )
        })
        .await
}

fn open_global(temp: &tempfile::TempDir) -> Arc<GlobalProviderLeaseAuthority> {
    Arc::new(GlobalProviderLeaseAuthority::open_test_root(temp.path().join("authority")).unwrap())
}

#[tokio::test]
async fn terminal_without_http_exposure_fails_closed_and_keeps_capacity() {
    let temp = tempfile::tempdir().unwrap();
    let global = open_global(&temp);
    let snapshot = fleet();
    let control = control(
        "no-exposure",
        snapshot.clone(),
        runtime(global.clone(), &snapshot),
        Arc::new(NullJournal),
    );
    let admitted = admitted(&control, "no-exposure").await;
    let lifecycle = admitted.lifecycle();
    assert!(lifecycle
        .provider_request_started("caller-forged-request")
        .await
        .unwrap_err()
        .to_string()
        .contains("caller-supplied"));
    let started = lifecycle.start_provider_request().await.unwrap();

    assert!(started
        .receipt()
        .key
        .provider_request_id
        .starts_with("engine-provider-request:"));
    assert!(lifecycle
        .provider_terminal(
            started.receipt().key.clone(),
            ProviderTerminalKind::Finished
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("caller-constructed"));
    let error = started
        .provider_terminal(ProviderTerminalKind::Finished)
        .await
        .unwrap_err();
    assert!(matches!(
        error.error(),
        ProviderLifecycleOperationError::Lease(ProviderLeaseError::InvalidTransition(_))
    ));
    let active = &global.snapshot().unwrap().active;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].status, goose_swarm::ProviderLeaseStatus::Reserved);
}

#[tokio::test]
async fn wrong_protocol_or_transport_never_exposes_and_reserved_can_be_abandoned() {
    for (protocol, transport) in [
        (ProviderHttpProtocol::OpenAiResponses, TRANSPORT),
        (ProviderHttpProtocol::OpenAiChatCompletions, OTHER_TRANSPORT),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let global = open_global(&temp);
        let snapshot = fleet();
        let control = control(
            "wrong-route",
            snapshot.clone(),
            runtime(global.clone(), &snapshot),
            Arc::new(NullJournal),
        );
        let admitted = admitted(&control, "wrong-route").await;
        let started = admitted.lifecycle().start_provider_request().await.unwrap();

        let result = started
            .scope_http(async { expose_current_provider_http_request(protocol, transport) })
            .await;
        assert!(result.unwrap_err().contains("receipt mismatch"));
        assert_eq!(
            global.snapshot().unwrap().active[0].status,
            goose_swarm::ProviderLeaseStatus::Reserved
        );
        started
            .abandon_before_exposure("verified route mismatch before POST")
            .await
            .unwrap();
        assert!(global.snapshot().unwrap().active.is_empty());
        admitted
            .complete_local(LocalCompletionKind::Error)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn a_second_post_under_one_engine_request_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let global = open_global(&temp);
    let snapshot = fleet();
    let control = control(
        "double-exposure",
        snapshot.clone(),
        runtime(global.clone(), &snapshot),
        Arc::new(NullJournal),
    );
    let admitted = admitted(&control, "double-exposure").await;
    let lifecycle = admitted.lifecycle();
    let started = lifecycle.start_provider_request().await.unwrap();
    let copied_key = started.receipt().key.clone();

    expose(&started).await.unwrap();
    let duplicate = expose(&started).await.unwrap_err();
    assert!(duplicate.contains("second POST is unsafe"));
    assert_eq!(
        global.snapshot().unwrap().active[0].status,
        goose_swarm::ProviderLeaseStatus::Exposed
    );
    started
        .provider_terminal(ProviderTerminalKind::Finished)
        .await
        .unwrap();
    assert!(lifecycle
        .provider_terminal(copied_key, ProviderTerminalKind::Finished)
        .await
        .unwrap_err()
        .to_string()
        .contains("caller-constructed"));
    assert!(global.snapshot().unwrap().active.is_empty());
    admitted
        .complete_local(LocalCompletionKind::Success)
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_exposed_http_scope_stays_occupied_and_cannot_be_reposted() {
    let temp = tempfile::tempdir().unwrap();
    let global = open_global(&temp);
    let snapshot = fleet();
    let control = control(
        "cancelled-post",
        snapshot.clone(),
        runtime(global.clone(), &snapshot),
        Arc::new(NullJournal),
    );
    let admitted = admitted(&control, "cancelled-post").await;
    let lifecycle = admitted.lifecycle();
    let started = lifecycle.start_provider_request().await.unwrap();
    let (visible, visible_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        started
            .scope_http(async move {
                expose_current_provider_http_request(
                    ProviderHttpProtocol::OpenAiChatCompletions,
                    TRANSPORT,
                )
                .unwrap();
                visible.send(()).unwrap();
                std::future::pending::<()>().await;
            })
            .await;
    });
    visible_rx.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());

    assert_eq!(
        global.snapshot().unwrap().active[0].status,
        goose_swarm::ProviderLeaseStatus::Exposed
    );
    assert!(matches!(
        lifecycle.start_provider_request().await,
        Err(ProviderLifecycleStartError::Operation(
            ProviderLifecycleOperationError::Unresolved(_)
        ))
    ));
    assert_eq!(global.snapshot().unwrap().active.len(), 1);
}

#[tokio::test]
async fn cancellation_while_waiting_for_global_capacity_reuses_the_exact_start() {
    let temp = tempfile::tempdir().unwrap();
    let first_global = open_global(&temp);
    let second_global = open_global(&temp);
    let snapshot = fleet();
    let first = control(
        "first-control",
        snapshot.clone(),
        runtime(first_global.clone(), &snapshot),
        Arc::new(NullJournal),
    );
    let signal = Arc::new(StartSignalJournal {
        started: AtomicBool::new(false),
    });
    let second = control(
        "second-control",
        snapshot.clone(),
        runtime(second_global, &snapshot),
        signal.clone(),
    );
    let first_admitted = admitted(&first, "first-control").await;
    let first_started = first_admitted
        .lifecycle()
        .start_provider_request()
        .await
        .unwrap();
    expose(&first_started).await.unwrap();

    let second_admitted = admitted(&second, "second-control").await;
    let second_lifecycle = second_admitted.lifecycle();
    let waiting = tokio::spawn({
        let lifecycle = second_lifecycle.clone();
        async move { lifecycle.start_provider_request().await }
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !signal.started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    waiting.abort();
    assert!(waiting.await.unwrap_err().is_cancelled());

    first_started
        .provider_terminal(ProviderTerminalKind::Finished)
        .await
        .unwrap();
    first_admitted
        .complete_local(LocalCompletionKind::Success)
        .await
        .unwrap();
    let resumed = tokio::time::timeout(
        Duration::from_secs(2),
        second_lifecycle.start_provider_request(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(resumed.receipt().key.ordinal, 0);
    resumed
        .abandon_before_exposure("cancelled capacity waiter resumed exactly")
        .await
        .unwrap();
    second_admitted
        .complete_local(LocalCompletionKind::Error)
        .await
        .unwrap();
    assert!(first_global.snapshot().unwrap().active.is_empty());
}

#[tokio::test]
async fn retryable_terminal_persistence_failure_returns_the_live_request_for_exact_retry() {
    let temp = tempfile::tempdir().unwrap();
    let global = open_global(&temp);
    let retrying = Arc::new(RetryTerminalOnce {
        inner: global.clone(),
        attempts: AtomicUsize::new(0),
    });
    let snapshot = fleet();
    let control = control(
        "retry-terminal",
        snapshot.clone(),
        runtime(retrying.clone(), &snapshot),
        Arc::new(NullJournal),
    );
    let admitted = admitted(&control, "retry-terminal").await;
    let started = admitted.lifecycle().start_provider_request().await.unwrap();
    expose(&started).await.unwrap();

    let error = started
        .provider_terminal(ProviderTerminalKind::Finished)
        .await
        .unwrap_err();
    assert_eq!(
        error.error().to_string(),
        ProviderLeaseError::AuthorityContended.to_string()
    );
    let started = error
        .into_retryable_request()
        .expect("retryable persistence failure must return engine ownership");
    assert_eq!(
        global.snapshot().unwrap().active[0].status,
        goose_swarm::ProviderLeaseStatus::Exposed
    );
    started
        .provider_terminal(ProviderTerminalKind::Finished)
        .await
        .unwrap();
    assert_eq!(retrying.attempts.load(Ordering::SeqCst), 2);
    assert!(global.snapshot().unwrap().active.is_empty());
    admitted
        .complete_local(LocalCompletionKind::Success)
        .await
        .unwrap();
}
