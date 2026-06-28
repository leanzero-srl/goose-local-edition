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
use crate::dag::{Dag, Difficulty, TaskId, TaskState};
use crate::dispatch::{
    DispatchError, DispatchRequest, TaskDispatcher, TaskRunOutput, ToolCallRecord,
};
use crate::event::{EventSink, NullSink, SwarmEvent};
use crate::judge::{Judge, JudgeConfig, JudgeOutcome, JudgeRequest, PreReviewRequest, PreReviewer};
use crate::replan::{ReplanContext, Replanner};
use anyhow::{bail, Result};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, Notify};

/// A pool device = one LM Link model id with a capacity weight.
#[derive(Clone, Debug)]
pub struct DeviceCfg {
    pub id: String,
    pub model_id: String,
    /// Max concurrent in-flight tasks routed to this device.
    pub weight: u32,
    pub enabled: bool,
    /// Relative throughput (higher = faster host → a LARGER share of the total tasks; the slowest host
    /// gets proportionally fewer). Default 1 = equal. On an identical-model fleet this is the lever for
    /// skewing load toward the quicker machines instead of splitting evenly.
    pub speed_weight: u32,
}

#[derive(Debug, Serialize)]
pub struct RunReport {
    pub done: Vec<TaskId>,
    pub failed: Vec<TaskId>,
    /// Ids of opportunistic/replanner-added (bonus) tasks — their failure must NOT fail the run.
    pub bonus: Vec<TaskId>,
    pub results: HashMap<TaskId, String>,
    pub context_json: serde_json::Value,
    /// Total tasks dispatched per device id (counts re-dispatches) — observability + weighting checks.
    pub dispatched_per_device: HashMap<String, u32>,
    /// Per-task outcome detail for verification (device, model, attempts, timing, session, tool calls).
    pub tasks: Vec<TaskOutcome>,
    /// Aggregates per device for cluster verification.
    pub per_device: HashMap<String, DeviceSummary>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TaskOutcome {
    pub task_id: TaskId,
    /// `done` | `failed` | `incomplete`.
    pub status: String,
    /// Device of the final attempt.
    pub device: Option<String>,
    pub model: Option<String>,
    pub attempts: u32,
    pub attempt_history: Vec<AttemptRecord>,
    /// Wall-clock of the final attempt.
    pub elapsed_ms: Option<u64>,
    pub session_id: Option<String>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub output: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AttemptRecord {
    pub device: Option<String>,
    pub model: Option<String>,
    /// `ok` | `transient` | `terminal`.
    pub outcome: String,
    pub error: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct DeviceSummary {
    pub dispatched: u32,
    pub tool_calls: u32,
    pub mcp_calls: u32,
    pub retries: u32,
    /// Sum of attempt durations on this device — NOT wall-clock (tasks overlap under concurrency).
    pub busy_ms: u64,
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
    sink: Arc<dyn EventSink>,
    attempt_started_at: HashMap<TaskId, Instant>,
    attempt_log: HashMap<TaskId, Vec<AttemptRecord>>,
    task_session: HashMap<TaskId, Option<String>>,
    task_tool_calls: HashMap<TaskId, Vec<ToolCallRecord>>,
    /// (device_id, model_id) of each task's most recent attempt.
    task_final_device: HashMap<TaskId, (String, String)>,
    /// The user goal (passed to the replanner) + how many replan rounds have run.
    goal: String,
    replans_done: u32,
    /// Ids of replanner-added (bonus) tasks — failures here are non-fatal to the run.
    bonus_ids: HashSet<TaskId>,
    /// Observed per-device speed: device index -> (total completed ms, count). Used to route the
    /// hardest tasks (incl. integrate-verify) to the proven-fastest node on an identical-model fleet.
    device_speed: HashMap<usize, (u64, u32)>,
    /// Judge support — empty/false unless a judge is attached. `abort_handles` lets the judge kill a
    /// stuck worker's future; `prior_hints` carries the judge's corrective note onto the re-dispatch;
    /// `interventions` caps kills per task; `judge_running` keeps at most one judge in flight at a time.
    abort_handles: HashMap<TaskId, tokio::task::AbortHandle>,
    prior_hints: HashMap<TaskId, String>,
    interventions: HashMap<TaskId, u32>,
    /// Split generation per task: 0 for original tasks, parent+1 for children injected by a split. Feeds
    /// JudgeRequest.split_count so the judge caps splitting at once (a split-child is never re-split).
    split_generation: HashMap<TaskId, u32>,
    judge_running: bool,
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

    /// Free worker slots across enabled devices (how much parallel work could start right now).
    fn idle_capacity(&self) -> u32 {
        self.devices
            .iter()
            .filter(|d| d.cfg.enabled)
            .map(|d| d.cfg.weight.saturating_sub(d.in_flight))
            .sum()
    }

    fn make_replan_context(&self) -> ReplanContext {
        let mut completed = Vec::new();
        let mut failed = Vec::new();
        let mut incomplete = Vec::new();
        for (id, n) in &self.dag.tasks {
            match n.state {
                TaskState::Done => completed.push((
                    id.clone(),
                    n.result
                        .clone()
                        .unwrap_or_default()
                        .chars()
                        .take(400)
                        .collect(),
                )),
                TaskState::Failed => failed.push(id.clone()),
                _ => incomplete.push(id.clone()),
            }
        }
        ReplanContext {
            goal: self.goal.clone(),
            existing_ids: self.dag.tasks.keys().cloned().collect(),
            completed,
            failed,
            incomplete,
            idle_capacity: self.idle_capacity(),
            round: self.replans_done.saturating_sub(1),
        }
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
        // Spread work across the fleet: the LEAST-LOADED device wins, so idle nodes get work before
        // any node doubles up; ties break toward the planner's preferred model, then by index for
        // determinism. (Honoring preferred_model first would pile every same-model task on one device
        // and leave the rest of the fleet idle — the opposite of what a swarm is for.)
        let pm = n.spec.preferred_model.as_deref();
        // A HARD task (the heaviest work, incl. integrate-verify) prefers the FASTEST free node: identical
        // models differ only in host speed, so the critical path shrinks if the big tasks land on the
        // quickest node. Load (in_flight) stays primary, so this never over-concentrates.
        let hard = matches!(n.spec.difficulty, Difficulty::Hard);
        pool.into_iter().min_by_key(|&i| {
            let d = &self.devices[i];
            let sw = d.cfg.speed_weight.max(1) as u64;
            let prefers_rank = match pm {
                Some(m) if m == d.cfg.model_id => 0,
                _ => 1,
            };
            // Hard-task speed: real observed avg ms/task if known (lower = faster). If not yet observed,
            // SEED from the configured speed_weight so the heaviest task lands on the known-fastest host
            // from the very first dispatch (higher speed_weight -> smaller key -> preferred).
            let speed = if hard {
                self.device_speed
                    .get(&i)
                    .map(|(t, c)| t / (*c).max(1) as u64)
                    .unwrap_or(u64::MAX - sw)
            } else {
                0
            };
            // SPEED-WEIGHTED share of the load: normalize the dispatch count by speed_weight so a faster
            // host accumulates proportionally MORE tasks before it is "even" (≈ ratio of speed_weights),
            // while the slowest host gets far fewer. Also rotates work so no host is starved.
            let weighted_load = self
                .dispatched_per_device
                .get(&d.cfg.id)
                .copied()
                .unwrap_or(0) as u64
                * 1000
                / sw;
            (d.in_flight, speed, prefers_rank, weighted_load, i)
        })
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
        let deps = self.dag.tasks[&tid].spec.deps.clone();
        let slice = self.ctx.slice_for(&deps);
        let (files, description, attempt) = {
            let n = self.dag.tasks.get_mut(&tid).unwrap();
            n.state = TaskState::Claimed;
            (
                n.spec.owned_files.clone(),
                n.spec.description.clone(),
                n.attempts,
            )
        };
        for f in &files {
            self.held_files.insert(f.clone());
        }
        self.devices[dev].in_flight += 1;
        self.claimed_device.insert(tid.clone(), dev);
        let device_id = self.devices[dev].cfg.id.clone();
        let model_id = self.devices[dev].cfg.model_id.clone();
        *self
            .dispatched_per_device
            .entry(device_id.clone())
            .or_default() += 1;
        self.attempt_started_at.insert(tid.clone(), Instant::now());
        self.task_final_device
            .insert(tid.clone(), (device_id.clone(), model_id.clone()));
        self.sink.emit(&SwarmEvent::TaskDispatched {
            task_id: tid.clone(),
            device: device_id.clone(),
            model: model_id.clone(),
            attempt,
            deps,
            owned_files: files.clone(),
            context_slice_len: slice.len(),
        });
        let owned_files = files.clone();
        let mut all_files: Vec<String> = self
            .dag
            .tasks
            .values()
            .flat_map(|n| n.spec.owned_files.iter().cloned())
            .collect();
        all_files.sort();
        all_files.dedup();
        self.held_by.insert(tid.clone(), files);
        let prior_hint = self.prior_hints.remove(&tid);
        out.push(Assignment {
            task_id: tid.clone(),
            request: DispatchRequest {
                task_id: tid,
                description,
                device_id,
                model_id,
                context_slice: slice,
                attempt,
                owned_files,
                all_files,
                prior_hint,
            },
        });
    }

    fn complete(&mut self, tid: &str, attempt: u32, res: Result<TaskRunOutput, DispatchError>) {
        // Ignore a completion from an attempt the judge already superseded (killed + re-dispatched):
        // its device and file holds were released when the judge intervened, so this stale future must
        // not touch the newer attempt's bookkeeping. `attempts` advances on every kill/retry, so a
        // mismatch uniquely identifies a dead attempt.
        if self.dag.tasks.get(tid).map(|n| n.attempts) != Some(attempt) {
            return;
        }
        self.abort_handles.remove(tid);
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

        let elapsed_ms = self
            .attempt_started_at
            .remove(tid)
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let (dev_id, model_id) = match self.task_final_device.get(tid).cloned() {
            Some((d, m)) => (Some(d), Some(m)),
            None => (None, None),
        };

        match res {
            Ok(run) => {
                // Record this device's throughput (successful completions only) for speed-aware routing.
                if let Some(dev) = released_dev {
                    let e = self.device_speed.entry(dev).or_insert((0, 0));
                    e.0 += elapsed_ms;
                    e.1 += 1;
                }
                let TaskRunOutput {
                    output,
                    session_id,
                    tool_calls,
                } = run;
                self.task_session
                    .insert(tid.to_string(), session_id.clone());
                self.task_tool_calls
                    .insert(tid.to_string(), tool_calls.clone());
                self.attempt_log
                    .entry(tid.to_string())
                    .or_default()
                    .push(AttemptRecord {
                        device: dev_id.clone(),
                        model: model_id.clone(),
                        outcome: "ok".to_string(),
                        error: None,
                        elapsed_ms,
                    });
                let attempts = self.attempt_log[tid].len() as u32;
                {
                    let n = self.dag.tasks.get_mut(tid).unwrap();
                    n.state = TaskState::Done;
                    n.result = Some(output.clone());
                    n.avoid_device = None;
                }
                self.ctx.merge(tid, output);
                self.sink.emit(&SwarmEvent::TaskCompleted {
                    task_id: tid.to_string(),
                    status: "done".to_string(),
                    device: dev_id,
                    model: model_id,
                    attempts,
                    elapsed_ms,
                    session_id,
                    tool_calls,
                });
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
            Err(DispatchError::Transient(msg)) => {
                self.attempt_log
                    .entry(tid.to_string())
                    .or_default()
                    .push(AttemptRecord {
                        device: dev_id.clone(),
                        model: model_id.clone(),
                        outcome: "transient".to_string(),
                        error: Some(msg.clone()),
                        elapsed_ms,
                    });
                let exhausted = {
                    // Judge kills advance n.attempts (for the epoch guard) but are SUPERVISORY, not task
                    // failures — and the judge can be wrong (a borderline over-read). Don't let a judge
                    // intervention burn the transient-retry budget: exclude it from the exhaustion count.
                    let judge_kills = self.interventions.get(tid).copied().unwrap_or(0);
                    let n = self.dag.tasks.get_mut(tid).unwrap();
                    n.attempts += 1;
                    n.attempts.saturating_sub(judge_kills) >= self.max_attempts
                };
                if exhausted {
                    self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Failed;
                    self.fail_descendants(tid);
                    let attempts = self.attempt_log[tid].len() as u32;
                    self.sink.emit(&SwarmEvent::TaskCompleted {
                        task_id: tid.to_string(),
                        status: "failed".to_string(),
                        device: dev_id,
                        model: model_id,
                        attempts,
                        elapsed_ms,
                        session_id: None,
                        tool_calls: Vec::new(),
                    });
                } else {
                    {
                        let n = self.dag.tasks.get_mut(tid).unwrap();
                        n.avoid_device = released_dev_id.clone();
                        n.state = TaskState::Ready;
                        let fan_out = n.fan_out;
                        self.ready.push(Ranked {
                            fan_out,
                            id: tid.to_string(),
                        });
                    }
                    self.sink.emit(&SwarmEvent::TaskRetry {
                        task_id: tid.to_string(),
                        from_device: released_dev_id,
                        error: msg,
                        transient: true,
                    });
                }
            }
            Err(DispatchError::Terminal(msg)) => {
                self.attempt_log
                    .entry(tid.to_string())
                    .or_default()
                    .push(AttemptRecord {
                        device: dev_id.clone(),
                        model: model_id.clone(),
                        outcome: "terminal".to_string(),
                        error: Some(msg),
                        elapsed_ms,
                    });
                self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Failed;
                self.fail_descendants(tid);
                let attempts = self.attempt_log[tid].len() as u32;
                self.sink.emit(&SwarmEvent::TaskCompleted {
                    task_id: tid.to_string(),
                    status: "failed".to_string(),
                    device: dev_id,
                    model: model_id,
                    attempts,
                    elapsed_ms,
                    session_id: None,
                    tool_calls: Vec::new(),
                });
            }
        }
    }

    /// Choose an in-flight worker for the judge to inspect: the longest-running Claimed task that is at
    /// least `min_age_secs` old and under its intervention cap, to be judged on a currently-idle device.
    /// Returns the request + the attempt inspected, and marks a judge running (at most one at a time).
    fn pick_judge_target(&mut self, cfg: &JudgeConfig) -> Option<(JudgeRequest, u32)> {
        // The LLM review wants an idle device; the deterministic checks (won't-compile / no-output /
        // wrote-then-stale) need no model at all. Prefer an idle device's model for the review, but fall
        // through with an empty model_id so the deterministic verdicts still fire when every node is busy
        // (weight-1 fully saturated) — otherwise a stuck worker goes unjudged until worker_max_turns.
        let judge_model_id = self
            .devices
            .iter()
            .find(|d| d.cfg.enabled && d.in_flight < d.cfg.weight)
            .map(|d| d.cfg.model_id.clone())
            .unwrap_or_default();
        // Two pools: `best` = under-cap tasks (normal judging — re-dispatch on a problem); `best_terminal`
        // = cap-exhausted tasks, surfaced ONLY so the judge can make a terminal decision (a task already
        // re-dispatched to its cap that is STILL broken should be failed, not left to spin a node to
        // worker_max_turns). Under-cap judging is always preferred so a cap-exhausted-but-fine task can
        // never starve a genuinely-stuck one of the single judge slot.
        let mut best: Option<(String, u64)> = None;
        let mut best_terminal: Option<(String, u64)> = None;
        for (tid, n) in &self.dag.tasks {
            if n.state != TaskState::Claimed {
                continue;
            }
            let elapsed = self
                .attempt_started_at
                .get(tid)
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            if elapsed < cfg.min_age_secs {
                continue;
            }
            let at_cap =
                self.interventions.get(tid).copied().unwrap_or(0) >= cfg.max_interventions_per_task;
            let slot = if at_cap { &mut best_terminal } else { &mut best };
            if slot.as_ref().map(|(_, e)| elapsed > *e).unwrap_or(true) {
                *slot = Some((tid.clone(), elapsed));
            }
        }
        let (tid, elapsed) = best.or(best_terminal)?;
        let (description, owned_files, attempt) = {
            let n = &self.dag.tasks[&tid];
            (
                n.spec.description.clone(),
                n.spec.owned_files.clone(),
                n.attempts,
            )
        };
        // High-level run state for the semantic judge: completed tasks (with a brief of what each
        // produced), the tasks still in flight / pending, and the failed ones. Lets it judge this worker
        // against the whole build — catch it re-doing finished work, depending on a failed task, or
        // diverging from the shape the rest of the run already set.
        let mut done = Vec::new();
        let mut remaining = Vec::new();
        let mut failed = Vec::new();
        for (id, node) in &self.dag.tasks {
            if id == &tid {
                continue;
            }
            match node.state {
                TaskState::Done => done.push((
                    id.clone(),
                    node.result
                        .clone()
                        .unwrap_or_default()
                        .chars()
                        .take(200)
                        .collect(),
                )),
                TaskState::Failed => failed.push(id.clone()),
                _ => remaining.push(id.clone()),
            }
        }
        let req = JudgeRequest {
            task_id: tid.clone(),
            description,
            owned_files,
            elapsed_secs: elapsed,
            judge_model_id,
            goal: self.goal.clone(),
            done,
            remaining,
            failed,
            split_count: self.split_generation.get(&tid).copied().unwrap_or(0),
        };
        self.judge_running = true;
        Some((req, attempt))
    }

    /// M5: pick a COMPLETED-but-unreviewed task (that owns files) for an idle-node correctness pre-review,
    /// claiming the single idle-job slot. Returns the request, or None if no idle device is free, nothing
    /// is reviewable, or the slot is taken. Marks the task pre_reviewed up front so it is picked at most
    /// once even while the review is in flight.
    fn pick_prereview_request(&mut self) -> Option<PreReviewRequest> {
        let reviewer_model_id = self
            .devices
            .iter()
            .find(|d| d.cfg.enabled && d.in_flight < d.cfg.weight)
            .map(|d| d.cfg.model_id.clone())?;
        let tid = self
            .dag
            .tasks
            .iter()
            .find(|(_, n)| {
                n.state == TaskState::Done && !n.pre_reviewed && !n.spec.owned_files.is_empty()
            })
            .map(|(id, _)| id.clone())?;
        let (description, owned_files) = {
            let n = &self.dag.tasks[&tid];
            (n.spec.description.clone(), n.spec.owned_files.clone())
        };
        self.dag.tasks.get_mut(&tid).unwrap().pre_reviewed = true;
        self.judge_running = true;
        Some(PreReviewRequest {
            task_id: tid,
            description,
            owned_files,
            goal: self.goal.clone(),
            reviewer_model_id,
        })
    }

    /// Apply a judge verdict. Always emits a `JudgeVerdict` event. If the verdict is an actionable
    /// problem, the inspected attempt is still the live one, the judge is confident enough, and the
    /// per-task intervention cap is not yet hit, the worker is killed and its task re-queued with the
    /// hint — otherwise the verdict is logged only (`observed`). The judge being a weak model is the
    /// reason these guards are strict.
    fn apply_judge_outcome(
        &mut self,
        tid: &str,
        attempt: u32,
        outcome: JudgeOutcome,
        cfg: &JudgeConfig,
    ) -> bool {
        let (device, model) = match self.task_final_device.get(tid) {
            Some((d, m)) => (Some(d.clone()), Some(m.clone())),
            None => (None, None),
        };
        let still_live = self
            .dag
            .tasks
            .get(tid)
            .map(|n| n.attempts == attempt && n.state == TaskState::Claimed)
            .unwrap_or(false);
        let interv = self.interventions.get(tid).copied().unwrap_or(0);
        let elapsed = self
            .attempt_started_at
            .get(tid)
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        // SPLIT is an action on healthy-but-too-big work, handled separately below — keep it out of the
        // kill/re-dispatch path (which is for misbehaving workers).
        let is_split = still_live
            && outcome.verdict == crate::judge::Verdict::Split
            && outcome.confidence >= cfg.intervene_confidence
            && outcome
                .proposed_split
                .as_ref()
                .is_some_and(|c| !c.is_empty());
        let actionable = outcome.verdict.is_problem()
            && outcome.verdict != crate::judge::Verdict::Split
            && still_live
            && outcome.confidence >= cfg.intervene_confidence;
        // The judge's three actions: observe (log only), re_dispatch (kill + retry with a hint, while
        // under the cap), or fail (cap exhausted and STILL broken — cut it loose so a doomed task can't
        // spin a node to worker_max_turns and so the run terminates instead of hanging on a dead worker).
        // Terminal-fail requires a positive cap that has been used up AND a final attempt that has run a
        // meaningful while, so a brief flag on a just-(re)started attempt never fails a task prematurely.
        let terminal = actionable
            && cfg.max_interventions_per_task > 0
            && interv >= cfg.max_interventions_per_task
            && elapsed >= cfg.terminal_min_secs;
        let redispatch = actionable && interv < cfg.max_interventions_per_task;
        let action = if is_split {
            "split"
        } else if terminal {
            "failed"
        } else if redispatch {
            "re_dispatch"
        } else {
            "observed"
        };
        self.sink.emit(&SwarmEvent::JudgeVerdict {
            task_id: tid.to_string(),
            device: device.clone().unwrap_or_default(),
            verdict: outcome.verdict.as_str().to_string(),
            confidence: outcome.confidence,
            hint: outcome.hint.clone(),
            action: action.to_string(),
        });
        if is_split {
            // proposed_split is present + non-empty here; apply_split validates the partition and returns
            // false (no-op, worker keeps running) if it is malformed — a bad proposal never corrupts the DAG.
            let children = outcome.proposed_split.clone().unwrap_or_default();
            return self.apply_split(tid, &children);
        }
        if terminal {
            if let Some(h) = self.abort_handles.remove(tid) {
                h.abort();
            }
            if let Some(dev) = self.claimed_device.remove(tid) {
                if self.devices[dev].in_flight > 0 {
                    self.devices[dev].in_flight -= 1;
                }
            }
            if let Some(files) = self.held_by.remove(tid) {
                for f in files {
                    self.held_files.remove(&f);
                }
            }
            self.attempt_started_at.remove(tid);
            self.attempt_log
                .entry(tid.to_string())
                .or_default()
                .push(AttemptRecord {
                    device: device.clone(),
                    model: model.clone(),
                    outcome: "judge_failed".to_string(),
                    error: Some(outcome.verdict.as_str().to_string()),
                    elapsed_ms: 0,
                });
            self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Failed;
            self.fail_descendants(tid);
            let attempts = self.attempt_log[tid].len() as u32;
            self.sink.emit(&SwarmEvent::TaskCompleted {
                task_id: tid.to_string(),
                status: "failed".to_string(),
                device,
                model,
                attempts,
                elapsed_ms: 0,
                session_id: None,
                tool_calls: Vec::new(),
            });
            return true;
        }
        if !redispatch {
            return false;
        }
        if let Some(h) = self.abort_handles.remove(tid) {
            h.abort();
        }
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
        self.attempt_started_at.remove(tid);
        *self.interventions.entry(tid.to_string()).or_default() += 1;
        self.prior_hints.insert(tid.to_string(), outcome.hint);
        self.attempt_log
            .entry(tid.to_string())
            .or_default()
            .push(AttemptRecord {
                device,
                model,
                outcome: "judge_killed".to_string(),
                error: Some(outcome.verdict.as_str().to_string()),
                elapsed_ms: 0,
            });
        // Advance the attempt epoch so the killed future's completion is ignored, then re-queue.
        let n = self.dag.tasks.get_mut(tid).unwrap();
        n.attempts += 1;
        n.avoid_device = released_dev_id;
        n.state = TaskState::Ready;
        let fan_out = n.fan_out;
        self.ready.push(Ranked {
            fan_out,
            id: tid.to_string(),
        });
        true
    }

    /// M3 task-splitting: replace a too-big task with the judge's proposed children that PARTITION its
    /// owned files. Returns true if a VALID split was applied (worker aborted, children injected, the
    /// original's dependents re-pointed onto ALL children); false if the proposal is malformed — the caller
    /// then takes no action and the worker keeps running, so a bad proposal can never corrupt the DAG.
    fn apply_split(&mut self, tid: &str, children: &[crate::judge::ChildSpec]) -> bool {
        // ---- validate the partition against the original (no mutation yet) ----
        let (orig_files, orig_deps, orig_diff, orig_model) = match self.dag.tasks.get(tid) {
            Some(n) => (
                n.spec
                    .owned_files
                    .iter()
                    .cloned()
                    .collect::<std::collections::BTreeSet<String>>(),
                n.spec.deps.clone(),
                n.spec.difficulty,
                n.spec.preferred_model.clone(),
            ),
            None => return false,
        };
        if children.len() < 2 {
            return false; // need >= 2 parts to be worth splitting
        }
        let mut child_ids = std::collections::HashSet::new();
        for c in children {
            if !child_ids.insert(c.id.as_str()) || self.dag.tasks.contains_key(&c.id) {
                return false; // duplicate child id, or collides with an existing task
            }
        }
        // every child file belongs to the original, children are pairwise-disjoint, and together they
        // cover ALL of the original's files (a true partition).
        let mut union = std::collections::BTreeSet::new();
        for c in children {
            if c.files.is_empty() {
                return false;
            }
            for f in &c.files {
                if !orig_files.contains(f) || !union.insert(f.clone()) {
                    return false; // foreign file or overlap between children
                }
            }
        }
        if union != orig_files {
            return false; // does not cover the original's files
        }
        // child sibling deps may only reference sibling child ids.
        if children
            .iter()
            .any(|c| c.depends_on.iter().any(|d| !child_ids.contains(d.as_str())))
        {
            return false;
        }
        // ---- abort + release the original worker (mirror the kill/re-dispatch cleanup) ----
        if let Some(h) = self.abort_handles.remove(tid) {
            h.abort();
        }
        if let Some(dev) = self.claimed_device.remove(tid) {
            if self.devices[dev].in_flight > 0 {
                self.devices[dev].in_flight -= 1;
            }
        }
        if let Some(files) = self.held_by.remove(tid) {
            for f in files {
                self.held_files.remove(&f);
            }
        }
        self.attempt_started_at.remove(tid);
        // ---- build + insert the children (deps = original's external deps + sibling deps) ----
        let child_id_list: Vec<TaskId> = children.iter().map(|c| c.id.clone()).collect();
        let specs: Vec<crate::dag::TaskSpec> = children
            .iter()
            .map(|c| {
                let mut deps = orig_deps.clone();
                deps.extend(c.depends_on.iter().cloned());
                crate::dag::TaskSpec {
                    id: c.id.clone(),
                    description: format!("(split of {tid}) {}", c.id),
                    difficulty: orig_diff,
                    preferred_model: orig_model.clone(),
                    owned_files: c.files.clone(),
                    deps,
                }
            })
            .collect();
        let newly_ready = match self.dag.splice_specs(specs) {
            Ok(r) => r,
            Err(_) => {
                // cycle/collision: abort the split. The worker is already gone, so fail the task cleanly.
                if let Some(n) = self.dag.tasks.get_mut(tid) {
                    n.state = TaskState::Failed;
                }
                self.fail_descendants(tid);
                return true;
            }
        };
        // ---- re-point every dependent of the original onto ALL children ----
        let dependents = self.dag.dependents.get(tid).cloned().unwrap_or_default();
        for d in &dependents {
            if let Some(n) = self.dag.tasks.get_mut(d) {
                n.spec.deps.retain(|x| x != tid);
                n.spec.deps.extend(child_id_list.iter().cloned());
                // the original counted as ONE unmet dependency; it is now N unmet children -> net +(N-1).
                n.indegree_remaining += child_id_list.len() - 1;
            }
            for cid in &child_id_list {
                self.dag
                    .dependents
                    .entry(cid.clone())
                    .or_default()
                    .push(d.clone());
                if let Some(cn) = self.dag.tasks.get_mut(cid) {
                    cn.fan_out += 1;
                }
            }
        }
        self.dag.dependents.remove(tid);
        // ---- record each child's split generation = parent + 1, so the cap (split once) holds: a child
        // that itself runs long carries split_count >= 1 and is never split again. ----
        let parent_gen = self.split_generation.get(tid).copied().unwrap_or(0);
        for cid in &child_id_list {
            self.split_generation.insert(cid.clone(), parent_gen + 1);
        }
        // ---- mark the original Done (no cascade) + advance its epoch so a late completion is ignored ----
        if let Some(n) = self.dag.tasks.get_mut(tid) {
            n.attempts += 1;
            n.state = TaskState::Done;
        }
        // ---- enqueue the children that are immediately ready ----
        for id in newly_ready {
            let fan_out = self.dag.tasks[&id].fan_out;
            self.ready.push(Ranked { fan_out, id });
        }
        true
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
        let mut tasks = Vec::new();
        let mut per_device: HashMap<String, DeviceSummary> = HashMap::new();
        for (id, n) in &self.dag.tasks {
            let status = match n.state {
                TaskState::Done => {
                    done.push(id.clone());
                    if let Some(r) = &n.result {
                        results.insert(id.clone(), r.clone());
                    }
                    "done"
                }
                TaskState::Failed => {
                    failed.push(id.clone());
                    "failed"
                }
                _ => "incomplete",
            };
            let history = self.attempt_log.get(id).cloned().unwrap_or_default();
            let elapsed_ms = history.last().map(|a| a.elapsed_ms);
            let (device, model) = match self.task_final_device.get(id) {
                Some((d, m)) => (Some(d.clone()), Some(m.clone())),
                None => (None, None),
            };
            let session_id = self.task_session.get(id).cloned().flatten();
            let tool_calls = self.task_tool_calls.get(id).cloned().unwrap_or_default();

            for a in &history {
                if let Some(d) = &a.device {
                    let e = per_device.entry(d.clone()).or_default();
                    e.busy_ms += a.elapsed_ms;
                    if a.outcome == "transient" {
                        e.retries += 1;
                    }
                }
            }
            if let Some(d) = &device {
                let e = per_device.entry(d.clone()).or_default();
                e.tool_calls += tool_calls.len() as u32;
                e.mcp_calls += tool_calls.iter().filter(|t| t.is_mcp).count() as u32;
            }

            tasks.push(TaskOutcome {
                task_id: id.clone(),
                status: status.to_string(),
                device,
                model,
                attempts: history.len() as u32,
                attempt_history: history,
                elapsed_ms,
                session_id,
                tool_calls,
                output: n.result.clone(),
            });
        }
        for (d, c) in &self.dispatched_per_device {
            per_device.entry(d.clone()).or_default().dispatched = *c;
        }
        done.sort();
        failed.sort();
        tasks.sort_by(|a, b| a.task_id.cmp(&b.task_id));
        let mut bonus: Vec<TaskId> = self.bonus_ids.iter().cloned().collect();
        bonus.sort();
        RunReport {
            done,
            failed,
            bonus,
            results,
            context_json: self.ctx.to_json(),
            dispatched_per_device: self.dispatched_per_device.clone(),
            tasks,
            per_device,
        }
    }
}

pub struct Scheduler {
    devices: Vec<DeviceCfg>,
    max_attempts: u32,
    sink: Arc<dyn EventSink>,
    replanner: Option<Arc<dyn Replanner>>,
    max_replans: u32,
    judge: Option<Arc<dyn Judge>>,
    judge_cfg: JudgeConfig,
    pre_reviewer: Option<Arc<dyn PreReviewer>>,
}

impl Scheduler {
    pub fn new(devices: Vec<DeviceCfg>, max_attempts: u32) -> Self {
        Self {
            devices,
            max_attempts,
            sink: Arc::new(NullSink),
            replanner: None,
            max_replans: 0,
            judge: None,
            judge_cfg: JudgeConfig::default(),
            pre_reviewer: None,
        }
    }

    /// Attach an idle-model judge: when a node would otherwise sit idle while tasks are still in
    /// flight, it inspects a busy worker and may kill + re-dispatch one that is looping, over-reading,
    /// or producing broken code. OFF by default — with no judge attached the scheduler is unchanged.
    pub fn with_judge(mut self, judge: Arc<dyn Judge>, cfg: JudgeConfig) -> Self {
        self.judge = Some(judge);
        self.judge_cfg = cfg;
        self
    }

    /// Attach an idle-node PRE-REVIEWER (M5): when a node would otherwise idle and NO in-flight worker
    /// needs judging, it correctness-checks a COMPLETED-but-unreviewed task's output and records findings
    /// for integrate-verify. OFF by default — with none attached the scheduler is unchanged.
    pub fn with_pre_reviewer(mut self, pre_reviewer: Arc<dyn PreReviewer>) -> Self {
        self.pre_reviewer = Some(pre_reviewer);
        self
    }

    /// Attach an event sink for structured observability (goose-cli writes JSONL through it).
    pub fn with_sink(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.sink = sink;
        self
    }

    /// Attach a dynamic replanner: when workers go idle mid-run (>=2 free slots while a task is still
    /// in flight) it is asked for more parallel work, up to `max_replans` rounds. Off by default.
    pub fn with_replanner(mut self, replanner: Arc<dyn Replanner>, max_replans: u32) -> Self {
        self.replanner = Some(replanner);
        self.max_replans = max_replans;
        self
    }

    /// Run the whole DAG to completion. Returns when every task is Done or Failed. `goal` is the user
    /// prompt, used only by the dynamic replanner (ignored when none is attached).
    pub async fn run(
        &self,
        dag: Dag,
        dispatcher: Arc<dyn TaskDispatcher>,
        goal: String,
    ) -> Result<RunReport> {
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
                bail!(
                    "device `{}` has weight 0 (enabled) — disable it instead",
                    d.id
                );
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
            sink: self.sink.clone(),
            attempt_started_at: HashMap::new(),
            attempt_log: HashMap::new(),
            task_session: HashMap::new(),
            task_tool_calls: HashMap::new(),
            task_final_device: HashMap::new(),
            goal,
            replans_done: 0,
            bonus_ids: HashSet::new(),
            device_speed: HashMap::new(),
            abort_handles: HashMap::new(),
            prior_hints: HashMap::new(),
            interventions: HashMap::new(),
            split_generation: HashMap::new(),
            judge_running: false,
        }));
        let notify = Arc::new(Notify::new());

        loop {
            let assignments = { state.lock().await.pick_assignments() };
            let dispatched_now = !assignments.is_empty();
            for a in assignments {
                let dispatcher = dispatcher.clone();
                let task_state = state.clone();
                let notify = notify.clone();
                let task_id = a.task_id.clone();
                let attempt = a.request.attempt;
                let request = a.request;
                let done_id = task_id.clone();
                let jh = tokio::spawn(async move {
                    let res = dispatcher.run(request).await;
                    {
                        let mut s = task_state.lock().await;
                        s.complete(&done_id, attempt, res);
                    }
                    notify.notify_one();
                });
                // Register the abort handle only when a judge is attached, so it can kill this attempt.
                // No judge -> the map stays empty and the default path is byte-identical to before.
                if self.judge.is_some() {
                    state
                        .lock()
                        .await
                        .abort_handles
                        .insert(task_id, jh.abort_handle());
                }
            }

            {
                let s = state.lock().await;
                if s.all_terminal() {
                    return Ok(s.build_report());
                }
                if !dispatched_now && s.total_in_flight() == 0 {
                    // Nothing assignable and nothing running, but not all terminal: the remaining
                    // tasks are permanently blocked (deps failed, or a file deadlock).
                    let remaining = s
                        .dag
                        .tasks
                        .values()
                        .filter(|n| !matches!(n.state, TaskState::Done | TaskState::Failed))
                        .count();
                    s.sink.emit(&SwarmEvent::SchedulerStuck { remaining });
                    bail!(
                        "scheduler stuck: {remaining} task(s) cannot proceed (blocked by failed deps or file holds)"
                    );
                }
            }
            // Dynamic replan: workers idle while a task is still in flight (e.g. a slow tail) — ask the
            // planner for more parallel work to fill them. Gated on in_flight > 0, so it is mutually
            // exclusive with the stuck-bail above (which needs in_flight == 0). The state lock is
            // released across the async planner call; completions fire meanwhile and splice_specs
            // re-validates against the now-current DAG.
            if self.replanner.is_some() {
                let ctx = {
                    let mut s = state.lock().await;
                    if !dispatched_now
                        && s.total_in_flight() > 0
                        && s.ready.is_empty()
                        && s.idle_capacity() >= 2
                        && s.replans_done < self.max_replans
                    {
                        s.replans_done += 1;
                        Some(s.make_replan_context())
                    } else {
                        None
                    }
                };
                if let Some(ctx) = ctx {
                    let round = ctx.round;
                    let specs = self
                        .replanner
                        .as_ref()
                        .unwrap()
                        .replan(ctx)
                        .await
                        .unwrap_or_default();
                    let mut s = state.lock().await;
                    if s.all_terminal() {
                        return Ok(s.build_report());
                    }
                    if specs.is_empty() {
                        s.sink.emit(&SwarmEvent::Replanned {
                            round,
                            added: Vec::new(),
                            stopped: true,
                        });
                        s.replans_done = self.max_replans;
                    } else {
                        // Replanner-added tasks are OPPORTUNISTIC (idle-fill) — record them as bonus so a
                        // bonus failure cannot fail an otherwise-complete run (run success = core plan).
                        let spliced_ids: Vec<TaskId> =
                            specs.iter().map(|sp| sp.id.clone()).collect();
                        match s.dag.splice_specs(specs) {
                            Ok(new_ready) => {
                                s.bonus_ids.extend(spliced_ids);
                                let added = new_ready.clone();
                                for id in new_ready {
                                    let fan_out = s.dag.tasks[&id].fan_out;
                                    s.ready.push(Ranked { fan_out, id });
                                }
                                s.sink.emit(&SwarmEvent::Replanned {
                                    round,
                                    added,
                                    stopped: false,
                                });
                                drop(s);
                                continue;
                            }
                            Err(_) => {
                                s.sink.emit(&SwarmEvent::Replanned {
                                    round,
                                    added: Vec::new(),
                                    stopped: true,
                                });
                                s.replans_done = self.max_replans;
                            }
                        }
                    }
                }
            }
            // Idle-model judge: when a node would otherwise sit idle while tasks are still in flight,
            // inspect the longest-running worker and possibly kill + re-dispatch a stuck one. At most one
            // judge runs at a time; the whole block is skipped when no judge is attached.
            if let Some(judge) = &self.judge {
                let target = {
                    let mut s = state.lock().await;
                    if s.judge_running || s.total_in_flight() == 0 {
                        None
                    } else {
                        s.pick_judge_target(&self.judge_cfg)
                    }
                };
                if let Some((req, attempt)) = target {
                    let tid = req.task_id.clone();
                    let judge = judge.clone();
                    let st = state.clone();
                    let nt = notify.clone();
                    let cfg = self.judge_cfg;
                    tokio::spawn(async move {
                        let outcome = judge.judge(req).await;
                        let intervened = {
                            let mut s = st.lock().await;
                            let r = s.apply_judge_outcome(&tid, attempt, outcome, &cfg);
                            s.judge_running = false;
                            r
                        };
                        // Only wake the loop when the judge actually intervened (the re-dispatched task
                        // needs to be picked up). An "observed" verdict changes nothing — notifying here
                        // would immediately respawn a judge and busy-loop; the 30s tick re-evaluates.
                        if intervened {
                            nt.notify_one();
                        }
                    });
                }
            }
            // M5: when no in-flight worker needed judging, put the idle node on a correctness PRE-REVIEW of
            // a completed-but-unreviewed task (findings feed integrate-verify). Shares the single idle-job
            // slot with the judge via judge_running, so at most one idle job runs at a time. Off unless a
            // pre-reviewer is attached; pick_prereview_request returns None when the slot is taken.
            if let Some(pr) = &self.pre_reviewer {
                let req = {
                    let mut s = state.lock().await;
                    if s.judge_running {
                        None
                    } else {
                        s.pick_prereview_request()
                    }
                };
                if let Some(req) = req {
                    let pr = pr.clone();
                    let st = state.clone();
                    tokio::spawn(async move {
                        let _ = pr.pre_review(req).await;
                        st.lock().await.judge_running = false;
                    });
                }
            }
            // Wake on a completion, or — when a judge is attached — at least every 15s, so it can
            // inspect a worker that crosses a threshold BETWEEN completions (a lone stuck worker produces
            // no completion to wake on). A short tick means the behavioral over-read signal (many actions,
            // zero output) and the terminal-fail decision act within ~15s of tripping, not minutes.
            // tokio::Notify stores one permit, so a completion that fires before this await is not lost.
            // With no judge this is an effectively-infinite wait: byte-identical to before.
            let tick = if self.judge.is_some() || self.pre_reviewer.is_some() {
                std::time::Duration::from_secs(15)
            } else {
                std::time::Duration::from_secs(86_400)
            };
            let _ = tokio::time::timeout(tick, notify.notified()).await;
        }
    }
}
