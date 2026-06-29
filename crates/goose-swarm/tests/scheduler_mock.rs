//! M1.0 de-risking gate: the scheduler concurrency core, tested against a MockDispatcher with no
//! model involved. Asserts the five invariants: no double-claim, dependency gating, per-device
//! weighting, transient re-dispatch (to a different device), and file-overlap serialization.

use async_trait::async_trait;
use goose_swarm::{
    ChildSpec, Dag, DeviceCfg, Difficulty, DispatchError, DispatchRequest, Judge, JudgeConfig,
    JudgeOutcome, JudgeRequest, PreReviewOutput, PreReviewRequest, PreReviewer, ReplanContext,
    Replanner, Scheduler, TaskDispatcher, TaskRunOutput, TaskSpec, Verdict,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn ordered_pair(a: &str, b: &str) -> (String, String) {
    if a < b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

#[derive(Default)]
struct Recorder {
    seq: usize,
    runs: HashMap<String, usize>,
    run_devices: HashMap<String, Vec<String>>,
    total_per_device: HashMap<String, usize>,
    cur_per_device: HashMap<String, usize>,
    peak_per_device: HashMap<String, usize>,
    cur_tasks: HashSet<String>,
    overlapped: HashSet<(String, String)>,
    first_start_seq: HashMap<String, usize>,
    end_seq: HashMap<String, usize>,
}

impl Recorder {
    fn on_start(&mut self, req: &DispatchRequest) {
        self.seq += 1;
        let s = self.seq;
        self.first_start_seq.entry(req.task_id.clone()).or_insert(s);
        *self.runs.entry(req.task_id.clone()).or_default() += 1;
        self.run_devices
            .entry(req.task_id.clone())
            .or_default()
            .push(req.device_id.clone());
        *self
            .total_per_device
            .entry(req.device_id.clone())
            .or_default() += 1;
        let c = self
            .cur_per_device
            .entry(req.device_id.clone())
            .or_default();
        *c += 1;
        let cur = *c;
        let p = self
            .peak_per_device
            .entry(req.device_id.clone())
            .or_default();
        if cur > *p {
            *p = cur;
        }
        for t in &self.cur_tasks {
            self.overlapped.insert(ordered_pair(&req.task_id, t));
        }
        self.cur_tasks.insert(req.task_id.clone());
    }

    fn on_end(&mut self, req: &DispatchRequest) {
        self.seq += 1;
        self.end_seq.insert(req.task_id.clone(), self.seq);
        self.cur_tasks.remove(&req.task_id);
        if let Some(c) = self.cur_per_device.get_mut(&req.device_id) {
            if *c > 0 {
                *c -= 1;
            }
        }
    }
}

struct MockDispatcher {
    rec: Arc<Mutex<Recorder>>,
    delay: Duration,
    fail_transient_first: HashSet<String>,
    terminal: HashSet<String>,
    slow: HashSet<String>,
}

#[async_trait]
impl TaskDispatcher for MockDispatcher {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        self.rec.lock().unwrap().on_start(&req);
        let d = if self.slow.contains(&req.task_id) {
            self.delay * 8
        } else {
            self.delay
        };
        tokio::time::sleep(d).await;
        let result = if self.terminal.contains(&req.task_id) {
            Err(DispatchError::Terminal("boom".into()))
        } else if self.fail_transient_first.contains(&req.task_id) && req.attempt == 0 {
            Err(DispatchError::Transient("Model is unloaded".into()))
        } else {
            Ok(format!("output-of-{}", req.task_id).into())
        };
        self.rec.lock().unwrap().on_end(&req);
        result
    }
}

fn spec(id: &str, deps: &[&str], files: &[&str]) -> TaskSpec {
    TaskSpec {
        id: id.to_string(),
        description: format!("do {id}"),
        difficulty: Difficulty::Easy,
        preferred_model: None,
        owned_files: files.iter().map(|s| s.to_string()).collect(),
        deps: deps.iter().map(|s| s.to_string()).collect(),
    }
}

fn dev(id: &str, model: &str, weight: u32) -> DeviceCfg {
    DeviceCfg {
        id: id.to_string(),
        model_id: model.to_string(),
        weight,
        enabled: true,
        speed_weight: 1,
    }
}

fn mock(rec: &Arc<Mutex<Recorder>>, delay_ms: u64) -> Arc<MockDispatcher> {
    Arc::new(MockDispatcher {
        rec: rec.clone(),
        delay: Duration::from_millis(delay_ms),
        fail_transient_first: HashSet::new(),
        terminal: HashSet::new(),
        slow: HashSet::new(),
    })
}

#[tokio::test]
async fn no_double_claim_and_all_done() {
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let specs: Vec<_> = (0..12).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let sched = Scheduler::new(vec![dev("a", "m-a", 2), dev("b", "m-b", 2)], 3);
    let report = sched.run(dag, mock(&rec, 20), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 12, "all tasks done");
    assert!(report.failed.is_empty());
    let r = rec.lock().unwrap();
    for i in 0..12 {
        assert_eq!(
            r.runs[&format!("t{i}")],
            1,
            "task t{i} dispatched exactly once (no double-claim)"
        );
    }
}

#[tokio::test]
async fn dependent_waits_for_dependency() {
    let specs = vec![
        spec("a", &[], &[]),
        spec("b", &["a"], &[]),
        spec("c", &["b"], &[]),
    ];
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let sched = Scheduler::new(vec![dev("d1", "m-1", 3), dev("d2", "m-2", 3)], 3);
    let report = sched.run(dag, mock(&rec, 20), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 3);
    let r = rec.lock().unwrap();
    assert!(
        r.first_start_seq["b"] > r.end_seq["a"],
        "b started before a finished"
    );
    assert!(
        r.first_start_seq["c"] > r.end_seq["b"],
        "c started before b finished"
    );
}

#[tokio::test]
async fn weighting_caps_in_flight_per_device() {
    let specs: Vec<_> = (0..24).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let sched = Scheduler::new(vec![dev("big", "m-big", 3), dev("small", "m-small", 1)], 3);
    let report = sched.run(dag, mock(&rec, 40), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 24);
    let r = rec.lock().unwrap();
    assert!(r.peak_per_device["big"] <= 3, "big never exceeds weight 3");
    assert!(
        r.peak_per_device.get("small").copied().unwrap_or(0) <= 1,
        "small never exceeds weight 1"
    );
    assert_eq!(r.peak_per_device["big"], 3, "big saturates to its weight");
    assert!(
        r.total_per_device["big"] > r.total_per_device["small"],
        "higher-weight device handled more work ({} vs {})",
        r.total_per_device["big"],
        r.total_per_device["small"]
    );
}

#[tokio::test]
async fn transient_redispatches_to_a_different_device() {
    let dag = Dag::from_specs(vec![spec("x", &[], &[])]).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let disp = Arc::new(MockDispatcher {
        rec: rec.clone(),
        delay: Duration::from_millis(10),
        fail_transient_first: HashSet::from(["x".to_string()]),
        terminal: HashSet::new(),
        slow: HashSet::new(),
    });
    let sched = Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3);
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert_eq!(report.done, vec!["x".to_string()], "x eventually succeeds");
    let r = rec.lock().unwrap();
    assert_eq!(
        r.runs["x"], 2,
        "x ran twice: one transient failure + one success"
    );
    let devs = &r.run_devices["x"];
    assert_ne!(
        devs[0], devs[1],
        "re-dispatch steered to a different device"
    );
}

#[tokio::test]
async fn spreads_independent_tasks_across_idle_devices() {
    // 9 independent tasks, three weight-1 devices, no preferred model: spread routing must use ALL
    // three devices (the first pass claims one task per idle device), not pile onto the first.
    let specs: Vec<_> = (0..9).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let sched = Scheduler::new(
        vec![dev("a", "m-a", 1), dev("b", "m-b", 1), dev("c", "m-c", 1)],
        3,
    );
    let report = sched.run(dag, mock(&rec, 30), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 9);
    let r = rec.lock().unwrap();
    for d in ["a", "b", "c"] {
        assert!(
            r.total_per_device.get(d).copied().unwrap_or(0) >= 1,
            "device {d} must receive work under spread routing"
        );
    }
    let active = ["a", "b", "c"]
        .iter()
        .filter(|d| r.peak_per_device.get(**d).copied().unwrap_or(0) >= 1)
        .count();
    assert_eq!(
        active, 3,
        "all three devices must run concurrently, not just one"
    );
}

#[tokio::test]
async fn preferred_model_breaks_ties_but_does_not_concentrate() {
    // Two independent tasks both preferring device `a`'s model, with `a` and `b` each weight 2.
    // Spread must place the second on `b` (idle) rather than doubling up on `a`.
    let mut s1 = spec("p1", &[], &[]);
    s1.preferred_model = Some("m-a".to_string());
    let mut s2 = spec("p2", &[], &[]);
    s2.preferred_model = Some("m-a".to_string());
    let dag = Dag::from_specs(vec![s1, s2]).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let sched = Scheduler::new(vec![dev("a", "m-a", 2), dev("b", "m-b", 2)], 3);
    let report = sched.run(dag, mock(&rec, 40), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 2);
    let r = rec.lock().unwrap();
    assert!(
        r.total_per_device.get("b").copied().unwrap_or(0) >= 1,
        "the second same-model task must spread to the idle device, not pile on the preferred one"
    );
}

#[tokio::test]
async fn file_overlap_serializes() {
    let specs = vec![
        spec("a", &[], &["shared.rs"]),
        spec("b", &[], &["shared.rs"]),
        spec("c", &[], &[]),
    ];
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    // Plenty of device capacity: only the file hold should prevent a+b from overlapping.
    let sched = Scheduler::new(vec![dev("d1", "m-1", 2), dev("d2", "m-2", 2)], 3);
    let report = sched.run(dag, mock(&rec, 30), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 3);
    let r = rec.lock().unwrap();
    assert!(
        !r.overlapped.contains(&ordered_pair("a", "b")),
        "tasks sharing a file must never run concurrently"
    );
}

#[tokio::test]
async fn terminal_failure_fails_descendants_without_deadlock() {
    // a (terminal fail) -> b -> c ; plus independent d which must still complete.
    let specs = vec![
        spec("a", &[], &[]),
        spec("b", &["a"], &[]),
        spec("c", &["b"], &[]),
        spec("d", &[], &[]),
    ];
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let disp = Arc::new(MockDispatcher {
        rec: rec.clone(),
        delay: Duration::from_millis(10),
        fail_transient_first: HashSet::new(),
        terminal: HashSet::from(["a".to_string()]),
        slow: HashSet::new(),
    });
    let sched = Scheduler::new(vec![dev("d1", "m-1", 2)], 3);
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert_eq!(
        report.done,
        vec!["d".to_string()],
        "independent task still completes"
    );
    let failed: HashSet<_> = report.failed.iter().cloned().collect();
    assert_eq!(
        failed,
        HashSet::from(["a".to_string(), "b".to_string(), "c".to_string()])
    );
}

struct MockReplanner {
    rounds: Mutex<VecDeque<Vec<TaskSpec>>>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Replanner for MockReplanner {
    async fn replan(&self, _ctx: ReplanContext) -> anyhow::Result<Vec<TaskSpec>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.rounds.lock().unwrap().pop_front().unwrap_or_default())
    }
}

fn slow_dispatcher(
    rec: &Arc<Mutex<Recorder>>,
    delay_ms: u64,
    slow: &[&str],
) -> Arc<MockDispatcher> {
    Arc::new(MockDispatcher {
        rec: rec.clone(),
        delay: Duration::from_millis(delay_ms),
        fail_transient_first: HashSet::new(),
        terminal: HashSet::new(),
        slow: slow.iter().map(|s| s.to_string()).collect(),
    })
}

#[tokio::test]
async fn idle_triggers_replan_and_fills_nodes() {
    // `slow` runs long while `fast` finishes and frees nodes -> idle window -> replan adds b,c.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let dag = Dag::from_specs(vec![spec("slow", &[], &[]), spec("fast", &[], &[])]).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let replanner = Arc::new(MockReplanner {
        rounds: Mutex::new(VecDeque::from(vec![vec![
            spec("b", &[], &[]),
            spec("c", &[], &[]),
        ]])),
        calls: calls.clone(),
    });
    let sched = Scheduler::new(
        vec![dev("d0", "m0", 1), dev("d1", "m1", 1), dev("d2", "m2", 1)],
        3,
    )
    .with_replanner(replanner, 3);
    let report = sched
        .run(dag, slow_dispatcher(&rec, 30, &["slow"]), "goal".into())
        .await
        .unwrap();
    let done: HashSet<_> = report.done.iter().cloned().collect();
    assert!(
        ["slow", "fast", "b", "c"].iter().all(|t| done.contains(*t)),
        "replan-added tasks must run: done={:?}",
        report.done
    );
    assert!(
        calls.load(Ordering::SeqCst) >= 1,
        "the replanner was invoked while nodes idled"
    );
}

#[tokio::test]
async fn empty_replan_stops_cleanly() {
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let dag = Dag::from_specs(vec![spec("slow", &[], &[]), spec("fast", &[], &[])]).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let replanner = Arc::new(MockReplanner {
        rounds: Mutex::new(VecDeque::new()), // always empty -> stop, no spin
        calls: calls.clone(),
    });
    let sched = Scheduler::new(
        vec![dev("d0", "m0", 1), dev("d1", "m1", 1), dev("d2", "m2", 1)],
        3,
    )
    .with_replanner(replanner, 3);
    let report = sched
        .run(dag, slow_dispatcher(&rec, 30, &["slow"]), "g".into())
        .await
        .unwrap();
    assert_eq!(
        report.done.len(),
        2,
        "an empty replan adds nothing and ends cleanly"
    );
}

#[tokio::test]
async fn replan_respects_max_replans() {
    // The replanner keeps offering NEW slow tasks; the budget must cap the rounds.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let dag = Dag::from_specs(vec![spec("slow", &[], &[]), spec("fast", &[], &[])]).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let replanner = Arc::new(MockReplanner {
        rounds: Mutex::new(VecDeque::from(vec![
            vec![spec("r1", &[], &[])],
            vec![spec("r2", &[], &[])],
            vec![spec("r3", &[], &[])],
        ])),
        calls: calls.clone(),
    });
    let sched = Scheduler::new(
        vec![dev("d0", "m0", 1), dev("d1", "m1", 1), dev("d2", "m2", 1)],
        3,
    )
    .with_replanner(replanner, 2);
    let report = sched
        .run(
            dag,
            slow_dispatcher(&rec, 25, &["slow", "r1", "r2", "r3"]),
            "g".into(),
        )
        .await
        .unwrap();
    assert!(
        calls.load(Ordering::SeqCst) <= 2,
        "replan rounds must not exceed max_replans"
    );
    assert!(
        !report.done.contains(&"r3".to_string()),
        "r3 must never be requested once the cap is hit"
    );
}

#[tokio::test]
async fn cycle_is_rejected_at_load() {
    let specs = vec![spec("a", &["b"], &[]), spec("b", &["a"], &[])];
    assert!(
        Dag::from_specs(specs).is_err(),
        "a dependency cycle must be rejected"
    );
}

/// A dispatcher whose target task hangs on its first attempt (long enough to be judged + killed) and
/// completes quickly on the re-dispatch. Records run counts and any hint the re-dispatch carried.
struct JudgeTestDispatcher {
    runs: Arc<Mutex<HashMap<String, u32>>>,
    hints: Arc<Mutex<Vec<(String, String)>>>,
    target: String,
    delay: Duration,
    // When true the target sleeps long on EVERY attempt (simulating a worker that never recovers), so a
    // cap-exhausted attempt stays alive long enough for the judge's terminal-fail to act on it.
    slow_all: bool,
}

#[async_trait]
impl TaskDispatcher for JudgeTestDispatcher {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        *self
            .runs
            .lock()
            .unwrap()
            .entry(req.task_id.clone())
            .or_default() += 1;
        if let Some(h) = &req.prior_hint {
            self.hints
                .lock()
                .unwrap()
                .push((req.task_id.clone(), h.clone()));
        }
        if req.task_id == self.target && (req.attempt == 0 || self.slow_all) {
            tokio::time::sleep(self.delay * 50).await; // long — the judge aborts this attempt
        } else {
            tokio::time::sleep(self.delay).await;
        }
        Ok(format!("out-{}", req.task_id).into())
    }
}

/// A judge that flags one target task as looping (confident) and passes everything else.
struct KillJudge {
    target: String,
}

#[async_trait]
impl Judge for KillJudge {
    async fn judge(&self, req: JudgeRequest) -> JudgeOutcome {
        if req.task_id == self.target {
            JudgeOutcome {
                verdict: Verdict::Looping,
                confidence: 1.0,
                hint: "STOP looping and WRITE the file now".to_string(),
                proposed_split: None,
            }
        } else {
            JudgeOutcome::ok()
        }
    }
}

#[tokio::test]
async fn judge_kills_and_redispatches_stuck_worker() {
    let runs = Arc::new(Mutex::new(HashMap::new()));
    let hints = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(JudgeTestDispatcher {
        runs: runs.clone(),
        hints: hints.clone(),
        target: "stuck".to_string(),
        delay: Duration::from_millis(20),
        slow_all: false,
    });
    // One ready task on a 2-device pool: it runs on one node, leaving the other idle for the judge.
    let dag = Dag::from_specs(vec![spec("stuck", &[], &["a.py"])]).unwrap();
    let judge = Arc::new(KillJudge {
        target: "stuck".to_string(),
    });
    let cfg = JudgeConfig {
        min_age_secs: 0,
        intervene_confidence: 0.5,
        max_interventions_per_task: 1,
        ..JudgeConfig::default()
    };
    let sched =
        Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3).with_judge(judge, cfg);
    let report = sched.run(dag, disp, String::new()).await.unwrap();

    assert!(
        report.done.contains(&"stuck".to_string()),
        "the killed task is re-dispatched and eventually completes"
    );
    assert!(report.failed.is_empty(), "no task fails");
    assert_eq!(
        runs.lock().unwrap()[&"stuck".to_string()],
        2,
        "stuck task ran twice: killed once by the judge, then completed on re-dispatch"
    );
    let h = hints.lock().unwrap();
    assert!(
        h.iter()
            .any(|(t, hint)| t == "stuck" && hint.contains("WRITE")),
        "the re-dispatch carried the judge's corrective hint"
    );
}

/// With the per-task intervention cap at 0, the judge may flag but must never kill — the worker runs
/// to completion untouched. Guards against a weak judge looping a task forever.
#[tokio::test]
async fn judge_respects_intervention_cap() {
    let runs = Arc::new(Mutex::new(HashMap::new()));
    let hints = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(JudgeTestDispatcher {
        runs: runs.clone(),
        hints: hints.clone(),
        target: "never-killed".to_string(),
        delay: Duration::from_millis(20),
        slow_all: false,
    });
    let dag = Dag::from_specs(vec![spec("never-killed", &[], &["a.py"])]).unwrap();
    let judge = Arc::new(KillJudge {
        target: "never-killed".to_string(),
    });
    let cfg = JudgeConfig {
        min_age_secs: 0,
        intervene_confidence: 0.5,
        max_interventions_per_task: 0,
        ..JudgeConfig::default()
    };
    let sched =
        Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3).with_judge(judge, cfg);
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert!(report.done.contains(&"never-killed".to_string()));
    assert_eq!(
        runs.lock().unwrap()[&"never-killed".to_string()],
        1,
        "intervention cap 0 -> the judge never kills; the task runs exactly once"
    );
    assert!(hints.lock().unwrap().is_empty(), "no re-dispatch hint");
}

/// Under weight-1 with every node busy there is NO idle device for the judge — but the deterministic
/// verdicts need no model, so the judge must still fire. A single-device pool running its one task is
/// fully saturated; the judge should still inspect + kill it (regression guard for judge-dark-saturation).
#[tokio::test]
async fn judge_fires_when_fleet_is_saturated() {
    let runs = Arc::new(Mutex::new(HashMap::new()));
    let hints = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(JudgeTestDispatcher {
        runs: runs.clone(),
        hints: hints.clone(),
        target: "stuck".to_string(),
        delay: Duration::from_millis(20),
        slow_all: false,
    });
    let dag = Dag::from_specs(vec![spec("stuck", &[], &["a.py"])]).unwrap();
    let judge = Arc::new(KillJudge {
        target: "stuck".to_string(),
    });
    let cfg = JudgeConfig {
        min_age_secs: 0,
        intervene_confidence: 0.5,
        max_interventions_per_task: 1,
        ..JudgeConfig::default()
    };
    // ONE device, weight 1: running its single task leaves NO idle device for the judge.
    let sched = Scheduler::new(vec![dev("only", "m-only", 1)], 3).with_judge(judge, cfg);
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert!(report.done.contains(&"stuck".to_string()));
    assert_eq!(
        runs.lock().unwrap()[&"stuck".to_string()],
        2,
        "no idle device, yet the judge still fired + re-dispatched the stuck worker"
    );
}

/// A worker that exhausts its re-dispatch cap and is STILL flagged must be terminal-failed, not left to
/// spin a node to worker_max_turns. The judge's third action: give up cleanly so the run terminates.
/// `terminal_min_secs: 0` lets the final attempt be failed immediately; `slow_all` keeps that attempt
/// alive so the judge acts on it rather than it self-completing.
#[tokio::test]
async fn judge_terminal_fails_worker_stuck_at_cap() {
    let runs = Arc::new(Mutex::new(HashMap::new()));
    let hints = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(JudgeTestDispatcher {
        runs: runs.clone(),
        hints: hints.clone(),
        target: "doomed".to_string(),
        delay: Duration::from_millis(20),
        slow_all: true,
    });
    let dag = Dag::from_specs(vec![spec("doomed", &[], &["a.py"])]).unwrap();
    let judge = Arc::new(KillJudge {
        target: "doomed".to_string(),
    });
    let cfg = JudgeConfig {
        min_age_secs: 0,
        intervene_confidence: 0.5,
        max_interventions_per_task: 1,
        terminal_min_secs: 0,
        ..JudgeConfig::default()
    };
    let sched =
        Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3).with_judge(judge, cfg);
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert!(
        report.failed.contains(&"doomed".to_string()),
        "a still-flagged worker at its cap is terminal-failed, not left to spin"
    );
    assert!(!report.done.contains(&"doomed".to_string()));
    assert_eq!(
        runs.lock().unwrap()[&"doomed".to_string()],
        2,
        "re-dispatched once (kill at cap-0), then terminal-failed on the still-flagged retry"
    );
}

/// Records the ORDER tasks FINISH in, so the test can prove a dependent ran only after ALL split children.
struct SplitTestDispatcher {
    order: Arc<Mutex<Vec<String>>>,
    delay: Duration,
}

#[async_trait]
impl TaskDispatcher for SplitTestDispatcher {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        // `big` runs long so the judge has time to split it (its future is then aborted). `big-b` is a
        // deliberately SLOW child: a correctly re-pointed dependent MUST wait for it, so if the dependent
        // finishes before `big-b` then the re-point/indegree logic dropped a child — caught by the order
        // assertion below (this is what makes the test catch the subtle indegree bug, not just deadlock).
        let mult = match req.task_id.as_str() {
            "big" => 50,
            "big-b" => 8,
            _ => 1,
        };
        tokio::time::sleep(self.delay * mult).await;
        self.order.lock().unwrap().push(req.task_id.clone());
        Ok(format!("out-{}", req.task_id).into())
    }
}

/// A judge that SPLITS the target into two file-partitioned children, passing everything else.
struct SplitJudge {
    target: String,
}

#[async_trait]
impl Judge for SplitJudge {
    async fn judge(&self, req: JudgeRequest) -> JudgeOutcome {
        // Mirror the real cap: only split a task that has never been split (split_count threaded from the
        // scheduler's generation map). A child of this split carries split_count >= 1 and is left alone.
        if req.task_id == self.target && req.split_count == 0 {
            JudgeOutcome::split(vec![
                ChildSpec {
                    id: "big-a".to_string(),
                    files: vec!["a.py".to_string()],
                    depends_on: vec![],
                },
                ChildSpec {
                    id: "big-b".to_string(),
                    files: vec!["b.py".to_string()],
                    depends_on: vec![],
                },
            ])
        } else {
            JudgeOutcome::ok()
        }
    }
}

/// M3 task-splitting: the judge SPLITS a too-big task into file-partitioned children, and the original's
/// dependent must be re-pointed onto ALL children — waiting for the whole split, never running early
/// (indegree dropped a child) and never deadlocking (indegree stuck too high).
#[tokio::test]
async fn judge_splits_task_and_dependent_waits_for_all_children() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(SplitTestDispatcher {
        order: order.clone(),
        delay: Duration::from_millis(20),
    });
    // `big` owns two files; `verify` depends on it. The judge partitions `big` into big-a (a.py) + big-b (b.py).
    let dag = Dag::from_specs(vec![
        spec("big", &[], &["a.py", "b.py"]),
        spec("verify", &["big"], &["v.py"]),
    ])
    .unwrap();
    let judge = Arc::new(SplitJudge {
        target: "big".to_string(),
    });
    let cfg = JudgeConfig {
        min_age_secs: 0,
        intervene_confidence: 0.5,
        max_interventions_per_task: 1,
        ..JudgeConfig::default()
    };
    let sched =
        Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3).with_judge(judge, cfg);
    let report = sched.run(dag, disp, String::new()).await.unwrap();

    assert!(
        report.failed.is_empty(),
        "no task fails: {:?}",
        report.failed
    );
    for child in ["big-a", "big-b", "verify"] {
        assert!(
            report.done.contains(&child.to_string()),
            "{child} completed; done = {:?}",
            report.done
        );
    }
    let order = order.lock().unwrap();
    let ia = order.iter().position(|t| t == "big-a").expect("big-a ran");
    let ib = order.iter().position(|t| t == "big-b").expect("big-b ran");
    let iv = order
        .iter()
        .position(|t| t == "verify")
        .expect("verify ran");
    assert!(
        iv > ia && iv > ib,
        "the dependent finished AFTER both split children — dependents were re-pointed onto the WHOLE \
         split with correct indegree (completion order = {:?})",
        *order
    );
}

/// Records which completed tasks an idle node pre-reviewed (M5).
struct RecordingPreReviewer {
    reviewed: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl PreReviewer for RecordingPreReviewer {
    async fn pre_review(&self, req: PreReviewRequest) -> PreReviewOutput {
        self.reviewed.lock().unwrap().push(req.task_id.clone());
        PreReviewOutput {
            had_findings: false,
            summary: String::new(),
        }
    }
}

/// M5 no-idle: with no in-flight worker to judge, an idle node correctness-pre-reviews a COMPLETED task.
/// `verify` is the slow target so that, while it runs, the other node is free to review the done `core`.
#[tokio::test]
async fn idle_node_pre_reviews_completed_task() {
    let reviewed = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(JudgeTestDispatcher {
        runs: Arc::new(Mutex::new(HashMap::new())),
        hints: Arc::new(Mutex::new(Vec::new())),
        target: "verify".to_string(), // verify runs long -> idle window to review the done `core`
        delay: Duration::from_millis(20),
        slow_all: false,
    });
    let dag = Dag::from_specs(vec![
        spec("core", &[], &["a.py"]),
        spec("verify", &["core"], &["v.py"]),
    ])
    .unwrap();
    let pr = Arc::new(RecordingPreReviewer {
        reviewed: reviewed.clone(),
    });
    let sched =
        Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3).with_pre_reviewer(pr);
    let report = sched.run(dag, disp, String::new()).await.unwrap();

    assert!(
        report.failed.is_empty(),
        "no task fails: {:?}",
        report.failed
    );
    assert!(
        report.done.contains(&"core".to_string()) && report.done.contains(&"verify".to_string()),
        "both tasks complete: {:?}",
        report.done
    );
    assert!(
        reviewed.lock().unwrap().contains(&"core".to_string()),
        "the idle node pre-reviewed the completed task `core`; reviewed = {:?}",
        reviewed.lock().unwrap()
    );
}

/// A judge that only OBSERVES (never intervenes). Used to prove the pre-reviewer runs CONCURRENTLY with a
/// firing judge instead of being starved by the single idle-slot they used to share.
struct ObservingJudge;

#[async_trait]
impl Judge for ObservingJudge {
    async fn judge(&self, _req: JudgeRequest) -> JudgeOutcome {
        JudgeOutcome::ok()
    }
}

/// Idle-jobs concurrency (the lone-idle-node fix): with BOTH a judge and a pre-reviewer attached and >=2
/// free slots, they must run CONCURRENTLY — the judge inspects the in-flight `slow` worker while a SECOND
/// idle node pre-reviews the completed `done` task. Under the OLD single `judge_running` slot the judge
/// starved the pre-review (gate was `if s.judge_running`), so `done` was never reviewed; the fix bounds idle
/// jobs by `idle_capacity()` instead, so both run. 3 devices: `slow` occupies one, leaving capacity 2.
#[tokio::test]
async fn pre_review_runs_concurrently_with_judge() {
    let reviewed = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(JudgeTestDispatcher {
        runs: Arc::new(Mutex::new(HashMap::new())),
        hints: Arc::new(Mutex::new(Vec::new())),
        target: "slow".to_string(), // slow stays in-flight -> the judge has a worker to inspect
        delay: Duration::from_millis(60),
        slow_all: false,
    });
    let dag = Dag::from_specs(vec![
        spec("done", &[], &["d.py"]), // completes fast -> a completed-unreviewed task to pre-review
        spec("slow", &[], &["s.py"]), // runs long -> the judge's in-flight target
    ])
    .unwrap();
    let pr = Arc::new(RecordingPreReviewer {
        reviewed: reviewed.clone(),
    });
    let cfg = JudgeConfig {
        min_age_secs: 0,
        ..JudgeConfig::default()
    };
    let sched = Scheduler::new(
        vec![dev("a", "m-a", 1), dev("b", "m-b", 1), dev("c", "m-c", 1)],
        3,
    )
    .with_judge(Arc::new(ObservingJudge), cfg)
    .with_pre_reviewer(pr);
    let report = sched.run(dag, disp, String::new()).await.unwrap();

    assert!(
        report.failed.is_empty(),
        "no task fails: {:?}",
        report.failed
    );
    assert!(
        reviewed.lock().unwrap().contains(&"done".to_string()),
        "pre-review ran on `done` CONCURRENTLY with the judge inspecting `slow` (lone-idle fix); \
         reviewed = {:?}",
        reviewed.lock().unwrap()
    );
}

/// A judge that proposes a MALFORMED split — a sibling cycle (big-a<->big-b). apply_split must reject it.
struct CyclicSplitJudge {
    target: String,
}

#[async_trait]
impl Judge for CyclicSplitJudge {
    async fn judge(&self, req: JudgeRequest) -> JudgeOutcome {
        if req.task_id == self.target && req.split_count == 0 {
            JudgeOutcome::split(vec![
                ChildSpec {
                    id: "big-a".to_string(),
                    files: vec!["a.py".to_string()],
                    depends_on: vec!["big-b".to_string()],
                },
                ChildSpec {
                    id: "big-b".to_string(),
                    files: vec!["b.py".to_string()],
                    depends_on: vec!["big-a".to_string()],
                },
            ])
        } else {
            JudgeOutcome::ok()
        }
    }
}

/// M3 robustness (audit fix A): a malformed split proposal (sibling cycle) must be a TRUE NO-OP — the
/// worker keeps running and completes, the task is NOT failed, and NO children are injected. Guards the
/// contract that a bad judge proposal can never corrupt the DAG (the cycle is rejected BEFORE the abort).
#[tokio::test]
async fn cyclic_split_proposal_is_a_noop() {
    let runs = Arc::new(Mutex::new(HashMap::new()));
    let disp = Arc::new(JudgeTestDispatcher {
        runs: runs.clone(),
        hints: Arc::new(Mutex::new(Vec::new())),
        target: "big".to_string(), // slow target so the judge fires on it repeatedly
        delay: Duration::from_millis(20),
        slow_all: false,
    });
    let dag = Dag::from_specs(vec![
        spec("big", &[], &["a.py", "b.py"]),
        spec("verify", &["big"], &["v.py"]),
    ])
    .unwrap();
    let judge = Arc::new(CyclicSplitJudge {
        target: "big".to_string(),
    });
    let cfg = JudgeConfig {
        min_age_secs: 0,
        intervene_confidence: 0.5,
        max_interventions_per_task: 1,
        ..JudgeConfig::default()
    };
    let sched =
        Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3).with_judge(judge, cfg);
    let report = sched.run(dag, disp, String::new()).await.unwrap();

    assert!(
        report.failed.is_empty(),
        "a malformed (cyclic) split must NOT fail the task: failed={:?}",
        report.failed
    );
    assert!(
        report.done.contains(&"big".to_string()) && report.done.contains(&"verify".to_string()),
        "the worker keeps running and the run completes normally: done={:?}",
        report.done
    );
    assert!(
        !report.done.contains(&"big-a".to_string()) && !report.done.contains(&"big-b".to_string()),
        "NO children are injected from a rejected proposal: done={:?}",
        report.done
    );
}

/// GOOSE_SWARM_DONE_GATE scoping: a `ContentRetry` (the pre-done syntax gate) must thread its error into
/// the retry's `prior_hint` so the fix is GUIDED; an infra `Transient` (model unloaded) must NOT — a stale
/// content note on an infra retry would mislead the worker. Guards the scheduler.rs combined-arm change.
#[tokio::test]
async fn content_retry_threads_hint_infra_transient_does_not() {
    struct Seen {
        task: String,
        attempt: u32,
        hint: Option<String>,
    }
    struct HintProbe {
        fail0: HashMap<String, DispatchError>,
        seen: Arc<Mutex<Vec<Seen>>>,
    }
    #[async_trait]
    impl TaskDispatcher for HintProbe {
        async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
            self.seen.lock().unwrap().push(Seen {
                task: req.task_id.clone(),
                attempt: req.attempt,
                hint: req.prior_hint.clone(),
            });
            if req.attempt == 0 {
                if let Some(e) = self.fail0.get(&req.task_id) {
                    return Err(e.clone());
                }
            }
            Ok(format!("ok-{}", req.task_id).into())
        }
    }
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut fail0 = HashMap::new();
    fail0.insert(
        "content".to_string(),
        DispatchError::ContentRetry("syntax error in a.py: bad token — FIX it".into()),
    );
    fail0.insert(
        "infra".to_string(),
        DispatchError::Transient("Model is unloaded".into()),
    );
    let disp = Arc::new(HintProbe {
        fail0,
        seen: seen.clone(),
    });
    let dag = Dag::from_specs(vec![
        spec("content", &[], &["a.py"]),
        spec("infra", &[], &["b.py"]),
    ])
    .unwrap();
    let report = Scheduler::new(vec![dev("d0", "m0", 1), dev("d1", "m1", 1)], 3)
        .run(dag, disp, String::new())
        .await
        .unwrap();
    assert_eq!(
        report.done.len(),
        2,
        "both tasks eventually succeed: {report:?}"
    );

    let seen = seen.lock().unwrap();
    let content_retry = seen
        .iter()
        .find(|s| s.task == "content" && s.attempt == 1)
        .expect("content task must be retried (attempt 1)");
    assert_eq!(
        content_retry.hint.as_deref(),
        Some("syntax error in a.py: bad token — FIX it"),
        "ContentRetry must thread its error into the retry's prior_hint"
    );
    let infra_retry = seen
        .iter()
        .find(|s| s.task == "infra" && s.attempt == 1)
        .expect("infra task must be retried (attempt 1)");
    assert_eq!(
        infra_retry.hint, None,
        "an infra Transient must NOT thread a content hint"
    );
}
