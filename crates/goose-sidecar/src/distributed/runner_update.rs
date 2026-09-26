//! A goose-managed split runner is goose's to keep current (Q-116). When goose moves a runner pin
//! (the fork commit, the mlx pair), every env built under the earlier pin fails its proof and the
//! preflight refuses the start — and a user who only updated goose has nothing to press.
//!
//! So a START whose preflight is blocked ONLY by goose-managed runner envs failing their pinned
//! proof (`runnerEnv` FAILs, plus the checks that ran under those very interpreters) rebuilds them
//! on every node that needs it — the provisioning "Save and provision" runs
//! (`NodeExec::provision` → `provision::provision_on`) — points the config at the rebuilt path
//! when an earlier goose named the directory, and preflights again. Anything else that blocks
//! refuses as before, with the runner lines beside it. An interpreter the operator chose is never
//! rebuilt: its `runnerEnv` is a WARN. A dry-run preflight rebuilds nothing.

use std::sync::Mutex as StdMutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::config::{DistributedConfig, Runner};
use super::exec::{BoxFuture, NodeExec};
use super::preflight::{now_ms, CheckVerdict, PreflightReport};
use super::provision::{self, EnvSpec};

/// The checks a node's probe runs under its `python` (the tensor env): a goose-managed one that
/// cannot run fails them too, so they are that env's consequence, not a second blocker.
const UNDER_PYTHON: [&str; 3] = ["python", "gpuCeiling", "loadLock"];

/// One env goose rebuilds on one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerJob {
    pub rank: usize,
    pub node: String,
    pub host: Option<String>,
    /// The config field naming the interpreter: "python" | "pipelinePython".
    pub field: String,
    pub spec: EnvSpec,
    /// Where it is built; the config names it once built.
    pub target: String,
}

/// One env's rebuild as it goes, for the card and the Set up panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunnerUpdateRow {
    pub rank: usize,
    pub node: String,
    pub host: Option<String>,
    pub python: String,
    pub env: String,
    /// "running" | "done" | "failed"
    pub state: String,
    /// The script's last `GOOSE_PROV` step: check | uv | venv | install | done | fail.
    pub step: Option<String>,
    pub detail: String,
    /// Every line the node printed, in order.
    pub lines: Vec<String>,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
}

/// The rebuild a start ran: "running" | "done" | "failed" (a node failed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunnerUpdate {
    pub state: String,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub rows: Vec<RunnerUpdateRow>,
}

impl RunnerUpdate {
    /// The Macs it rebuilds on, by name, each once, in rank order.
    pub fn nodes(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for row in &self.rows {
            if !names.contains(&row.node) {
                names.push(row.node.clone());
            }
        }
        names
    }
}

/// Why a rebuild did not finish: the Mac, plain words, and the raw output for Details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFailure {
    pub node: String,
    pub message: String,
    pub detail: String,
}

/// The rebuilds that would unblock this start, or `None` when anything else blocks it (or
/// nothing does). A FAIL is explained only by an env in the list: its own `runnerEnv`, the
/// checks that ran under that node's `python` when that is the env being rebuilt, the node's
/// `runner` (`serve`) check under its fork env, the cluster's mlx-versions check under a tensor
/// env, and the plan when it could not be made for a reason an env being rebuilt explains.
pub fn stale_only(report: &PreflightReport) -> Option<Vec<RunnerJob>> {
    let mut jobs: Vec<RunnerJob> = Vec::new();
    for node in &report.nodes {
        for check in &node.checks {
            let Some(env) = check.env.as_ref() else {
                continue;
            };
            if check.id != "runnerEnv" || check.verdict != CheckVerdict::Fail || !env.managed {
                continue;
            }
            let (Some(spec), Some(target)) = (EnvSpec::named(&env.env), env.target.clone()) else {
                return None;
            };
            jobs.push(RunnerJob {
                rank: node.rank,
                node: node.name.clone(),
                host: node.host.clone(),
                field: env.field.clone(),
                spec,
                target,
            });
        }
    }
    if jobs.is_empty() {
        return None;
    }
    let has = |rank: usize, field: &str| jobs.iter().any(|j| j.rank == rank && j.field == field);
    for node in &report.nodes {
        for check in node
            .checks
            .iter()
            .filter(|c| c.verdict == CheckVerdict::Fail)
        {
            let explained = match check.id.as_str() {
                "runnerEnv" => check.env.as_ref().is_some_and(|e| e.managed),
                "runner" => has(node.rank, "pipelinePython"),
                id => UNDER_PYTHON.contains(&id) && has(node.rank, "python"),
            };
            if !explained {
                return None;
            }
        }
    }
    let unmeasured: Vec<usize> = report
        .nodes
        .iter()
        .filter(|n| n.ceiling_bytes.is_none() || n.available_bytes.is_none())
        .map(|n| n.rank)
        .collect();
    let plan_explained = (report.runner == Some(Runner::PipelineQwen4) && has(0, "pipelinePython"))
        || (!unmeasured.is_empty()
            && unmeasured.iter().all(|rank| {
                let node = report.nodes.iter().find(|n| n.rank == *rank);
                node.is_some_and(|n| n.available_bytes.is_some()) && has(*rank, "python")
            }));
    for check in report
        .checks
        .iter()
        .filter(|c| c.verdict == CheckVerdict::Fail)
    {
        let explained = match check.id.as_str() {
            "python" => jobs.iter().any(|j| j.field == "python"),
            "plan" => plan_explained,
            _ => false,
        };
        if !explained {
            return None;
        }
    }
    Some(jobs)
}

/// Point the config at each rebuilt env (the same path, unless an earlier goose named it).
pub fn point_at_targets(config: &mut DistributedConfig, jobs: &[RunnerJob]) {
    for job in jobs {
        let Some(node) = config.nodes.get_mut(job.rank) else {
            continue;
        };
        match job.field.as_str() {
            "pipelinePython" => node.pipeline_python = Some(job.target.clone()),
            _ => node.python = job.target.clone(),
        }
    }
}

/// Whether `ran` is `asked` with nothing changed but runner interpreters pointed at the paths
/// goose rebuilt them at — the one change a start makes to its config, which the saved setup
/// then follows. A different config (another run's, an edited one) is never taken for it.
pub fn only_repointed(asked: &DistributedConfig, ran: &DistributedConfig) -> bool {
    if asked == ran || asked.nodes.len() != ran.nodes.len() {
        return false;
    }
    let rebuilt = |python: &str| {
        provision::EnvSpec::goose_managed(python).is_some_and(|m| m.target() == python)
    };
    let mut rest = ran.clone();
    for (node, before) in rest.nodes.iter_mut().zip(&asked.nodes) {
        let python_ok = node.python == before.python || rebuilt(&node.python);
        let fork_ok = node.pipeline_python == before.pipeline_python
            || node.pipeline_python.as_deref().is_some_and(rebuilt);
        if !(python_ok && fork_ok) {
            return false;
        }
        node.python = before.python.clone();
        node.pipeline_python = before.pipeline_python.clone();
    }
    rest == *asked
}

/// How one node's provisioning ended, from its exit and its last `GOOSE_PROV` step: `Ok` only
/// when the script exited 0 AFTER its `done` line; otherwise what went wrong, said once.
pub fn provision_verdict(
    result: &Result<Option<i32>>,
    last_step: Option<&str>,
) -> std::result::Result<(), String> {
    match (result, last_step) {
        (Ok(Some(0)), Some("done")) => Ok(()),
        (Ok(code), _) => Err(format!(
            "the provisioning script exited {code:?} without finishing{}",
            if code == &Some(255) {
                " (ssh failed)"
            } else {
                ""
            }
        )),
        (Err(e), _) => Err(format!("{e:#}")),
    }
}

/// Rebuild every job's env, all nodes at once, reporting every line through `on_progress` (called
/// once with every row running before the first line). No time bound: a cold install downloads
/// wheels; ssh's transport options and the Link poll end a vanished peer.
pub async fn update_runners(
    exec: &dyn NodeExec,
    jobs: &[RunnerJob],
    on_progress: &(dyn Fn(&RunnerUpdate) + Send + Sync),
) -> std::result::Result<RunnerUpdate, UpdateFailure> {
    let started_ms = now_ms();
    let record = StdMutex::new(RunnerUpdate {
        state: "running".to_string(),
        started_ms,
        finished_ms: None,
        rows: jobs
            .iter()
            .map(|job| RunnerUpdateRow {
                rank: job.rank,
                node: job.node.clone(),
                host: job.host.clone(),
                python: job.target.clone(),
                env: job.spec.name.clone(),
                state: "running".to_string(),
                step: None,
                detail: format!("{} — {}", job.spec.name, job.spec.packages.join(" ")),
                lines: Vec::new(),
                started_ms,
                finished_ms: None,
            })
            .collect(),
    });
    on_progress(&record.lock().unwrap());
    let record = &record;
    let codes: Vec<std::result::Result<(), String>> =
        futures::future::join_all(jobs.iter().enumerate().map(|(index, job)| async move {
            let mut on_line = |line: &str| {
                let mut record = record.lock().unwrap();
                let row = &mut record.rows[index];
                row.lines.push(line.to_string());
                if let Some(progress) = provision::parse_progress(line) {
                    row.step = Some(progress.step);
                    row.detail = progress.detail;
                }
                on_progress(&record);
            };
            let result = exec
                .provision(job.host.as_deref(), &job.spec, &mut on_line)
                .await;
            let mut record = record.lock().unwrap();
            let row = &mut record.rows[index];
            let verdict = provision_verdict(&result, row.step.as_deref());
            row.finished_ms = Some(now_ms());
            match &verdict {
                Ok(()) => row.state = "done".to_string(),
                Err(why) => {
                    row.state = "failed".to_string();
                    if row.step.as_deref() != Some("fail") || result.is_err() {
                        row.detail = why.clone();
                    }
                }
            }
            on_progress(&record);
            verdict
        }))
        .await;
    let mut record = record.lock().unwrap().clone();
    record.finished_ms = Some(now_ms());
    let failed = codes.iter().position(|c| c.is_err());
    record.state = if failed.is_some() { "failed" } else { "done" }.to_string();
    on_progress(&record);
    match failed {
        None => Ok(record),
        Some(index) => Err(failure_words(&record.rows[index])),
    }
}

/// A failed row in words the owner can act on, naming the Mac; the node's own output for Details.
fn failure_words(row: &RunnerUpdateRow) -> UpdateFailure {
    let node = &row.node;
    let message = if row.step.as_deref() == Some("fail") && row.detail.starts_with("uv not found") {
        format!(
            "{node} has no uv, which goose builds the split's runner with — install it there \
             (brew install uv), then press Run again"
        )
    } else if row.detail.contains("(ssh failed)") {
        format!("goose could not reach {node} over ssh to update its split runner")
    } else {
        format!("Updating the split's runner on {node} failed")
    };
    let mut detail = format!("{} ({}): {}", row.python, row.env, row.detail);
    if !row.lines.is_empty() {
        detail.push('\n');
        detail.push_str(&row.lines.join("\n"));
    }
    UpdateFailure {
        node: node.clone(),
        message,
        detail,
    }
}

/// What a start's preflight came to once the runner rule had its say.
#[derive(Debug)]
pub enum Preflighted<T> {
    /// The preflight to judge the start on — the second one when envs were rebuilt (`updated`).
    Ready {
        report: PreflightReport,
        extra: T,
        updated: Option<RunnerUpdate>,
    },
    /// A rebuild failed; `report` is the preflight that asked for it.
    UpdateFailed {
        failure: UpdateFailure,
        report: PreflightReport,
    },
}

/// Preflight; when the only blockers are goose-managed runner envs ([`stale_only`]), rebuild
/// them ONCE, point `config` at them and preflight again. A second preflight that still finds a
/// stale env refuses by its checks like any other — a press rebuilds once, never in a loop.
pub async fn preflight_updating_runners<'a, T>(
    config: &mut DistributedConfig,
    exec: &'a dyn NodeExec,
    on_progress: &'a (dyn Fn(&RunnerUpdate) + Send + Sync),
    mut preflight: impl FnMut(DistributedConfig) -> BoxFuture<'a, Result<(PreflightReport, T)>>,
) -> Result<Preflighted<T>> {
    let (report, extra) = preflight(config.clone()).await?;
    let Some(jobs) = stale_only(&report) else {
        return Ok(Preflighted::Ready {
            report,
            extra,
            updated: None,
        });
    };
    match update_runners(exec, &jobs, on_progress).await {
        Err(failure) => Ok(Preflighted::UpdateFailed { failure, report }),
        Ok(update) => {
            point_at_targets(config, &jobs);
            let (report, extra) = preflight(config.clone()).await?;
            Ok(Preflighted::Ready {
                report,
                extra,
                updated: Some(update),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::config::tests::two_mac_config;
    use crate::distributed::exec::ExecOutput;
    use crate::distributed::preflight::{Check, NodePreflight, RunnerEnv};
    use crate::distributed::Backend;

    const MB_HOME: &str = "/Users/mihaiperdum";
    const WH_HOME: &str = "/Users/workhorse";

    fn fail(id: &str, message: &str) -> Check {
        serde_json::from_value(serde_json::json!({
            "id": id, "verdict": "fail", "message": message
        }))
        .unwrap()
    }

    fn pass(id: &str) -> Check {
        serde_json::from_value(serde_json::json!({
            "id": id, "verdict": "pass", "message": "ok"
        }))
        .unwrap()
    }

    /// The measured Q-116 check: a goose-managed fork env on 2f02ac645 while goose pins b7bd1afc2.
    fn stale_fork(home: &str) -> Check {
        let python = EnvSpec::pipeline().python(home);
        let mut check = fail(
            "runnerEnv",
            "this Mac's split runner is from an older goose — goose updates it when you press Run",
        );
        check.env = Some(RunnerEnv {
            env: EnvSpec::pipeline().name,
            field: "pipelinePython".to_string(),
            python: python.clone(),
            target: Some(python),
            managed: true,
        });
        check
    }

    fn node(rank: usize, name: &str, host: Option<&str>, checks: Vec<Check>) -> NodePreflight {
        NodePreflight {
            name: name.to_string(),
            rank,
            host: host.map(str::to_string),
            checks,
            available_bytes: Some(100),
            total_bytes: Some(200),
            pressure: Some("normal".to_string()),
            plan: None,
            link_speed: None,
            mlx_version: None,
            ceiling_bytes: Some(150),
            wired_limit_mb: None,
            short_bytes: None,
            top_apps: Vec::new(),
            leftovers: Vec::new(),
            foreign_splits: Vec::new(),
            loading: None,
        }
    }

    fn report(ok: bool, nodes: Vec<NodePreflight>) -> PreflightReport {
        PreflightReport {
            ok,
            ran_at_ms: 1,
            backend: Backend::Jaccl,
            runner: Some(Runner::PipelineQwen4),
            model_type: Some("qwen4_exp".to_string()),
            context_limit: None,
            context_source: None,
            max_context_fits: None,
            pipeline_starts: None,
            slots: None,
            checks: Vec::new(),
            nodes,
            repairs: Vec::new(),
        }
    }

    /// The 3.0.46 preflight: both Macs' fork envs stale, nothing else failing.
    fn measured_stale() -> PreflightReport {
        report(
            false,
            vec![
                node(
                    0,
                    "Mihai Macbook",
                    None,
                    vec![pass("reachable"), stale_fork(MB_HOME)],
                ),
                node(
                    1,
                    "Work's Mac Studio",
                    Some("workhorse"),
                    vec![pass("reachable"), stale_fork(WH_HOME)],
                ),
            ],
        )
    }

    /// Provisioning as data: every call recorded, each answering the script's own lines.
    struct Builds {
        seen: StdMutex<Vec<(Option<String>, String)>>,
        fail_on: Option<&'static str>,
    }

    impl NodeExec for Builds {
        fn run<'a>(
            &'a self,
            _host: Option<&'a str>,
            _script: &'a str,
        ) -> BoxFuture<'a, Result<ExecOutput>> {
            Box::pin(async { panic!("the rebuild runs no raw script") })
        }

        fn provision<'a>(
            &'a self,
            host: Option<&'a str>,
            spec: &'a EnvSpec,
            on_line: &'a mut (dyn FnMut(&str) + Send),
        ) -> BoxFuture<'a, Result<Option<i32>>> {
            self.seen
                .lock()
                .unwrap()
                .push((host.map(str::to_string), spec.name.clone()));
            let fails = self.fail_on.is_some() && host == self.fail_on;
            Box::pin(async move {
                on_line("GOOSE_PROV check /p");
                on_line("GOOSE_PROV install rapid-mlx @ git+…");
                if fails {
                    on_line("  × Failed to download `mlx-vlm==0.7.1`");
                    on_line("GOOSE_PROV fail uv pip install exited 1");
                    return Ok(Some(5));
                }
                on_line("GOOSE_PROV done installed /p 0.32.2 0.31.3 b7bd");
                Ok(Some(0))
            })
        }
    }

    fn builds(fail_on: Option<&'static str>) -> Builds {
        Builds {
            seen: StdMutex::new(Vec::new()),
            fail_on,
        }
    }

    /// Stale on both Macs and nothing else: the start rebuilds both, preflights again, and is
    /// judged on that second preflight — which passes, so it starts.
    #[tokio::test]
    async fn stale_only_rebuilds_on_every_node_then_preflights_again_and_starts() {
        let exec = builds(None);
        let seen_progress = StdMutex::new(Vec::<String>::new());
        let progress = |u: &RunnerUpdate| {
            seen_progress
                .lock()
                .unwrap()
                .push(format!("{}:{}", u.state, u.rows.len()))
        };
        let preflights = StdMutex::new(vec![report(true, Vec::new()), measured_stale()]);
        let mut config = two_mac_config();
        let outcome = preflight_updating_runners(&mut config, &exec, &progress, |_| {
            let next = preflights.lock().unwrap().pop().unwrap();
            Box::pin(async move { Ok((next, ())) })
        })
        .await
        .unwrap();
        let Preflighted::Ready {
            report, updated, ..
        } = outcome
        else {
            panic!("{outcome:?}")
        };
        assert!(report.ok, "the start is judged on the second preflight");
        assert!(preflights.lock().unwrap().is_empty(), "preflighted twice");
        let updated = updated.unwrap();
        assert_eq!(updated.state, "done");
        assert_eq!(updated.nodes(), ["Mihai Macbook", "Work's Mac Studio"]);
        assert_eq!(
            *exec.seen.lock().unwrap(),
            vec![
                (None, EnvSpec::pipeline().name),
                (Some("workhorse".to_string()), EnvSpec::pipeline().name)
            ]
        );
        let progress = seen_progress.lock().unwrap();
        assert_eq!(progress.first().map(String::as_str), Some("running:2"));
        assert_eq!(progress.last().map(String::as_str), Some("done:2"));
        assert!(updated
            .rows
            .iter()
            .all(|r| r.step.as_deref() == Some("done")));
    }

    /// A stale env beside ANOTHER failure (a port already bound): nothing is rebuilt, and the
    /// refusal carries both.
    #[tokio::test]
    async fn a_stale_env_plus_another_failure_refuses_with_both_and_rebuilds_nothing() {
        let mut blocked = measured_stale();
        blocked.nodes[0]
            .checks
            .push(fail("ports", "already listening: 8190 (pid 4242)"));
        assert!(stale_only(&blocked).is_none());
        let exec = builds(None);
        let mut config = two_mac_config();
        let first = blocked.clone();
        let outcome =
            preflight_updating_runners(&mut config, &exec, &|_: &RunnerUpdate| {}, |_| {
                let report = first.clone();
                Box::pin(async move { Ok((report, ())) })
            })
            .await
            .unwrap();
        let Preflighted::Ready {
            report, updated, ..
        } = outcome
        else {
            panic!("{outcome:?}")
        };
        assert!(updated.is_none());
        assert!(exec.seen.lock().unwrap().is_empty());
        let failures = report.failures().join("; ");
        assert!(
            failures.contains("Mihai Macbook ports: already listening")
                && failures.contains(
                    "Mihai Macbook runnerEnv: this Mac's split runner is from an older goose"
                )
                && failures.contains("Work's Mac Studio runnerEnv"),
            "{failures}"
        );
    }

    /// An interpreter the operator chose is never rebuilt: its `runnerEnv` is a WARN (so the
    /// start is judged as before), and even a FAIL on it would not qualify.
    #[test]
    fn a_stale_env_the_user_chose_is_never_rebuilt() {
        let own = "/Users/me/Projects/Rapid-MLX/.venv/bin/python";
        let mut warn = stale_fork(MB_HOME);
        warn.verdict = CheckVerdict::Warn;
        warn.env = Some(RunnerEnv {
            python: own.to_string(),
            managed: false,
            ..warn.env.unwrap()
        });
        assert!(stale_only(&report(
            true,
            vec![node(0, "Mihai Macbook", None, vec![warn.clone()])]
        ))
        .is_none());
        let mut failing = warn;
        failing.verdict = CheckVerdict::Fail;
        assert!(stale_only(&report(
            false,
            vec![node(0, "Mihai Macbook", None, vec![failing])]
        ))
        .is_none());
    }

    /// A MISSING tensor env fails the checks that ran under it and leaves the plan unmade: those
    /// are its consequences, so the rebuild still goes — the plan's other causes do not.
    #[test]
    fn a_missing_tensor_env_explains_the_checks_run_under_it() {
        let python = EnvSpec::tensor().python(WH_HOME);
        let mut env_check = fail(
            "runnerEnv",
            "this Mac has no working split runner — goose builds it when you press Run",
        );
        env_check.env = Some(RunnerEnv {
            env: EnvSpec::tensor().name,
            field: "python".to_string(),
            python: python.clone(),
            target: Some(python.clone()),
            managed: true,
        });
        let mut studio = node(
            1,
            "Work's Mac Studio",
            Some("workhorse"),
            vec![
                fail("python", "cannot import mlx + mlx_lm"),
                fail("gpuCeiling", "could not be read"),
                fail("loadLock", "the node's load lock could not be read"),
                env_check,
            ],
        );
        studio.ceiling_bytes = None;
        let mut blocked = report(
            false,
            vec![node(0, "Mihai Macbook", None, Vec::new()), studio],
        );
        blocked.runner = Some(Runner::MlxLmTensor);
        blocked.checks.push(fail(
            "plan",
            "no plan: a node's memory or GPU ceiling could not be measured",
        ));
        let jobs = stale_only(&blocked).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].field, "python");
        assert_eq!(jobs[0].host.as_deref(), Some("workhorse"));

        // The same plan failure on a node whose MEMORY could not be read is not the env's doing.
        blocked.nodes[1].available_bytes = None;
        assert!(stale_only(&blocked).is_none());
    }

    /// An earlier goose's directory: the rebuild goes to the current path and the config follows.
    #[test]
    fn the_config_follows_a_renamed_env_to_its_rebuilt_path() {
        let mut config = two_mac_config();
        let old = format!("{WH_HOME}/.goose/distributed/mlx0.31.0-mlxlm0.30.2-py3.12/bin/python");
        config.nodes[1].python = old;
        let asked = config.clone();
        let job = RunnerJob {
            rank: 1,
            node: "workhorse".to_string(),
            host: Some("workhorse".to_string()),
            field: "python".to_string(),
            spec: EnvSpec::tensor(),
            target: EnvSpec::tensor().python(WH_HOME),
        };
        point_at_targets(&mut config, &[job]);
        assert_eq!(config.nodes[1].python, EnvSpec::tensor().python(WH_HOME));
        assert!(only_repointed(&asked, &config), "the saved setup follows");
        assert!(
            !only_repointed(&asked, &asked),
            "nothing changed, nothing saved"
        );
        let mut other = config.clone();
        other.port += 1;
        assert!(
            !only_repointed(&asked, &other),
            "another run's config is never saved over the asked one"
        );
        let mut own = asked.clone();
        own.nodes[1].python = "/tmp/elsewhere/bin/python".to_string();
        assert!(!only_repointed(&asked, &own));
    }

    /// A failed rebuild names the Mac in plain words; the node's output rides Details.
    #[tokio::test]
    async fn a_failed_rebuild_names_the_mac_and_keeps_the_raw_output_for_details() {
        let exec = builds(Some("workhorse"));
        let jobs = stale_only(&measured_stale()).unwrap();
        let failure = update_runners(&exec, &jobs, &|_: &RunnerUpdate| {})
            .await
            .unwrap_err();
        assert_eq!(failure.node, "Work's Mac Studio");
        assert_eq!(
            failure.message,
            "Updating the split's runner on Work's Mac Studio failed"
        );
        assert!(
            failure.detail.contains("uv pip install exited 1")
                && failure
                    .detail
                    .contains("Failed to download `mlx-vlm==0.7.1`"),
            "{}",
            failure.detail
        );

        let mut row = RunnerUpdateRow {
            rank: 1,
            node: "Work's Mac Studio".to_string(),
            host: Some("workhorse".to_string()),
            python: "/p".to_string(),
            env: "e".to_string(),
            state: "failed".to_string(),
            step: Some("fail".to_string()),
            detail: "uv not found on this node (looked: …)".to_string(),
            lines: Vec::new(),
            started_ms: 0,
            finished_ms: Some(1),
        };
        assert!(failure_words(&row).message.contains("has no uv"));
        row.step = None;
        row.detail = provision_verdict(&Ok(Some(255)), None).unwrap_err();
        assert_eq!(
            failure_words(&row).message,
            "goose could not reach Work's Mac Studio over ssh to update its split runner"
        );
    }
}
