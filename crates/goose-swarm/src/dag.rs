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
            bail!("plan has a dependency cycle ({} of {} tasks orderable)", removed, self.tasks.len());
        }
        Ok(())
    }

    /// Build from the planner recipe's JSON output:
    /// `{ "subtasks": [ {id, description, difficulty?, model?, depends_on?, files?} ], "integration"? }`
    pub fn from_planner_json(json: &str) -> Result<Self> {
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
        let specs = plan
            .subtasks
            .into_iter()
            .map(|t| TaskSpec {
                id: t.id,
                description: t.description,
                difficulty: match t.difficulty.as_deref() {
                    Some("hard") => Difficulty::Hard,
                    _ => Difficulty::Easy,
                },
                preferred_model: t.model,
                owned_files: t.files,
                deps: t.depends_on,
            })
            .collect();
        Dag::from_specs(specs)
    }
}
