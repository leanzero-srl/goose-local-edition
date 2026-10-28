use super::*;
use goose::session::work_catalog::WorkCatalog;
use goose::work_control::adapters::{DesktopBridge, LOG_PAGE_BYTES};
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const LAUNCH: &str = "2026-10-26T10:00:00Z";
const FIRST_SCORE: &str = "2026-10-26T11:00:00Z";
const SECOND_SCORE: &str = "2026-10-26T12:00:00Z";

struct DesktopState {
    active: bool,
    activity: Option<String>,
    checkpoint_delivered: Option<String>,
    requests: Vec<Value>,
    checkpoint_serial: usize,
    log_bytes: u64,
    invalid_log_source: bool,
    invalid_receipt_cursor: bool,
}

impl DesktopState {
    fn reply(&mut self, request: &Value) -> Value {
        assert_eq!(request["secret"], "fixture-secret");
        self.requests.push(request.clone());
        let params = &request["params"];
        let result = match request["method"].as_str().unwrap() {
            "benchmark.catalog" => json!({
                "sessions":[{"runId":"original-run","startedAt":LAUNCH,"slotDir":"/fixture/slot",
                    "outcome":if self.active {"running"} else {"finished"},
                    "logActivityStartedAt":self.activity}],
                "active":if self.active {json!({"runId":"original-run","startedAt":LAUNCH,
                    "activityStartedAt":self.activity,"noteSupported":self.activity.is_none(),
                    "phase":"build","logBytes":self.log_bytes})} else {Value::Null}
            }),
            "benchmark.command" => {
                self.checkpoint_serial += 1;
                json!({"state":"waiting","checkpoint_id":format!("1761472800000-{}.json", self.checkpoint_serial),"reason":"user_note_queued"})
            }
            "benchmark.receipt" => {
                assert_eq!(params["run_id"], "original-run");
                assert_eq!(params["started_at"], LAUNCH);
                assert_eq!(params["activity_started_at"], Value::Null);
                let from = params["from"].as_u64().unwrap();
                let delivered =
                    params["checkpoint_id"].as_str() == self.checkpoint_delivered.as_deref();
                json!({"state":if delivered {"delivered"} else {"waiting"},
                    "next":from + if self.invalid_receipt_cursor {LOG_PAGE_BYTES+1} else {10}})
            }
            "benchmark.log" => {
                assert_eq!(params["slot_dir"], "/fixture/slot");
                let source = params
                    .get("activity_started_at")
                    .cloned()
                    .unwrap_or_else(|| json!(self.activity));
                let ch = match source.as_str() {
                    Some(FIRST_SCORE) => "R",
                    Some(SECOND_SCORE) => "S",
                    None if source.is_null() => "O",
                    _ => panic!("unknown log source"),
                };
                let end = params["to"].as_u64().unwrap_or(200000);
                let from = params["from"]
                    .as_u64()
                    .unwrap_or_else(|| end.saturating_sub(LOG_PAGE_BYTES));
                json!({"from":from,"next":end,"data":ch.repeat((end-from) as usize),
                    "done":!self.active,"activity_started_at":if self.invalid_log_source {json!("wrong-source")} else {source}})
            }
            other => panic!("unexpected method {other}"),
        };
        json!({"ok":true,"result":result})
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    server: tokio::task::JoinHandle<()>,
    state: Arc<Mutex<DesktopState>>,
    backend: GooseControllerBackend,
    adapters: Arc<WorkAdapters>,
    catalog: WorkCatalog,
    device: AuthenticatedDevice,
    dispatch: DispatchAuthorization,
    owner: Uuid,
    account: Uuid,
    generation: Uuid,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("bench-controller-")
            .tempdir_in("/tmp")
            .unwrap();
        let socket = root.path().join("owner.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        let state = Arc::new(Mutex::new(DesktopState {
            active: true,
            activity: None,
            checkpoint_delivered: None,
            requests: Vec::new(),
            checkpoint_serial: 0,
            log_bytes: 200000,
            invalid_log_source: false,
            invalid_receipt_cursor: false,
        }));
        let shared = state.clone();
        let server = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let mut reader = BufReader::new(stream);
                let mut request = String::new();
                reader.read_line(&mut request).await.unwrap();
                let reply = shared
                    .lock()
                    .unwrap()
                    .reply(&serde_json::from_str(&request).unwrap());
                let line = format!("{reply}\n");
                reader.get_mut().write_all(line.as_bytes()).await.unwrap();
            }
        });
        let sessions = Arc::new(SessionManager::new(root.path().join("profile")));
        let catalog = sessions.work_catalog().await.unwrap();
        let generation = Uuid::parse_str(&catalog.watermark().await.unwrap().generation).unwrap();
        let owner = Uuid::new_v4();
        let account = Uuid::new_v4();
        let runs = OwnerRunRegistry::for_store(root.path().join("profile"), sessions);
        let adapters = Arc::new(WorkAdapters::new(
            Arc::new(JobRegistry::new(JobsConfig {
                root: root.path().join("jobs"),
                ..JobsConfig::default()
            })),
            root.path().join("config"),
            Some(DesktopBridge::new(socket, "fixture-secret".into())),
        ));
        let backend = GooseControllerBackend::new(
            ControllerBackendConfig {
                host: HostDescriptor {
                    owner_node_id: owner,
                    name: "Owner".into(),
                    device_role: DeviceRole::Host,
                    protocol_version: 1,
                    availability: HostAvailability::Online,
                    last_seen_at: None,
                    endpoints: vec![],
                    capabilities: vec![],
                },
                account_id: account,
                cursor_key_path: root.path().join("cursor.key"),
                policy: CursorPolicy::default(),
            },
            catalog.clone(),
            runs,
            None,
        )
        .await
        .unwrap()
        .with_adapters(adapters.clone())
        .await
        .unwrap();
        let (device, dispatch) = controller_auth(owner, account, generation).await;
        Self {
            _root: root,
            server,
            state,
            backend,
            adapters,
            catalog,
            device,
            dispatch,
            owner,
            account,
            generation,
        }
    }

    async fn entry(&self) -> CatalogEntry {
        self.backend
            .catalog(&self.device, None)
            .await
            .unwrap()
            .entries
            .remove(0)
    }

    async fn activity(&self, activity: Option<&str>, active: bool) {
        {
            let mut state = self.state.lock().unwrap();
            state.activity = activity.map(str::to_owned);
            state.active = active;
        }
        self.adapters.refresh(&self.catalog).await.unwrap();
    }

    async fn history(&self, work: &WorkRef, cursor: Option<&str>) -> HistoryPage {
        self.backend
            .history(&self.device, work, self.generation, cursor)
            .await
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn assert_source(page: &HistoryPage, ch: char) {
    let text = page.items[0].message["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(!text.is_empty());
    assert!(text.chars().all(|c| c == ch));
}

#[tokio::test]
async fn completed_rescore_keeps_latest_source_and_old_pages_keep_their_attempt() {
    let fixture = Fixture::new().await;
    let original = fixture.entry().await;
    let original_tail = fixture.history(&original.work, None).await;
    assert_source(&original_tail, 'O');
    fixture.activity(Some(FIRST_SCORE), true).await;
    let scoring = fixture.entry().await;
    assert_ne!(scoring.current_run_id, original.current_run_id);
    assert_source(
        &fixture
            .history(&original.work, original_tail.next_page_token.as_deref())
            .await,
        'O',
    );
    let scoring_tail = fixture.history(&original.work, None).await;
    assert_source(&scoring_tail, 'R');
    fixture.activity(Some(FIRST_SCORE), false).await;
    let terminal = fixture.entry().await;
    assert!(terminal.current_run_id.is_none());
    assert!(terminal.capabilities.is_empty());
    assert_source(&fixture.history(&original.work, None).await, 'R');
    let older = fixture
        .history(&original.work, scoring_tail.next_page_token.as_deref())
        .await;
    assert_source(&older, 'R');
    assert_eq!(older.revision, scoring_tail.revision);
    fixture.activity(Some(SECOND_SCORE), false).await;
    assert!(
        fixture.entry().await.revision > terminal.revision,
        "a completed scoring attempt missed between polls must still refresh selected history"
    );
    fixture.activity(Some(SECOND_SCORE), true).await;
    assert_source(
        &fixture
            .history(&original.work, scoring_tail.next_page_token.as_deref())
            .await,
        'R',
    );
    assert_source(&fixture.history(&original.work, None).await, 'S');
    let latest = fixture.entry().await;
    let stale = WorkCommand {
        command_id: Uuid::new_v4(),
        work: original.work.clone(),
        expected_store_generation: fixture.generation,
        expected_revision: latest.revision,
        expected_run_id: original.current_run_id,
        action: ActionPayload::Stop {},
    };
    assert_eq!(
        fixture
            .backend
            .accept_command(&fixture.device, stale, fixture.dispatch.clone())
            .await
            .unwrap_err()
            .body
            .code,
        "stale_run"
    );
    fixture.state.lock().unwrap().invalid_log_source = true;
    assert!(fixture
        .backend
        .history(
            &fixture.device,
            &original.work,
            fixture.generation,
            scoring_tail.next_page_token.as_deref()
        )
        .await
        .is_err());
}

#[tokio::test]
async fn benchmark_receipts_follow_exact_checkpoint_and_device_scoped_scan_offsets() {
    let fixture = Fixture::new().await;
    let entry = fixture.entry().await;
    let command = WorkCommand {
        command_id: Uuid::new_v4(),
        work: entry.work.clone(),
        expected_store_generation: fixture.generation,
        expected_revision: entry.revision,
        expected_run_id: entry.current_run_id,
        action: ActionPayload::Note {
            text: "Read the real output".into(),
        },
    };
    let first = fixture
        .backend
        .accept_command(&fixture.device, command.clone(), fixture.dispatch.clone())
        .await
        .unwrap();
    assert!(matches!(first.state, CommandState::Waiting));
    fixture.state.lock().unwrap().checkpoint_delivered = Some("different-checkpoint.json".into());
    assert!(matches!(
        fixture
            .backend
            .receipt(&fixture.device, first.command_id)
            .await
            .unwrap()
            .state,
        CommandState::Waiting
    ));
    let (other_device, other_dispatch) =
        controller_auth(fixture.owner, fixture.account, fixture.generation).await;
    let other = fixture
        .backend
        .accept_command(&other_device, command, other_dispatch)
        .await
        .unwrap();
    assert_ne!(first.checkpoint_id, other.checkpoint_id);
    assert!(matches!(
        fixture
            .backend
            .receipt(&other_device, other.command_id)
            .await
            .unwrap()
            .state,
        CommandState::Waiting
    ));
    let from: Vec<_> = fixture
        .state
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|r| r["method"] == "benchmark.receipt")
        .map(|r| r["params"]["from"].as_u64().unwrap())
        .collect();
    assert_eq!(
        from,
        vec![0, 0],
        "one device must not skip another device's receipt evidence"
    );
    fixture.activity(Some(FIRST_SCORE), false).await;
    {
        let mut state = fixture.state.lock().unwrap();
        state.checkpoint_delivered = first.checkpoint_id.clone();
        state.invalid_receipt_cursor = true;
    }
    assert!(fixture
        .backend
        .receipt(&fixture.device, first.command_id)
        .await
        .is_err());
    fixture.state.lock().unwrap().invalid_receipt_cursor = false;
    assert!(matches!(
        fixture
            .backend
            .receipt(&fixture.device, first.command_id)
            .await
            .unwrap()
            .state,
        CommandState::Delivered
    ));
    let before = fixture.state.lock().unwrap().requests.len();
    assert!(matches!(
        fixture
            .backend
            .receipt(&fixture.device, first.command_id)
            .await
            .unwrap()
            .state,
        CommandState::Delivered
    ));
    assert_eq!(
        fixture.state.lock().unwrap().requests.len(),
        before,
        "delivery is stored and never redispatched"
    );
    assert!(matches!(
        fixture
            .backend
            .receipt(&other_device, other.command_id)
            .await
            .unwrap()
            .state,
        CommandState::Waiting
    ));
    let state = fixture.state.lock().unwrap();
    let receipts: Vec<_> = state
        .requests
        .iter()
        .filter(|r| r["method"] == "benchmark.receipt")
        .collect();
    assert_eq!(receipts[2]["params"]["from"], 10);
    assert_eq!(
        receipts[3]["params"]["from"], 10,
        "invalid responses cannot advance the scan"
    );
    assert!(receipts
        .iter()
        .all(|r| r["params"]["activity_started_at"].is_null()));
}

#[tokio::test]
async fn benchmark_note_and_stop_survive_log_growth_during_admission() {
    let fixture = Fixture::new().await;
    for action in [
        ActionPayload::Note {
            text: "Observe the real checkpoint".into(),
        },
        ActionPayload::Stop {},
    ] {
        let entry = fixture.entry().await;
        fixture.state.lock().unwrap().log_bytes += 10;
        let cmd = WorkCommand {
            command_id: Uuid::new_v4(),
            work: entry.work,
            expected_store_generation: fixture.generation,
            expected_revision: entry.revision,
            expected_run_id: entry.current_run_id,
            action,
        };
        let receipt = fixture
            .backend
            .accept_command(&fixture.device, cmd, fixture.dispatch.clone())
            .await
            .unwrap();
        assert!(matches!(receipt.state, CommandState::Waiting));
        assert!(receipt.checkpoint_id.is_some());
    }
    assert_eq!(
        fixture
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|r| r["method"] == "benchmark.command")
            .count(),
        2
    );
}
