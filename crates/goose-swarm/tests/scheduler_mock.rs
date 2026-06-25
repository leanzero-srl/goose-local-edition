//! M1.0 de-risking gate: the scheduler concurrency core, tested against a MockDispatcher with no
//! model involved. Asserts the five invariants: no double-claim, dependency gating, per-device
//! weighting, transient re-dispatch (to a different device), and file-overlap serialization.

use async_trait::async_trait;
use goose_swarm::{
    Dag, DeviceCfg, Difficulty, DispatchError, DispatchRequest, ReplanContext, Replanner, Scheduler,
    TaskDispatcher, TaskRunOutput, TaskSpec,
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
        *self.total_per_device.entry(req.device_id.clone()).or_default() += 1;
        let c = self.cur_per_device.entry(req.device_id.clone()).or_default();
        *c += 1;
        let cur = *c;
        let p = self.peak_per_device.entry(req.device_id.clone()).or_default();
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
        assert_eq!(r.runs[&format!("t{i}")], 1, "task t{i} dispatched exactly once (no double-claim)");
    }
}

#[tokio::test]
async fn dependent_waits_for_dependency() {
    let specs = vec![spec("a", &[], &[]), spec("b", &["a"], &[]), spec("c", &["b"], &[])];
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let sched = Scheduler::new(vec![dev("d1", "m-1", 3), dev("d2", "m-2", 3)], 3);
    let report = sched.run(dag, mock(&rec, 20), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 3);
    let r = rec.lock().unwrap();
    assert!(r.first_start_seq["b"] > r.end_seq["a"], "b started before a finished");
    assert!(r.first_start_seq["c"] > r.end_seq["b"], "c started before b finished");
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
    assert!(r.peak_per_device.get("small").copied().unwrap_or(0) <= 1, "small never exceeds weight 1");
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
    assert_eq!(r.runs["x"], 2, "x ran twice: one transient failure + one success");
    let devs = &r.run_devices["x"];
    assert_ne!(devs[0], devs[1], "re-dispatch steered to a different device");
}

#[tokio::test]
async fn spreads_independent_tasks_across_idle_devices() {
    // 9 independent tasks, three weight-1 devices, no preferred model: spread routing must use ALL
    // three devices (the first pass claims one task per idle device), not pile onto the first.
    let specs: Vec<_> = (0..9).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let sched = Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1), dev("c", "m-c", 1)], 3);
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
    assert_eq!(active, 3, "all three devices must run concurrently, not just one");
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
    assert_eq!(report.done, vec!["d".to_string()], "independent task still completes");
    let failed: HashSet<_> = report.failed.iter().cloned().collect();
    assert_eq!(failed, HashSet::from(["a".to_string(), "b".to_string(), "c".to_string()]));
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

fn slow_dispatcher(rec: &Arc<Mutex<Recorder>>, delay_ms: u64, slow: &[&str]) -> Arc<MockDispatcher> {
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
        rounds: Mutex::new(VecDeque::from(vec![vec![spec("b", &[], &[]), spec("c", &[], &[])]])),
        calls: calls.clone(),
    });
    let sched = Scheduler::new(vec![dev("d0", "m0", 1), dev("d1", "m1", 1), dev("d2", "m2", 1)], 3)
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
    assert!(calls.load(Ordering::SeqCst) >= 1, "the replanner was invoked while nodes idled");
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
    let sched = Scheduler::new(vec![dev("d0", "m0", 1), dev("d1", "m1", 1), dev("d2", "m2", 1)], 3)
        .with_replanner(replanner, 3);
    let report = sched
        .run(dag, slow_dispatcher(&rec, 30, &["slow"]), "g".into())
        .await
        .unwrap();
    assert_eq!(report.done.len(), 2, "an empty replan adds nothing and ends cleanly");
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
    let sched = Scheduler::new(vec![dev("d0", "m0", 1), dev("d1", "m1", 1), dev("d2", "m2", 1)], 3)
        .with_replanner(replanner, 2);
    let report = sched
        .run(dag, slow_dispatcher(&rec, 25, &["slow", "r1", "r2", "r3"]), "g".into())
        .await
        .unwrap();
    assert!(calls.load(Ordering::SeqCst) <= 2, "replan rounds must not exceed max_replans");
    assert!(
        !report.done.contains(&"r3".to_string()),
        "r3 must never be requested once the cap is hit"
    );
}

#[tokio::test]
async fn cycle_is_rejected_at_load() {
    let specs = vec![spec("a", &["b"], &[]), spec("b", &["a"], &[])];
    assert!(Dag::from_specs(specs).is_err(), "a dependency cycle must be rejected");
}
