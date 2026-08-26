use async_trait::async_trait;
use goose_swarm::{
    AdmissionReceipt, AdmittedWork, AuthorityScope, BrokerError, Dag, DeviceCfg, Difficulty,
    DispatchError, DispatchRequest, EventSink, HostCapacityEvidence, Judge, JudgeConfig,
    JudgeOutcome, JudgeRequest, LocalCompletionKind, PhysicalAdmissionControl,
    PhysicalExecutionAuthority, PhysicalFleetSnapshot, ProviderDispatchClass, ProviderLifecycle,
    ProviderLifecycleDispatcher, ProviderRequestKey, ProviderTerminalKind, Scheduler,
    SourceRevisionKind, SwarmEvent, TaskRunOutput, TaskSpec, TaskVersion, VerifiedPhysicalLane,
    WorkOpportunity, WorkPriority, WorkRole,
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{oneshot, Notify};

const VERIFIED_TRANSPORT: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<serde_json::Value>>,
}

impl EventSink for RecordingSink {
    fn emit(&self, event: &SwarmEvent) {
        self.events
            .lock()
            .unwrap()
            .push(serde_json::to_value(event).unwrap());
    }
}

fn lane(
    device: &str,
    model: &str,
    host: &str,
    instance: &str,
    host_capacity: u32,
) -> VerifiedPhysicalLane {
    VerifiedPhysicalLane {
        logical_device_id: device.to_string(),
        model_id: model.to_string(),
        host_id: host.to_string(),
        model_instance_id: instance.to_string(),
        provider_transport_id: VERIFIED_TRANSPORT.to_string(),
        advertised_instance_capacity: host_capacity.max(1),
        routing_weight: 1,
        capacity_evidence: HostCapacityEvidence::MeasuredProfile {
            profile_hash: format!("fixture:{host}:{host_capacity}"),
            profile_key: "test-runtime:model:context:role".to_string(),
            max_concurrent: host_capacity,
        },
        route_evidence_id: format!("fixture-route:{host}:{instance}"),
    }
}

fn control(
    scope: &str,
    lanes: Vec<VerifiedPhysicalLane>,
    sink: Arc<dyn EventSink>,
) -> PhysicalAdmissionControl {
    let snapshot = PhysicalFleetSnapshot::new(format!("snapshot:{scope}"), lanes).unwrap();
    PhysicalAdmissionControl::new(scope, snapshot, sink).unwrap()
}

fn device(id: &str, model: &str, weight: u32) -> DeviceCfg {
    DeviceCfg {
        id: id.to_string(),
        model_id: model.to_string(),
        weight,
        enabled: true,
        speed_weight: 1,
        supervision: false,
    }
}

fn spec(id: &str, deps: &[&str]) -> TaskSpec {
    TaskSpec {
        id: id.to_string(),
        description: format!("implement {id}"),
        difficulty: Difficulty::Easy,
        preferred_model: None,
        owned_files: vec![format!("{id}.txt")],
        deps: deps.iter().map(|dep| dep.to_string()).collect(),
        subsplit: Vec::new(),
        replan_authority: None,
    }
}

fn repair(task: &str, attempt: u32) -> TaskVersion {
    TaskVersion {
        authority_scope: fixture_scope(),
        phase_epoch: 0,
        task_id: task.to_string(),
        attempt,
        revision: u64::from(attempt) + 1,
        kind: SourceRevisionKind::TaskAttempt,
    }
}

fn build(task: &str) -> TaskVersion {
    TaskVersion {
        authority_scope: fixture_scope(),
        phase_epoch: 0,
        task_id: task.to_string(),
        attempt: 0,
        revision: 1,
        kind: SourceRevisionKind::TaskAttempt,
    }
}

fn artifact(task: &str, revision: u64) -> TaskVersion {
    TaskVersion {
        authority_scope: fixture_scope(),
        phase_epoch: 0,
        task_id: task.to_string(),
        attempt: 0,
        revision,
        kind: SourceRevisionKind::Artifact {
            snapshot_hash: format!("sha256:{task}:{revision}"),
        },
    }
}

fn fixture_scope() -> AuthorityScope {
    AuthorityScope::new("physical-control-replay", "main")
}

fn scoped_build(scope: AuthorityScope, phase_epoch: u64, task: &str) -> TaskVersion {
    TaskVersion {
        authority_scope: scope,
        phase_epoch,
        task_id: task.to_string(),
        attempt: 0,
        revision: 1,
        kind: SourceRevisionKind::TaskAttempt,
    }
}

fn execution(role: WorkRole) -> PhysicalExecutionAuthority {
    PhysicalExecutionAuthority::new(
        AuthorityScope::new("physical-scheduler-replay", "execute"),
        0,
        role,
    )
}

fn opportunity(
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

async fn finish(admitted: &AdmittedWork, request_id: &str) {
    let lifecycle = admitted.lifecycle();
    let key = lifecycle
        .provider_request_started(request_id)
        .await
        .unwrap();
    lifecycle
        .provider_terminal(key, ProviderTerminalKind::Finished)
        .await
        .unwrap();
    admitted
        .complete_local(LocalCompletionKind::Success)
        .await
        .unwrap();
}

async fn wait_for_occupancy(control: &PhysicalAdmissionControl, expected: (usize, usize)) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if control.occupancy().await == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("control plane never reached occupancy {expected:?}"));
}

struct LifecycleRecorder {
    calls: Mutex<Vec<String>>,
    current: AtomicUsize,
    peak: AtomicUsize,
    withheld: Mutex<Option<(ProviderLifecycle, ProviderRequestKey)>>,
    withheld_ready: Notify,
}

impl LifecycleRecorder {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            current: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            withheld: Mutex::new(None),
            withheld_ready: Notify::new(),
        }
    }

    async fn release_withheld(&self) {
        loop {
            let held = { self.withheld.lock().unwrap().take() };
            if let Some((lifecycle, key)) = held {
                lifecycle
                    .provider_terminal(key, ProviderTerminalKind::Finished)
                    .await
                    .unwrap();
                return;
            }
            self.withheld_ready.notified().await;
        }
    }
}

struct LifecycleMock {
    recorder: Arc<LifecycleRecorder>,
    delay: Duration,
    withhold_terminal_for: Option<String>,
}

#[derive(Default)]
struct ProviderClassMock {
    provider_free_calls: AtomicUsize,
    admitted_calls: AtomicUsize,
}

#[async_trait]
impl ProviderLifecycleDispatcher for ProviderClassMock {
    fn provider_dispatch_class(&self, req: &DispatchRequest) -> ProviderDispatchClass {
        if req.task_id.starts_with("skeleton::") || req.task_id.starts_with("join::") {
            ProviderDispatchClass::DeterministicProviderFree
        } else {
            ProviderDispatchClass::ProviderRequired
        }
    }

    async fn run_provider_free(
        &self,
        req: DispatchRequest,
    ) -> Result<TaskRunOutput, DispatchError> {
        self.provider_free_calls.fetch_add(1, Ordering::SeqCst);
        Ok(format!("local:{}", req.task_id).into())
    }

    async fn run_admitted(
        &self,
        req: DispatchRequest,
        _admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        self.admitted_calls.fetch_add(1, Ordering::SeqCst);
        let key = lifecycle
            .provider_request_started(format!("provider:{}", req.task_id))
            .await
            .unwrap();
        lifecycle
            .provider_terminal(key, ProviderTerminalKind::Finished)
            .await
            .unwrap();
        Ok(format!("provider:{}", req.task_id).into())
    }
}

struct ToolGapSignals {
    first_turn_finished: Notify,
    allow_second_turn: Notify,
    other_task_finished: Notify,
    live_provider_turns: AtomicUsize,
    peak_provider_turns: AtomicUsize,
}

struct ToolGapDispatcher {
    signals: Arc<ToolGapSignals>,
}

impl ToolGapDispatcher {
    async fn provider_turn(&self, lifecycle: &ProviderLifecycle, request_id: &str) {
        let key = lifecycle
            .provider_request_started(request_id)
            .await
            .unwrap();
        let live = self
            .signals
            .live_provider_turns
            .fetch_add(1, Ordering::SeqCst)
            + 1;
        self.signals
            .peak_provider_turns
            .fetch_max(live, Ordering::SeqCst);
        tokio::task::yield_now().await;
        lifecycle
            .provider_terminal(key, ProviderTerminalKind::Finished)
            .await
            .unwrap();
        self.signals
            .live_provider_turns
            .fetch_sub(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl ProviderLifecycleDispatcher for ToolGapDispatcher {
    async fn run_admitted(
        &self,
        req: DispatchRequest,
        _admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        if req.task_id == "a" {
            self.provider_turn(&lifecycle, "provider:a:0").await;
            self.signals.first_turn_finished.notify_one();
            self.signals.allow_second_turn.notified().await;
            self.provider_turn(&lifecycle, "provider:a:1").await;
        } else {
            self.provider_turn(&lifecycle, "provider:b:0").await;
            self.signals.other_task_finished.notify_one();
        }
        Ok(format!("output:{}", req.task_id).into())
    }
}

struct FailedTerminalDispatcher;

#[async_trait]
impl ProviderLifecycleDispatcher for FailedTerminalDispatcher {
    async fn run_admitted(
        &self,
        _req: DispatchRequest,
        _admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        let key = lifecycle
            .provider_request_started("provider:failed")
            .await
            .unwrap();
        lifecycle
            .provider_terminal(key, ProviderTerminalKind::Failed)
            .await
            .unwrap();
        Ok("claimed-success".to_string().into())
    }
}

struct RecoveredTerminalDispatcher;

#[async_trait]
impl ProviderLifecycleDispatcher for RecoveredTerminalDispatcher {
    async fn run_admitted(
        &self,
        _req: DispatchRequest,
        _admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        let failed = lifecycle
            .provider_request_started("provider:failed")
            .await
            .unwrap();
        lifecycle
            .provider_terminal(failed, ProviderTerminalKind::Failed)
            .await
            .unwrap();
        let recovered = lifecycle
            .provider_request_started("provider:recovered")
            .await
            .unwrap();
        lifecycle
            .provider_terminal(recovered, ProviderTerminalKind::Finished)
            .await
            .unwrap();
        Ok("recovered-success".to_string().into())
    }
}

struct AdmissionRoleRecorder {
    seen: Mutex<Vec<(WorkRole, u32, AuthorityScope, u64)>>,
    transient_first: bool,
}

impl AdmissionRoleRecorder {
    fn new(transient_first: bool) -> Self {
        Self {
            seen: Mutex::new(Vec::new()),
            transient_first,
        }
    }
}

#[async_trait]
impl ProviderLifecycleDispatcher for AdmissionRoleRecorder {
    async fn run_admitted(
        &self,
        req: DispatchRequest,
        admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        self.seen.lock().unwrap().push((
            admission.role,
            admission.source.attempt,
            admission.source.authority_scope.clone(),
            admission.source.phase_epoch,
        ));
        let key = lifecycle
            .provider_request_started(format!("provider:role:{}", req.attempt))
            .await
            .unwrap();
        lifecycle
            .provider_terminal(key, ProviderTerminalKind::Finished)
            .await
            .unwrap();
        if self.transient_first && req.attempt == 0 {
            Err(DispatchError::Transient(
                "role-preserving retry fixture".to_string(),
            ))
        } else {
            Ok("role-recorded".to_string().into())
        }
    }
}

struct LateStartDispatcher {
    gate: Arc<Notify>,
    result: Mutex<Option<oneshot::Sender<Result<(), String>>>>,
}

struct QueuedCloseSignals {
    second_task_started: Notify,
    allow_first_task_return: Notify,
    allow_second_task_terminal: Notify,
    late_result: Mutex<Option<oneshot::Sender<Result<(), String>>>>,
}

struct QueuedCloseDispatcher {
    signals: Arc<QueuedCloseSignals>,
}

#[async_trait]
impl ProviderLifecycleDispatcher for LateStartDispatcher {
    async fn run_admitted(
        &self,
        _req: DispatchRequest,
        _admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        let gate = self.gate.clone();
        let sender = self.result.lock().unwrap().take().unwrap();
        tokio::spawn(async move {
            gate.notified().await;
            let result = lifecycle
                .provider_request_started("provider:too-late")
                .await
                .map(|_| ())
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        Ok("returned-before-provider".to_string().into())
    }
}

#[async_trait]
impl ProviderLifecycleDispatcher for QueuedCloseDispatcher {
    async fn run_admitted(
        &self,
        req: DispatchRequest,
        _admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        if req.task_id == "a" {
            let first = lifecycle
                .provider_request_started("provider:a:0")
                .await
                .unwrap();
            lifecycle
                .provider_terminal(first, ProviderTerminalKind::Finished)
                .await
                .unwrap();
            self.signals.second_task_started.notified().await;

            let sender = self.signals.late_result.lock().unwrap().take().unwrap();
            tokio::spawn(async move {
                let result = lifecycle
                    .provider_request_started("provider:a:late")
                    .await
                    .map(|_| ())
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            });
            self.signals.allow_first_task_return.notified().await;
        } else {
            let key = lifecycle
                .provider_request_started("provider:b:0")
                .await
                .unwrap();
            self.signals.second_task_started.notify_one();
            self.signals.allow_second_task_terminal.notified().await;
            lifecycle
                .provider_terminal(key, ProviderTerminalKind::Finished)
                .await
                .unwrap();
        }
        Ok(format!("output:{}", req.task_id).into())
    }
}

#[async_trait]
impl ProviderLifecycleDispatcher for LifecycleMock {
    async fn run_admitted(
        &self,
        req: DispatchRequest,
        admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        assert_eq!(admission.logical_device_id, req.device_id);
        assert_eq!(admission.model_id, req.model_id);
        self.recorder
            .calls
            .lock()
            .unwrap()
            .push(req.task_id.clone());
        let current = self.recorder.current.fetch_add(1, Ordering::SeqCst) + 1;
        self.recorder.peak.fetch_max(current, Ordering::SeqCst);
        let key = lifecycle
            .provider_request_started(format!("provider:{}", req.task_id))
            .await
            .unwrap();
        tokio::time::sleep(self.delay).await;
        if self.withhold_terminal_for.as_deref() == Some(req.task_id.as_str()) {
            *self.recorder.withheld.lock().unwrap() = Some((lifecycle, key));
            self.recorder.withheld_ready.notify_one();
        } else {
            lifecycle
                .provider_terminal(key, ProviderTerminalKind::Finished)
                .await
                .unwrap();
        }
        self.recorder.current.fetch_sub(1, Ordering::SeqCst);
        Ok(format!("output:{}", req.task_id).into())
    }
}

struct PhysicalDegradedDispatcher {
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl ProviderLifecycleDispatcher for PhysicalDegradedDispatcher {
    async fn run_admitted(
        &self,
        req: DispatchRequest,
        _admission: AdmissionReceipt,
        lifecycle: ProviderLifecycle,
    ) -> Result<TaskRunOutput, DispatchError> {
        self.calls.lock().unwrap().push(req.task_id.clone());
        let key = lifecycle
            .provider_request_started(format!("provider:{}", req.task_id))
            .await
            .unwrap();
        lifecycle
            .provider_terminal(key, ProviderTerminalKind::Finished)
            .await
            .unwrap();
        if req.task_id == "missing" {
            return Err(DispatchError::ContentRetry(
                "ordinary model response omitted its task".to_string(),
            ));
        }
        for file in &req.owned_files {
            let path = std::path::Path::new(file);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, format!("{} output", req.task_id)).unwrap();
        }
        Ok(format!("output:{}", req.task_id).into())
    }
}

#[tokio::test]
async fn physical_exhaustion_continues_through_a_deep_provisional_chain() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "physical-degraded-chain",
        vec![
            lane("lane-a", "model-a", "host-a", "instance-a", 1),
            lane("lane-b", "model-b", "host-b", "instance-b", 1),
        ],
        sink.clone(),
    );
    let mut tasks = vec![
        spec("missing", &[]),
        spec("dependent", &["missing"]),
        spec("deep", &["dependent"]),
        spec("independent", &[]),
    ];
    for task in &mut tasks {
        task.owned_files = vec![format!("target/physical-degraded/{}.txt", task.id)];
        let _ = std::fs::remove_file(&task.owned_files[0]);
    }
    let dispatcher = Arc::new(PhysicalDegradedDispatcher {
        calls: Mutex::new(Vec::new()),
    });
    let report = Scheduler::new(
        vec![
            device("lane-a", "model-a", 1),
            device("lane-b", "model-b", 1),
        ],
        1,
    )
    .with_sink(sink)
    .with_unavailable_continuation()
    .run_with_physical_admission(
        Dag::from_specs(tasks).unwrap(),
        dispatcher.clone(),
        control.clone(),
        execution(WorkRole::Build),
        String::new(),
        String::new(),
    )
    .await
    .unwrap();

    assert_eq!(report.unavailable.len(), 1);
    assert_eq!(report.provisional, vec!["deep", "dependent"]);
    assert_eq!(report.done, vec!["independent"]);
    assert_eq!(dispatcher.calls.lock().unwrap().len(), 4);
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test]
async fn scheduled_dag_and_provider_calls_are_identical_on_one_physical_host() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "scheduled-identity",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 2)],
        sink.clone(),
    );
    let specs = vec![spec("a", &[]), spec("b", &[]), spec("join", &["a", "b"])];
    let mut advertised: Vec<String> = specs.iter().map(|task| task.id.clone()).collect();
    advertised.sort();
    let dag = Dag::from_specs(specs).unwrap();
    let recorder = Arc::new(LifecycleRecorder::new());
    let dispatcher = Arc::new(LifecycleMock {
        recorder: recorder.clone(),
        delay: Duration::from_millis(5),
        withhold_terminal_for: None,
    });
    let report = Scheduler::new(vec![device("lane-a", "model-a", 3)], 2)
        .with_sink(sink.clone())
        .run_with_physical_admission(
            dag,
            dispatcher,
            control.clone(),
            execution(WorkRole::Build),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();

    let mut called = recorder.calls.lock().unwrap().clone();
    called.sort();
    let mut done = report.done;
    done.sort();
    assert_eq!(called, advertised);
    assert_eq!(done, advertised);
    assert_eq!(control.occupancy().await, (0, 0));
    let events = sink.events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "broker_admission_granted")
            .count(),
        3
    );
    let mut dispatched: Vec<String> = events
        .iter()
        .filter(|event| event["event"] == "task_dispatched")
        .map(|event| event["task_id"].as_str().unwrap().to_string())
        .collect();
    dispatched.sort();
    assert_eq!(dispatched, advertised);
    assert_eq!(
        dispatched.len(),
        3,
        "duplicate dispatches must not be hidden"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "broker_admission_released")
            .count(),
        3
    );
}

#[tokio::test]
async fn explicit_scheduler_role_survives_retries_and_does_not_depend_on_attempt_number() {
    let build_sink = Arc::new(RecordingSink::default());
    let build_control = control(
        "explicit-build-role",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        build_sink.clone(),
    );
    let build_dispatcher = Arc::new(AdmissionRoleRecorder::new(true));
    let build_report = Scheduler::new(vec![device("lane-a", "model-a", 1)], 2)
        .with_sink(build_sink)
        .run_with_physical_admission(
            Dag::from_specs(vec![spec("task", &[])]).unwrap(),
            build_dispatcher.clone(),
            build_control,
            execution(WorkRole::Build),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();
    assert_eq!(build_report.done, vec!["task".to_string()]);
    assert_eq!(
        *build_dispatcher.seen.lock().unwrap(),
        vec![
            (
                WorkRole::Build,
                0,
                AuthorityScope::new("physical-scheduler-replay", "execute"),
                0,
            ),
            (
                WorkRole::Build,
                1,
                AuthorityScope::new("physical-scheduler-replay", "execute"),
                0,
            ),
        ]
    );

    let repair_sink = Arc::new(RecordingSink::default());
    let repair_control = control(
        "explicit-repair-role",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        repair_sink.clone(),
    );
    let repair_dispatcher = Arc::new(AdmissionRoleRecorder::new(false));
    let repair_report = Scheduler::new(vec![device("lane-a", "model-a", 1)], 1)
        .with_sink(repair_sink)
        .run_with_physical_admission(
            Dag::from_specs(vec![spec("task", &[])]).unwrap(),
            repair_dispatcher.clone(),
            repair_control,
            execution(WorkRole::Repair),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();
    assert_eq!(repair_report.done, vec!["task".to_string()]);
    assert_eq!(
        *repair_dispatcher.seen.lock().unwrap(),
        vec![(
            WorkRole::Repair,
            0,
            AuthorityScope::new("physical-scheduler-replay", "execute"),
            0,
        )]
    );
}

#[tokio::test]
async fn identical_task_ids_in_separate_run_or_phase_scopes_do_not_collide() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "authority-scope-isolation",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 3)],
        sink,
    );
    let sources = [
        scoped_build(AuthorityScope::new("run-a", "build"), 0, "same-task"),
        scoped_build(AuthorityScope::new("run-b", "build"), 0, "same-task"),
        scoped_build(AuthorityScope::new("run-a", "repair"), 0, "same-task"),
    ];
    let mut admitted = Vec::new();
    for (index, source) in sources.iter().cloned().enumerate() {
        control.set_source_revision(source.clone()).await.unwrap();
        admitted.push(
            control
                .admit(opportunity(
                    &format!("scoped:{index}"),
                    WorkRole::Build,
                    WorkPriority::Implementation,
                    source,
                ))
                .await
                .unwrap(),
        );
    }
    assert_eq!(control.occupancy().await, (0, 3));
    for (index, work) in admitted.iter().enumerate() {
        assert_eq!(work.receipt().source, sources[index]);
        finish(work, &format!("provider:scoped:{index}")).await;
    }
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test]
async fn newer_phase_epoch_supersedes_an_older_version_in_the_same_lineage() {
    let control = control(
        "phase-epoch-supersession",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        Arc::new(RecordingSink::default()),
    );
    let scope = AuthorityScope::new("run-a", "build");
    let old = scoped_build(scope.clone(), 0, "same-task");
    let current = scoped_build(scope, 1, "same-task");
    control.set_source_revision(old.clone()).await.unwrap();
    control.set_source_revision(current.clone()).await.unwrap();

    assert!(matches!(
        control
            .admit(opportunity(
                "old-epoch",
                WorkRole::Build,
                WorkPriority::Implementation,
                old,
            ))
            .await,
        Err(BrokerError::StaleOpportunity { .. })
    ));
    let admitted = control
        .admit(opportunity(
            "current-epoch",
            WorkRole::Build,
            WorkPriority::Implementation,
            current.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(admitted.receipt().source, current);
    finish(&admitted, "provider:current-epoch").await;
}

#[tokio::test]
async fn typed_provider_free_work_bypasses_admission_while_default_work_stays_brokered() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "provider-free",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let dispatcher = Arc::new(ProviderClassMock::default());
    let dag = Dag::from_specs(vec![spec("skeleton::manifest", &[]), spec("build", &[])]).unwrap();

    let report = Scheduler::new(vec![device("lane-a", "model-a", 2)], 2)
        .with_sink(sink.clone())
        .run_with_physical_admission(
            dag,
            dispatcher.clone(),
            control.clone(),
            execution(WorkRole::Build),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();

    assert_eq!(dispatcher.provider_free_calls.load(Ordering::SeqCst), 1);
    assert_eq!(dispatcher.admitted_calls.load(Ordering::SeqCst), 1);
    assert_eq!(control.occupancy().await, (0, 0));
    assert_eq!(report.done.len(), 2);

    let events = sink.events.lock().unwrap();
    let provider_free: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "provider_free_dispatch_started")
        .collect();
    assert_eq!(provider_free.len(), 1);
    assert_eq!(provider_free[0]["task_id"], "skeleton::manifest");
    assert_eq!(provider_free[0]["class"], "deterministic_provider_free");
    let admissions: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "broker_admission_granted")
        .collect();
    assert_eq!(admissions.len(), 1);
    assert_eq!(admissions[0]["receipt"]["source"]["task_id"], "build");
    assert!(events
        .iter()
        .filter(|event| {
            matches!(
                event["event"].as_str(),
                Some("broker_provider_request_permitted")
                    | Some("broker_provider_terminal_observed")
                    | Some("broker_provider_not_started")
            )
        })
        .all(|event| event["admission"]["source"]["task_id"] == "build"));
}

#[tokio::test]
async fn two_logical_lanes_on_one_host_never_enter_two_provider_dispatches() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "aliased-scheduler",
        vec![
            lane("lane-a", "model-a", "same-host", "instance-a", 1),
            lane("lane-b", "model-b", "same-host", "instance-b", 1),
        ],
        sink.clone(),
    );
    let dag = Dag::from_specs(vec![spec("a", &[]), spec("b", &[])]).unwrap();
    let recorder = Arc::new(LifecycleRecorder::new());
    let dispatcher = Arc::new(LifecycleMock {
        recorder: recorder.clone(),
        delay: Duration::from_millis(20),
        withhold_terminal_for: None,
    });
    let report = Scheduler::new(
        vec![
            device("lane-a", "model-a", 1),
            device("lane-b", "model-b", 1),
        ],
        2,
    )
    .with_sink(sink)
    .run_with_physical_admission(
        dag,
        dispatcher,
        control,
        execution(WorkRole::Build),
        String::new(),
        String::new(),
    )
    .await
    .unwrap();

    assert_eq!(report.done.len(), 2);
    assert_eq!(recorder.peak.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn terminal_not_yet_observed_blocks_the_next_scheduled_provider_call() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "terminal-gap",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let dag = Dag::from_specs(vec![spec("first", &[]), spec("next", &["first"])]).unwrap();
    let recorder = Arc::new(LifecycleRecorder::new());
    let dispatcher = Arc::new(LifecycleMock {
        recorder: recorder.clone(),
        delay: Duration::from_millis(1),
        withhold_terminal_for: Some("first".to_string()),
    });
    let run_control = control.clone();
    let run_sink = sink.clone();
    let run = tokio::spawn(async move {
        Scheduler::new(vec![device("lane-a", "model-a", 1)], 2)
            .with_sink(run_sink)
            .run_with_physical_admission(
                dag,
                dispatcher,
                run_control,
                execution(WorkRole::Build),
                String::new(),
                String::new(),
            )
            .await
    });

    wait_for_occupancy(&control, (0, 1)).await;
    assert_eq!(
        recorder.calls.lock().unwrap().as_slice(),
        &["first".to_string()],
        "the dependent may be logically ready, but its provider dispatcher must not run"
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if sink.events.lock().unwrap().iter().any(|event| {
                event["event"] == "broker_drain_pending"
                    && event["unresolved"][0]["admission"]["source"]["task_id"] == "first"
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    recorder.release_withheld().await;
    let report = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("run should drain after the exact terminal")
        .unwrap()
        .unwrap();
    assert_eq!(report.done, vec!["first".to_string(), "next".to_string()]);
}

#[tokio::test]
async fn stale_auxiliary_is_removed_and_new_critical_work_passes_valid_auxiliary() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "auxiliary-order",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let blocker_source = repair("blocker", 1);
    control
        .set_source_revision(blocker_source.clone())
        .await
        .unwrap();
    let blocker = control
        .admit(opportunity(
            "build:blocker",
            WorkRole::Repair,
            WorkPriority::CriticalPath,
            blocker_source,
        ))
        .await
        .unwrap();

    let stale_source = artifact("reviewed", 1);
    control
        .set_source_revision(stale_source.clone())
        .await
        .unwrap();
    let stale_wait = tokio::spawn({
        let control = control.clone();
        async move {
            control
                .admit(opportunity(
                    "review:stale",
                    WorkRole::CompletedArtifactReview,
                    WorkPriority::AuxiliaryEvidence,
                    stale_source,
                ))
                .await
        }
    });
    wait_for_occupancy(&control, (1, 1)).await;
    control
        .set_source_revision(artifact("reviewed", 2))
        .await
        .unwrap();
    assert!(matches!(
        stale_wait.await.unwrap(),
        Err(BrokerError::StaleOpportunity { .. })
    ));

    let valid_review = artifact("valid-review", 1);
    control
        .set_source_revision(valid_review.clone())
        .await
        .unwrap();
    let review_wait = tokio::spawn({
        let control = control.clone();
        async move {
            control
                .admit(opportunity(
                    "review:valid",
                    WorkRole::CompletedArtifactReview,
                    WorkPriority::AuxiliaryEvidence,
                    valid_review,
                ))
                .await
        }
    });
    let critical_source = repair("critical", 1);
    control
        .set_source_revision(critical_source.clone())
        .await
        .unwrap();
    let critical_wait = tokio::spawn({
        let control = control.clone();
        async move {
            control
                .admit(opportunity(
                    "build:critical",
                    WorkRole::Repair,
                    WorkPriority::CriticalPath,
                    critical_source,
                ))
                .await
        }
    });
    wait_for_occupancy(&control, (2, 1)).await;
    finish(&blocker, "provider:blocker").await;
    let critical = tokio::time::timeout(Duration::from_secs(2), critical_wait)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(critical.receipt().work_id, "build:critical");
    assert_eq!(control.occupancy().await, (1, 1));
    finish(&critical, "provider:critical").await;
    let review = review_wait.await.unwrap().unwrap();
    assert_eq!(review.receipt().work_id, "review:valid");
    finish(&review, "provider:review").await;

    let events = sink.events.lock().unwrap();
    assert!(events
        .iter()
        .any(|event| event["event"] == "broker_work_stale"));
}

struct NeverJudge;

#[async_trait]
impl Judge for NeverJudge {
    async fn judge(&self, _req: JudgeRequest) -> JudgeOutcome {
        panic!("the legacy judge must be rejected before it can run")
    }
}

#[tokio::test]
async fn physical_mode_rejects_legacy_judge_and_reserved_supervision_lanes() {
    let control = control(
        "judge-guard",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        Arc::new(RecordingSink::default()),
    );
    let recorder = Arc::new(LifecycleRecorder::new());
    let dispatcher = Arc::new(LifecycleMock {
        recorder,
        delay: Duration::from_millis(1),
        withhold_terminal_for: None,
    });
    let dag = Dag::from_specs(vec![spec("task", &[])]).unwrap();
    let judge_error = Scheduler::new(vec![device("lane-a", "model-a", 1)], 2)
        .with_judge(Arc::new(NeverJudge), JudgeConfig::default())
        .run_with_physical_admission(
            dag,
            dispatcher.clone(),
            control.clone(),
            execution(WorkRole::Build),
            String::new(),
            String::new(),
        )
        .await
        .unwrap_err();
    assert!(judge_error.to_string().contains("legacy judge path"));

    let mut supervision = device("supervision", "model-s", 1);
    supervision.supervision = true;
    let dag = Dag::from_specs(vec![spec("task", &[])]).unwrap();
    let supervision_error = Scheduler::new(vec![device("lane-a", "model-a", 1), supervision], 2)
        .run_with_physical_admission(
            dag,
            dispatcher,
            control,
            execution(WorkRole::Build),
            String::new(),
            String::new(),
        )
        .await
        .unwrap_err();
    assert!(supervision_error
        .to_string()
        .contains("no permanent supervision lane"));
}

#[tokio::test]
async fn tool_gap_releases_the_host_and_a_later_turn_reacquires_through_the_queue() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "tool-gap",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let signals = Arc::new(ToolGapSignals {
        first_turn_finished: Notify::new(),
        allow_second_turn: Notify::new(),
        other_task_finished: Notify::new(),
        live_provider_turns: AtomicUsize::new(0),
        peak_provider_turns: AtomicUsize::new(0),
    });
    let dispatcher = Arc::new(ToolGapDispatcher {
        signals: signals.clone(),
    });
    let dag = Dag::from_specs(vec![spec("a", &[]), spec("b", &[])]).unwrap();
    let run_control = control.clone();
    let run_sink = sink.clone();
    let run = tokio::spawn(async move {
        Scheduler::new(vec![device("lane-a", "model-a", 4)], 2)
            .with_sink(run_sink)
            .run_with_physical_admission(
                dag,
                dispatcher,
                run_control,
                execution(WorkRole::Build),
                String::new(),
                String::new(),
            )
            .await
    });

    tokio::time::timeout(
        Duration::from_secs(2),
        signals.first_turn_finished.notified(),
    )
    .await
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        signals.other_task_finished.notified(),
    )
    .await
    .expect("task b must use the decoder while task a is doing local tool work");
    assert_eq!(signals.peak_provider_turns.load(Ordering::SeqCst), 1);
    let physical = control.physical_occupancy().await;
    assert_eq!(physical.len(), 1);
    assert_eq!(physical[0].physical_host_id, "host-a");
    assert_eq!(physical[0].provider_turn_permits_held, 0);
    signals.allow_second_turn.notify_one();
    let report = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(report.done.len(), 2);
    assert_eq!(control.occupancy().await, (0, 0));
}

#[tokio::test]
async fn broker_routes_across_verified_hosts_instead_of_pinning_scheduler_placeholders() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "adaptive-routing",
        vec![
            lane("lane-a", "model-a", "host-a", "instance-a", 1),
            lane("lane-b", "model-b", "host-b", "instance-b", 1),
        ],
        sink.clone(),
    );
    let recorder = Arc::new(LifecycleRecorder::new());
    let dispatcher = Arc::new(LifecycleMock {
        recorder,
        delay: Duration::from_millis(10),
        withhold_terminal_for: None,
    });
    let dag = Dag::from_specs(vec![spec("a", &[]), spec("b", &[])]).unwrap();
    Scheduler::new(
        vec![
            device("lane-a", "model-a", 8),
            device("lane-b", "model-b", 8),
        ],
        2,
    )
    .with_sink(sink.clone())
    .run_with_physical_admission(
        dag,
        dispatcher,
        control,
        execution(WorkRole::Build),
        String::new(),
        String::new(),
    )
    .await
    .unwrap();

    let devices: HashSet<String> = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| event["event"] == "task_dispatched")
        .map(|event| event["device"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        devices,
        HashSet::from(["lane-a".to_string(), "lane-b".to_string()])
    );
}

#[tokio::test]
async fn fan_out_rank_reaches_the_capacity_one_broker_before_lower_ranked_ready_work() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "ranked-order",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let recorder = Arc::new(LifecycleRecorder::new());
    let dispatcher = Arc::new(LifecycleMock {
        recorder: recorder.clone(),
        delay: Duration::from_millis(1),
        withhold_terminal_for: None,
    });
    let dag = Dag::from_specs(vec![
        spec("high", &[]),
        spec("low", &[]),
        spec("child-a", &["high"]),
        spec("child-b", &["high"]),
    ])
    .unwrap();
    Scheduler::new(vec![device("lane-a", "model-a", 8)], 2)
        .with_sink(sink)
        .run_with_physical_admission(
            dag,
            dispatcher,
            control,
            execution(WorkRole::Build),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();
    assert_eq!(recorder.calls.lock().unwrap().first().unwrap(), "high");
}

#[tokio::test]
async fn cancelled_queued_admission_is_withdrawn_without_a_phantom_host_claim() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "cancelled-admission",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let blocker_source = build("blocker");
    control
        .set_source_revision(blocker_source.clone())
        .await
        .unwrap();
    let blocker = control
        .admit(opportunity(
            "build:blocker",
            WorkRole::Build,
            WorkPriority::Implementation,
            blocker_source,
        ))
        .await
        .unwrap();
    let queued_source = build("queued");
    control
        .set_source_revision(queued_source.clone())
        .await
        .unwrap();
    let queued = tokio::spawn({
        let control = control.clone();
        async move {
            control
                .admit(opportunity(
                    "build:queued",
                    WorkRole::Build,
                    WorkPriority::Implementation,
                    queued_source,
                ))
                .await
        }
    });
    wait_for_occupancy(&control, (1, 1)).await;
    queued.abort();
    let _ = queued.await;
    wait_for_occupancy(&control, (0, 1)).await;
    finish(&blocker, "provider:blocker").await;
    assert_eq!(control.occupancy().await, (0, 0));
    assert!(sink
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|event| event["event"] == "broker_work_withdrawn"));
}

#[tokio::test]
async fn cancelled_provider_reacquisition_does_not_leave_a_phantom_turn_permit() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "cancelled-provider",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let review_source = artifact("review", 1);
    control
        .set_source_revision(review_source.clone())
        .await
        .unwrap();
    let review = control
        .admit(opportunity(
            "review:artifact",
            WorkRole::CompletedArtifactReview,
            WorkPriority::AuxiliaryEvidence,
            review_source,
        ))
        .await
        .unwrap();
    let lifecycle = review.lifecycle();
    let first = lifecycle
        .provider_request_started("provider:review:0")
        .await
        .unwrap();
    lifecycle
        .provider_terminal(first, ProviderTerminalKind::Finished)
        .await
        .unwrap();

    let blocker_source = build("blocker");
    control
        .set_source_revision(blocker_source.clone())
        .await
        .unwrap();
    let blocker = control
        .admit(opportunity(
            "build:blocker",
            WorkRole::Build,
            WorkPriority::Implementation,
            blocker_source,
        ))
        .await
        .unwrap();
    let reacquire = tokio::spawn({
        let lifecycle = lifecycle.clone();
        async move {
            lifecycle
                .provider_request_started("provider:review:1")
                .await
        }
    });
    wait_for_occupancy(&control, (1, 2)).await;
    reacquire.abort();
    let _ = reacquire.await;
    wait_for_occupancy(&control, (0, 2)).await;
    finish(&blocker, "provider:blocker").await;
    review
        .complete_local(LocalCompletionKind::Success)
        .await
        .unwrap();
    assert_eq!(control.occupancy().await, (0, 0));
    assert!(sink
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|event| { event["event"] == "broker_provider_request_withdrawn" }));
}

#[tokio::test]
async fn measured_capacity_update_admits_waiting_work_and_emits_its_evidence() {
    let sink = Arc::new(RecordingSink::default());
    let mut measured_lane = lane("lane-a", "model-a", "host-a", "instance-a", 1);
    measured_lane.advertised_instance_capacity = 2;
    let control = control("capacity-update", vec![measured_lane], sink.clone());
    let first_source = build("first");
    control
        .set_source_revision(first_source.clone())
        .await
        .unwrap();
    let first = control
        .admit(opportunity(
            "build:first",
            WorkRole::Build,
            WorkPriority::Implementation,
            first_source,
        ))
        .await
        .unwrap();
    let initial_snapshot_id = first.receipt().fleet_snapshot_id.clone();
    let second_source = build("second");
    control
        .set_source_revision(second_source.clone())
        .await
        .unwrap();
    let second_wait = tokio::spawn({
        let control = control.clone();
        async move {
            control
                .admit(opportunity(
                    "build:second",
                    WorkRole::Build,
                    WorkPriority::Implementation,
                    second_source,
                ))
                .await
        }
    });
    wait_for_occupancy(&control, (1, 1)).await;
    let update = control
        .update_host_capacity(
            "host-a",
            &initial_snapshot_id,
            HostCapacityEvidence::MeasuredProfile {
                profile_hash: "measured-capacity-two".to_string(),
                profile_key: "test-runtime:model:context:role".to_string(),
                max_concurrent: 2,
            },
        )
        .await
        .unwrap();
    let second = second_wait.await.unwrap().unwrap();
    assert_eq!(
        update.previous_fleet_snapshot_id,
        first.receipt().fleet_snapshot_id
    );
    assert_eq!(
        update.new_fleet_snapshot_id,
        second.receipt().fleet_snapshot_id
    );
    assert_ne!(
        first.receipt().fleet_snapshot_id,
        second.receipt().fleet_snapshot_id
    );
    let stale_update = control
        .update_host_capacity(
            "host-a",
            &initial_snapshot_id,
            HostCapacityEvidence::MeasuredProfile {
                profile_hash: "stale-capacity-three".to_string(),
                profile_key: "test-runtime:model:context:role".to_string(),
                max_concurrent: 3,
            },
        )
        .await;
    assert!(matches!(
        stale_update,
        Err(BrokerError::FleetSnapshotMismatch { .. })
    ));
    assert_eq!(
        control.snapshot().await.snapshot_id,
        update.new_fleet_snapshot_id
    );
    assert_eq!(control.occupancy().await, (0, 2));
    finish(&first, "provider:first").await;
    finish(&second, "provider:second").await;
    assert!(sink
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|event| event["event"] == "broker_capacity_updated"));
}

#[tokio::test]
async fn provider_failure_cannot_be_reported_as_a_successful_scheduler_task() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "provider-failure",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let report = Scheduler::new(vec![device("lane-a", "model-a", 1)], 1)
        .with_sink(sink)
        .run_with_physical_admission(
            Dag::from_specs(vec![spec("task", &[])]).unwrap(),
            Arc::new(FailedTerminalDispatcher),
            control,
            execution(WorkRole::Build),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();
    assert!(report.done.is_empty());
    assert_eq!(report.failed, vec!["task".to_string()]);
}

#[tokio::test]
async fn later_correlated_provider_success_recovers_the_scheduler_task() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "provider-recovery",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let report = Scheduler::new(vec![device("lane-a", "model-a", 1)], 1)
        .with_sink(sink)
        .run_with_physical_admission(
            Dag::from_specs(vec![spec("task", &[])]).unwrap(),
            Arc::new(RecoveredTerminalDispatcher),
            control,
            execution(WorkRole::Build),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();
    assert_eq!(report.done, vec!["task".to_string()]);
    assert!(report.failed.is_empty());
}

#[tokio::test]
async fn explicit_provider_not_started_closes_once_and_releases_without_a_phantom_permit() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "provider-not-started",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let source = build("task");
    control.set_source_revision(source.clone()).await.unwrap();
    let admitted = control
        .admit(opportunity(
            "build:task",
            WorkRole::Build,
            WorkPriority::Implementation,
            source,
        ))
        .await
        .unwrap();
    admitted
        .lifecycle()
        .provider_not_started("provider adapter rejected the request before dispatch")
        .await
        .unwrap();
    let duplicate = admitted
        .lifecycle()
        .provider_not_started("duplicate provider-not-started receipt")
        .await
        .expect_err("an accepted terminal receipt cannot be replayed");
    assert!(duplicate.to_string().contains("already closed"));
    admitted
        .complete_local(LocalCompletionKind::Error)
        .await
        .unwrap();
    assert_eq!(control.occupancy().await, (0, 0));
    assert_eq!(
        control.physical_occupancy().await[0].provider_turn_permits_held,
        0
    );

    let events = sink.events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "broker_provider_starts_closed")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "broker_provider_not_started")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event["event"] == "broker_receipt_rejected"
                    && event["receipt_kind"] == "provider_not_started"
            })
            .count(),
        1
    );
}

#[tokio::test]
async fn lifecycle_clone_cannot_start_a_request_after_dispatch_return_closed_the_admission() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "late-provider",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let gate = Arc::new(Notify::new());
    let (sender, receiver) = oneshot::channel();
    let dispatcher = Arc::new(LateStartDispatcher {
        gate: gate.clone(),
        result: Mutex::new(Some(sender)),
    });
    let report = Scheduler::new(vec![device("lane-a", "model-a", 1)], 1)
        .with_sink(sink.clone())
        .run_with_physical_admission(
            Dag::from_specs(vec![spec("task", &[])]).unwrap(),
            dispatcher,
            control,
            execution(WorkRole::Build),
            String::new(),
            String::new(),
        )
        .await
        .unwrap();
    assert_eq!(report.failed, vec!["task".to_string()]);
    gate.notify_one();
    assert!(tokio::time::timeout(Duration::from_secs(2), receiver)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert!(!sink
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|event| { event["event"] == "broker_provider_request_permitted" }));
}

#[tokio::test]
async fn closing_provider_starts_rejects_a_clone_already_queued_for_physical_capacity() {
    let sink = Arc::new(RecordingSink::default());
    let control = control(
        "queued-close",
        vec![lane("lane-a", "model-a", "host-a", "instance-a", 1)],
        sink.clone(),
    );
    let (sender, receiver) = oneshot::channel();
    let signals = Arc::new(QueuedCloseSignals {
        second_task_started: Notify::new(),
        allow_first_task_return: Notify::new(),
        allow_second_task_terminal: Notify::new(),
        late_result: Mutex::new(Some(sender)),
    });
    let scheduler = Scheduler::new(vec![device("lane-a", "model-a", 1)], 1).with_sink(sink.clone());
    let dag = Dag::from_specs(vec![spec("a", &[]), spec("b", &[])]).unwrap();
    let run_control = control.clone();
    let run_signals = signals.clone();
    let run = tokio::spawn(async move {
        scheduler
            .run_with_physical_admission(
                dag,
                Arc::new(QueuedCloseDispatcher {
                    signals: run_signals,
                }),
                run_control,
                execution(WorkRole::Build),
                String::new(),
                String::new(),
            )
            .await
    });

    wait_for_occupancy(&control, (1, 2)).await;
    signals.allow_first_task_return.notify_one();
    let late_result = tokio::time::timeout(Duration::from_secs(2), receiver)
        .await
        .expect("closing the admission must wake the queued provider caller")
        .unwrap();
    assert!(late_result.is_err());
    signals.allow_second_task_terminal.notify_one();
    let report = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("the run must drain after the live request terminates")
        .unwrap()
        .unwrap();
    assert_eq!(report.done.len(), 2);
    assert!(report.failed.is_empty());

    let events = sink.events.lock().unwrap();
    assert!(events.iter().any(|event| {
        event["event"] == "broker_provider_request_queued"
            && event["receipt"]["request"]["key"]["provider_request_id"] == "provider:a:late"
            && event["receipt"]["queue_sequence"].as_u64().is_some()
    }));
    assert!(events.iter().any(|event| {
        event["event"] == "broker_provider_request_withdrawn"
            && event["receipt"]["key"]["provider_request_id"] == "provider:a:late"
    }));
    assert!(!events.iter().any(|event| {
        event["event"] == "broker_provider_request_permitted"
            && event["receipt"]["key"]["provider_request_id"] == "provider:a:late"
    }));
}
