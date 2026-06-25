//! The deterministic weighted work-queue scheduler.
//!
//! One central dispatch loop owns the [`Dag`]; per-device capacity is `weight` (max concurrent
//! in-flight tasks on that device). Each loop pass claims as many ready tasks as devices have free
//! capacity (work-stealing: a ready task prefers its planner-suggested device but falls back to any
//! free one), spawns their dispatch futures, then waits on a [`Notify`] that completions fire. A
//! task is locked (state `Claimed`, its files held) while in flight, so it is never double-claimed
//! and two tasks owning the same file never run concurrently. Completions relax dependents
//! (unlocking the DAG), merge output into the shared context, and free device capacity.

use crate::context::SharedContext;
use crate::dag::{Dag, TaskId, TaskState};
use crate::dispatch::{DispatchError, DispatchRequest, TaskDispatcher};
use anyhow::{bail, Result};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

/// A pool device = one LM Link model id with a capacity weight.
#[derive(Clone, Debug)]
pub struct DeviceCfg {
    pub id: String,
    pub model_id: String,
    /// Max concurrent in-flight tasks routed to this device.
    pub weight: u32,
    pub enabled: bool,
}

#[derive(Debug)]
pub struct RunReport {
    pub done: Vec<TaskId>,
    pub failed: Vec<TaskId>,
    pub results: HashMap<TaskId, String>,
    pub context_json: serde_json::Value,
    /// Total tasks dispatched per device id (counts re-dispatches) — observability + weighting checks.
    pub dispatched_per_device: HashMap<String, u32>,
}

struct DeviceRt {
    cfg: DeviceCfg,
    in_flight: u32,
}

/// Ready-set ordering: higher fan-out first (unblock the most work), tie-break by id ascending for
/// determinism. `BinaryHeap` is a max-heap, so `Ord` returns Greater for higher priority.
#[derive(Eq, PartialEq)]
struct Ranked {
    fan_out: usize,
    id: TaskId,
}

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        self.fan_out
            .cmp(&other.fan_out)
            .then_with(|| other.id.cmp(&self.id))
    }
}
impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct Assignment {
    task_id: TaskId,
    request: DispatchRequest,
}

struct State {
    dag: Dag,
    ready: BinaryHeap<Ranked>,
    devices: Vec<DeviceRt>,
    held_files: HashSet<String>,
    held_by: HashMap<TaskId, Vec<String>>,
    claimed_device: HashMap<TaskId, usize>,
    dispatched_per_device: HashMap<String, u32>,
    ctx: SharedContext,
    max_attempts: u32,
}

impl State {
    fn all_terminal(&self) -> bool {
        self.dag
            .tasks
            .values()
            .all(|n| matches!(n.state, TaskState::Done | TaskState::Failed))
    }

    fn total_in_flight(&self) -> u32 {
        self.devices.iter().map(|d| d.in_flight).sum()
    }

    fn files_conflict(&self, tid: &str) -> bool {
        self.dag.tasks[tid]
            .spec
            .owned_files
            .iter()
            .any(|f| self.held_files.contains(f))
    }

    /// Choose a device for a ready task: prefer its suggested model, avoiding the device that just
    /// failed it on a transient retry; otherwise the least-loaded enabled device with free capacity.
    fn pick_device(&self, tid: &str) -> Option<usize> {
        let n = &self.dag.tasks[tid];
        let free: Vec<usize> = self
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| d.cfg.enabled && d.in_flight < d.cfg.weight)
            .map(|(i, _)| i)
            .collect();
        if free.is_empty() {
            return None;
        }
        let allowed: Vec<usize> = free
            .iter()
            .copied()
            .filter(|&i| n.avoid_device.as_deref() != Some(self.devices[i].cfg.id.as_str()))
            .collect();
        // If avoiding the failed device leaves nothing, fall back to any free device.
        let pool = if allowed.is_empty() { free } else { allowed };
        if let Some(pm) = &n.spec.preferred_model {
            if let Some(&i) = pool.iter().find(|&&i| &self.devices[i].cfg.model_id == pm) {
                return Some(i);
            }
        }
        pool.into_iter().min_by_key(|&i| self.devices[i].in_flight)
    }

    /// Claim as many ready tasks as can be placed right now (respecting weights + file holds).
    fn pick_assignments(&mut self) -> Vec<Assignment> {
        let mut out = Vec::new();
        let mut ranked: Vec<TaskId> = Vec::new();
        while let Some(r) = self.ready.pop() {
            ranked.push(r.id);
        }
        let mut leftover: Vec<TaskId> = Vec::new();
        for tid in ranked {
            if self.dag.tasks[&tid].state != TaskState::Ready {
                continue; // defensive: stale heap entry
            }
            if self.files_conflict(&tid) {
                leftover.push(tid);
                continue;
            }
            match self.pick_device(&tid) {
                Some(dev) => self.do_claim(tid, dev, &mut out),
                None => leftover.push(tid),
            }
        }
        for tid in leftover {
            let fan_out = self.dag.tasks[&tid].fan_out;
            self.ready.push(Ranked { fan_out, id: tid });
        }
        out
    }

    fn do_claim(&mut self, tid: TaskId, dev: usize, out: &mut Vec<Assignment>) {
        let slice = {
            let deps = self.dag.tasks[&tid].spec.deps.clone();
            self.ctx.slice_for(&deps)
        };
        let (files, description, attempt) = {
            let n = self.dag.tasks.get_mut(&tid).unwrap();
            n.state = TaskState::Claimed;
            (n.spec.owned_files.clone(), n.spec.description.clone(), n.attempts)
        };
        for f in &files {
            self.held_files.insert(f.clone());
        }
        self.held_by.insert(tid.clone(), files);
        self.devices[dev].in_flight += 1;
        self.claimed_device.insert(tid.clone(), dev);
        let device_id = self.devices[dev].cfg.id.clone();
        let model_id = self.devices[dev].cfg.model_id.clone();
        *self.dispatched_per_device.entry(device_id.clone()).or_default() += 1;
        out.push(Assignment {
            task_id: tid.clone(),
            request: DispatchRequest {
                task_id: tid,
                description,
                device_id,
                model_id,
                context_slice: slice,
                attempt,
            },
        });
    }

    fn complete(&mut self, tid: &str, res: Result<String, DispatchError>) {
        let released_dev = self.claimed_device.remove(tid);
        let released_dev_id = released_dev.map(|i| self.devices[i].cfg.id.clone());
        if let Some(dev) = released_dev {
            if self.devices[dev].in_flight > 0 {
                self.devices[dev].in_flight -= 1;
            }
        }
        if let Some(files) = self.held_by.remove(tid) {
            for f in files {
                self.held_files.remove(&f);
            }
        }

        match res {
            Ok(output) => {
                {
                    let n = self.dag.tasks.get_mut(tid).unwrap();
                    n.state = TaskState::Done;
                    n.result = Some(output.clone());
                    n.avoid_device = None;
                }
                self.ctx.merge(tid, output);
                let dependents = self.dag.dependents.get(tid).cloned().unwrap_or_default();
                for d in dependents {
                    let nd = self.dag.tasks.get_mut(&d).unwrap();
                    if nd.indegree_remaining > 0 {
                        nd.indegree_remaining -= 1;
                    }
                    if nd.indegree_remaining == 0 && nd.state == TaskState::Pending {
                        nd.state = TaskState::Ready;
                        let fan_out = nd.fan_out;
                        self.ready.push(Ranked { fan_out, id: d });
                    }
                }
            }
            Err(DispatchError::Transient(_)) => {
                let exhausted = {
                    let n = self.dag.tasks.get_mut(tid).unwrap();
                    n.attempts += 1;
                    n.attempts >= self.max_attempts
                };
                if exhausted {
                    self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Failed;
                    self.fail_descendants(tid);
                } else {
                    let n = self.dag.tasks.get_mut(tid).unwrap();
                    n.avoid_device = released_dev_id;
                    n.state = TaskState::Ready;
                    let fan_out = n.fan_out;
                    self.ready.push(Ranked {
                        fan_out,
                        id: tid.to_string(),
                    });
                }
            }
            Err(DispatchError::Terminal(_)) => {
                self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Failed;
                self.fail_descendants(tid);
            }
        }
    }

    /// A failed task can never produce output, so its (transitive) dependents can never run —
    /// mark them Failed so the run terminates instead of deadlocking on blocked tasks.
    fn fail_descendants(&mut self, tid: &str) {
        let mut q: VecDeque<TaskId> = self
            .dag
            .dependents
            .get(tid)
            .cloned()
            .unwrap_or_default()
            .into();
        while let Some(d) = q.pop_front() {
            let n = self.dag.tasks.get_mut(&d).unwrap();
            if matches!(n.state, TaskState::Done | TaskState::Failed) {
                continue;
            }
            n.state = TaskState::Failed;
            for dd in self.dag.dependents.get(&d).cloned().unwrap_or_default() {
                q.push_back(dd);
            }
        }
    }

    fn build_report(&self) -> RunReport {
        let mut done = Vec::new();
        let mut failed = Vec::new();
        let mut results = HashMap::new();
        for (id, n) in &self.dag.tasks {
            match n.state {
                TaskState::Done => {
                    done.push(id.clone());
                    if let Some(r) = &n.result {
                        results.insert(id.clone(), r.clone());
                    }
                }
                TaskState::Failed => failed.push(id.clone()),
                _ => {}
            }
        }
        done.sort();
        failed.sort();
        RunReport {
            done,
            failed,
            results,
            context_json: self.ctx.to_json(),
            dispatched_per_device: self.dispatched_per_device.clone(),
        }
    }
}

pub struct Scheduler {
    devices: Vec<DeviceCfg>,
    max_attempts: u32,
}

impl Scheduler {
    pub fn new(devices: Vec<DeviceCfg>, max_attempts: u32) -> Self {
        Self {
            devices,
            max_attempts,
        }
    }

    /// Run the whole DAG to completion. Returns when every task is Done or Failed.
    pub async fn run(&self, dag: Dag, dispatcher: Arc<dyn TaskDispatcher>) -> Result<RunReport> {
        if !self.devices.iter().any(|d| d.enabled) {
            bail!("no enabled devices in the pool");
        }
        // model-id uniqueness invariant across enabled devices (LM Link routes by id alone).
        let mut seen = HashSet::new();
        for d in self.devices.iter().filter(|d| d.enabled) {
            if !seen.insert(d.model_id.clone()) {
                bail!("duplicate model_id `{}` across enabled devices — LM Link cannot distinguish them", d.model_id);
            }
            if d.weight == 0 {
                bail!("device `{}` has weight 0 (enabled) — disable it instead", d.id);
            }
        }

        let mut ready = BinaryHeap::new();
        for (id, n) in &dag.tasks {
            if n.state == TaskState::Ready {
                ready.push(Ranked {
                    fan_out: n.fan_out,
                    id: id.clone(),
                });
            }
        }
        let state = Arc::new(Mutex::new(State {
            dag,
            ready,
            devices: self
                .devices
                .iter()
                .cloned()
                .map(|cfg| DeviceRt { cfg, in_flight: 0 })
                .collect(),
            held_files: HashSet::new(),
            held_by: HashMap::new(),
            claimed_device: HashMap::new(),
            dispatched_per_device: HashMap::new(),
            ctx: SharedContext::new(),
            max_attempts: self.max_attempts,
        }));
        let notify = Arc::new(Notify::new());

        loop {
            let assignments = { state.lock().await.pick_assignments() };
            let dispatched_now = !assignments.is_empty();
            for a in assignments {
                let dispatcher = dispatcher.clone();
                let state = state.clone();
                let notify = notify.clone();
                let task_id = a.task_id.clone();
                let request = a.request;
                tokio::spawn(async move {
                    let res = dispatcher.run(request).await;
                    {
                        let mut s = state.lock().await;
                        s.complete(&task_id, res);
                    }
                    notify.notify_one();
                });
            }

            {
                let s = state.lock().await;
                if s.all_terminal() {
                    return Ok(s.build_report());
                }
                if !dispatched_now && s.total_in_flight() == 0 {
                    // Nothing assignable and nothing running, but not all terminal: the remaining
                    // tasks are permanently blocked (deps failed, or a file deadlock).
                    bail!(
                        "scheduler stuck: {} task(s) cannot proceed (blocked by failed deps or file holds)",
                        s.dag
                            .tasks
                            .values()
                            .filter(|n| !matches!(n.state, TaskState::Done | TaskState::Failed))
                            .count()
                    );
                }
            }
            // A completion (or nothing yet) — wake and re-evaluate. tokio::Notify stores one permit,
            // so a completion that fires before this await is not lost.
            notify.notified().await;
        }
    }
}
