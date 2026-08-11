//! The task DAG: built from a planner plan or directly from specs. Validates at load time
//! (unknown deps, cycles) and computes fan-out + initial ready set.

use anyhow::{bail, Result};
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};

pub type TaskId = String;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Easy,
    Hard,
}

#[derive(Clone, Debug)]
pub struct TaskSpec {
    pub id: TaskId,
    pub description: String,
    pub difficulty: Difficulty,
    /// Preferred LM Link model id (the device the planner suggests); the scheduler steers here
    /// first but may work-steal to another free device.
    pub preferred_model: Option<String>,
    /// Files this task owns/edits; two tasks holding the same file never run concurrently.
    pub owned_files: Vec<String>,
    pub deps: Vec<TaskId>,
    /// S3 i2 (LATENT): 2-4 top-level function/class names the detailer marked as independently
    /// implementable — parsed from the spec's trailing `SUBSPLIT:` line, re-anchored against the
    /// module's frozen stub at consumption. Nothing dispatches differently until a fill fan
    /// consumes it; empty for every task whose spec carries no such line.
    pub subsplit: Vec<String>,
}

/// The detailer's optional latent decomposition: the LAST `SUBSPLIT:` line of a spec,
/// comma-split into 2-4 valid Python identifiers — 1 name is no split, 5+ is the model listing
/// rather than decomposing, and anything non-identifier poisons the whole line (a name the
/// splicer cannot find would refuse every fill). The prompt is a hope; the contract re-anchors
/// the names at consumption.
pub fn extract_subsplit(spec_text: &str) -> Vec<String> {
    let is_ident = |n: &str| {
        !n.is_empty()
            && n.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    let names: Vec<String> = spec_text
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("SUBSPLIT:"))
        .map(|rest| rest.split(',').map(|n| n.trim().to_string()).collect())
        .unwrap_or_default();
    if (2..=4).contains(&names.len()) && names.iter().all(|n| is_ident(n)) {
        names
    } else {
        Vec::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Ready,
    Claimed,
    Done,
    Failed,
}

#[derive(Debug)]
pub struct Node {
    pub spec: TaskSpec,
    pub indegree_remaining: usize,
    pub fan_out: usize,
    pub state: TaskState,
    pub attempts: u32,
    pub result: Option<String>,
    /// On a transient re-dispatch, steer away from the device that just failed this task.
    pub avoid_device: Option<String>,
    /// M5: set once an idle node has correctness-pre-reviewed this task's output (so it is reviewed at
    /// most once). Only meaningful for Done tasks; false for everything not yet pre-reviewed.
    pub pre_reviewed: bool,
}

#[derive(Debug)]
pub struct Dag {
    pub tasks: HashMap<TaskId, Node>,
    /// reverse edges: dep_id -> tasks that depend on it.
    pub dependents: HashMap<TaskId, Vec<TaskId>>,
}

impl Dag {
    /// Build + validate the DAG from specs. Errors on duplicate ids, unknown deps, or cycles.
    pub fn from_specs(specs: Vec<TaskSpec>) -> Result<Self> {
        let mut tasks: HashMap<TaskId, Node> = HashMap::new();
        for spec in specs {
            if tasks.contains_key(&spec.id) {
                bail!("duplicate task id: {}", spec.id);
            }
            let indegree = spec.deps.len();
            tasks.insert(
                spec.id.clone(),
                Node {
                    indegree_remaining: indegree,
                    fan_out: 0,
                    state: if indegree == 0 {
                        TaskState::Ready
                    } else {
                        TaskState::Pending
                    },
                    attempts: 0,
                    result: None,
                    avoid_device: None,
                    pre_reviewed: false,
                    spec,
                },
            );
        }

        // validate deps + build reverse edges
        let mut dependents: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
        let ids: Vec<TaskId> = tasks.keys().cloned().collect();
        for id in &ids {
            let deps = tasks[id].spec.deps.clone();
            for d in deps {
                if !tasks.contains_key(&d) {
                    bail!("task `{}` depends on unknown task `{}`", id, d);
                }
                dependents.entry(d).or_default().push(id.clone());
            }
        }
        for (dep, ds) in &dependents {
            if let Some(n) = tasks.get_mut(dep) {
                n.fan_out = ds.len();
            }
        }

        let dag = Dag { tasks, dependents };
        dag.assert_acyclic()?;
        Ok(dag)
    }

    /// Kahn's algorithm over a copy of the indegrees; errors if a cycle remains.
    fn assert_acyclic(&self) -> Result<()> {
        let mut indeg: HashMap<&str, usize> = self
            .tasks
            .iter()
            .map(|(id, n)| (id.as_str(), n.spec.deps.len()))
            .collect();
        let mut q: VecDeque<&str> = indeg
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut removed = 0usize;
        while let Some(id) = q.pop_front() {
            removed += 1;
            if let Some(ds) = self.dependents.get(id) {
                for d in ds {
                    let e = indeg.get_mut(d.as_str()).unwrap();
                    *e -= 1;
                    if *e == 0 {
                        q.push_back(d.as_str());
                    }
                }
            }
        }
        if removed != self.tasks.len() {
            bail!(
                "plan has a dependency cycle ({} of {} tasks orderable)",
                removed,
                self.tasks.len()
            );
        }
        Ok(())
    }

    /// Build from the planner recipe's JSON output:
    /// `{ "subtasks": [ {id, description, difficulty?, model?, depends_on?, files?} ], "integration"? }`
    pub fn from_planner_json(json: &str) -> Result<Self> {
        Dag::from_specs(specs_from_plan_json(json)?)
    }

    /// Splice additional specs into a LIVE dag at an idle point (the dynamic replanner). Validated
    /// exactly like `from_specs` so the safety net is identical: rejects ids that collide with
    /// existing tasks, deps on unknown OR failed tasks, intra-batch dup ids, and any cycle. On ANY
    /// error the dag is left UNCHANGED (validation runs before mutation). Returns the ids that became
    /// Ready (deps already Done) so the caller can enqueue them; Pending ones unlock via `complete`.
    pub fn splice_specs(&mut self, specs: Vec<TaskSpec>) -> Result<Vec<TaskId>> {
        // --- validate, mutate nothing ---
        let mut batch_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for s in &specs {
            if !batch_ids.insert(s.id.as_str()) {
                bail!("duplicate id `{}` within replan batch", s.id);
            }
            if self.tasks.contains_key(&s.id) {
                bail!("replan id `{}` collides with an existing task", s.id);
            }
        }
        for s in &specs {
            for d in &s.deps {
                if !self.tasks.contains_key(d) && !batch_ids.contains(d.as_str()) {
                    bail!("replan task `{}` depends on unknown task `{}`", s.id, d);
                }
                if let Some(n) = self.tasks.get(d) {
                    if n.state == TaskState::Failed {
                        bail!("replan task `{}` depends on failed task `{}`", s.id, d);
                    }
                }
            }
        }
        // cycle check over the union of existing + new edges (Kahn), before committing.
        {
            let mut indeg: HashMap<String, usize> = HashMap::new();
            let mut deps_of: HashMap<String, Vec<String>> = HashMap::new();
            for id in self.tasks.keys() {
                indeg.entry(id.clone()).or_insert(0);
            }
            for s in &specs {
                indeg.entry(s.id.clone()).or_insert(0);
            }
            for (id, n) in &self.tasks {
                for d in &n.spec.deps {
                    *indeg.get_mut(id).unwrap() += 1;
                    deps_of.entry(d.clone()).or_default().push(id.clone());
                }
            }
            for s in &specs {
                for d in &s.deps {
                    *indeg.get_mut(&s.id).unwrap() += 1;
                    deps_of.entry(d.clone()).or_default().push(s.id.clone());
                }
            }
            let mut q: VecDeque<String> = indeg
                .iter()
                .filter(|(_, d)| **d == 0)
                .map(|(k, _)| k.clone())
                .collect();
            let mut removed = 0usize;
            while let Some(id) = q.pop_front() {
                removed += 1;
                if let Some(ds) = deps_of.get(&id) {
                    for dd in ds {
                        let e = indeg.get_mut(dd).unwrap();
                        *e -= 1;
                        if *e == 0 {
                            q.push_back(dd.clone());
                        }
                    }
                }
            }
            if removed != indeg.len() {
                bail!("replan would create a dependency cycle");
            }
        }

        // --- commit (insert nodes, then wire reverse edges so all targets exist) ---
        let mut newly_ready = Vec::new();
        let mut wiring: Vec<(String, Vec<String>)> = Vec::new();
        for s in specs {
            // a dep already Done does not count toward indegree (it will never re-fire `complete`).
            let indeg_remaining = s
                .deps
                .iter()
                .filter(|d| {
                    self.tasks
                        .get(d.as_str())
                        .map(|n| n.state != TaskState::Done)
                        .unwrap_or(true)
                })
                .count();
            let state = if indeg_remaining == 0 {
                newly_ready.push(s.id.clone());
                TaskState::Ready
            } else {
                TaskState::Pending
            };
            wiring.push((s.id.clone(), s.deps.clone()));
            self.tasks.insert(
                s.id.clone(),
                Node {
                    indegree_remaining: indeg_remaining,
                    fan_out: 0,
                    state,
                    attempts: 0,
                    result: None,
                    avoid_device: None,
                    pre_reviewed: false,
                    spec: s,
                },
            );
        }
        for (id, deps) in wiring {
            for d in deps {
                self.dependents
                    .entry(d.clone())
                    .or_default()
                    .push(id.clone());
                if let Some(n) = self.tasks.get_mut(&d) {
                    n.fan_out += 1;
                }
            }
        }
        Ok(newly_ready)
    }
}

/// Parse the planner's `{ "subtasks": [...] }` JSON into specs (shared by the initial plan + replans).
pub fn specs_from_plan_json(json: &str) -> Result<Vec<TaskSpec>> {
    #[derive(Deserialize)]
    struct PlanJson {
        subtasks: Vec<PlanTask>,
    }
    #[derive(Deserialize)]
    struct PlanTask {
        id: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        difficulty: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        depends_on: Vec<String>,
        #[serde(default)]
        files: Vec<String>,
    }
    let plan: PlanJson = serde_json::from_str(json)?;
    Ok(plan
        .subtasks
        .into_iter()
        .map(|t| {
            let subsplit = extract_subsplit(&t.description);
            TaskSpec {
                id: t.id,
                description: t.description,
                difficulty: match t.difficulty.as_deref() {
                    Some("hard") => Difficulty::Hard,
                    _ => Difficulty::Easy,
                },
                preferred_model: t.model,
                owned_files: t.files,
                deps: t.depends_on,
                subsplit,
            }
        })
        .collect())
}
