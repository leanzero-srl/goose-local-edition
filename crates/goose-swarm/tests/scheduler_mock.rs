//! M1.0 de-risking gate: the scheduler concurrency core, tested against a MockDispatcher with no
//! model involved. Asserts the five invariants: no double-claim, dependency gating, per-device
//! weighting, transient re-dispatch (to a different device), and file-overlap serialization.

use async_trait::async_trait;
use goose_swarm::{
    ChildSpec, Dag, DeviceCfg, Difficulty, DispatchError, DispatchRequest, EventSink, Judge,
    JudgeConfig, JudgeOutcome, JudgeRequest, PreReviewOutput, PreReviewRequest, PreReviewer,
    ReplanAuthorityFact, ReplanAuthorityReceipt, ReplanContext, Replanner, Scheduler, SwarmEvent,
    TaskDispatcher, TaskRunOutput, TaskSpec, Verdict,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
        subsplit: Vec::new(),
        replan_authority: None,
    }
}

fn review_spec(id: &str, source_task_id: &str, source_file: &str) -> TaskSpec {
    let task_id = format!("replan-review::{id}");
    let requirement_id = format!("REQ-{id}");
    let requirement = format!("The completed `{source_file}` must satisfy acceptance slice {id}.");
    let evidence_id = format!("EVID-{id}");
    let evidence = format!("Review evidence for {source_file}.");
    let interface_id = format!("IFACE-{id}");
    let interface = format!("{source_file} preserves interface {id}.");
    let objective = format!("Review acceptance slice {id} without modifying files");
    let acceptance = format!("Record file evidence for {requirement_id}");
    let description = format!(
        "RUNTIME ACCEPTANCE REVIEW\nObjective: {objective}\nRead-only source files:\n- {source_file}\n\
         REQUIREMENT [{requirement_id}] SOURCE: {requirement}\nEVIDENCE [{evidence_id}] SOURCE: {evidence}\n\
         INTERFACE [{interface_id}] CONTRACT: {interface}\nRequired acceptance evidence:\n- {acceptance}"
    );
    TaskSpec {
        id: task_id,
        description,
        difficulty: Difficulty::Hard,
        preferred_model: None,
        owned_files: Vec::new(),
        deps: vec![source_task_id.to_string()],
        subsplit: Vec::new(),
        replan_authority: Some(ReplanAuthorityReceipt {
            source_task_id: source_task_id.to_string(),
            binding_task_id: source_task_id.to_string(),
            slice_id: id.to_string(),
            source_files: vec![source_file.to_string()],
            objective,
            requirements: vec![ReplanAuthorityFact {
                id: requirement_id,
                text: requirement,
            }],
            evidence: vec![ReplanAuthorityFact {
                id: evidence_id,
                text: evidence,
            }],
            interfaces: vec![ReplanAuthorityFact {
                id: interface_id,
                text: interface,
            }],
            acceptance_evidence: vec![acceptance],
        }),
    }
}

#[derive(Default)]
struct RecordingEventSink {
    events: Mutex<Vec<serde_json::Value>>,
}

impl EventSink for RecordingEventSink {
    fn emit(&self, event: &SwarmEvent) {
        self.events
            .lock()
            .unwrap()
            .push(serde_json::to_value(event).unwrap());
    }

    fn write_value(&self, value: serde_json::Value) {
        self.events.lock().unwrap().push(value);
    }
}

fn dev(id: &str, model: &str, weight: u32) -> DeviceCfg {
    dev_sw(id, model, weight, 1)
}

fn dev_sw(id: &str, model: &str, weight: u32, speed_weight: u32) -> DeviceCfg {
    DeviceCfg {
        id: id.to_string(),
        model_id: model.to_string(),
        weight,
        enabled: true,
        speed_weight,
        supervision: false,
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
async fn the_configured_speed_weight_decides_every_free_slot_tie() {
    // Operator directive (2026-08-20): the highest-weight host is the TOP unit — at any
    // placement choice between free devices, the weight-4 host wins, and it wins FIRST
    // (task ordering), not just eventually. Pinned after the weight chain was found
    // silently inert on model-id lookups: an unproven weight system reads as "flat for
    // some stupid reason".
    let specs = vec![spec("t0", &[], &[])];
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    // Deliberately list the SLOW device first: index order must not decide.
    let sched = Scheduler::new(
        vec![
            dev_sw("slowhost", "m-s", 2, 1),
            dev_sw("fasthost", "m-f", 2, 4),
        ],
        3,
    );
    let report = sched.run(dag, mock(&rec, 20), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 1);
    let r = rec.lock().unwrap();
    assert_eq!(
        r.run_devices["t0"],
        vec!["fasthost".to_string()],
        "a single task with the whole fleet free must land on the highest speed_weight host"
    );
}

#[tokio::test]
async fn a_retry_waits_for_a_different_device_instead_of_rebinding() {
    // r8 web-viz starvation loop, pinned: "victim" fails transient on attempt 0; at that
    // instant the ONLY free slot is the device that just killed it ("blocker" still occupies
    // the other). The retry must WAIT and land on a DIFFERENT device when the blocker frees —
    // rebinding to the killer re-runs the same starvation (measured: four 420s stalls, all on
    // the same slowest node).
    let specs = vec![spec("blocker", &[], &[]), spec("victim", &[], &[])];
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let disp = Arc::new(MockDispatcher {
        rec: rec.clone(),
        delay: Duration::from_millis(30),
        fail_transient_first: ["victim".to_string()].into_iter().collect(),
        terminal: HashSet::new(),
        slow: ["blocker".to_string()].into_iter().collect(),
    });
    let sched = Scheduler::new(vec![dev("other", "m-o", 1), dev("slowhost", "m-s", 1)], 3);
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert_eq!(
        report.done.len(),
        2,
        "both tasks finish — waiting must not deadlock"
    );
    let r = rec.lock().unwrap();
    let devs = &r.run_devices["victim"];
    assert_eq!(devs.len(), 2, "one failed attempt + one retry: {devs:?}");
    assert_ne!(
        devs[0], devs[1],
        "the retry must land on a DIFFERENT device, not rebind to the one that killed it"
    );
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
async fn speed_weight_wins_every_equal_load_tie_without_stacking() {
    // Operator directive: the highest-speed-weight host is the unit that gets the MOST tasks.
    // Placement used to break equal-load ties by INDEX, which on the real fleet always chose the
    // slowest host. One independent task against a fully idle fleet must land on the fastest
    // device even though it sorts LAST by index; with two tasks, load-primary must still spread
    // the second to another device rather than stacking the fastest.
    let mut fast = dev("z-fast", "m-z", 2);
    fast.speed_weight = 3;
    let mut mid = dev("b-mid", "m-b", 2);
    mid.speed_weight = 2;
    let slow = dev("a-slow", "m-a", 2); // speed_weight 1, sorts FIRST by index

    let dag = Dag::from_specs(vec![spec("only", &[], &[])]).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let sched = Scheduler::new(vec![slow.clone(), mid.clone(), fast.clone()], 3);
    let report = sched.run(dag, mock(&rec, 20), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 1);
    assert_eq!(
        rec.lock()
            .unwrap()
            .total_per_device
            .get("z-fast")
            .copied()
            .unwrap_or(0),
        1,
        "an idle-fleet tie must go to the highest speed_weight, not the first index"
    );

    let dag2 = Dag::from_specs(vec![spec("x1", &[], &[]), spec("x2", &[], &[])]).unwrap();
    let rec2 = Arc::new(Mutex::new(Recorder::default()));
    let sched2 = Scheduler::new(vec![slow, mid, fast], 3);
    let report2 = sched2
        .run(dag2, mock(&rec2, 40), String::new())
        .await
        .unwrap();
    assert_eq!(report2.done.len(), 2);
    let r2 = rec2.lock().unwrap();
    assert!(
        r2.total_per_device.get("z-fast").copied().unwrap_or(0) >= 1,
        "the fastest device gets the first of two tasks"
    );
    assert!(
        r2.peak_per_device.get("z-fast").copied().unwrap_or(0) <= 1,
        "load stays primary: the second task spreads instead of stacking the fastest device"
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
    // a (terminal fail) -> b -> c (both FILE-LESS: verification-shaped, so they RELAX THROUGH
    // the failure and still run — the wall-time hunt's rule: a verifier that writes nothing is
    // strictly more informative run-against-the-broken-tree than cascaded Failed);
    // a -> w which OWNS a file and must still fail exactly as before;
    // plus independent d which must still complete. No deadlock either way.
    let specs = vec![
        spec("a", &[], &["a_owned.rs"]),
        spec("b", &["a"], &[]),
        spec("c", &["b"], &[]),
        spec("w", &["a"], &["w_owned.rs"]),
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
    let done: HashSet<_> = report.done.iter().cloned().collect();
    assert_eq!(
        done,
        HashSet::from(["b".to_string(), "c".to_string(), "d".to_string()]),
        "file-less dependents relax through the failure and run; independent task completes"
    );
    let failed: HashSet<_> = report.failed.iter().cloned().collect();
    assert_eq!(
        failed,
        HashSet::from(["a".to_string(), "w".to_string()]),
        "the write-owning dependent still fails with its dependency"
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
    let dag = Dag::from_specs(vec![
        spec("slow", &[], &["slow.py"]),
        spec("fast", &[], &["fast.py"]),
        spec("integrate-verify", &["slow", "fast"], &[]),
    ])
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let replanner = Arc::new(MockReplanner {
        rounds: Mutex::new(VecDeque::from(vec![vec![
            review_spec("b", "fast", "fast.py"),
            review_spec("c", "fast", "fast.py"),
        ]])),
        calls: calls.clone(),
    });
    let event_sink = Arc::new(RecordingEventSink::default());
    let sched = Scheduler::new(
        vec![dev("d0", "m0", 1), dev("d1", "m1", 1), dev("d2", "m2", 1)],
        3,
    )
    .with_sink(event_sink.clone())
    .with_replanner(replanner, 3);
    let report = sched
        .run(dag, slow_dispatcher(&rec, 30, &["slow"]), "goal".into())
        .await
        .unwrap();
    let done: HashSet<_> = report.done.iter().cloned().collect();
    assert!(
        [
            "slow",
            "fast",
            "replan-review::b",
            "replan-review::c",
            "integrate-verify",
        ]
        .iter()
        .all(|t| done.contains(*t)),
        "replan-added tasks must run: done={:?}",
        report.done
    );
    assert!(
        calls.load(Ordering::SeqCst) >= 1,
        "the replanner was invoked while nodes idled"
    );
    let events = event_sink.events.lock().unwrap();
    let replanned = events
        .iter()
        .find(|event| {
            event["event"] == "replanned" && event["stopped"] == serde_json::Value::Bool(false)
        })
        .expect("successful replan event");
    let advertised: HashSet<String> = replanned["dag"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|task| task["id"].as_str().map(str::to_string))
        .collect();
    let scheduled: HashSet<String> = report
        .tasks
        .iter()
        .map(|task| task.task_id.clone())
        .collect();
    assert_eq!(advertised, scheduled, "advertised DAG != scheduled DAG");
    let sink = replanned["dag"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == "integrate-verify")
        .unwrap();
    let sink_deps: HashSet<&str> = sink["depends_on"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(sink_deps.contains("replan-review::b"));
    assert!(sink_deps.contains("replan-review::c"));
    assert!(
        sink["depends_on"][0]
            .as_str()
            .is_some_and(|id| id.starts_with("replan-review::")),
        "runtime evidence was appended behind the sink's capped legacy context"
    );
}

#[tokio::test]
async fn two_idle_nodes_reject_raw_generic_and_path_invalid_bonus_tasks() {
    let mut path_invalid = review_spec("path-invalid", "fast", "fast.py");
    path_invalid.replan_authority.as_mut().unwrap().source_files = vec!["../fast.py".to_string()];
    let cases = vec![
        ("raw-generic", spec("generic-hardening", &["fast"], &[])),
        ("path-invalid", path_invalid),
    ];
    for (case, candidate) in cases {
        let candidate_id = candidate.id.clone();
        let dag = Dag::from_specs(vec![
            spec("slow", &[], &["slow.py"]),
            spec("fast", &[], &["fast.py"]),
            spec("integrate-verify", &["slow", "fast"], &[]),
        ])
        .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let replanner = Arc::new(MockReplanner {
            rounds: Mutex::new(VecDeque::from(vec![vec![candidate]])),
            calls: calls.clone(),
        });
        let rec = Arc::new(Mutex::new(Recorder::default()));
        let report = Scheduler::new(
            vec![dev("d0", "m0", 1), dev("d1", "m1", 1), dev("d2", "m2", 1)],
            3,
        )
        .with_replanner(replanner, 1)
        .run(dag, slow_dispatcher(&rec, 30, &["slow"]), "goal".into())
        .await
        .unwrap();
        assert!(
            calls.load(Ordering::SeqCst) >= 1,
            "{case}: idle_capacity >= 2 never exercised the admission gate"
        );
        assert!(
            !report.done.contains(&candidate_id) && !report.bonus.contains(&candidate_id),
            "{case}: unbound dynamic task entered the scheduled DAG"
        );
    }
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

/// An empty replan answer must not disable the replanner for the rest of the run.
///
/// MEASURED on a live 3-node run: the replan was asked at +50min with 9 of 18 tasks done, correctly
/// declined because half the DAG was still queued, and the engine then set `replans_done =
/// max_replans`. At +68min ONE task was in flight, two nodes sat idle with idle_capacity()==5, and the
/// only mechanism built to fill them had been switched off by that early "no thanks".
///
/// The shape here reproduces it exactly: the first idle window occurs while a dependent task is still
/// blocked (incomplete == 2) and gets an empty answer; the second occurs after the blocker clears
/// (incomplete == 1), which is strictly fewer and so earns a fresh ask. Under the old behaviour the
/// second ask never happened and `late` never ran — with max_replans = 1, one decline was the whole
/// budget.
#[tokio::test]
async fn an_empty_replan_answer_does_not_disable_the_replanner_for_a_smaller_dag() {
    let rec = Arc::new(Mutex::new(Recorder::default()));
    // `x` frees a node early -> the FIRST idle window, while `dep`/`y` are still blocked behind `slow`
    // (incomplete == 3) -> the honest decline. When `slow` lands, `dep` (long) and `y` (short) both
    // dispatch; `y` finishing is the completion edge that produces the SECOND window, now with only
    // `dep` outstanding (incomplete == 1) -> strictly fewer, so the replanner is asked again.
    let dag = Dag::from_specs(vec![
        spec("slow", &[], &["slow.py"]),
        spec("dep", &["slow"], &[]),
        spec("y", &["slow"], &["y.py"]),
        spec("x", &[], &[]),
        spec("integrate-verify", &["dep", "y", "x"], &[]),
    ])
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let replanner = Arc::new(MockReplanner {
        // First answer EMPTY (the honest decline), then real work once the DAG has shrunk.
        rounds: Mutex::new(VecDeque::from(vec![
            vec![],
            vec![review_spec("late", "y", "y.py")],
        ])),
        calls: calls.clone(),
    });
    let sched = Scheduler::new(
        vec![dev("d0", "m0", 1), dev("d1", "m1", 1), dev("d2", "m2", 1)],
        3,
    )
    // Budget of ONE: an empty answer must not consume it, or the second ask is unreachable.
    .with_replanner(replanner, 1);
    let report = sched
        // BOTH are slow: the second idle window only exists while `dep` is still running, and a
        // fast `dep` finishes inside one loop iteration so the window is never observed.
        .run(dag, slow_dispatcher(&rec, 30, &["slow", "dep"]), "g".into())
        .await
        .unwrap();
    let done: std::collections::HashSet<_> = report.done.iter().cloned().collect();
    assert!(
        done.contains("replan-review::late"),
        "an early decline burned the whole replan budget, so the tail never got its ask: done={:?}",
        report.done
    );
    assert!(
        calls.load(Ordering::SeqCst) >= 2,
        "the replanner must be re-asked once strictly fewer tasks remain, got {} call(s)",
        calls.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn replan_respects_max_replans() {
    // The replanner keeps offering NEW slow tasks; the budget must cap the rounds.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let dag = Dag::from_specs(vec![
        spec("slow", &[], &["slow.py"]),
        spec("fast", &[], &["fast.py"]),
        spec("integrate-verify", &["slow", "fast"], &[]),
    ])
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let replanner = Arc::new(MockReplanner {
        rounds: Mutex::new(VecDeque::from(vec![
            vec![review_spec("r1", "fast", "fast.py")],
            vec![review_spec("r2", "fast", "fast.py")],
            vec![review_spec("r3", "fast", "fast.py")],
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
            slow_dispatcher(
                &rec,
                25,
                &[
                    "slow",
                    "replan-review::r1",
                    "replan-review::r2",
                    "replan-review::r3",
                ],
            ),
            "g".into(),
        )
        .await
        .unwrap();
    assert!(
        calls.load(Ordering::SeqCst) <= 2,
        "replan rounds must not exceed max_replans"
    );
    assert!(
        !report.done.contains(&"replan-review::r3".to_string()),
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

/// A worker that lands genuine bytes before remaining in-flight long enough for the deterministic
/// judge to exhaust its intervention budget and exercise finalize-spin salvage.
struct SalvageWritingDispatcher {
    runs: Arc<Mutex<HashMap<String, u32>>>,
    hints: Arc<Mutex<Vec<(String, String)>>>,
    target: String,
    delay: Duration,
}

#[async_trait]
impl TaskDispatcher for SalvageWritingDispatcher {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        *self
            .runs
            .lock()
            .unwrap()
            .entry(req.task_id.clone())
            .or_default() += 1;
        if let Some(hint) = &req.prior_hint {
            self.hints
                .lock()
                .unwrap()
                .push((req.task_id.clone(), hint.clone()));
        }
        if req.task_id == self.target {
            for path in &req.owned_files {
                std::fs::write(path, format!("attempt {} produced bytes\n", req.attempt)).unwrap();
            }
            tokio::time::sleep(self.delay * 50).await;
        } else {
            tokio::time::sleep(self.delay).await;
        }
        Ok(format!("out-{}", req.task_id).into())
    }
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

/// Same as `KillJudge` but its verdict carries DETERMINISTIC provenance — i.e. it stands in for an engine
/// FACT (a compile error, an owned file never written), not for the judge model's opinion. Only this kind of
/// verdict is allowed to terminal-fail a task.
struct DeterministicKillJudge {
    target: String,
}

#[async_trait]
impl Judge for DeterministicKillJudge {
    async fn judge(&self, req: JudgeRequest) -> JudgeOutcome {
        if req.task_id == self.target {
            JudgeOutcome {
                verdict: Verdict::Looping,
                confidence: 1.0,
                hint: "STOP looping and WRITE the file now".to_string(),
                proposed_split: None,
                deterministic: true,
            }
        } else {
            JudgeOutcome::ok()
        }
    }
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
                // This mock stands in for the MODEL judge (it is a `Judge` impl), so it is NOT deterministic
                // even at confidence 1.0 — which is the whole point of the flag. It can still RE-DISPATCH
                // (what this test asserts); it just can no longer terminal-fail a task.
                deterministic: false,
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

/// Backlog #7 regression: a non-test task that LOOPS to exhaustion is provisionally salvaged after writing
/// its owned file. Its dependent still runs, but the unresolved FullRepairRuler obligation blocks green.
#[tokio::test]
async fn salvaged_looping_task_relaxes_dependents() {
    let fixture = tempfile::Builder::new()
        .prefix("goose-wf7-salvage-")
        .tempdir_in(".")
        .unwrap();
    let owned = fixture.path().join("main.rs");
    std::fs::write(&owned, "pre-existing bytes\n").unwrap();
    let root = std::env::current_dir().unwrap().canonicalize().unwrap();
    let owned_str = owned
        .canonicalize()
        .unwrap()
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let runs = Arc::new(Mutex::new(HashMap::new()));
    let hints = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(SalvageWritingDispatcher {
        runs: runs.clone(),
        hints: hints.clone(),
        target: "app".to_string(),
        delay: Duration::from_millis(15),
    });
    // app (the looping, salvageable non-test task) -> verify (the sink that must still run after the salvage).
    let dag = Dag::from_specs(vec![
        spec("app", &[], &[&owned_str]),
        spec("verify", &["app"], &[]),
    ])
    .unwrap();
    let judge = Arc::new(DeterministicKillJudge {
        target: "app".to_string(),
    });
    let cfg = JudgeConfig {
        min_age_secs: 0,
        intervene_confidence: 0.5,
        max_interventions_per_task: 1,
        rejudge_cooldown_secs: 0,
        terminal_min_secs: 0,
        ..JudgeConfig::default()
    };
    let sched =
        Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3).with_judge(judge, cfg);
    let report = sched.run(dag, disp, String::new()).await.unwrap();

    assert!(
        report.salvaged.contains(&"app".to_string()),
        "the task wrote new bytes and is retained as provisional salvage"
    );
    assert!(
        report.done.contains(&"verify".to_string()),
        "backlog #7: the salvage must relax dependents so the verify sink dispatches and completes (not stuck)"
    );
    assert_eq!(
        report.failed,
        vec!["app"],
        "without a FullRepairRuler consumer, the provisional task must block green"
    );
}

/// Counts how many times the judge inspects each task; always passes (never kills).
struct CountJudge {
    counts: Arc<Mutex<HashMap<String, u32>>>,
}

#[async_trait]
impl Judge for CountJudge {
    async fn judge(&self, req: JudgeRequest) -> JudgeOutcome {
        *self
            .counts
            .lock()
            .unwrap()
            .entry(req.task_id.clone())
            .or_default() += 1;
        JudgeOutcome::ok()
    }
}

/// Runs a scenario with ONE long-running task `target` (owning `files`) plus a chain of short filler
/// tasks that wake the scheduler loop repeatedly (the judge inspects only on a wake / its 15s tick), so
/// the judge gets many chances to re-inspect `target` — the single clear longest task, so no selection
/// tie. cooldown=0. Returns how many times the judge inspected `target`. Three devices: target on one, a
/// serialized filler on the second, the third always free for the judge.
async fn count_target_rejudges(target: &str, files: &[&str]) -> u32 {
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let counts = Arc::new(Mutex::new(HashMap::new()));
    let judge = Arc::new(CountJudge {
        counts: counts.clone(),
    });
    let mut specs = vec![spec(target, &[], files)];
    for i in 0..8 {
        let id = format!("f{i}");
        let f = format!("f{i}.py");
        if i == 0 {
            specs.push(spec(&id, &[], &[&f]));
        } else {
            let dep = format!("f{}", i - 1);
            specs.push(spec(&id, &[&dep], &[&f]));
        }
    }
    let dag = Dag::from_specs(specs).unwrap();
    let disp = Arc::new(MockDispatcher {
        rec: rec.clone(),
        delay: Duration::from_millis(80),
        fail_transient_first: HashSet::new(),
        terminal: HashSet::new(),
        slow: HashSet::from([target.to_string()]),
    });
    let cfg = JudgeConfig {
        min_age_secs: 0,
        rejudge_cooldown_secs: 0,
        ..JudgeConfig::default()
    };
    let sched = Scheduler::new(
        vec![dev("a", "m-a", 1), dev("b", "m-b", 1), dev("c", "m-c", 1)],
        3,
    )
    .with_judge(judge, cfg);
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert!(report.failed.is_empty(), "no task fails");
    let n = counts.lock().unwrap().get(target).copied().unwrap_or(0);
    n
}

/// The scoped fix: an owns-NOTHING task (the integrate-verify sink) is judged AT MOST ONCE, even though
/// the scenario churns many wakes with cooldown=0 (a FILE-OWNING target in the same harness would be
/// re-judged on every wake). Every deterministic gate is disarmed for an owns-nothing task and its verdict
/// is always a non-actionable "ok", so re-judging it only steals an idle node from sink-review;
/// worker_timeout stays its hard-stall backstop. The `<= 1` cap is deterministic (the skip stamps
/// last_judged under the lock at first selection), so this never flakes under concurrent test load — yet
/// removing the skip makes the sink exceed 1 in this multi-wake scenario, so the regression is still caught.
#[tokio::test]
async fn judge_skips_rejudging_owns_nothing_sink() {
    let sink = count_target_rejudges("sink", &[]).await;
    assert!(
        sink <= 1,
        "owns-nothing sink judged at most once, got {sink}"
    );
}

/// SPEED-PILLAR INSTRUMENT: a judge-terminated attempt must report the time it really ran.
///
/// Every emit inside `apply_judge_outcome` — accept, kill, salvage, terminal-fail — hard-coded
/// `elapsed_ms: 0` while the elapsed time sat in scope one screen above. `finish()` then sums
/// `per_device.busy_ms += a.elapsed_ms` across the whole attempt history, so a judge kill (per F489 the
/// commonest restart in the engine) contributed ZERO node-seconds to its device, and a task ending in a
/// judge accept reported zero for itself. MEASURED: a task that ran 80.2 minutes across five attempts
/// was recorded as taking no time at all on three of them. `busy_ms` is the engine's own answer to how
/// busy each node was — the question the entire node-scaling goal turns on.
///
/// The elapsed floor comes from a judge that DELIBERATES for 50 ms, not from the worker's own delay.
/// `min_age_secs: 0` lets the judge fire on the dispatch wake, so an attempt is often barely a
/// millisecond old when it is inspected — asserting on the worker's sleep would race the scheduler and
/// flake. The clock starts at dispatch, so a judge that takes 50 ms to answer guarantees at least that
/// much elapsed by the time the verdict is applied, whatever the loop does around it.
#[tokio::test]
async fn a_judge_killed_attempt_reports_the_time_it_really_ran() {
    /// Kills its target like `KillJudge`, but takes measurable wall-clock to say so.
    struct SlowKillJudge {
        target: String,
    }
    #[async_trait]
    impl Judge for SlowKillJudge {
        async fn judge(&self, req: JudgeRequest) -> JudgeOutcome {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if req.task_id == self.target {
                JudgeOutcome {
                    verdict: Verdict::Looping,
                    confidence: 1.0,
                    hint: "STOP looping and WRITE the file now".to_string(),
                    proposed_split: None,
                    deterministic: false,
                }
            } else {
                JudgeOutcome::ok()
            }
        }
    }

    let runs = Arc::new(Mutex::new(HashMap::new()));
    let hints = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(JudgeTestDispatcher {
        runs: runs.clone(),
        hints: hints.clone(),
        target: "stuck".to_string(),
        delay: Duration::from_millis(40),
        slow_all: false,
    });
    let dag = Dag::from_specs(vec![spec("stuck", &[], &["a.py"])]).unwrap();
    let judge = Arc::new(SlowKillJudge {
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

    let stuck = report
        .tasks
        .iter()
        .find(|t| t.task_id == "stuck")
        .expect("the killed task is in the report");
    let killed: Vec<_> = stuck
        .attempt_history
        .iter()
        .filter(|a| a.outcome == "judge_killed")
        .collect();
    assert!(
        !killed.is_empty(),
        "the scenario must actually produce a judge kill, or this asserts nothing; history={:?}",
        stuck.attempt_history
    );
    for a in &killed {
        assert!(
            a.elapsed_ms > 0,
            "a judge-killed attempt ran for real time; reporting 0 ms erases it from per_device.busy_ms"
        );
    }
    let busy: u64 = report.per_device.values().map(|d| d.busy_ms).sum();
    assert!(
        busy > 0,
        "the fleet was busy; per_device.busy_ms must not sum to zero"
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

/// A worker that exhausts its re-dispatch cap and is STILL flagged by a DETERMINISTIC verdict must be
/// terminal-failed, not left to spin a node to worker_max_turns. The judge's third action: give up cleanly
/// so the run terminates. `terminal_min_secs: 0` lets the final attempt be failed immediately; `slow_all`
/// keeps that attempt alive so the judge acts on it rather than it self-completing.
///
/// THIS TEST USED TO USE THE MODEL JUDGE (`KillJudge`) AND ASSERT THE SAME THING. That encoded a rule
/// violation: the judge MODEL produces its own `confidence`, so gating an irreversible terminal-fail on
/// confidence alone let a model OPINION fail a task — and because integrate-verify depends on every
/// verify::<M> under fan-verify, one opinion turned a whole run red (MEASURED: nf-ts-cadence,
/// over_reading -> re_dispatch x2 -> FAILED at confidence 0.90). `terminal` now also requires
/// `outcome.deterministic`, so the protection is kept exactly where it is legitimate — an engine FACT —
/// and the companion test below pins the other half: a model verdict at cap must NOT fail the task.
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
    let judge = Arc::new(DeterministicKillJudge {
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

/// THE OTHER HALF OF THE RULE: a MODEL-authored verdict must NEVER terminal-fail a task, no matter how
/// confident it is. `KillJudge` flags at confidence 1.0 and the cap is 1, so under the old code this task
/// was FAILED. It must now survive: the model keeps its steering power (it re-dispatched once, below) but
/// the kill decision belongs to a deterministic engine event alone.
///
/// The residual cost is real and deliberate: a task only a MODEL can tell is doomed now runs to a
/// DETERMINISTIC backstop (worker_timeout, or the spiral/repeat breakers) instead of being cut at the judge
/// cap. That is the accepted trade — a slower doomed task is recoverable, a wrongly-failed run is not.
#[tokio::test]
async fn a_model_verdict_at_cap_does_not_terminal_fail() {
    let runs = Arc::new(Mutex::new(HashMap::new()));
    let hints = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(JudgeTestDispatcher {
        runs: runs.clone(),
        hints: hints.clone(),
        target: "opinionated".to_string(),
        delay: Duration::from_millis(20),
        slow_all: false,
    });
    let dag = Dag::from_specs(vec![spec("opinionated", &[], &["a.py"])]).unwrap();
    // The MODEL judge — same flag, same 1.0 confidence, but no deterministic provenance.
    let judge = Arc::new(KillJudge {
        target: "opinionated".to_string(),
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
        !report.failed.contains(&"opinionated".to_string()),
        "a MODEL opinion must never terminal-fail a task — only a deterministic engine event may"
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

/// Records PEAK concurrent pre-reviews so the idle_jobs invariant can be asserted.
struct PeakPreReviewer {
    cur: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

#[async_trait]
impl PreReviewer for PeakPreReviewer {
    async fn pre_review(&self, _req: PreReviewRequest) -> PreReviewOutput {
        let n = self.cur.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(n, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(40)).await;
        self.cur.fetch_sub(1, Ordering::SeqCst);
        PreReviewOutput {
            had_findings: false,
            summary: String::new(),
        }
    }
}

/// idle_jobs accounting invariant: concurrent pre-reviews must NEVER exceed idle_capacity(). `slow` holds
/// one of 3 weight-1 nodes (idle_capacity 2 while it runs); four completed tasks are pre-review targets. The
/// double-decrement-on-normal-exit bug undercounts idle_jobs after each review and lets a 3rd concurrent
/// review spawn on the 2-slot fleet; with the IdleSlotGuard as the SOLE releaser the gate caps peak at 2.
#[tokio::test]
async fn pre_review_never_oversubscribes_free_nodes() {
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let dag = Dag::from_specs(vec![
        spec("slow", &[], &["sl.py"]),
        spec("d1", &[], &["d1.py"]),
        spec("d2", &[], &["d2.py"]),
        spec("d3", &[], &["d3.py"]),
        spec("d4", &[], &["d4.py"]),
    ])
    .unwrap();
    let peak = Arc::new(AtomicUsize::new(0));
    let pr = Arc::new(PeakPreReviewer {
        cur: Arc::new(AtomicUsize::new(0)),
        peak: peak.clone(),
    });
    let sched = Scheduler::new(
        vec![dev("a", "m-a", 1), dev("b", "m-b", 1), dev("c", "m-c", 1)],
        3,
    )
    .with_pre_reviewer(pr);
    let report = sched
        .run(dag, slow_dispatcher(&rec, 30, &["slow"]), String::new())
        .await
        .unwrap();
    assert!(
        report.failed.is_empty(),
        "no task fails: {:?}",
        report.failed
    );
    assert!(
        peak.load(Ordering::SeqCst) <= 2,
        "concurrent pre-reviews ({}) must not exceed idle_capacity 2 while `slow` holds one of 3 nodes \
         (an idle_jobs double-decrement would let a 3rd spawn and oversubscribe the fleet)",
        peak.load(Ordering::SeqCst)
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

/// Dispatcher for the SPECULATIVE-EXECUTION tests. The PRIMARY of `slow` runs long; a SPECULATIVE twin
/// (req.speculative) returns FAST so it wins the race — exercising resolve_speculation's twin-win path
/// (abort the primary, accept the twin's output via complete()). Records that a twin was seen + the peak
/// concurrent in-flight per device (to assert 1-task-per-node).
struct SpecDispatcher {
    saw_speculative: Arc<AtomicBool>,
    twin_delay_ms: u64,
    primary_slow_delay_ms: u64,
}

#[async_trait]
impl TaskDispatcher for SpecDispatcher {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        if req.speculative {
            self.saw_speculative.store(true, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(self.twin_delay_ms)).await;
            return Ok(format!("twin-{}", req.task_id).into());
        }
        let d = if req.task_id == "slow" {
            Duration::from_millis(self.primary_slow_delay_ms) // the chokepoint primary
        } else {
            Duration::from_millis(10)
        };
        tokio::time::sleep(d).await;
        Ok(format!("output-of-{}", req.task_id).into())
    }
}

/// SPECULATIVE EXECUTION (flag ON): `slow` is a chokepoint that d1/d2/d3 all depend on — while it runs, 2
/// nodes idle, so a TWIN of `slow` is raced on an idle device and (being fast) WINS. The run must complete
/// cleanly with NO device leak (a leak would hang the loop) and `slow` accepted exactly once.
#[tokio::test]
async fn speculation_twin_wins_chokepoint_and_no_leak() {
    let saw = Arc::new(AtomicBool::new(false));
    let disp = Arc::new(SpecDispatcher {
        saw_speculative: saw.clone(),
        twin_delay_ms: 5,
        primary_slow_delay_ms: 400,
    });
    let dag = Dag::from_specs(vec![
        spec("slow", &[], &["slow.py"]),
        spec("d1", &["slow"], &["d1.py"]),
        spec("d2", &["slow"], &["d2.py"]),
        spec("d3", &["slow"], &["d3.py"]),
    ])
    .unwrap();
    let sched = Scheduler::new(
        vec![dev("a", "m-a", 1), dev("b", "m-b", 1), dev("c", "m-c", 1)],
        3,
    )
    .with_speculation();
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert!(
        report.failed.is_empty(),
        "no task fails (no device leak / deadlock): {:?}",
        report.failed
    );
    assert_eq!(report.done.len(), 4, "all 4 tasks Done exactly once");
    assert!(
        saw.load(Ordering::SeqCst),
        "a speculative twin of the chokepoint was actually spawned on an idle node"
    );
}

/// Flag OFF (no with_speculation): the SAME chokepoint DAG must complete with NO twin ever spawned —
/// byte-identical to the non-speculative scheduler.
#[tokio::test]
async fn speculation_off_spawns_no_twin() {
    let saw = Arc::new(AtomicBool::new(false));
    let disp = Arc::new(SpecDispatcher {
        saw_speculative: saw.clone(),
        twin_delay_ms: 5,
        primary_slow_delay_ms: 400,
    });
    let dag = Dag::from_specs(vec![
        spec("slow", &[], &["slow.py"]),
        spec("d1", &["slow"], &["d1.py"]),
    ])
    .unwrap();
    let sched = Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3); // no with_speculation
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert!(report.failed.is_empty());
    assert_eq!(report.done.len(), 2);
    assert!(
        !saw.load(Ordering::SeqCst),
        "with speculation OFF, no twin is ever spawned (byte-identical path)"
    );
}

/// SPECULATIVE primary-wins-first (the review's untested ordering): the chokepoint PRIMARY finishes BEFORE
/// the (now slow) twin, so the abort-loser hook in complete() aborts the twin + releases its device. The run
/// must complete with NO leak, the primary's output accepted exactly once, and the twin still spawned.
#[tokio::test]
async fn speculation_primary_wins_aborts_twin_no_leak() {
    let saw = Arc::new(AtomicBool::new(false));
    let disp = Arc::new(SpecDispatcher {
        saw_speculative: saw.clone(),
        twin_delay_ms: 400,        // twin is SLOW -> loses
        primary_slow_delay_ms: 50, // primary long enough to spawn the twin, but finishes first
    });
    let dag = Dag::from_specs(vec![
        spec("slow", &[], &["slow.py"]),
        spec("d1", &["slow"], &["d1.py"]),
        spec("d2", &["slow"], &["d2.py"]),
    ])
    .unwrap();
    let sched = Scheduler::new(
        vec![dev("a", "m-a", 1), dev("b", "m-b", 1), dev("c", "m-c", 1)],
        3,
    )
    .with_speculation();
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert!(
        report.failed.is_empty(),
        "no leak/deadlock when the primary wins: {:?}",
        report.failed
    );
    assert_eq!(report.done.len(), 3, "all 3 done exactly once");
    assert!(
        saw.load(Ordering::SeqCst),
        "a twin was spawned (then lost the race + was aborted)"
    );
}

/// Records what EVERY worker was actually handed. This is the test that the user-decisions bug needed and
/// did not have.
struct DecisionSpy {
    seen: Arc<Mutex<Vec<(String, String)>>>,
}

#[async_trait]
impl TaskDispatcher for DecisionSpy {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        self.seen
            .lock()
            .unwrap()
            .push((req.task_id.clone(), req.user_decisions.clone()));
        Ok(format!("out-{}", req.task_id).into())
    }
}

/// THE USER'S ANSWERS MUST REACH EVERY WORKER, VERBATIM.
///
/// Before `DispatchRequest.user_decisions` existed there was NO path from an answer to a worker at all:
/// `research_findings` never leaves the planner, and the amended spec stops at `Scheduler::goal`, whose
/// only readers are the replanner, the judge and the pre-reviewer. The engine nevertheless printed
/// "✓ ... clarifications injected into every worker via research findings + spec". Nothing failed, because
/// nothing checked — the answers survived only as an LLM paraphrase riding pillars/contracts, which is
/// exactly how "use a PIPE separator" came back as a comma.
#[tokio::test]
async fn user_decisions_reach_every_worker_verbatim() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let specs: Vec<_> = (0..6).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let sched = Scheduler::new(vec![dev("a", "m-a", 2), dev("b", "m-b", 2)], 3);

    let decisions = "## USER DECISIONS — BINDING\nUse a PIPE separator, not a comma.";
    let report = sched
        .run_with_decisions(
            dag,
            Arc::new(DecisionSpy { seen: seen.clone() }),
            "the goal".to_string(),
            decisions.to_string(),
        )
        .await
        .unwrap();
    assert_eq!(report.done.len(), 6);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 6, "every task dispatched");
    for (task_id, got) in seen.iter() {
        assert_eq!(
            got, decisions,
            "worker {task_id} must receive the user's decisions VERBATIM — not paraphrased, not dropped"
        );
    }
}

/// A run that never asked must be byte-identical: no decisions => the field is empty => no injected block.
#[tokio::test]
async fn no_ask_means_no_decisions_and_run_is_unchanged() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let specs: Vec<_> = (0..3).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let sched = Scheduler::new(vec![dev("a", "m-a", 2)], 3);
    // `run` is the pre-existing entry point — it must keep behaving exactly as before.
    let report = sched
        .run(
            dag,
            Arc::new(DecisionSpy { seen: seen.clone() }),
            "the goal".to_string(),
        )
        .await
        .unwrap();
    assert_eq!(report.done.len(), 3);
    for (task_id, got) in seen.lock().unwrap().iter() {
        assert!(
            got.is_empty(),
            "worker {task_id} must get an EMPTY decisions field when the run never asked"
        );
    }
}

#[tokio::test]
async fn supervision_device_never_takes_build_work() {
    // F779 i3: a supervision device (the MAX_NODES-excluded machine a capped run borrows for
    // read-only idle work) must be INVISIBLE to build dispatch — every task lands on the build
    // device even while the supervision device sits enabled and idle beside it.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let specs: Vec<_> = (0..8).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let sup = DeviceCfg {
        supervision: true,
        ..dev("s", "m-s", 2)
    };
    let sched = Scheduler::new(vec![dev("a", "m-a", 2), sup], 3);
    let report = sched.run(dag, mock(&rec, 10), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 8, "all tasks done");
    let r = rec.lock().unwrap();
    for (tid, devs) in &r.run_devices {
        for d in devs {
            assert_eq!(
                d, "a",
                "task {tid} landed on the supervision device — build dispatch must never do that"
            );
        }
    }
    assert!(
        !r.total_per_device.contains_key("s") && !r.total_per_device.contains_key("m-s"),
        "the supervision device took build work: {:?}",
        r.total_per_device
    );
}

#[tokio::test]
async fn supervision_only_pool_refuses_to_run() {
    // A pool with no BUILD device cannot build anything — bail loudly, never hang.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let dag = Dag::from_specs(vec![spec("t0", &[], &[])]).unwrap();
    let sup = DeviceCfg {
        supervision: true,
        ..dev("s", "m-s", 2)
    };
    let sched = Scheduler::new(vec![sup], 3);
    let err = sched.run(dag, mock(&rec, 10), String::new()).await;
    assert!(err.is_err(), "supervision-only pool must refuse to run");
}

#[tokio::test]
async fn supervision_devices_append_flagged_and_drop_collisions() {
    // F779 i3: appended entries are forced supervision=true and take no build work; a model_id
    // collision (worker or pushed planner) drops the entry instead of bailing the run.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let specs: Vec<_> = (0..6).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let sched = Scheduler::new(vec![dev("a", "m-a", 2)], 3)
        .with_supervision_devices(vec![dev("s", "m-s", 2), dev("dup", "m-a", 2)]);
    let report = sched.run(dag, mock(&rec, 10), String::new()).await.unwrap();
    assert_eq!(
        report.done.len(),
        6,
        "the run completes with the borrow attached"
    );
    let r = rec.lock().unwrap();
    assert!(
        !r.total_per_device.contains_key("s") && !r.total_per_device.contains_key("dup"),
        "no supervision or collision-dropped device ever built: {:?}",
        r.total_per_device
    );
}
