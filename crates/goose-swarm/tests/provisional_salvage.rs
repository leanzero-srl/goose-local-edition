use async_trait::async_trait;
use goose_swarm::{
    Dag, DeviceCfg, Difficulty, DispatchError, DispatchRequest, RequiredVerification, Scheduler,
    TaskCompletionDisposition, TaskDispatcher, TaskRunOutput, TaskSpec,
};
use std::sync::Arc;

struct SalvageDispatcher {
    salvage_task: &'static str,
    rewrite_owned_files: bool,
}

#[async_trait]
impl TaskDispatcher for SalvageDispatcher {
    async fn run(&self, request: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        if request.task_id == self.salvage_task && self.rewrite_owned_files {
            for path in &request.owned_files {
                if let Some(parent) = std::path::Path::new(path).parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(path, b"artifact changed by the dispatched task\n").unwrap();
            }
        }
        Ok(TaskRunOutput {
            output: format!("worker output for {}", request.task_id),
            session_id: Some("salvage-session".to_string()),
            tool_calls: Vec::new(),
            salvaged: request.task_id == self.salvage_task,
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
        deps: deps
            .iter()
            .map(|dependency| (*dependency).to_string())
            .collect(),
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

fn fixture_file(relative: &str, bytes: &[u8]) -> (tempfile::TempDir, String) {
    let fixture = tempfile::Builder::new()
        .prefix("provisional-integration-")
        .tempdir_in(".")
        .unwrap();
    let path = fixture.path().join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    let root = std::env::current_dir().unwrap().canonicalize().unwrap();
    let relative = path
        .canonicalize()
        .unwrap()
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    (fixture, relative)
}

#[tokio::test]
async fn unresolved_salvage_releases_dependents_but_blocks_a_green_report() {
    let (_fixture, module) = fixture_file("src/module.rs", b"pub fn value() -> u32 { 7 }\n");
    let dag = Dag::from_specs(vec![
        task("module", &[], vec![module.clone()]),
        task("consumer", &["module"], Vec::new()),
    ])
    .unwrap();
    let report = scheduler()
        .run(
            dag,
            Arc::new(SalvageDispatcher {
                salvage_task: "module",
                rewrite_owned_files: true,
            }),
            String::new(),
        )
        .await
        .unwrap();

    assert_eq!(report.done, vec!["consumer"]);
    assert_eq!(report.salvaged, vec!["module"]);
    assert_eq!(report.failed, vec!["module"]);
    assert!(
        !report.bonus.contains(&"module".to_string()),
        "unverified salvage cannot receive the CLI's bonus-failure exemption"
    );
    let outcome = report
        .tasks
        .iter()
        .find(|outcome| outcome.task_id == "module")
        .unwrap();
    assert_eq!(outcome.status, "salvaged");
    assert!(outcome.salvaged);
    let receipt = outcome
        .completion
        .as_ref()
        .and_then(TaskCompletionDisposition::provisional_receipt)
        .unwrap();
    assert_eq!(receipt.task_id(), "module");
    assert_eq!(receipt.artifacts().len(), 1);
    assert!(receipt.artifacts().contains_key(&module));
    assert_eq!(
        receipt.required_verification(),
        RequiredVerification::FullRepairRuler
    );
}

#[tokio::test]
async fn partial_artifact_salvage_fails_closed_and_blocks_dependents() {
    let (_fixture, first) = fixture_file("src/first.rs", b"pub fn first() {}\n");
    let missing = first.replace("first.rs", "missing.rs");
    let dag = Dag::from_specs(vec![
        task("module", &[], vec![first.clone(), missing]),
        task(
            "consumer",
            &["module"],
            vec![first.replace("first.rs", "consumer.rs")],
        ),
    ])
    .unwrap();
    let report = scheduler()
        .run(
            dag,
            Arc::new(SalvageDispatcher {
                salvage_task: "module",
                rewrite_owned_files: false,
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
async fn archived_qwen_r1_test_task_cannot_repeat_done_salvage_contradiction() {
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

    let (_fixture, test_file) = fixture_file(
        "tests/test_webhook.py",
        b"def test_webhook():\n    assert True\n",
    );
    let report = scheduler()
        .run(
            Dag::from_specs(vec![task("test-webhook", &[], vec![test_file])]).unwrap(),
            Arc::new(SalvageDispatcher {
                salvage_task: "test-webhook",
                rewrite_owned_files: false,
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

#[tokio::test]
async fn preexisting_unchanged_artifact_cannot_unlock_salvage() {
    let (_fixture, module) = fixture_file("src/preexisting.rs", b"pub fn old() {}\n");
    let report = scheduler()
        .run(
            Dag::from_specs(vec![task("module", &[], vec![module])]).unwrap(),
            Arc::new(SalvageDispatcher {
                salvage_task: "module",
                rewrite_owned_files: false,
            }),
            String::new(),
        )
        .await
        .unwrap();

    assert!(report.done.is_empty());
    assert!(report.salvaged.is_empty());
    assert_eq!(report.failed, vec!["module"]);
    let outcome = &report.tasks[0];
    assert_eq!(outcome.status, "failed");
    assert_eq!(outcome.attempt_history[0].outcome, "invalid_salvage");
    assert!(outcome.completion.is_none());
}
