use async_trait::async_trait;
use goose::work_control::{
    ControllerBackendConfig, CursorPolicy, GooseControllerBackend, OwnerRunRegistry,
};
use goose::{
    session::{work_catalog::ExternalWork, SessionManager},
    work_control::adapters::WorkAdapters,
};
use leanzero_link::{
    controller::ControllerBackend,
    controller_auth::{
        authorize, AuthenticatedDevice, AuthorizationError, ControllerAuthority,
        DispatchAuthorization, ReceiverBinding,
    },
    controller_wire::*,
};
use leanzero_link::{
    controller_wire::WorkAction,
    jobs::{JobRegistry, JobsConfig},
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

struct Authority(AuthorizationLease);
#[async_trait]
impl ControllerAuthority for Authority {
    async fn introspect(
        &self,
        _: &str,
        _: &ReceiverBinding,
        _: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError> {
        Ok(Some(self.0.clone()))
    }
}

async fn controller_auth(
    owner: Uuid,
    account: Uuid,
    generation: Uuid,
) -> (AuthenticatedDevice, DispatchAuthorization) {
    let binding = ReceiverBinding {
        owner_node_id: owner,
        runtime_id: Uuid::new_v4(),
        store_generation: generation,
        host_lease_id: Uuid::new_v4(),
        host_revision: 1,
    };
    let authority = Arc::new(Authority(AuthorizationLease {
        active: true,
        account_id: account,
        device_id: Uuid::new_v4(),
        device_revision: 1,
        role: DeviceRole::Controller,
        owner_node_id: owner,
        scopes: vec![
            ControllerScope::CatalogRead,
            ControllerScope::HistoryRead,
            ControllerScope::WorkSteer,
            ControllerScope::WorkStop,
        ],
        token_expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
        lease_seconds: 60,
        host_revision: 1,
        store_generation: generation,
        host_lease_id: binding.host_lease_id,
    }));
    let device = authorize(
        authority.as_ref(),
        "fixture",
        &binding,
        ControllerScope::WorkSteer,
    )
    .await
    .unwrap();
    let dispatch = DispatchAuthorization::new(
        authority,
        "fixture".into(),
        binding,
        ControllerScope::WorkSteer,
        device.clone(),
    );
    (device, dispatch)
}

#[tokio::test]
async fn real_agent_inbox_has_idempotent_command_and_waits_for_consumption() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let desk = root.path().join("desk");
    let runtime = desk.join(".swarm/agent");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(runtime.join("inbox/consumed")).unwrap();
    std::fs::write(
        config.join("agent-work.json"),
        json!({"agents":[{"dir":desk,"addedAt":"2026-10-26T10:00:00Z"}]}).to_string(),
    )
    .unwrap();
    std::fs::write(runtime.join("state.json"),json!({"title":"Real desk","status":"waiting","run_id":"desk-run","updated_at":"2026-10-26T10:00:00Z"}).to_string()).unwrap();
    std::fs::write(runtime.join("lock"), std::process::id().to_string()).unwrap();
    let sessions = Arc::new(SessionManager::new(root.path().join("profile")));
    let catalog = sessions.work_catalog().await.unwrap();
    let generation = Uuid::parse_str(&catalog.watermark().await.unwrap().generation).unwrap();
    let owner = Uuid::new_v4();
    let account = Uuid::new_v4();
    let runs = OwnerRunRegistry::for_store(root.path().join("profile"), sessions);
    let jobs = Arc::new(JobRegistry::new(JobsConfig {
        root: root.path().join("jobs"),
        ..JobsConfig::default()
    }));
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
    .with_adapters(Arc::new(WorkAdapters::new(jobs, config, None)))
    .await
    .unwrap();
    let (device, dispatch) = controller_auth(owner, account, generation).await;
    let entry = backend
        .catalog(&device, None)
        .await
        .unwrap()
        .entries
        .remove(0);
    let command = WorkCommand {
        command_id: Uuid::new_v4(),
        work: entry.work,
        expected_store_generation: generation,
        expected_revision: entry.revision,
        expected_run_id: entry.current_run_id,
        action: ActionPayload::Note {
            text: "Inspect the failing test first".into(),
        },
    };
    let receipt = backend
        .accept_command(&device, command.clone(), dispatch.clone())
        .await
        .unwrap();
    assert!(matches!(receipt.state, CommandState::Waiting));
    let duplicate = backend
        .accept_command(&device, command.clone(), dispatch.clone())
        .await
        .unwrap();
    assert_eq!(duplicate.command_id, receipt.command_id);
    let note = runtime
        .join("inbox")
        .join(format!("{}.json", receipt.command_id));
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&note).unwrap()).unwrap();
    assert_eq!(value["text"], "Inspect the failing test first");
    assert_eq!(value["human_assisted"], true);
    assert_eq!(
        std::fs::read_dir(runtime.join("inbox"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .count(),
        1
    );
    let mut conflict = command.clone();
    conflict.action = ActionPayload::Note {
        text: "Different note".into(),
    };
    assert_eq!(
        backend
            .accept_command(&device, conflict, dispatch.clone())
            .await
            .unwrap_err()
            .body
            .code,
        "command_id_conflict"
    );
    let mut stale = command;
    stale.command_id = Uuid::new_v4();
    stale.expected_run_id = Some(Uuid::new_v4());
    assert_eq!(
        backend
            .accept_command(&device, stale, dispatch)
            .await
            .unwrap_err()
            .body
            .code,
        "stale_run"
    );
    assert!(matches!(
        backend
            .receipt(&device, receipt.command_id)
            .await
            .unwrap()
            .state,
        CommandState::Waiting
    ));
    std::fs::rename(
        note,
        runtime
            .join("inbox/consumed")
            .join(format!("{}.json", receipt.command_id)),
    )
    .unwrap();
    assert!(matches!(
        backend
            .receipt(&device, receipt.command_id)
            .await
            .unwrap()
            .state,
        CommandState::Delivered
    ));
}

fn work(kind: &str, locator: &str) -> ExternalWork {
    ExternalWork {
        kind: kind.into(),
        locator: locator.into(),
        title: "A real job".into(),
        working_dir: "/fixture".into(),
        state: "running".into(),
        run_key: Some("original-run".into()),
        updated_at: "2026-10-26T10:00:00Z".into(),
        event_kind: "started".into(),
    }
}

#[tokio::test]
async fn external_identity_and_run_survive_refresh_and_changed_run_is_fenced() {
    let root = tempfile::tempdir().unwrap();
    let sessions = SessionManager::new(root.path().into());
    let catalog = sessions.work_catalog().await.unwrap();
    let initial = catalog.watermark().await.unwrap();
    let mut source = work("shell_job", "real-job-id");
    let first = catalog.upsert_external(&source).await.unwrap();
    assert_eq!(catalog.upsert_external(&source).await.unwrap(), first);
    assert_eq!(catalog.watermark().await.unwrap().revision, first.revision);
    source.title = "Renamed job".into();
    let renamed = catalog.upsert_external(&source).await.unwrap();
    assert_eq!(renamed.work_id, first.work_id);
    assert_eq!(renamed.current_run_id, first.current_run_id);
    source.run_key = Some("new-run".into());
    let restarted = catalog.upsert_external(&source).await.unwrap();
    assert_eq!(restarted.work_id, first.work_id);
    assert_ne!(restarted.current_run_id, first.current_run_id);
    let delta = catalog.latest_changes(&initial, 10).await.unwrap();
    assert_eq!(delta.records, vec![restarted.clone()]);
    let selected = catalog
        .work_events(&first.work_id, &initial, 10)
        .await
        .unwrap();
    assert_eq!(selected.len(), 3, "selected replay retains every event");
    let page = catalog.catalog_page(None, 10).await.unwrap();
    assert_eq!(page.records, vec![restarted]);
    catalog
        .remove_missing_external("shell_job", &[])
        .await
        .unwrap();
    let removed = catalog
        .lookup(&first.work_id, &initial.generation)
        .await
        .unwrap();
    assert!(removed.deleted);
    assert!(removed.current_run_id.is_none());
}

#[tokio::test]
async fn metadata_fanout_is_one_row_per_work_and_repeated_reads_write_nothing() {
    let root = tempfile::tempdir().unwrap();
    let sessions = SessionManager::new(root.path().into());
    let catalog = sessions.work_catalog().await.unwrap();
    let initial = catalog.watermark().await.unwrap();
    let mut source = work("agent_work", "/fixture/agent");
    for i in 0..64 {
        source.event_kind = format!("phase_{i}");
        catalog.upsert_external(&source).await.unwrap();
    }
    let before = catalog.watermark().await.unwrap();
    for _ in 0..64 {
        catalog.upsert_external(&source).await.unwrap();
    }
    assert_eq!(catalog.watermark().await.unwrap(), before);
    let journal = catalog.changes(&initial, 100).await.unwrap();
    let coalesced = catalog.latest_changes(&initial, 100).await.unwrap();
    assert_eq!(journal.records.len(), 64);
    assert_eq!(coalesced.records.len(), 1);
    eprintln!("catalog measurement: 64 durable changes -> 1 metadata row; 64 unchanged refreshes -> 0 new revisions");
}

#[tokio::test]
async fn registered_agent_and_benchmark_metadata_use_actual_state_and_explicit_absence() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let desk = root.path().join("desk");
    std::fs::create_dir_all(config.join("benchmark")).unwrap();
    std::fs::create_dir_all(desk.join(".swarm/agent")).unwrap();
    std::fs::write(
        config.join("agent-work.json"),
        json!({"agents":[{"dir":desk,"addedAt":"2026-10-26T10:00:00Z"}]}).to_string(),
    )
    .unwrap();
    std::fs::write(config.join("benchmark/sessions.json"),json!({"sessions":[{"runId":null,"startedAt":"2026-10-26T10:00:00Z","slotDir":"/real/slot","outcome":"running"}]}).to_string()).unwrap();
    let jobs = Arc::new(JobRegistry::new(JobsConfig {
        root: root.path().join("jobs"),
        ..JobsConfig::default()
    }));
    let adapters = WorkAdapters::new(jobs, config.clone(), None);
    let sessions = SessionManager::new(root.path().join("sessions-profile"));
    let catalog = sessions.work_catalog().await.unwrap();
    adapters.refresh(&catalog).await.unwrap();
    let rows = catalog.catalog_page(None, 10).await.unwrap().records;
    assert_eq!(rows.len(), 2);
    let bench = rows.iter().find(|r| r.kind == "benchmark").unwrap();
    assert_eq!(
        bench.state, "interrupted",
        "stale index cannot prove a running benchmark"
    );
    assert!(adapters.capabilities(bench).is_empty());
    let agent = rows.iter().find(|r| r.kind == "agent_work").unwrap();
    assert_eq!(agent.event_kind, "agent_not_started");
    assert!(adapters.capabilities(agent).is_empty());
    std::fs::write(desk.join(".swarm/agent/state.json"),json!({"title":"My desk","status":"paused","run_id":"real-run","updated_at":"2026-10-26T10:01:00Z"}).to_string()).unwrap();
    std::fs::write(
        desk.join(".swarm/agent/lock"),
        std::process::id().to_string(),
    )
    .unwrap();
    adapters.refresh(&catalog).await.unwrap();
    let agent = catalog
        .lookup(
            &agent.work_id,
            &catalog.watermark().await.unwrap().generation,
        )
        .await
        .unwrap();
    assert_eq!(agent.state, "paused");
    assert!(adapters.capabilities(&agent).contains(&WorkAction::Resume));
    assert!(!adapters.capabilities(&agent).contains(&WorkAction::Steer));
    std::fs::write(config.join("agent-work.json"), "broken").unwrap();
    assert!(
        adapters.refresh(&catalog).await.is_err(),
        "a corrupt registry must not become a healthy empty catalog"
    );
}
