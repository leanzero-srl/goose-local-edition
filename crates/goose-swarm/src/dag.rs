//! The task DAG: built from a planner plan or directly from specs. Validates at load time
//! (unknown deps, cycles) and computes fan-out + initial ready set.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

pub type TaskId = String;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Easy,
    Hard,
}

/// One authoritative binder fact carried by a runtime acceptance review. The scheduler does not
/// infer these facts from prose: goose-cli compiles them from the frozen requirement/evidence/interface
/// registries, and the scheduler checks that the exact id and text survived into the worker payload.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ReplanAuthorityFact {
    pub id: String,
    pub text: String,
}

/// Proof that a dynamically admitted task is a read-only review of an already-bound acceptance slice.
/// Raw planner `TaskSpec`s have no receipt and are rejected by the runtime-replan admission gate.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ReplanAuthorityReceipt {
    pub source_task_id: TaskId,
    pub binding_task_id: TaskId,
    pub slice_id: String,
    pub source_files: Vec<String>,
    pub objective: String,
    pub requirements: Vec<ReplanAuthorityFact>,
    pub evidence: Vec<ReplanAuthorityFact>,
    pub interfaces: Vec<ReplanAuthorityFact>,
    pub acceptance_evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TaskSpec {
    pub id: TaskId,
    pub description: String,
    pub difficulty: Difficulty,
    /// Preferred LM Link model id (the device the planner suggests); the scheduler steers here
    /// first but may work-steal to another free device.
    #[serde(rename = "model", skip_serializing_if = "Option::is_none")]
    pub preferred_model: Option<String>,
    /// Files this task owns/edits; two tasks holding the same file never run concurrently.
    #[serde(rename = "files")]
    pub owned_files: Vec<String>,
    #[serde(rename = "depends_on")]
    pub deps: Vec<TaskId>,
    /// S3 i2 (LATENT): 2-4 top-level function/class names the detailer marked as independently
    /// implementable — parsed from the spec's trailing `SUBSPLIT:` line, re-anchored against the
    /// module's frozen stub at consumption. Nothing dispatches differently until a fill fan
    /// consumes it; empty for every task whose spec carries no such line.
    pub subsplit: Vec<String>,
    /// Present only on binder-compiled runtime acceptance reviews. Initial plan tasks and all legacy
    /// planner output carry `None`; that is deliberately insufficient for dynamic admission.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replan_authority: Option<ReplanAuthorityReceipt>,
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Ready,
    Claimed,
    Done,
    Salvaged,
    Failed,
}

impl TaskState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Salvaged | Self::Failed)
    }

    pub fn releases_dependents(self) -> bool {
        matches!(self, Self::Done | Self::Salvaged)
    }
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
    /// S3 fill fan: the subsplit expansion applies HERE — the one chokepoint every DAG build
    /// shares — so eligible hard modules become skeleton/fills/join uniformly (no-op with the
    /// gate off). Plan-time contract ANCHORING is impossible in the run's order (the DAG exists
    /// before the stub pass); the skeleton step's Terminal guard and the splice's
    /// SlotMissingInSkeleton refusals are the anchor, each honest and observable. The dynamic
    /// REPLANNER path deliberately does NOT expand (splice_specs takes specs directly):
    /// mid-run fills against a live tree are unproven, and a replanned module simply builds
    /// serially as before.
    /// F804: expansion is NO LONGER done here. The skeleton step builds from a module's frozen
    /// contract stub, and stubs do not exist at plan-load — expanding here manufactured
    /// skeleton:: tasks that could only refuse at execution (measured 3-for-3 across two runs).
    /// Callers that can answer "does this module's stub parse" expand via
    /// `from_planner_json_with` AFTER the contracts freeze; this bare form parses only.
    pub fn from_planner_json(json: &str) -> Result<Self> {
        Dag::from_specs(specs_from_plan_json(json)?)
    }

    /// F804/F809: parse AND expand, with each module's slots taken FROM ITS OWN STUB. The
    /// mapper returns the stub's definition names when the frozen contract stub parses (None
    /// otherwise) — so the fan fires exactly when it can succeed AND the slot names match the
    /// skeleton BY CONSTRUCTION. The first live exercise proved why: detailer free-text slots
    /// (`_VendorSyncHandler`) missed the stub-derived skeleton's names and every fill died
    /// SlotMissingInSkeleton. `fill_fan_enabled` still gates inside `expand_subsplits`.
    pub fn from_planner_json_with(
        json: &str,
        stub_slots: &dyn Fn(&str) -> Option<Vec<String>>,
    ) -> Result<Self> {
        let specs = specs_from_plan_json(json)?
            .into_iter()
            .map(|mut t| {
                if !t.subsplit.is_empty() {
                    match stub_slots(&t.id) {
                        Some(names) if names.len() >= 2 => {
                            t.subsplit = names.into_iter().take(6).collect();
                        }
                        _ => t.subsplit = Vec::new(),
                    }
                }
                t
            })
            .collect();
        Dag::from_specs(expand_subsplits(specs))
    }

    /// Splice additional specs into a LIVE dag at an idle point (the dynamic replanner). Validated
    /// exactly like `from_specs` so the safety net is identical: rejects ids that collide with
    /// existing tasks, deps on unknown OR failed tasks, intra-batch dup ids, and any cycle. On ANY
    /// error the dag is left UNCHANGED (validation runs before mutation). Returns the ids that became
    /// Ready (deps already Done) so the caller can enqueue them; Pending ones unlock via `complete`.
    pub fn splice_specs(&mut self, specs: Vec<TaskSpec>) -> Result<Vec<TaskId>> {
        self.splice_specs_inner(specs, None)
    }

    /// Atomically splice specs and make `before_task` depend on every added task. Runtime acceptance
    /// reviews use this to put their evidence in the integration sink's ordinary dependency context.
    /// Validation includes those reverse edges, so a bad proposal cannot leave a half-mutated DAG.
    pub fn splice_specs_before(
        &mut self,
        specs: Vec<TaskSpec>,
        before_task: &str,
    ) -> Result<Vec<TaskId>> {
        self.splice_specs_inner(specs, Some(before_task))
    }

    fn splice_specs_inner(
        &mut self,
        specs: Vec<TaskSpec>,
        before_task: Option<&str>,
    ) -> Result<Vec<TaskId>> {
        // --- validate, mutate nothing ---
        if let Some(target) = before_task {
            let node = self
                .tasks
                .get(target)
                .ok_or_else(|| anyhow::anyhow!("replan sink `{target}` is not in the live DAG"))?;
            if !matches!(node.state, TaskState::Pending | TaskState::Ready) {
                bail!("replan sink `{target}` is no longer dependency-mutable");
            }
        }
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
            if let Some(target) = before_task {
                for s in &specs {
                    *indeg.get_mut(target).expect("target validated above") += 1;
                    deps_of
                        .entry(s.id.clone())
                        .or_default()
                        .push(target.to_string());
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
        let added_ids: Vec<String> = specs.iter().map(|spec| spec.id.clone()).collect();
        for s in specs {
            // A dependency that already released its consumers does not count toward indegree; neither
            // accepted nor provisional completion will re-fire the scheduler transition.
            let indeg_remaining = s
                .deps
                .iter()
                .filter(|d| {
                    self.tasks
                        .get(d.as_str())
                        .map(|n| !n.state.releases_dependents())
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
        if let Some(target) = before_task {
            let target_node = self
                .tasks
                .get_mut(target)
                .expect("target validated before mutation");
            // Runtime review results are the evidence this edge exists to deliver. Put them first so
            // SharedContext's legacy high-fan-in backstop cannot truncate them behind every original
            // implementation dependency before the sink sees a byte.
            target_node
                .spec
                .deps
                .splice(0..0, added_ids.iter().cloned());
            target_node.indegree_remaining += added_ids.len();
            if target_node.indegree_remaining > 0 {
                target_node.state = TaskState::Pending;
            }
            for id in &added_ids {
                self.dependents
                    .entry(id.clone())
                    .or_default()
                    .push(target.to_string());
                self.tasks
                    .get_mut(id)
                    .expect("new task inserted above")
                    .fan_out += 1;
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
        #[serde(default)]
        subsplit: Vec<String>,
        #[serde(default)]
        replan_authority: Option<ReplanAuthorityReceipt>,
    }
    let plan: PlanJson = serde_json::from_str(json)?;
    Ok(plan
        .subtasks
        .into_iter()
        .map(|t| {
            let subsplit = if t.subsplit.is_empty() {
                extract_subsplit(&t.description)
            } else {
                t.subsplit
            };
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
                replan_authority: t.replan_authority,
            }
        })
        .collect())
}

/// The ONE resolution of GOOSE_SWARM_FILL_FAN (S3 i3, DAG-native): expand a hard module with a
/// contract-anchored subsplit into skeleton -> parallel fills -> deterministic join. Default
/// OFF — an arm, not a silent flip.
pub fn fill_fan_enabled() -> bool {
    std::env::var("GOOSE_SWARM_FILL_FAN")
        .map(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

/// S3's fill fan as a PLAN POST-PASS: each eligible task (Hard, one .py owned file, subsplit
/// with 2+ names) is rewritten into
///   skeleton::<M>            — deterministic dispatcher step (writes the contract skeleton),
///   fill::<M>::<slot> × N    — one filler per slot, depending on the skeleton; each owns the
///                              VIRTUAL name `<file>#<slot>` so held_files stays disjoint by
///                              construction while every filler works the same real file in a
///                              shadow,
///   join::<M>                — deterministic dispatcher step (splices verified fills),
///                              depending on every fill and owning the real file,
/// and every OTHER task that depended on <M> now depends on join::<M> — downstream work must
/// wait for the assembled module, not the skeleton. Non-eligible tasks pass through untouched;
/// with the gate off the input returns unchanged. Pure (gate applied by the public wrapper) so
/// the expansion shape is testable without an environment.
pub fn expand_subsplits(specs: Vec<TaskSpec>) -> Vec<TaskSpec> {
    if !fill_fan_enabled() {
        return specs;
    }
    expand_subsplits_inner(specs)
}

fn expand_subsplits_inner(specs: Vec<TaskSpec>) -> Vec<TaskSpec> {
    let eligible: std::collections::HashSet<String> = specs
        .iter()
        .filter(|t| {
            t.difficulty == Difficulty::Hard
                && t.owned_files.len() == 1
                && t.owned_files[0].ends_with(".py")
                && t.subsplit.len() >= 2
        })
        .map(|t| t.id.clone())
        .collect();
    if eligible.is_empty() {
        return specs;
    }
    let mut out = Vec::with_capacity(specs.len());
    for mut t in specs {
        if !eligible.contains(&t.id) {
            // Downstream rewiring: an edge onto an expanded module now waits for its JOIN —
            // depending on the skeleton would release consumers against an unfilled file.
            for d in t.deps.iter_mut() {
                if eligible.contains(d) {
                    *d = format!("join::{d}");
                }
            }
            out.push(t);
            continue;
        }
        let file = t.owned_files[0].clone();
        let skeleton_id = format!("skeleton::{}", t.id);
        let join_id = format!("join::{}", t.id);
        out.push(TaskSpec {
            id: skeleton_id.clone(),
            description: format!(
                "SKELETON STEP for [{}]: write the frozen-contract skeleton of `{file}` — \
                 every top-level def/class from the module's contract stub with \
                 `raise NotImplementedError` bodies. Deterministic; the dispatcher \
                 short-circuits this task without a model call.",
                t.id
            ),
            difficulty: Difficulty::Easy,
            preferred_model: t.preferred_model.clone(),
            owned_files: vec![file.clone()],
            deps: t.deps.clone(),
            subsplit: Vec::new(),
            replan_authority: None,
        });
        let mut fill_ids = Vec::new();
        for slot in &t.subsplit {
            let fid = format!("fill::{}::{slot}", t.id);
            fill_ids.push(fid.clone());
            out.push(TaskSpec {
                id: fid,
                description: format!(
                    "{}\n\nYOU IMPLEMENT ONLY `{slot}` in `{file}`. Every other def stays \
                     EXACTLY as the skeleton gives it — the merge REFUSES your whole \
                     contribution if anything outside `{slot}` changes.",
                    t.description
                ),
                difficulty: Difficulty::Hard,
                preferred_model: t.preferred_model.clone(),
                owned_files: vec![format!("{file}#{slot}")],
                deps: vec![skeleton_id.clone()],
                subsplit: Vec::new(),
                replan_authority: None,
            });
        }
        out.push(TaskSpec {
            id: join_id,
            description: format!(
                "JOIN STEP for [{}]: splice each verified fill's `{file}` back into the real \
                 tree by its owned slot (splice_functions; refusals fall back serially). \
                 Deterministic; the dispatcher short-circuits this task without a model call.",
                t.id
            ),
            difficulty: Difficulty::Easy,
            preferred_model: t.preferred_model,
            owned_files: vec![file],
            deps: fill_ids,
            subsplit: Vec::new(),
            replan_authority: None,
        });
    }
    out
}

#[cfg(test)]
mod expand_tests {
    use super::*;

    fn spec(id: &str, diff: Difficulty, files: &[&str], deps: &[&str], sub: &[&str]) -> TaskSpec {
        TaskSpec {
            id: id.into(),
            description: format!("{id} module"),
            difficulty: diff,
            preferred_model: None,
            owned_files: files.iter().map(|s| s.to_string()).collect(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            subsplit: sub.iter().map(|s| s.to_string()).collect(),
            replan_authority: None,
        }
    }

    #[test]
    fn an_eligible_module_expands_and_its_consumers_wait_for_the_join() {
        let specs = vec![
            spec("store", Difficulty::Easy, &["store.py"], &[], &[]),
            spec(
                "api",
                Difficulty::Hard,
                &["api.py"],
                &["store"],
                &["handle_get", "handle_post"],
            ),
            spec("cli", Difficulty::Easy, &["cli.py"], &["api"], &[]),
        ];
        let out = expand_subsplits_inner(specs);
        let ids: Vec<&str> = out.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "store",
                "skeleton::api",
                "fill::api::handle_get",
                "fill::api::handle_post",
                "join::api",
                "cli"
            ]
        );
        // Ownership stays DISJOINT: fills own virtual names, never the real file.
        let mut owned: Vec<&str> = out
            .iter()
            .flat_map(|t| t.owned_files.iter())
            .map(|s| s.as_str())
            .collect();
        let n = owned.len();
        owned.sort();
        owned.dedup();
        // skeleton::api and join::api both own api.py by design — they are serial by deps, so
        // the ONE allowed duplicate is the real file between those two.
        assert_eq!(
            n - owned.len(),
            1,
            "only the skeleton/join pair shares the real file"
        );
        // Wiring: fills depend on the skeleton; the join on every fill; cli on the JOIN.
        let get = |id: &str| out.iter().find(|t| t.id == id).unwrap();
        assert_eq!(get("fill::api::handle_get").deps, ["skeleton::api"]);
        assert_eq!(
            get("join::api").deps,
            ["fill::api::handle_get", "fill::api::handle_post"]
        );
        assert_eq!(get("cli").deps, ["join::api"]);
        assert_eq!(
            get("skeleton::api").deps,
            ["store"],
            "the skeleton inherits the module's deps"
        );
        // Fill prompts carry the one-slot discipline; virtual ownership is file#slot.
        assert!(get("fill::api::handle_post")
            .description
            .contains("ONLY `handle_post`"));
        assert_eq!(
            get("fill::api::handle_get").owned_files,
            ["api.py#handle_get"]
        );
    }

    #[test]
    fn non_eligible_shapes_pass_through_byte_identical() {
        let cases = vec![
            spec("easy", Difficulty::Easy, &["a.py"], &[], &["x", "y"]),
            spec(
                "two-files",
                Difficulty::Hard,
                &["a.py", "b.py"],
                &[],
                &["x", "y"],
            ),
            spec("one-slot", Difficulty::Hard, &["a.py"], &[], &["x"]),
            spec("not-py", Difficulty::Hard, &["a.rs"], &[], &["x", "y"]),
            spec("no-subsplit", Difficulty::Hard, &["a.py"], &[], &[]),
        ];
        let before: Vec<String> = cases.iter().map(|t| format!("{t:?}")).collect();
        let out = expand_subsplits_inner(cases);
        let after: Vec<String> = out.iter().map(|t| format!("{t:?}")).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn deferred_expansion_keeps_only_parseable_stub_modules() {
        // F804: from_planner_json no longer expands; from_planner_json_with expands only where
        // the stub predicate says the module is buildable.
        std::env::set_var("GOOSE_SWARM_FILL_FAN", "1");
        let plan = r#"{"subtasks":[
            {"id":"api","description":"build api\nSUBSPLIT: handle_get, handle_post","difficulty":"hard","files":["api.py"],"depends_on":[]},
            {"id":"store","description":"build store\nSUBSPLIT: upsert, query","difficulty":"hard","files":["store.py"],"depends_on":[]}
        ]}"#;
        let bare = Dag::from_planner_json(plan).unwrap();
        assert!(
            bare.tasks.keys().all(|k| !k.starts_with("skeleton::")),
            "bare parse must not expand: {:?}",
            bare.tasks.keys().collect::<Vec<_>>()
        );
        let gated = Dag::from_planner_json_with(plan, &|m| {
            (m == "api").then(|| vec!["handle_get".to_string(), "handle_post".to_string()])
        })
        .unwrap();
        assert!(
            gated.tasks.keys().any(|k| k == "skeleton::api"),
            "api's stub parses -> its fan fires: {:?}",
            gated.tasks.keys().collect::<Vec<_>>()
        );
        assert!(
            gated.tasks.contains_key("store")
                && !gated.tasks.keys().any(|k| k == "skeleton::store"),
            "store's stub does not parse -> ordinary serial task"
        );
        std::env::remove_var("GOOSE_SWARM_FILL_FAN");
    }
}
