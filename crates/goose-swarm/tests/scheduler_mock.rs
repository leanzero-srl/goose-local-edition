//! M1.0 de-risking gate: the scheduler concurrency core, tested against a MockDispatcher with no
//! model involved. Asserts the five invariants: no double-claim, dependency gating, per-device
//! weighting, transient re-dispatch (to a different device), and file-overlap serialization.

use async_trait::async_trait;
use goose_swarm::{
    Dag, DeviceCfg, Difficulty, DispatchError, DispatchRequest, EventSink, Judge, JudgeConfig,
    JudgeOutcome, JudgeRequest, PreReviewOutput, PreReviewRequest, PreReviewer, ReplanContext,
    Replanner, Scheduler, SwarmEvent, TaskDispatcher, TaskRunOutput, TaskSpec, Verdict,
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
        is_cloud: false,
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

/// Records the scheduler's synchronous claim-time events, so ordering asserts see the CLAIM
/// (emitted under the state lock), never the racy start of a spawned dispatcher future.
struct LogSink {
    log: Arc<Mutex<Vec<String>>>,
}

impl EventSink for LogSink {
    fn emit(&self, event: &SwarmEvent) {
        match event {
            SwarmEvent::TaskDispatched {
                task_id, device, ..
            } => self
                .log
                .lock()
                .unwrap()
                .push(format!("dispatch:{task_id}:{device}")),
            SwarmEvent::RetryReusedAvoidedDevice { task_id, device } => self
                .log
                .lock()
                .unwrap()
                .push(format!("reused:{task_id}:{device}")),
            _ => {}
        }
    }
}

#[tokio::test]
async fn a_retry_reuses_the_sole_free_device_instead_of_waiting() {
    // A-3 (r3), superseding the r8 pin that lived here: "victim" fails transient on attempt 0;
    // at that instant the ONLY free slot is the device that just failed it ("blocker" still
    // occupies the other). The retry must dispatch THERE immediately — avoidance is a
    // preference, never a wait — and the reuse must be visible as a
    // `retry_reused_avoided_device` event.
    //
    // r8 pinned the OPPOSITE (wait for a different device): its harm was the 420s-x-attempt
    // stopwatch re-killing the same slow-but-working call on every re-land. II-7 (7803faffd)
    // deleted that stopwatch (see `no_time_wall_survives_on_the_reuse_path`), and r2 measured
    // the wait's own harm: the sink's body-drop retry starved 11 minutes (21:26:35Z ->
    // 21:37:37Z, zero events) waiting for a slot that idle-fill claims never released.
    let specs = vec![spec("blocker", &[], &[]), spec("victim", &[], &[])];
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let log = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(MockDispatcher {
        rec: rec.clone(),
        delay: Duration::from_millis(30),
        fail_transient_first: ["victim".to_string()].into_iter().collect(),
        terminal: HashSet::new(),
        slow: ["blocker".to_string()].into_iter().collect(),
    });
    let sched = Scheduler::new(vec![dev("other", "m-o", 1), dev("slowhost", "m-s", 1)], 3)
        .with_sink(Arc::new(LogSink { log: log.clone() }));
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert_eq!(report.done.len(), 2, "both tasks finish");
    let r = rec.lock().unwrap();
    let devs = &r.run_devices["victim"];
    assert_eq!(devs.len(), 2, "one failed attempt + one retry: {devs:?}");
    assert_eq!(
        devs[0], devs[1],
        "the retry must REUSE the sole free device immediately instead of waiting for the \
         busy rest of the fleet"
    );
    let log = log.lock().unwrap();
    assert!(
        log.iter()
            .any(|e| e == &format!("reused:victim:{}", devs[1])),
        "the reuse is an emitted event, not an inference from device equality; log = {log:?}"
    );
}

#[tokio::test]
async fn a_retry_prefers_a_different_free_device_over_the_avoided_one() {
    // The preference half of A-3: with ANY other device free at retry time, the avoided
    // device is still skipped. "victim" fails transient after "quick" has already freed the
    // other device — the retry must land on that other device, and no reuse event fires.
    let specs = vec![spec("quick", &[], &[]), spec("victim", &[], &[])];
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let log = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(MockDispatcher {
        rec: rec.clone(),
        delay: Duration::from_millis(30),
        fail_transient_first: ["victim".to_string()].into_iter().collect(),
        terminal: HashSet::new(),
        slow: ["victim".to_string()].into_iter().collect(),
    });
    let sched = Scheduler::new(vec![dev("other", "m-o", 1), dev("slowhost", "m-s", 1)], 3)
        .with_sink(Arc::new(LogSink { log: log.clone() }));
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert_eq!(report.done.len(), 2, "both tasks finish");
    let r = rec.lock().unwrap();
    let devs = &r.run_devices["victim"];
    assert_eq!(devs.len(), 2, "one failed attempt + one retry: {devs:?}");
    assert_ne!(
        devs[0], devs[1],
        "with another device free the retry must still avoid the one that failed it"
    );
    let log = log.lock().unwrap();
    assert!(
        !log.iter().any(|e| e.starts_with("reused:")),
        "no reuse event when the avoided device was not the sole free one; log = {log:?}"
    );
}

/// A-3 (r3): no time wall may survive on the reuse path. A sole-free reuse followed by a
/// content failure must not be re-killable by a clock — r8's starvation loop was the
/// 420s-x-attempt stopwatch re-killing every re-land, and reuse is only safe because II-7
/// (7803faffd) deleted that stopwatch and the provider read/total cut. This pins the deletion
/// as code (comment lines that merely record the history are ignored).
#[test]
fn no_time_wall_survives_on_the_reuse_path() {
    let strip_comments = |src: &str| -> String {
        src.lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let judge = strip_comments(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/judge.rs")).unwrap(),
    );
    for banned in [
        "no_output_deadline_secs",
        "is_still_producing",
        "no_file_hint",
    ] {
        assert!(
            !judge.contains(banned),
            "judge.rs regrew `{banned}` — the II-7-deleted clock kill that made r8's \
             same-device re-land fatal; the A-3 reuse path is only safe without it"
        );
    }
    let api = strip_comments(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../goose-providers/src/api_client.rs"
        ))
        .unwrap(),
    );
    for banned in [
        "GOOSE_PROVIDER_READ_TIMEOUT_SECS",
        "GOOSE_PROVIDER_TOTAL_TIMEOUT",
        "DEFAULT_PROVIDER_TIMEOUT_SECS",
        "read_timeout",
    ] {
        assert!(
            !api.contains(banned),
            "api_client.rs regrew `{banned}` — a provider read/total window would discard the \
             reused slot's stream mid-generation (II-7 deleted it; only connect_timeout stays)"
        );
    }
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
        spec("slow", &[], &[]),
        spec("dep", &["slow"], &[]),
        spec("y", &["slow"], &[]),
        spec("x", &[], &[]),
    ])
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let replanner = Arc::new(MockReplanner {
        // First answer EMPTY (the honest decline), then real work once the DAG has shrunk.
        rounds: Mutex::new(VecDeque::from(vec![vec![], vec![spec("late", &[], &[])]])),
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
        done.contains("late"),
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
                verdict: Verdict::Restart,
                confidence: 1.0,
                hint: "STOP looping and WRITE the file now".to_string(),
                established: "the CSV has a header row".to_string(),
                next_action: "write the file now".to_string(),
                proposed_split: None,
                // This mock stands in for the MODEL judge, so it is NOT deterministic even at confidence
                // 1.0. That flag used to be what let a verdict terminal-fail a task; nothing can do that
                // from here any more. RESTART is the judge's only remaining action.
                deterministic: false,
            }
        } else {
            JudgeOutcome::ok()
        }
    }
}

/// The judge's ONLY remaining action, and the two things that make it a restart rather than a retry:
/// it stays on the SAME device, and the fresh session is SEEDED with what the last attempt established.
///
/// This replaces `judge_kills_and_redispatches_stuck_worker`, which asserted the opposite on both counts
/// (that the task moved to a different node, and that it carried a bare corrective hint). Moving it was
/// pointless — every node in this fleet runs the same model on a different host — and a bare hint throws
/// away the half of the call that was working, which is the whole thing the four-field contract exists to
/// keep.
#[tokio::test]
async fn judge_restarts_in_place_and_seeds_what_was_established() {
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
        "stuck task ran twice: restarted once by the judge, then completed"
    );
    let h = hints.lock().unwrap();
    let seeded = h
        .iter()
        .find(|(t, _)| t == "stuck")
        .map(|(_, hint)| hint.clone())
        .expect("the restart must carry a seed");
    assert!(
        seeded.contains("the CSV has a header row"),
        "the fresh session is seeded with what the last attempt ESTABLISHED — that is the difference \
         between a restart and a cold retry. got: {seeded:?}"
    );
    assert!(
        seeded.contains("write the file now"),
        "...and with the concrete next action. got: {seeded:?}"
    );
}

/// THE LIVENESS RULE. The judge's restarts are unbounded by any counter, so the only thing standing
/// between it and a task it restarts forever is PROGRESS: a restart is permitted only while the task's
/// owned files moved since the last one. A judge that asks on every look, against a tree that never
/// changes, must get exactly ONE restart and then be withheld — and crucially the task must still
/// COMPLETE rather than be failed, because at the moment the ask is withheld the current attempt is
/// still running and failing it would destroy live work to prevent a loop that withholding already
/// prevents.
///
/// This is the guard the plan's §8.1 liveness argument rests on, and it had no test until here.
#[tokio::test]
async fn a_barren_restart_is_withheld_and_never_fails_the_task() {
    /// Asks for a restart on EVERY look, forever — the pathological judge the rule exists for.
    struct AlwaysRestartJudge;
    #[async_trait]
    impl Judge for AlwaysRestartJudge {
        async fn judge(&self, _req: JudgeRequest) -> JudgeOutcome {
            JudgeOutcome {
                verdict: Verdict::Restart,
                confidence: 1.0,
                hint: "start over".to_string(),
                established: String::new(),
                next_action: "start over".to_string(),
                proposed_split: None,
                deterministic: false,
            }
        }
    }

    let runs = Arc::new(Mutex::new(HashMap::new()));
    let hints = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(JudgeTestDispatcher {
        runs: runs.clone(),
        hints: hints.clone(),
        target: "stuck".to_string(),
        delay: Duration::from_millis(20),
        slow_all: false,
    });
    // The owned file is never written by the mock, so the fingerprint never moves — which is exactly
    // the "produced nothing" case the rule is about.
    let dag = Dag::from_specs(vec![spec("stuck", &[], &["never-written.py"])]).unwrap();
    let cfg = JudgeConfig {
        min_age_secs: 0,
        intervene_confidence: 0.5,
        ..JudgeConfig::default()
    };
    let sched = Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3)
        .with_judge(Arc::new(AlwaysRestartJudge), cfg);
    let report = sched.run(dag, disp, String::new()).await.unwrap();

    assert!(
        report.failed.is_empty(),
        "a withheld restart must never fail the task: {:?}",
        report.failed
    );
    assert!(
        report.done.contains(&"stuck".to_string()),
        "the task still completes on its own"
    );
    let n = runs.lock().unwrap()[&"stuck".to_string()];
    assert_eq!(
        n, 2,
        "exactly one restart is granted; every later ask is withheld because the tree never moved. \
         An unbounded judge with no progress rule would run this task forever. got {n} runs"
    );
}

/// Backlog #7 regression: a non-test task that LOOPS to exhaustion is SALVAGED (marked Done because its owned
/// file was written), and that salvage MUST relax its dependents. Before the fix the salvage set state=Done
/// but never decremented the dependents' indegree, so a downstream sink (the CLI / integrate-verify task)
/// stayed Pending forever and the run ended `scheduler stuck` — a working library shipped with no entry point
/// (observed on expense/tmpl). This asserts the dependent now dispatches and completes.
#[tokio::test]
async fn salvaged_looping_task_relaxes_dependents() {
    // The salvage gate requires the looping task's owned file to exist non-empty on disk; use an absolute
    // path under the temp dir so the check passes regardless of the run cwd.
    let owned = std::env::temp_dir().join("goose_wf7_salvage_owned.rs");
    std::fs::write(&owned, "fn main() {}\n").unwrap();
    let owned_str = owned.to_string_lossy().to_string();

    let runs = Arc::new(Mutex::new(HashMap::new()));
    let hints = Arc::new(Mutex::new(Vec::new()));
    // slow_all: the target loops on EVERY attempt, so after the intervention cap it terminal-fails -> salvage.
    let disp = Arc::new(JudgeTestDispatcher {
        runs: runs.clone(),
        hints: hints.clone(),
        target: "app".to_string(),
        delay: Duration::from_millis(15),
        slow_all: true,
    });
    // app (the looping, salvageable non-test task) -> verify (the sink that must still run after the salvage).
    let dag = Dag::from_specs(vec![
        spec("app", &[], &[&owned_str]),
        spec("verify", &["app"], &[]),
    ])
    .unwrap();
    let judge = Arc::new(KillJudge {
        target: "app".to_string(),
    });
    let cfg = JudgeConfig {
        min_age_secs: 0,
        intervene_confidence: 0.5,
        max_interventions_per_task: 1,
        rejudge_cooldown_secs: 0,
        ..JudgeConfig::default()
    };
    let sched =
        Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3).with_judge(judge, cfg);
    let report = sched.run(dag, disp, String::new()).await.unwrap();

    let _ = std::fs::remove_file(&owned);
    assert!(
        report.done.contains(&"app".to_string()),
        "the looping task is salvaged to Done (its owned file was written)"
    );
    assert!(
        report.done.contains(&"verify".to_string()),
        "backlog #7: the salvage must relax dependents so the verify sink dispatches and completes (not stuck)"
    );
    assert!(report.failed.is_empty(), "no task fails");
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

/// SPEED-PILLAR INSTRUMENT: a judge-RESTARTED attempt must report the time it really ran.
///
/// This matters MORE now, not less: node-busy fraction is the falsifier for the whole
/// semantic-plan change, and it is computed from these numbers. If a restarted attempt reports
/// 0 ms, the fleet looks idler than it was and the falsifier lies.
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
    /// Restarts its target like `KillJudge`, but takes measurable wall-clock to say so.
    struct SlowKillJudge {
        target: String,
    }
    #[async_trait]
    impl Judge for SlowKillJudge {
        async fn judge(&self, req: JudgeRequest) -> JudgeOutcome {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if req.task_id == self.target {
                JudgeOutcome {
                    verdict: Verdict::Restart,
                    confidence: 1.0,
                    hint: "STOP looping and WRITE the file now".to_string(),
                    established: String::new(),
                    next_action: String::new(),
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
        .expect("the restarted task is in the report");
    let restarted: Vec<_> = stuck
        .attempt_history
        .iter()
        .filter(|a| a.outcome == "judge_restart")
        .collect();
    assert!(
        !restarted.is_empty(),
        "the scenario must actually produce a judge restart, or this asserts nothing; history={:?}",
        stuck.attempt_history
    );
    for a in &restarted {
        assert!(
            a.elapsed_ms > 0,
            "a judge-restarted attempt ran for real time; reporting 0 ms erases it from \
             per_device.busy_ms, which is what node-occupancy is measured from"
        );
    }
    let busy: u64 = report.per_device.values().map(|d| d.busy_ms).sum();
    assert!(
        busy > 0,
        "the fleet was busy; per_device.busy_ms must not sum to zero"
    );
}

/// Under weight-1 with every node busy there is NO idle device for the judge — but the deterministic
/// verdicts need no model, so the judge must still fire. A single-device pool running its one task is
/// fully saturated; the judge must still inspect + act on it (regression guard for judge-dark-saturation).
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

/// Pushes into the same ordered log the LogSink writes, so a review's start can be ordered
/// against claim-time dispatch events.
struct LoggingPreReviewer {
    log: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl PreReviewer for LoggingPreReviewer {
    async fn pre_review(&self, req: PreReviewRequest) -> PreReviewOutput {
        self.log
            .lock()
            .unwrap()
            .push(format!("review_start:{}", req.task_id));
        PreReviewOutput {
            had_findings: false,
            summary: String::new(),
        }
    }
}

/// A-3 (r3) ready-work yield: an idle-fill claim must never outrank a real task's dispatch.
/// `waiter` is READY the whole run but unplaceable (its file is held by the slow `blocker`),
/// while `done1` completes early and becomes a pre-review candidate with 2 free devices. The
/// old rule only yielded the LAST free slot (`idle_capacity() <= 1`), so the review claimed a
/// node here while real work waited — the shape that held nodes through r2's 11-minute retry
/// starvation (one pre_review call alone held a node 7,535s). Now no idle-fill claim happens
/// while ANY task sits in `ready`: every review in the log must start after `waiter` was
/// dispatched. (Claim order is read from the sink, which emits under the state lock; the
/// review's own start is spawned after its claim, so the ordering is lock-guaranteed.)
#[tokio::test]
async fn ready_real_work_outranks_idle_fill_claims() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let rec = Arc::new(Mutex::new(Recorder::default()));
    // `child` gives blocker fan_out 1, so blocker deterministically claims before waiter and
    // waiter is the one left ready-but-unplaceable.
    let dag = Dag::from_specs(vec![
        spec("blocker", &[], &["shared.py"]),
        spec("waiter", &[], &["shared.py"]),
        spec("done1", &[], &["d1.py"]),
        spec("child", &["blocker"], &["c.py"]),
    ])
    .unwrap();
    let pr = Arc::new(LoggingPreReviewer { log: log.clone() });
    let sched = Scheduler::new(
        vec![dev("a", "m-a", 1), dev("b", "m-b", 1), dev("c", "m-c", 1)],
        3,
    )
    .with_pre_reviewer(pr)
    .with_sink(Arc::new(LogSink { log: log.clone() }));
    let report = sched
        .run(dag, slow_dispatcher(&rec, 30, &["blocker"]), String::new())
        .await
        .unwrap();
    assert!(
        report.failed.is_empty(),
        "no task fails: {:?}",
        report.failed
    );
    let log = log.lock().unwrap();
    let waiter_at = log
        .iter()
        .position(|e| e.starts_with("dispatch:waiter:"))
        .expect("waiter was dispatched");
    for (i, e) in log.iter().enumerate() {
        if e.starts_with("review_start:") {
            assert!(
                i > waiter_at,
                "an idle-fill review claimed a node while `waiter` sat READY — idle-fill \
                 outranked a real task's dispatch; log = {log:?}"
            );
        }
    }
}

/// RETRIES END ON PROGRESS, NOT ON A COUNT. This replaces `max_attempts`, which was the last counted
/// limit in the engine.
///
/// It never capped THINKING — judge interventions and transport drops were already excluded from it.
/// What it capped was REAL failures: a missing owned file, code that will not compile, a stall. Something
/// has to end those, but a literal 3 is the wrong something: a task whose next attempt writes materially
/// different code has earned another go, and one that writes the byte-identical broken file has not.
///
/// So: a failed attempt that leaves the owned files exactly as the previous failure did is the end. A
/// task that keeps CHANGING its output keeps going — here it fails four times, twice as many as the old
/// cap allowed, and still lands.
#[tokio::test]
async fn retries_continue_while_the_output_changes_and_stop_when_it_does_not() {
    struct ChangingFailures {
        path: std::path::PathBuf,
        attempts: Arc<Mutex<u32>>,
    }
    #[async_trait]
    impl TaskDispatcher for ChangingFailures {
        async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
            let n = {
                let mut g = self.attempts.lock().unwrap();
                *g += 1;
                *g
            };
            // Every failure writes DIFFERENT bytes into the owned file: the model is producing new
            // (still wrong) code each time, which is exactly the case that deserves another attempt.
            if n <= 4 {
                std::fs::write(&self.path, format!("attempt {n} — still broken\n")).unwrap();
                return Err(DispatchError::ContentRetry(format!(
                    "attempt {n}: does not compile"
                )));
            }
            std::fs::write(&self.path, "finally correct\n").unwrap();
            Ok(format!("output-of-{}", req.task_id).into())
        }
    }

    let path = std::env::temp_dir().join("goose_retry_progress_owned.rs");
    let _ = std::fs::remove_file(&path);
    let attempts = Arc::new(Mutex::new(0u32));
    let disp = Arc::new(ChangingFailures {
        path: path.clone(),
        attempts: attempts.clone(),
    });
    let owned = path.to_string_lossy().to_string();
    let dag = Dag::from_specs(vec![spec("m", &[], &[owned.as_str()])]).unwrap();
    // max_attempts is retired and ignored; pass the old default to prove it no longer binds.
    let sched = Scheduler::new(vec![dev("a", "m-a", 1)], 3);
    let report = sched.run(dag, disp, String::new()).await.unwrap();

    let n = *attempts.lock().unwrap();
    assert!(
        n >= 5,
        "a task whose output KEEPS CHANGING keeps being retried past the old cap of 3; got {n} attempts"
    );
    assert!(
        report.done.contains(&"m".to_string()),
        "and it lands: {:?} / failed {:?}",
        report.done,
        report.failed
    );
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------------------------
// THE SHRANK TERMINATOR (DESIGN-STABILITY-FIRST §9 row 7, MILD). A content failure re-dispatches
// unless the output is byte-identical or the verify finding set failed to shrink across two
// consecutive measured attempts; either way the end is Done(degraded) with dependents RELAXED —
// never Failed + cascade. Transport drops and judge kills are never measured and never counted.
// ---------------------------------------------------------------------------------------------

/// One scripted step per attempt of the target task. Content steps write attempt-UNIQUE bytes into
/// the first `write` owned files, so the byte-identical terminator can never fire and whatever ends
/// the task is the finding-set rule alone.
enum ShrankStep {
    /// Write the first `write` owned files (attempt-unique bytes), then content-fail.
    Content { write: usize },
    /// A mid-stream transport drop: writes nothing, fails with the excluded error shape.
    Drop,
    /// Write every owned file and succeed.
    Ok,
}

struct ShrankScript {
    target: String,
    steps: Vec<ShrankStep>,
    calls: Arc<Mutex<u32>>,
}

#[async_trait]
impl TaskDispatcher for ShrankScript {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        if req.task_id != self.target {
            return Ok(format!("ok-{}", req.task_id).into());
        }
        let n = {
            let mut g = self.calls.lock().unwrap();
            let n = *g;
            *g += 1;
            n as usize
        };
        match self.steps.get(n).unwrap_or(&ShrankStep::Ok) {
            ShrankStep::Content { write } => {
                for f in req.owned_files.iter().take(*write) {
                    std::fs::write(f, format!("attempt {n} body for {f}\n")).unwrap();
                }
                Err(DispatchError::ContentRetry(format!(
                    "attempt {n}: the done gate rejected the deliverable"
                )))
            }
            ShrankStep::Drop => Err(DispatchError::Transient(
                "stream decode error (mid-stream body drop)".into(),
            )),
            ShrankStep::Ok => {
                for f in &req.owned_files {
                    std::fs::write(f, format!("final body for {f}\n")).unwrap();
                }
                Ok(format!("output-of-{}", req.task_id).into())
            }
        }
    }
}

/// A fresh temp dir of `n` owned-file paths. `.js` on purpose: the finding measure needs no python
/// and no subprocess — a missing file is the finding.
fn shrank_files(tag: &str, n: usize) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("goose_shrank_{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    (0..n)
        .map(|i| dir.join(format!("f{i}.js")).to_string_lossy().to_string())
        .collect()
}

fn degraded_hint(events: &EventLog) -> Option<String> {
    events
        .named("judge_verdict")
        .iter()
        .find(|v| v["verdict"] == "degraded_stall")
        .and_then(|v| v["hint"].as_str().map(|s| s.to_string()))
}

/// Finding sets 5→3→3: the first two content failures are progress (5, then a strictly smaller 3 —
/// retry allowed); the third measures 3 again, the set failed to shrink across two consecutive
/// attempts, and that ENDS the task through the existing degrade path: Done(degraded), dependents
/// relaxed, never Failed. Byte-identity cannot be what ended it — every content attempt here writes
/// different bytes.
#[tokio::test]
async fn finding_set_5_3_3_ends_done_degraded_with_dependents_relaxed() {
    let files = shrank_files("533", 5);
    let calls = Arc::new(Mutex::new(0u32));
    let disp = Arc::new(ShrankScript {
        target: "m".into(),
        steps: vec![
            ShrankStep::Content { write: 0 }, // 5 findings: nothing written
            ShrankStep::Content { write: 2 }, // 3 findings: two files landed
            ShrankStep::Content { write: 2 }, // 3 findings again, new bytes — flat
        ],
        calls: calls.clone(),
    });
    let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    let dag = Dag::from_specs(vec![spec("m", &[], &refs), spec("after", &["m"], &[])]).unwrap();
    let events = Arc::new(EventLog::default());
    let report = Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3)
        .with_sink(events.clone())
        .run(dag, disp, String::new())
        .await
        .unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        3,
        "the flat third attempt is the last — no fourth dispatch"
    );
    assert!(
        report.failed.is_empty(),
        "a settled content task must not cascade-fail: {:?}",
        report.failed
    );
    assert!(
        report.done.contains(&"m".to_string()) && report.done.contains(&"after".to_string()),
        "m ends Done(degraded) and its dependent still runs: {:?}",
        report.done
    );
    let hint = degraded_hint(&events).expect("a degraded_stall verdict is emitted");
    assert!(
        hint.contains("failed to shrink") && hint.contains("3 → 3"),
        "the verdict names the terminator and the flat pair: {hint}"
    );
}

/// Byte-identical consecutive CONTENT outputs end the task likewise: Done(degraded) with dependents
/// relaxed — no longer Failed + cascade. The finding set cannot be what ends it here: the owned file
/// exists non-empty from the first attempt, so the measured set is empty, and an empty set is no
/// baseline. What ends it is writing the exact same bytes twice.
#[tokio::test]
async fn byte_identical_content_failures_end_done_degraded() {
    struct SameBytes {
        path: String,
        calls: Arc<Mutex<u32>>,
    }
    #[async_trait]
    impl TaskDispatcher for SameBytes {
        async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
            if req.task_id != "m" {
                return Ok(format!("ok-{}", req.task_id).into());
            }
            *self.calls.lock().unwrap() += 1;
            std::fs::write(&self.path, "the same broken file, every time\n").unwrap();
            Err(DispatchError::ContentRetry(
                "the done gate rejected the deliverable".into(),
            ))
        }
    }
    let files = shrank_files("bytes", 1);
    let calls = Arc::new(Mutex::new(0u32));
    let disp = Arc::new(SameBytes {
        path: files[0].clone(),
        calls: calls.clone(),
    });
    let dag = Dag::from_specs(vec![
        spec("m", &[], &[files[0].as_str()]),
        spec("after", &["m"], &[]),
    ])
    .unwrap();
    let events = Arc::new(EventLog::default());
    let report = Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3)
        .with_sink(events.clone())
        .run(dag, disp, String::new())
        .await
        .unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        2,
        "the second identical output is the last"
    );
    assert!(
        report.failed.is_empty() && report.done.contains(&"after".to_string()),
        "degraded, dependents relaxed: done={:?} failed={:?}",
        report.done,
        report.failed
    );
    let hint = degraded_hint(&events).expect("a degraded_stall verdict is emitted");
    assert!(
        hint.contains("byte-identical"),
        "the verdict names byte-identity as the terminator: {hint}"
    );
}

/// A transport drop between content attempts is NEVER counted: it adds no finding measurement and
/// resets none, so 5 → (drop) → 3 → 3 ends exactly where 5→3→3 does — on the flat pair — after four
/// dispatches, and the drop itself terminates nothing (the real-failure count excludes it even
/// though the tree did not move across it).
#[tokio::test]
async fn transport_drop_between_content_attempts_is_not_counted() {
    let files = shrank_files("drop", 5);
    let calls = Arc::new(Mutex::new(0u32));
    let disp = Arc::new(ShrankScript {
        target: "m".into(),
        steps: vec![
            ShrankStep::Content { write: 0 }, // 5 findings
            ShrankStep::Drop,                 // no measurement, no count
            ShrankStep::Content { write: 2 }, // 3 findings — shrank, retry
            ShrankStep::Content { write: 2 }, // 3 findings — flat, settle
        ],
        calls: calls.clone(),
    });
    let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    let dag = Dag::from_specs(vec![spec("m", &[], &refs)]).unwrap();
    let events = Arc::new(EventLog::default());
    let report = Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3)
        .with_sink(events.clone())
        .run(dag, disp, String::new())
        .await
        .unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        4,
        "the drop re-dispatches uncounted; the flat pair still ends it on the fourth dispatch"
    );
    assert!(
        report.done.contains(&"m".to_string()) && report.failed.is_empty(),
        "settled, not failed: done={:?} failed={:?}",
        report.done,
        report.failed
    );
    let hint = degraded_hint(&events).expect("a degraded_stall verdict is emitted");
    assert!(
        hint.contains("3 → 3"),
        "the flat pair is 3→3 — the drop contributed no measurement: {hint}"
    );
}

/// A SHRINKING set retries INDEFINITELY — the rule is progress, never a count. Seven findings melt
/// one per attempt (7→6→5→4→3→2→1) across seven content failures — more than double the old
/// max_attempts of 3 — and the task still gets its eighth attempt, which lands. Only flatness or
/// byte-identity may end a content retry.
#[tokio::test]
async fn a_shrinking_finding_set_retries_past_any_count() {
    let files = shrank_files("shrink", 7);
    let calls = Arc::new(Mutex::new(0u32));
    let disp = Arc::new(ShrankScript {
        target: "m".into(),
        steps: (0..7).map(|i| ShrankStep::Content { write: i }).collect(),
        calls: calls.clone(),
    });
    let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    let dag = Dag::from_specs(vec![spec("m", &[], &refs)]).unwrap();
    let events = Arc::new(EventLog::default());
    let report = Scheduler::new(vec![dev("a", "m-a", 1), dev("b", "m-b", 1)], 3)
        .with_sink(events.clone())
        .run(dag, disp, String::new())
        .await
        .unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        8,
        "seven shrinking failures each earn a retry; the eighth attempt lands"
    );
    assert!(
        report.done.contains(&"m".to_string()) && report.failed.is_empty(),
        "it lands by FINISHING: done={:?} failed={:?}",
        report.done,
        report.failed
    );
    assert!(
        degraded_hint(&events).is_none(),
        "nothing settled — the task completed on its own"
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
        is_cloud: false,
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
        is_cloud: false,
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

// ---------------------------------------------------------------------------------------------
// MID-RUN DEVICE ADMISSION. A fleet node dropped out of `lms ps` before a run started, the run
// resolved worker_count=2, and the node came back partway through an eight-hour build: it took
// ZERO calls and the log carried ZERO events naming it, because the pool is read once at run
// start and `Scheduler::run` snapshots it into the run state on entry.
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct EventLog(Mutex<Vec<serde_json::Value>>);

impl goose_swarm::EventSink for EventLog {
    fn emit(&self, event: &goose_swarm::SwarmEvent) {
        if let Ok(v) = serde_json::to_value(event) {
            self.0.lock().unwrap().push(v);
        }
    }
}

impl EventLog {
    fn named(&self, name: &str) -> Vec<serde_json::Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v["event"] == name)
            .cloned()
            .collect()
    }
}

/// Offer `cfgs` the moment the run has actually started dispatching — no sleep, no clock: the
/// offer is triggered by the run's own first dispatch, so the device arrives strictly MID-run.
fn offer_after_first_dispatch(
    adm: goose_swarm::DeviceAdmission,
    rec: Arc<Mutex<Recorder>>,
    cfgs: Vec<DeviceCfg>,
) {
    tokio::spawn(async move {
        loop {
            if rec.lock().unwrap().seq > 0 {
                adm.offer(cfgs);
                return;
            }
            tokio::task::yield_now().await;
        }
    });
}

#[tokio::test]
async fn a_device_admitted_mid_run_actually_takes_work() {
    // The measured hole, inverted: the returning node must receive dispatches in the SAME run.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let specs: Vec<_> = (0..14).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let events = Arc::new(EventLog::default());
    let adm = goose_swarm::DeviceAdmission::new();
    offer_after_first_dispatch(adm.clone(), rec.clone(), vec![dev("gabee", "m-gabee", 2)]);
    let sched = Scheduler::new(vec![dev("a", "m-a", 1)], 3)
        .with_admission(adm)
        .with_sink(events.clone());
    let report = sched.run(dag, mock(&rec, 10), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 14, "every task still completes");
    let r = rec.lock().unwrap();
    assert!(
        r.total_per_device.get("gabee").copied().unwrap_or(0) > 0,
        "the mid-run device must actually build: {:?}",
        r.total_per_device
    );
    let admitted = events.named("device_admitted");
    assert_eq!(admitted.len(), 1, "exactly one admission event");
    assert_eq!(admitted[0]["id"], "gabee");
    assert_eq!(
        admitted[0]["build_devices"], 2,
        "the event reports the pool size AFTER admission"
    );
}

#[tokio::test]
async fn a_mid_run_device_is_reported_in_the_run_report() {
    // Invisibility was half the defect: the returning node must appear in the report's per-device
    // breakdown, not just in the dispatch counts a harness would have to infer it from.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let specs: Vec<_> = (0..10).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let adm = goose_swarm::DeviceAdmission::new();
    offer_after_first_dispatch(adm.clone(), rec.clone(), vec![dev("gabee", "m-gabee", 2)]);
    let sched = Scheduler::new(vec![dev("a", "m-a", 1)], 3).with_admission(adm);
    let report = sched.run(dag, mock(&rec, 10), String::new()).await.unwrap();
    assert!(
        report.per_device.contains_key("gabee"),
        "per_device must carry the admitted node: {:?}",
        report.per_device.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_duplicate_offer_is_dropped_and_never_bails_the_run() {
    // LM Link routes by model_id ALONE, so two enabled devices sharing one are indistinguishable
    // to it. `run_with_decisions` bails on that at run start; mid-run it must DROP instead —
    // killing an eight-hour build because a returning node re-announced a model it already has is
    // strictly worse than ignoring the offer. Same for a duplicate device id and a weight of 0.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let specs: Vec<_> = (0..8).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let events = Arc::new(EventLog::default());
    let adm = goose_swarm::DeviceAdmission::new();
    offer_after_first_dispatch(
        adm.clone(),
        rec.clone(),
        vec![
            dev("other-host", "m-a", 2),
            dev("a", "m-other", 2),
            dev("zero", "m-zero", 0),
        ],
    );
    let sched = Scheduler::new(vec![dev("a", "m-a", 2)], 3)
        .with_admission(adm)
        .with_sink(events.clone());
    let report = sched.run(dag, mock(&rec, 10), String::new()).await.unwrap();
    assert_eq!(
        report.done.len(),
        8,
        "the run survives every rejected offer"
    );
    let r = rec.lock().unwrap();
    assert_eq!(
        r.total_per_device.len(),
        1,
        "nothing but the original device built: {:?}",
        r.total_per_device
    );
    let reasons: Vec<String> = events
        .named("device_rejected")
        .iter()
        .map(|v| v["reason"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        reasons,
        vec!["duplicate model_id", "duplicate device id", "weight 0"],
        "every rejection is logged with why"
    );
    assert!(events.named("device_admitted").is_empty());
}

#[tokio::test]
async fn admission_while_work_is_in_flight_keeps_every_device_index_valid() {
    // The append-only contract. `claimed_device`, `spec_device` and `device_speed` key devices by
    // POSITION, and an in-flight task decrements `devices[i].in_flight` by an index captured at
    // claim time. Admitting while a slow task is mid-flight is exactly the window where a reorder
    // or a removal would corrupt that bookkeeping: the slow task must still complete, and the
    // fleet must still be fully claimable afterwards (a leaked in_flight would starve it).
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let mut specs: Vec<_> = (0..10).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    specs.push(spec("slowpoke", &[], &[]));
    let dag = Dag::from_specs(specs).unwrap();
    let disp = Arc::new(MockDispatcher {
        rec: rec.clone(),
        delay: Duration::from_millis(10),
        fail_transient_first: HashSet::new(),
        terminal: HashSet::new(),
        slow: ["slowpoke".to_string()].into_iter().collect(),
    });
    let adm = goose_swarm::DeviceAdmission::new();
    offer_after_first_dispatch(
        adm.clone(),
        rec.clone(),
        vec![dev("gabee", "m-gabee", 2), dev("workhorse", "m-work", 2)],
    );
    let sched = Scheduler::new(vec![dev("a", "m-a", 2)], 3).with_admission(adm);
    let report = sched.run(dag, disp, String::new()).await.unwrap();
    assert_eq!(report.done.len(), 11, "including the in-flight straggler");
    assert!(report.failed.is_empty());
    let r = rec.lock().unwrap();
    assert!(
        r.total_per_device.len() == 3,
        "all three devices took work: {:?}",
        r.total_per_device
    );
    assert!(
        r.peak_per_device.values().all(|p| *p <= 2),
        "no device ever exceeded its weight — in_flight accounting survived the append: {:?}",
        r.peak_per_device
    );
}

#[tokio::test]
async fn without_an_admission_handle_an_offer_reaches_nothing() {
    // The off path. No handle attached -> the queue is never drained, the loop keeps its
    // single-future wake, and a run behaves exactly as it did before admission existed.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let specs: Vec<_> = (0..6).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let events = Arc::new(EventLog::default());
    let orphan = goose_swarm::DeviceAdmission::new();
    orphan.offer(vec![dev("gabee", "m-gabee", 2)]);
    let sched = Scheduler::new(vec![dev("a", "m-a", 2)], 3).with_sink(events.clone());
    let report = sched.run(dag, mock(&rec, 10), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 6);
    let r = rec.lock().unwrap();
    assert!(!r.total_per_device.contains_key("gabee"));
    assert!(events.named("device_admitted").is_empty());
}

#[tokio::test]
async fn demand_fires_only_when_the_fleet_is_saturated_with_queued_work() {
    // The clock-free trigger. A saturated fleet with a ready queue is the ONLY state in which a
    // returning node is worth probing for, and the signal must actually arrive — a rescan loop
    // parked on `wanted()` forever is the same invisibility, one level up.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let specs: Vec<_> = (0..12).map(|i| spec(&format!("t{i}"), &[], &[])).collect();
    let dag = Dag::from_specs(specs).unwrap();
    let adm = goose_swarm::DeviceAdmission::new();
    let probes = Arc::new(AtomicUsize::new(0));
    {
        let adm = adm.clone();
        let probes = probes.clone();
        tokio::spawn(async move {
            loop {
                adm.wanted().await;
                probes.fetch_add(1, Ordering::SeqCst);
            }
        });
    }
    let sched = Scheduler::new(vec![dev("a", "m-a", 1)], 3).with_admission(adm);
    let report = sched.run(dag, mock(&rec, 10), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 12);
    let n = probes.load(Ordering::SeqCst);
    assert!(
        n > 0,
        "a one-slot fleet with 12 ready tasks must raise demand"
    );
    assert!(
        n <= 12,
        "demand is armed by a CLAIM, so it can never outrun the dispatch count: {n} probes for 12 claims"
    );
}

#[tokio::test]
async fn an_unsaturated_fleet_never_asks_for_another_node() {
    // The other half: two tasks on a four-slot fleet is never short of nodes, so nothing should
    // ever be probed. A signal that fires whenever the loop wakes would spawn `lms ps` subprocesses
    // against a fleet that has nothing to give.
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let specs = vec![spec("t0", &[], &[]), spec("t1", &[], &[])];
    let dag = Dag::from_specs(specs).unwrap();
    let adm = goose_swarm::DeviceAdmission::new();
    let probes = Arc::new(AtomicUsize::new(0));
    {
        let adm = adm.clone();
        let probes = probes.clone();
        tokio::spawn(async move {
            loop {
                adm.wanted().await;
                probes.fetch_add(1, Ordering::SeqCst);
            }
        });
    }
    let sched = Scheduler::new(vec![dev("a", "m-a", 2), dev("b", "m-b", 2)], 3).with_admission(adm);
    let report = sched.run(dag, mock(&rec, 10), String::new()).await.unwrap();
    assert_eq!(report.done.len(), 2);
    assert_eq!(
        probes.load(Ordering::SeqCst),
        0,
        "no queued work was ever blocked on a full fleet"
    );
}

// ---------------------------------------------------------------------------------------------
// A CASCADE IS A TERMINAL OUTCOME. `fail_descendants` used to set `TaskState::Failed` and emit
// nothing at all: a task killed by its dependency was never dispatched and never completed, so a
// finished run rendered it as a PENDING row with no reason and every downstream failure count
// silently excluded it.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_cascade_failed_task_reports_its_outcome_once_and_says_which_dependency_died() {
    // A DIAMOND: root -> left, root -> right, both -> join. `join` is reachable by two failed
    // paths and must be reported EXACTLY ONCE — a second report would double-count the failure.
    let specs = vec![
        spec("root", &[], &["root.rs"]),
        spec("left", &["root"], &["left.rs"]),
        spec("right", &["root"], &["right.rs"]),
        spec("join", &["left", "right"], &["join.rs"]),
        spec("solo", &[], &["solo.rs"]),
    ];
    let dag = Dag::from_specs(specs).unwrap();
    let rec = Arc::new(Mutex::new(Recorder::default()));
    let disp = Arc::new(MockDispatcher {
        rec: rec.clone(),
        delay: Duration::from_millis(5),
        fail_transient_first: HashSet::new(),
        terminal: HashSet::from(["root".to_string()]),
        slow: HashSet::new(),
    });
    let events = Arc::new(EventLog::default());
    let report = Scheduler::new(vec![dev("d1", "m-1", 2)], 3)
        .with_sink(events.clone())
        .run(dag, disp, String::new())
        .await
        .unwrap();
    assert_eq!(
        report.failed.iter().cloned().collect::<HashSet<_>>(),
        HashSet::from([
            "root".to_string(),
            "left".to_string(),
            "right".to_string(),
            "join".to_string()
        ]),
        "the whole write-owning cone fails"
    );

    let completions = events.named("task_completed");
    for cascaded in ["left", "right", "join"] {
        let mine: Vec<_> = completions
            .iter()
            .filter(|v| v["task_id"] == cascaded)
            .collect();
        assert_eq!(
            mine.len(),
            1,
            "{cascaded} must report its terminal outcome exactly once: {mine:?}"
        );
        assert_eq!(mine[0]["status"], "failed", "{cascaded}");
        let why = mine[0]["error"].as_str().unwrap_or_default();
        assert!(
            why.starts_with("dependency '") && why.ends_with("' failed"),
            "a failed row must say WHY, naming its DIRECT failed dependency. got: {why:?}"
        );
    }
    let root = completions
        .iter()
        .find(|v| v["task_id"] == "root")
        .expect("the task that actually failed still reports");
    assert_eq!(root["status"], "failed");
    assert_eq!(
        root["error"], "boom",
        "the dispatched failure keeps its own error, not a cascade reason"
    );
    let join_why = completions.iter().find(|v| v["task_id"] == "join").unwrap()["error"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        join_why.contains("'left'") || join_why.contains("'right'"),
        "the reason names the DIRECT dependency, not the BFS root. got: {join_why:?}"
    );
    assert!(
        completions
            .iter()
            .any(|v| v["task_id"] == "solo" && v["status"] == "done"),
        "an independent task is untouched"
    );
}

// ---------------------------------------------------------------------------------------------
// THE TREE WARDEN. The judge inspects ONE in-flight worker's own files, single-flight and
// cooldown-gated, so a dependency that reported done while writing nothing — or only the stub the
// engine pre-created — was invisible to the whole fan building on top of it until integrate-verify.
// ---------------------------------------------------------------------------------------------

/// One dispatch as the probe saw it: which task, which attempt, and what hint it carried.
struct SeenDispatch {
    task: String,
    attempt: u32,
    hint: Option<String>,
}

struct HintRecordingDispatcher {
    seen: Arc<Mutex<Vec<SeenDispatch>>>,
    transient_first: HashSet<String>,
}

#[async_trait]
impl TaskDispatcher for HintRecordingDispatcher {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        self.seen.lock().unwrap().push(SeenDispatch {
            task: req.task_id.clone(),
            attempt: req.attempt,
            hint: req.prior_hint.clone(),
        });
        if req.attempt == 0 && self.transient_first.contains(&req.task_id) {
            return Err(DispatchError::Transient("Model is unloaded".into()));
        }
        Ok(format!("out-{}", req.task_id).into())
    }
}

#[tokio::test]
async fn the_warden_reports_a_hollow_dependency_and_routes_it_to_the_next_dispatch() {
    // `core` is marked done having written nothing. `feature` builds on it and, on its retry, must
    // be TOLD — that is the whole point: a finding nobody routes is a finding nobody acts on.
    let dag = Dag::from_specs(vec![
        spec("core", &[], &["warden_core_never_written.rs"]),
        spec("feature", &["core"], &["warden_feature.rs"]),
    ])
    .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(HintRecordingDispatcher {
        seen: seen.clone(),
        transient_first: HashSet::from(["feature".to_string()]),
    });
    let events = Arc::new(EventLog::default());
    let report = Scheduler::new(vec![dev("d1", "m-1", 1)], 3)
        .with_sink(events.clone())
        .run(dag, disp, String::new())
        .await
        .unwrap();
    assert_eq!(report.done.len(), 2, "the warden never changes an outcome");

    let found = events.named("tree_defect");
    assert_eq!(
        found.len(),
        1,
        "one finding, stated ONCE however many sweeps see it: {found:?}"
    );
    assert_eq!(found[0]["task_id"], "feature");
    assert_eq!(found[0]["dependency"], "core");
    let detail = found[0]["detail"].as_str().unwrap();
    assert!(
        detail.contains("warden_core_never_written.rs"),
        "the finding names the file: {detail}"
    );

    let seen = seen.lock().unwrap();
    let retry = seen
        .iter()
        .find(|d| d.task == "feature" && d.attempt == 1)
        .expect("feature must be re-dispatched");
    let hint = retry.hint.clone().unwrap_or_default();
    assert!(
        hint.contains("warden_core_never_written.rs"),
        "the warden's finding must REACH the worker that has to live with it. got: {hint:?}"
    );
}

#[tokio::test]
async fn the_warden_is_silent_when_the_dependency_actually_delivered() {
    // THE NEGATIVE CONTROL, on the same shape and the same sweep: an empty finding list must mean
    // "the tree is fine", not "the sweep cannot see this dependency". The test above is the
    // positive control that proves it can.
    let dir = std::env::temp_dir().join(format!("goose-warden-ok-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let delivered = dir.join("core_delivered.rs");
    std::fs::write(&delivered, "pub fn go() -> u32 { 1 }\n").unwrap();

    let dag = Dag::from_specs(vec![
        spec("core", &[], &[delivered.to_str().unwrap()]),
        spec("feature", &["core"], &["warden_feature2.rs"]),
    ])
    .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let disp = Arc::new(HintRecordingDispatcher {
        seen: seen.clone(),
        transient_first: HashSet::from(["feature".to_string()]),
    });
    let events = Arc::new(EventLog::default());
    let report = Scheduler::new(vec![dev("d1", "m-1", 1)], 3)
        .with_sink(events.clone())
        .run(dag, disp, String::new())
        .await
        .unwrap();
    assert_eq!(report.done.len(), 2);
    assert!(
        events.named("tree_defect").is_empty(),
        "a healthy tree must produce no finding: {:?}",
        events.named("tree_defect")
    );
    let seen = seen.lock().unwrap();
    let retry = seen
        .iter()
        .find(|d| d.task == "feature" && d.attempt == 1)
        .expect("feature must be re-dispatched");
    assert_eq!(
        retry.hint, None,
        "and no correction is put in the worker's mouth: {:?}",
        retry.hint
    );
    let _ = std::fs::remove_dir_all(&dir);
}
