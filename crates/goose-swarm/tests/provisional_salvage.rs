use async_trait::async_trait;
use goose_swarm::{
    salvage_artifact_hashes, Dag, DeviceCfg, Difficulty, DispatchError, DispatchRequest,
    SalvageReason, Scheduler, TaskCompletionDisposition, TaskDispatcher, TaskRunOutput, TaskSpec,
};
use std::collections::BTreeMap;
use std::sync::Arc;

struct SalvageDispatcher {
    forged_partial: bool,
}

#[async_trait]
impl TaskDispatcher for SalvageDispatcher {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        if req.task_id != "module" && !self.forged_partial {
            return Ok(format!("completed {}", req.task_id).into());
        }
        let artifact_hashes = if self.forged_partial {
            BTreeMap::from([(req.owned_files[0].clone(), "sha256:forged".to_string())])
        } else {
            salvage_artifact_hashes(&req.task_id, &req.owned_files)
                .expect("fixture has complete eligible artifacts")
        };
        Ok(TaskRunOutput {
            output: "partial agent turn; artifacts survived".to_string(),
            session_id: Some("salvage-session".to_string()),
            tool_calls: Vec::new(),
            completion: TaskCompletionDisposition::salvaged(
                SalvageReason::ProgressWatchdog,
                artifact_hashes,
            ),
        })
    }
}

fn task(id: &str, deps: &[&str], files: Vec<String>) -> TaskSpec {
    TaskSpec {
        id: id.to_string(),
        description: format!("implement {id}"),
        difficulty: Difficulty::Easy,
        preferred_model: None,
        owned_files: files,
        deps: deps.iter().map(|dep| (*dep).to_string()).collect(),
        subsplit: Vec::new(),
        replan_authority: None,
    }
}

fn scheduler() -> Scheduler {
    Scheduler::new(
        vec![DeviceCfg {
            id: "node".to_string(),
            model_id: "model".to_string(),
            weight: 1,
            enabled: true,
            speed_weight: 1,
            supervision: false,
        }],
        1,
    )
}

#[tokio::test]
async fn salvaged_artifacts_release_dependents_but_never_become_done() {
    let fixture = tempfile::TempDir::new_in(".").unwrap();
    let rel = fixture
        .path()
        .strip_prefix(std::env::current_dir().unwrap())
        .unwrap_or(fixture.path());
    let module = rel.join("src/module.rs");
    std::fs::create_dir_all(module.parent().unwrap()).unwrap();
    std::fs::write(&module, "pub fn value() -> u32 { 7 }\n").unwrap();
    let module_rel = module.to_string_lossy().to_string();

    let dag = Dag::from_specs(vec![
        task("module", &[], vec![module_rel]),
        task("consumer", &["module"], Vec::new()),
    ])
    .unwrap();
    let report = scheduler()
        .run(
            dag,
            Arc::new(SalvageDispatcher {
                forged_partial: false,
            }),
            String::new(),
        )
        .await
        .unwrap();

    assert_eq!(report.done, vec!["consumer"]);
    assert_eq!(report.salvaged, vec!["module"]);
    assert!(report.failed.is_empty());
    let outcome = report
        .tasks
        .iter()
        .find(|outcome| outcome.task_id == "module")
        .unwrap();
    assert_eq!(outcome.status, "salvaged");
    assert!(outcome.salvaged);
    assert!(matches!(
        &outcome.completion,
        Some(TaskCompletionDisposition::Salvaged {
            reason: SalvageReason::ProgressWatchdog,
            ..
        })
    ));
}

#[tokio::test]
async fn forged_or_partial_salvage_fails_closed_and_blocks_dependents() {
    let fixture = tempfile::TempDir::new_in(".").unwrap();
    let rel = fixture
        .path()
        .strip_prefix(std::env::current_dir().unwrap())
        .unwrap_or(fixture.path());
    let first = rel.join("src/first.rs");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::write(&first, "pub fn first() {}\n").unwrap();

    let dag = Dag::from_specs(vec![
        task(
            "module",
            &[],
            vec![
                first.to_string_lossy().to_string(),
                rel.join("src/missing.rs").to_string_lossy().to_string(),
            ],
        ),
        task("consumer", &["module"], Vec::new()),
    ])
    .unwrap();
    let report = scheduler()
        .run(
            dag,
            Arc::new(SalvageDispatcher {
                forged_partial: true,
            }),
            String::new(),
        )
        .await
        .unwrap();

    assert!(report.done.is_empty());
    assert!(report.salvaged.is_empty());
    assert_eq!(report.failed, vec!["consumer", "module"]);
    let outcome = report
        .tasks
        .iter()
        .find(|outcome| outcome.task_id == "module")
        .unwrap();
    assert_eq!(outcome.status, "failed");
    assert!(!outcome.salvaged);
    assert!(outcome.completion.is_none());
    assert_eq!(outcome.attempt_history[0].outcome, "invalid_salvage");
}

#[tokio::test]
async fn archived_r1_test_task_cannot_repeat_the_done_salvage_contradiction() {
    let archived: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evals/swarm-bench/fixtures/qwen38-r1-salvage.json"
    ))
    .unwrap();
    assert_eq!(
        archived["source"]["run_log_sha256"],
        "6402923479726a0a1533493955c0b5625caa59661db630e7b903d274dfcdd5b6"
    );
    assert_eq!(archived["transient_completion"]["salvaged"], true);
    assert_eq!(archived["final_report"]["status"], "done");
    assert!(archived["final_report"]["salvaged"].is_null());

    let fixture = tempfile::TempDir::new_in(".").unwrap();
    let rel = fixture
        .path()
        .strip_prefix(std::env::current_dir().unwrap())
        .unwrap_or(fixture.path());
    let test_file = rel.join("tests/test_webhook.py");
    std::fs::create_dir_all(test_file.parent().unwrap()).unwrap();
    std::fs::write(&test_file, "def test_webhook():\n    assert True\n").unwrap();

    let report = scheduler()
        .run(
            Dag::from_specs(vec![task(
                "test-webhook",
                &[],
                vec![test_file.to_string_lossy().to_string()],
            )])
            .unwrap(),
            Arc::new(SalvageDispatcher {
                forged_partial: true,
            }),
            String::new(),
        )
        .await
        .unwrap();

    assert!(report.done.is_empty());
    assert!(report.salvaged.is_empty());
    assert_eq!(report.failed, vec!["test-webhook"]);
    let outcome = &report.tasks[0];
    assert_eq!(outcome.status, "failed");
    assert!(!outcome.salvaged);
    assert!(outcome.completion.is_none());
}
