use super::*;
use leanzero_link::{
    jobs::{JobRequest, JobsSwitch, UserActivePolicy},
    load::{JobKind, NodeLimits, OwnLoad},
    state::SwarmStateSource,
    wire::{LinkEvent, NodeState, NodeStatus, SessionSummary},
};
use std::{io::Write, time::Duration};

struct IdleSource;
#[async_trait]
impl SwarmStateSource for IdleSource {
    async fn local_node(&self) -> NodeState {
        NodeState {
            controller: None,
            node_id: "shell-fixture".into(),
            hostname: "shell-fixture".into(),
            mesh_ip: None,
            status: NodeStatus::Idle,
            sessions_active: 0,
            updated_at: chrono::Utc::now(),
            last_poll_error: None,
            computer_name: None,
            allows: None,
            leaving: None,
            load: None,
        }
    }
    async fn local_sessions(&self) -> Result<Vec<SessionSummary>, String> {
        Ok(vec![])
    }
    fn subscribe_local_deltas(&self) -> futures::stream::BoxStream<'static, LinkEvent> {
        Box::pin(futures::stream::empty())
    }
}

struct ReleaseOnDrop(std::path::PathBuf);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, b"release");
    }
}

#[tokio::test]
async fn shell_stop_survives_log_growth_during_admission() {
    let root = tempfile::tempdir().unwrap();
    let _release = ReleaseOnDrop(root.path().join("release"));
    let jobs_root = root.path().join("jobs");
    let jobs = Arc::new(JobRegistry::new(JobsConfig {
        root: jobs_root.clone(),
        switch: JobsSwitch::fixed(true),
        heavy_lock: root.path().join("heavy.lock"),
        limits: Some(NodeLimits::default()),
        user_active: UserActivePolicy::Allow,
        sampler: Arc::new(|| OwnLoad {
            cpu_load_per_core: Some(0.1),
            ncpu: 8,
            mem_free_pct: Some(80.0),
            mem_total_gb: Some(128.0),
            on_ac: Some(true),
            thermal_limit: Some(0),
            ..Default::default()
        }),
        tick: Duration::from_millis(50),
        kill_grace: Duration::from_secs(1),
        ..JobsConfig::default()
    }));
    let job = jobs
        .submit(
            JobRequest {
                kind: JobKind::Shell,
                command: r#"while [ -d "$PWD" ] && [ ! -f release ]; do sleep 0.05; done"#.into(),
                cwd: Some(root.path().to_string_lossy().into_owned()),
                need_mem_gb: None,
                heavy: false,
                timeout_s: Some(30),
                origin_node: None,
                allow_user_active: true,
                strategy: None,
            },
            None,
            Arc::new(IdleSource),
        )
        .await
        .unwrap();
    let sessions = Arc::new(SessionManager::new(root.path().join("profile")));
    let catalog = sessions.work_catalog().await.unwrap();
    let generation = Uuid::parse_str(&catalog.watermark().await.unwrap().generation).unwrap();
    let owner = Uuid::new_v4();
    let account = Uuid::new_v4();
    let runs = OwnerRunRegistry::for_store(root.path().join("profile"), sessions);
    let backend = GooseControllerBackend::new(
        ControllerBackendConfig {
            host: HostDescriptor {
                owner_node_id: owner,
                name: "Shell fixture".into(),
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
        catalog,
        runs,
        None,
    )
    .await
    .unwrap()
    .with_adapters(Arc::new(WorkAdapters::new(
        jobs.clone(),
        root.path().join("config"),
        None,
    )))
    .await
    .unwrap();
    let (device, dispatch) = controller_auth(owner, account, generation).await;
    let entry = backend
        .catalog(&device, None)
        .await
        .unwrap()
        .entries
        .remove(0);
    assert_eq!(entry.work.kind, WorkKind::ShellJob);
    std::fs::OpenOptions::new()
        .append(true)
        .open(jobs_root.join(&job.job_id).join("log"))
        .unwrap()
        .write_all(b"output after control snapshot\n")
        .unwrap();
    let result = backend
        .accept_command(
            &device,
            WorkCommand {
                command_id: Uuid::new_v4(),
                work: entry.work,
                expected_store_generation: generation,
                expected_revision: entry.revision,
                expected_run_id: entry.current_run_id,
                action: ActionPayload::Stop {},
            },
            dispatch,
        )
        .await;
    // Always stop our fixture before inspecting a failed admission result.
    if !jobs.view(&job.job_id).unwrap().brief.state.is_terminal() {
        jobs.cancel(&job.job_id).await.unwrap();
    }
    let receipt = result.unwrap();
    assert!(matches!(
        receipt.state,
        CommandState::Delivered | CommandState::Waiting
    ));
    assert!(jobs.view(&job.job_id).unwrap().brief.state.is_terminal());
}
