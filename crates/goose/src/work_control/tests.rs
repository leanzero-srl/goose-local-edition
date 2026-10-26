use super::*;
use agent_client_protocol::schema::v1::StopReason;
use leanzero_link::controller_auth::{
    authorize, AuthorizationError, ControllerAuthority, DispatchAuthorization, ReceiverBinding,
};
use leanzero_link::controller_wire::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Authority {
    lease: AuthorizationLease,
    revoked: AtomicBool,
}
#[async_trait]
impl ControllerAuthority for Authority {
    async fn introspect(
        &self,
        _: &str,
        _: &ReceiverBinding,
        _: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError> {
        Ok((!self.revoked.load(Ordering::SeqCst)).then(|| self.lease.clone()))
    }
}
struct Executor {
    session_manager: Arc<SessionManager>,
    manager: Arc<AgentManager>,
    agent: Arc<Agent>,
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl OwnerExecutor for Executor {
    async fn prepare(&self, _: &str) -> anyhow::Result<(Arc<Agent>, Arc<AgentManager>)> {
        Ok((self.agent.clone(), self.manager.clone()))
    }
    async fn execute(
        &self,
        args: PromptRequest,
        ticket: RunTicket,
    ) -> Result<PromptResponse, agent_client_protocol::Error> {
        self.session_manager
            .add_message(
                &ticket.session_id,
                &crate::conversation::message::Message::user()
                    .with_id(ticket.message_id.clone().unwrap())
                    .with_text(serde_json::to_string(&args.prompt).unwrap()),
            )
            .await
            .unwrap();
        self.calls.fetch_add(1, Ordering::SeqCst);
        ticket.cancel_token.cancelled().await;
        Ok(PromptResponse::new(StopReason::Cancelled))
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    runs: Arc<OwnerRunRegistry>,
    manager: Arc<SessionManager>,
    session: String,
    authority: Arc<Authority>,
    receiver: ReceiverBinding,
    calls: Arc<AtomicUsize>,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(SessionManager::new(dir.path().to_path_buf()));
        let session = manager
            .create_session(
                dir.path().to_path_buf(),
                "commands".into(),
                crate::session::SessionType::User,
                crate::config::GooseMode::Auto,
            )
            .await
            .unwrap()
            .id;
        let runs = OwnerRunRegistry::for_store(dir.path().to_path_buf(), manager.clone());
        runs.ensure_store().await.unwrap();
        let generation = Uuid::parse_str(
            &manager
                .work_catalog()
                .await
                .unwrap()
                .watermark()
                .await
                .unwrap()
                .generation,
        )
        .unwrap();
        let owner = Uuid::new_v4();
        runs.bind_owner(owner).unwrap();
        let receiver = ReceiverBinding {
            owner_node_id: owner,
            runtime_id: Uuid::new_v4(),
            store_generation: generation,
            host_lease_id: Uuid::new_v4(),
            host_revision: 1,
        };
        let authority = Arc::new(Authority {
            revoked: AtomicBool::new(false),
            lease: AuthorizationLease {
                active: true,
                account_id: Uuid::new_v4(),
                device_id: Uuid::new_v4(),
                device_revision: 1,
                role: DeviceRole::Controller,
                owner_node_id: owner,
                scopes: vec![
                    ControllerScope::WorkSend,
                    ControllerScope::WorkSteer,
                    ControllerScope::WorkStop,
                    ControllerScope::InputAnswer,
                ],
                token_expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                lease_seconds: 30,
                host_revision: 1,
                store_generation: generation,
                host_lease_id: receiver.host_lease_id,
            },
        });
        let agent_manager = Arc::new(
            AgentManager::new(
                crate::agents::AgentConfig::new(
                    manager.clone(),
                    crate::config::permission::PermissionManager::instance(),
                    None,
                    crate::config::GooseMode::Auto,
                    true,
                    crate::agents::GoosePlatform::GooseCli,
                ),
                None,
            )
            .await
            .unwrap(),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        runs.bind_executor(Arc::new(Executor {
            session_manager: manager.clone(),
            manager: agent_manager,
            agent: Arc::new(Agent::new()),
            calls: calls.clone(),
        }));
        Self {
            _dir: dir,
            runs,
            manager,
            session,
            authority,
            receiver,
            calls,
        }
    }
    async fn command(&self, action: ActionPayload) -> WorkCommand {
        let record = self
            .manager
            .work_catalog()
            .await
            .unwrap()
            .lookup_local_session(&self.session)
            .await
            .unwrap();
        WorkCommand {
            command_id: Uuid::new_v4(),
            work: WorkRef {
                owner_node_id: self.receiver.owner_node_id,
                kind: WorkKind::Chat,
                work_id: Uuid::parse_str(&record.work_id).unwrap(),
            },
            expected_store_generation: self.receiver.store_generation,
            expected_revision: record.revision.try_into().unwrap(),
            expected_run_id: record
                .current_run_id
                .map(|id| Uuid::parse_str(&id).unwrap()),
            action,
        }
    }
    async fn submit(&self, command: WorkCommand) -> Result<CommandReceipt, ControllerError> {
        let scope = command.action.action().required_scope();
        let device = authorize(
            self.authority.as_ref(),
            "fixture-device",
            &self.receiver,
            scope,
        )
        .await
        .unwrap();
        let guard = DispatchAuthorization::new(
            self.authority.clone(),
            "fixture-device".into(),
            self.receiver.clone(),
            scope,
            device.clone(),
        );
        self.runs.accept_command(&device, command, guard).await
    }
}
async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn send(text: &str) -> ActionPayload {
    ActionPayload::Send {
        content: vec![ControllerContent::Text { text: text.into() }],
    }
}

#[tokio::test]
async fn duplicate_lost_ack_send_has_one_owner_execution_and_payload_reuse_conflicts() {
    let fixture = Fixture::new().await;
    let command = fixture.command(send("once")).await;
    let accepted = fixture.submit(command.clone()).await.unwrap();
    until(|| fixture.calls.load(Ordering::SeqCst) == 1).await;
    let replay = fixture.submit(command.clone()).await.unwrap();
    assert_eq!(replay.command_id, accepted.command_id);
    assert_eq!(replay.run_id, accepted.run_id);
    let mut conflict = command;
    conflict.action = send("changed");
    assert_eq!(
        fixture.submit(conflict).await.unwrap_err().body.code,
        "command_id_conflict"
    );
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    fixture
        .runs
        .stop(
            &fixture.session,
            accepted.run_id.map(|id| id.to_string()).as_deref(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn stale_stop_cannot_stop_the_next_run_and_revoked_queued_author_cannot_dispatch() {
    let fixture = Fixture::new().await;
    let first = fixture
        .submit(fixture.command(send("first")).await)
        .await
        .unwrap();
    until(|| fixture.calls.load(Ordering::SeqCst) == 1).await;
    let stop = fixture.command(ActionPayload::Stop {}).await;
    let queued = fixture
        .submit(fixture.command(send("queued")).await)
        .await
        .unwrap();
    assert!(matches!(queued.state, CommandState::Waiting));
    fixture.authority.revoked.store(true, Ordering::SeqCst);
    fixture
        .runs
        .stop(
            &fixture.session,
            first.run_id.map(|id| id.to_string()).as_deref(),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let receipt: String =
                sqlx::query_scalar("SELECT receipt_json FROM owner_commands WHERE command_id=?")
                    .bind(queued.command_id.to_string())
                    .fetch_one(fixture.runs.pool().await.unwrap())
                    .await
                    .unwrap();
            let receipt: CommandReceipt = serde_json::from_str(&receipt).unwrap();
            if matches!(receipt.state, CommandState::Rejected) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    fixture.authority.revoked.store(false, Ordering::SeqCst);
    let next = fixture
        .submit(fixture.command(send("next")).await)
        .await
        .unwrap();
    until(|| fixture.calls.load(Ordering::SeqCst) == 2).await;
    assert!(fixture.submit(stop).await.is_err());
    assert_eq!(
        fixture.runs.current_run(&fixture.session).await,
        next.run_id
    );
    assert!(!fixture
        .runs
        .runs
        .lock()
        .await
        .get(&fixture.session)
        .unwrap()
        .cancel_token
        .is_cancelled());
    fixture.runs.stop(&fixture.session, None).await.unwrap();
}

#[tokio::test]
async fn crash_reconciliation_never_replays_a_dispatch_without_persisted_proof() {
    let fixture = Fixture::new().await;
    let command = fixture.command(send("crash marker")).await;
    let receipt = CommandReceipt {
        command_id: command.command_id,
        work: command.work.clone(),
        authenticated_device_id: fixture.authority.lease.device_id,
        revision: command.expected_revision,
        state: CommandState::Dispatching,
        run_id: Some(Uuid::new_v4()),
        message_id: Some("not-persisted".into()),
        input_id: None,
        checkpoint_id: None,
        reason: None,
        updated_at: chrono::Utc::now(),
    };
    sqlx::query("INSERT INTO owner_commands(account_id,device_id,command_id,work_id,generation,session_id,command_json,receipt_json,state,message_id,run_id) VALUES (?,?,?,?,?,?,?,?,'dispatching',?,?)")
        .bind(fixture.authority.lease.account_id.to_string()).bind(fixture.authority.lease.device_id.to_string()).bind(command.command_id.to_string()).bind(command.work.work_id.to_string()).bind(command.expected_store_generation.to_string()).bind(&fixture.session).bind(serde_json::to_string(&command).unwrap()).bind(serde_json::to_string(&receipt).unwrap()).bind(&receipt.message_id).bind(receipt.run_id.unwrap().to_string()).execute(fixture.runs.pool().await.unwrap()).await.unwrap();
    let restarted = OwnerRunRegistry {
        runs: Arc::default(),
        session_manager: fixture.manager.clone(),
        owner: RwLock::new(Some(fixture.receiver.owner_node_id)),
        executor: RwLock::new(None),
        agent_manager: OnceLock::new(),
        initialized: OnceCell::new(),
        input_handlers: Mutex::default(),
        queued: Mutex::default(),
        observers: RwLock::default(),
    };
    restarted.ensure_store().await.unwrap();
    let json: String =
        sqlx::query_scalar("SELECT receipt_json FROM owner_commands WHERE command_id=?")
            .bind(command.command_id.to_string())
            .fetch_one(restarted.pool().await.unwrap())
            .await
            .unwrap();
    let reconciled: CommandReceipt = serde_json::from_str(&json).unwrap();
    assert!(matches!(reconciled.state, CommandState::OutcomeUnknown));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

struct AnswerOnce {
    run_id: String,
    consumed: Arc<AtomicUsize>,
}
#[async_trait]
impl InputHandler for AnswerOnce {
    fn run_id(&self) -> &str {
        &self.run_id
    }
    fn observe(&self, _: &ConnectionTo<Client>, _: Arc<OwnerRunRegistry>, _: PendingInput) {}
    fn validate(&self, _: &InputAnswer) -> anyhow::Result<()> {
        Ok(())
    }
    async fn answer(&self, _: InputAnswer) -> anyhow::Result<()> {
        self.consumed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
#[tokio::test]
async fn competing_input_answers_consume_exactly_once_and_partial_snapshots_reconcile_by_message_id(
) {
    let fixture = Fixture::new().await;
    let receipt = fixture
        .submit(fixture.command(send("question run")).await)
        .await
        .unwrap();
    until(|| fixture.calls.load(Ordering::SeqCst) == 1).await;
    let run = receipt.run_id.unwrap();
    let consumed = Arc::new(AtomicUsize::new(0));
    let input = fixture
        .runs
        .register_input(
            &fixture.session,
            "fixture-request",
            "Proceed?".into(),
            vec![InputOption {
                option_id: "yes".into(),
                label: "Yes".into(),
                accepts_value: false,
            }],
            Arc::new(AnswerOnce {
                run_id: run.to_string(),
                consumed: consumed.clone(),
            }),
        )
        .await
        .unwrap();
    let answer = InputAnswer {
        option_id: "yes".into(),
        value: None,
    };
    let (a, b) = tokio::join!(
        fixture
            .runs
            .answer_input(input.input_id, run, answer.clone()),
        fixture.runs.answer_input(input.input_id, run, answer)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(consumed.load(Ordering::SeqCst), 1);
    let first = crate::conversation::message::Message::assistant()
        .with_id("stable-assistant")
        .with_text("α ");
    let second = crate::conversation::message::Message::assistant()
        .with_id("stable-assistant")
        .with_text("beta");
    fixture
        .runs
        .record_stream_message(&fixture.session, &run.to_string(), &first)
        .await
        .unwrap();
    fixture
        .runs
        .record_stream_message(&fixture.session, &run.to_string(), &second)
        .await
        .unwrap();
    let catalog = fixture.manager.work_catalog().await.unwrap();
    let watermark = catalog.watermark().await.unwrap();
    let work = receipt.work.work_id;
    assert_eq!(
        fixture
            .runs
            .partial_offset_at(work, watermark.revision)
            .await
            .unwrap(),
        "α ".len() as u64
    );
    let chunks = fixture
        .runs
        .current_partials_at(work, watermark.revision)
        .await
        .unwrap();
    assert_eq!(chunks.len(), 1);
    let content: Vec<crate::conversation::message::MessageContent> =
        serde_json::from_str(&chunks[0].content_json).unwrap();
    let accumulated =
        crate::conversation::message::Message::assistant().with_id("stable-assistant");
    let accumulated = crate::conversation::message::Message {
        content,
        ..accumulated
    };
    assert_eq!(accumulated.as_concat_text(), "α beta");
    fixture
        .manager
        .add_message(&fixture.session, &accumulated)
        .await
        .unwrap();
    let committed = catalog.watermark().await.unwrap();
    assert!(fixture
        .runs
        .current_partials_at(work, committed.revision)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        fixture
            .runs
            .current_partials_at(work, watermark.revision)
            .await
            .unwrap()
            .len(),
        1,
        "older snapshot must still see the then-uncommitted answer"
    );
    fixture.runs.stop(&fixture.session, None).await.unwrap();
}
