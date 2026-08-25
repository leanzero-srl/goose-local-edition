use crate::context::SharedContext;
use crate::dag::{Dag, TaskId, TaskSpec, TaskState};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

const SCHEMA_VERSION: u32 = 1;
const GENESIS_HASH: &str = "genesis";
const CHECKPOINT_DIRECTORY: &str = "scheduler-checkpoint-v1";
const WAL_FILE: &str = "tasks.wal.jsonl";
const HEAD_FILE: &str = "tasks.head.json";
const LOCK_FILE: &str = "control.lock";
const ARTIFACT_DIRECTORY: &str = "artifacts";
const ARTIFACT_OBJECT_DIRECTORY: &str = "sha256";

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct SchedulerCheckpointError {
    detail: String,
}

impl SchedulerCheckpointError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    fn io(action: &str, error: std::io::Error) -> Self {
        Self::new(format!("{action}: {error}"))
    }
}

impl std::fmt::Display for SchedulerCheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for SchedulerCheckpointError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OwnedArtifactCheckpoint {
    path: String,
    sha256: String,
    bytes: u64,
    mode: u32,
}

#[derive(Debug)]
struct CapturedArtifact {
    checkpoint: OwnedArtifactCheckpoint,
    contents: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DependencyCheckpoint {
    task_id: TaskId,
    task_spec_digest: String,
    completion_order: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckpointTransition {
    TaskDone,
    TaskInvalidated,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WalMaterial {
    schema_version: u32,
    sequence: u64,
    previous_hash: String,
    working_root: PathBuf,
    transition: CheckpointTransition,
    task_id: TaskId,
    task_spec_digest: String,
    output: String,
    completion_order: u64,
    attempts: u32,
    artifacts: Vec<OwnedArtifactCheckpoint>,
    dependency_checkpoints: Vec<DependencyCheckpoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WalRecord {
    #[serde(flatten)]
    material: WalMaterial,
    entry_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeadMaterial {
    schema_version: u32,
    working_root: PathBuf,
    next_sequence: u64,
    entry_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HeadRecord {
    #[serde(flatten)]
    material: HeadMaterial,
    checkpoint_hash: String,
}

#[derive(Debug)]
struct ReplayState {
    next_sequence: u64,
    previous_hash: String,
    expected_len: u64,
    records: Vec<WalMaterial>,
    prefixes: Vec<(u64, String)>,
}

struct StoreState {
    wal: File,
    next_sequence: u64,
    previous_hash: String,
    expected_len: u64,
    records: Vec<WalMaterial>,
    active: BTreeMap<TaskId, DependencyCheckpoint>,
    poisoned: Option<String>,
}

pub struct SchedulerCheckpointStore {
    root: PathBuf,
    artifact_object_directory: PathBuf,
    wal_path: PathBuf,
    head_path: PathBuf,
    _lock: File,
    state: Mutex<StoreState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulerCheckpointReceipt {
    pub sequence: u64,
    pub completion_order: u64,
    pub artifact_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulerCompletedTaskEvidence {
    pub task_id: TaskId,
    pub output: String,
    pub attempts: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SchedulerDagResealReceipt {
    pub tasks: Vec<TaskId>,
    pub first_sequence: u64,
    pub next_sequence: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SchedulerRestoreSummary {
    pub restored: Vec<TaskId>,
    pub invalidated: Vec<TaskId>,
}

impl SchedulerCheckpointStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SchedulerCheckpointError> {
        let root = std::fs::canonicalize(root.as_ref()).map_err(|error| {
            SchedulerCheckpointError::io("cannot canonicalize scheduler checkpoint root", error)
        })?;
        if !root.is_dir() {
            return Err(SchedulerCheckpointError::new(format!(
                "scheduler checkpoint root is not a directory: {}",
                root.display()
            )));
        }

        let swarm_directory = root.join(".swarm");
        let directory = swarm_directory.join(CHECKPOINT_DIRECTORY);
        ensure_control_directory(&root, &swarm_directory, "swarm state directory")?;
        ensure_control_directory(
            &swarm_directory,
            &directory,
            "scheduler checkpoint directory",
        )?;
        let artifact_directory = directory.join(ARTIFACT_DIRECTORY);
        ensure_control_directory(
            &directory,
            &artifact_directory,
            "scheduler checkpoint artifact directory",
        )?;
        let artifact_object_directory = artifact_directory.join(ARTIFACT_OBJECT_DIRECTORY);
        ensure_control_directory(
            &artifact_directory,
            &artifact_object_directory,
            "scheduler checkpoint artifact object directory",
        )?;

        let lock_path = directory.join(LOCK_FILE);
        reject_symlink_if_present(&lock_path, "scheduler checkpoint lock")?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                SchedulerCheckpointError::io("cannot open scheduler checkpoint lock", error)
            })?;
        FileExt::try_lock_exclusive(&lock).map_err(|error| {
            SchedulerCheckpointError::io("scheduler checkpoint is already open", error)
        })?;
        verify_linked_file(&lock_path, &lock, "scheduler checkpoint lock")?;

        let wal_path = directory.join(WAL_FILE);
        let head_path = directory.join(HEAD_FILE);
        reject_symlink_if_present(&wal_path, "scheduler checkpoint WAL")?;
        reject_symlink_if_present(&head_path, "scheduler checkpoint head")?;
        let wal_created = !wal_path.exists();
        let mut wal = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&wal_path)
            .map_err(|error| {
                SchedulerCheckpointError::io("cannot open scheduler checkpoint WAL", error)
            })?;
        if wal_created {
            wal.sync_all().map_err(|error| {
                SchedulerCheckpointError::io("cannot sync new scheduler checkpoint WAL", error)
            })?;
            sync_directory(&directory)?;
        }

        let replay = replay_wal(&mut wal, &root)?;
        let head = match read_head(&head_path, &root) {
            Ok(head) => head,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if replay.next_sequence != 0 {
                    return Err(SchedulerCheckpointError::new(
                        "scheduler checkpoint head is missing for a non-empty WAL",
                    ));
                }
                write_head_atomic(&head_path, &root, 0, GENESIS_HASH)?;
                HeadMaterial {
                    schema_version: SCHEMA_VERSION,
                    working_root: root.clone(),
                    next_sequence: 0,
                    entry_hash: GENESIS_HASH.to_string(),
                }
            }
            Err(error) => {
                return Err(SchedulerCheckpointError::io(
                    "cannot read scheduler checkpoint head",
                    error,
                ));
            }
        };
        if !replay
            .prefixes
            .iter()
            .any(|prefix| prefix == &(head.next_sequence, head.entry_hash.clone()))
        {
            return Err(SchedulerCheckpointError::new(
                "scheduler checkpoint head is not a valid WAL prefix",
            ));
        }
        if head.next_sequence != replay.next_sequence || head.entry_hash != replay.previous_hash {
            write_head_atomic(
                &head_path,
                &root,
                replay.next_sequence,
                &replay.previous_hash,
            )?;
        }

        Ok(Self {
            root,
            artifact_object_directory,
            wal_path,
            head_path,
            _lock: lock,
            state: Mutex::new(StoreState {
                wal,
                next_sequence: replay.next_sequence,
                previous_hash: replay.previous_hash,
                expected_len: replay.expected_len,
                records: replay.records,
                active: BTreeMap::new(),
                poisoned: None,
            }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn persist_done(
        &self,
        spec: &TaskSpec,
        output: &str,
        attempts: u32,
    ) -> Result<SchedulerCheckpointReceipt, SchedulerCheckpointError> {
        if attempts == 0 {
            return Err(SchedulerCheckpointError::new(
                "cannot checkpoint a completed task with zero attempts",
            ));
        }
        validate_task_spec_for_checkpoint(spec)?;
        let captured = capture_owned_artifacts(&self.root, &spec.owned_files)?;
        let mut state = lock(&self.state);
        if let Some(error) = &state.poisoned {
            return Err(SchedulerCheckpointError::new(format!(
                "scheduler checkpoint is latched after a prior durability failure: {error}"
            )));
        }
        verify_live_file(&self.wal_path, &state.wal, state.expected_len)?;
        self.persist_artifact_objects(&captured)?;
        let dependency_checkpoints = spec
            .deps
            .iter()
            .map(|task_id| {
                state.active.get(task_id).cloned().ok_or_else(|| {
                    SchedulerCheckpointError::new(format!(
                        "task {:?} dependency {task_id:?} has no restorable checkpoint",
                        spec.id
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let material = WalMaterial {
            schema_version: SCHEMA_VERSION,
            sequence: state.next_sequence,
            previous_hash: state.previous_hash.clone(),
            working_root: self.root.clone(),
            transition: CheckpointTransition::TaskDone,
            task_id: spec.id.clone(),
            task_spec_digest: scheduler_task_spec_digest(spec),
            output: output.to_string(),
            completion_order: state.next_sequence,
            attempts,
            artifacts: captured
                .iter()
                .map(|artifact| artifact.checkpoint.clone())
                .collect(),
            dependency_checkpoints,
        };
        self.append_material(&mut state, material.clone())?;
        state
            .active
            .insert(spec.id.clone(), dependency_checkpoint(&material));

        Ok(SchedulerCheckpointReceipt {
            sequence: material.sequence,
            completion_order: material.completion_order,
            artifact_count: material.artifacts.len(),
        })
    }

    pub fn reseal_completed_dag(
        &self,
        specs: &[TaskSpec],
        evidence: &[SchedulerCompletedTaskEvidence],
    ) -> Result<SchedulerDagResealReceipt, SchedulerCheckpointError> {
        let ordered = deterministic_topological_specs(specs)?;
        let evidence = exact_completion_evidence(&ordered, evidence)?;
        let captured = ordered
            .iter()
            .map(|spec| {
                validate_task_spec_for_checkpoint(spec)?;
                Ok((
                    spec.id.clone(),
                    capture_owned_artifacts(&self.root, &spec.owned_files)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        let mut state = lock(&self.state);
        if let Some(error) = &state.poisoned {
            return Err(SchedulerCheckpointError::new(format!(
                "scheduler checkpoint is latched after a prior durability failure: {error}"
            )));
        }
        verify_live_file(&self.wal_path, &state.wal, state.expected_len)?;
        for artifacts in captured.values() {
            self.persist_artifact_objects(artifacts)?;
        }

        let first_sequence = state.next_sequence;
        let mut tasks = Vec::with_capacity(ordered.len());
        for spec in ordered {
            let completion = evidence
                .get(&spec.id)
                .expect("completion evidence was preflighted for every task");
            let dependency_checkpoints = spec
                .deps
                .iter()
                .map(|task_id| {
                    state.active.get(task_id).cloned().ok_or_else(|| {
                        SchedulerCheckpointError::new(format!(
                            "task {:?} dependency {task_id:?} has no fresh reseal generation",
                            spec.id
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let material = WalMaterial {
                schema_version: SCHEMA_VERSION,
                sequence: state.next_sequence,
                previous_hash: state.previous_hash.clone(),
                working_root: self.root.clone(),
                transition: CheckpointTransition::TaskDone,
                task_id: spec.id.clone(),
                task_spec_digest: scheduler_task_spec_digest(spec),
                output: completion.output.clone(),
                completion_order: state.next_sequence,
                attempts: completion.attempts,
                artifacts: captured[&spec.id]
                    .iter()
                    .map(|artifact| artifact.checkpoint.clone())
                    .collect(),
                dependency_checkpoints,
            };
            self.append_material(&mut state, material.clone())?;
            state
                .active
                .insert(spec.id.clone(), dependency_checkpoint(&material));
            tasks.push(spec.id.clone());
        }

        Ok(SchedulerDagResealReceipt {
            tasks,
            first_sequence,
            next_sequence: state.next_sequence,
        })
    }

    fn persist_artifact_objects(
        &self,
        artifacts: &[CapturedArtifact],
    ) -> Result<(), SchedulerCheckpointError> {
        for artifact in artifacts {
            persist_artifact_object(&self.artifact_object_directory, artifact)?;
        }
        Ok(())
    }

    pub fn can_persist_done(&self, spec: &TaskSpec) -> Result<bool, SchedulerCheckpointError> {
        validate_task_spec_for_checkpoint(spec)?;
        let state = lock(&self.state);
        if let Some(error) = &state.poisoned {
            return Err(SchedulerCheckpointError::new(format!(
                "scheduler checkpoint is latched after a prior durability failure: {error}"
            )));
        }
        verify_live_file(&self.wal_path, &state.wal, state.expected_len)?;
        Ok(spec
            .deps
            .iter()
            .all(|dependency| state.active.contains_key(dependency)))
    }

    fn append_material(
        &self,
        state: &mut StoreState,
        material: WalMaterial,
    ) -> Result<(), SchedulerCheckpointError> {
        validate_wal_material(&material)?;
        let entry_hash = hash_serializable(&material)?;
        let record = WalRecord {
            material: material.clone(),
            entry_hash: entry_hash.clone(),
        };
        let mut encoded = serde_json::to_vec(&record).map_err(|error| {
            SchedulerCheckpointError::new(format!(
                "cannot encode scheduler checkpoint WAL record: {error}"
            ))
        })?;
        encoded.push(b'\n');
        let expected_len = state
            .expected_len
            .checked_add(encoded.len() as u64)
            .ok_or_else(|| SchedulerCheckpointError::new("checkpoint WAL length overflowed"))?;
        let next_sequence = state
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| SchedulerCheckpointError::new("checkpoint WAL sequence overflowed"))?;

        let append_result = (|| {
            state.wal.write_all(&encoded).map_err(|error| {
                SchedulerCheckpointError::io("cannot append scheduler checkpoint WAL", error)
            })?;
            state.wal.sync_all().map_err(|error| {
                SchedulerCheckpointError::io("cannot sync scheduler checkpoint WAL", error)
            })?;
            Ok::<(), SchedulerCheckpointError>(())
        })();
        if let Err(error) = append_result {
            state.poisoned = Some(error.to_string());
            return Err(error);
        }

        state.expected_len = expected_len;
        state.next_sequence = next_sequence;
        state.previous_hash = entry_hash;
        state.records.push(material);

        if let Err(_error) = write_head_atomic(
            &self.head_path,
            &self.root,
            state.next_sequence,
            &state.previous_hash,
        ) {
            // The synced hash-linked WAL is authoritative. The head is a recoverable index cache,
            // and reopening promotes it to the verified WAL tip before restoration.
        }
        Ok(())
    }

    pub fn restore_into(
        &self,
        dag: &mut Dag,
        context: &mut SharedContext,
    ) -> Result<SchedulerRestoreSummary, SchedulerCheckpointError> {
        let mut state = lock(&self.state);
        if let Some(error) = &state.poisoned {
            return Err(SchedulerCheckpointError::new(format!(
                "scheduler checkpoint cannot restore after a durability failure: {error}"
            )));
        }
        verify_live_file(&self.wal_path, &state.wal, state.expected_len)?;
        let records = state.records.clone();
        let mut latest = BTreeMap::<TaskId, WalMaterial>::new();
        for record in records {
            latest.insert(record.task_id.clone(), record);
        }

        let mut valid = BTreeMap::<TaskId, WalMaterial>::new();
        let mut invalidated = BTreeSet::<TaskId>::new();
        let mut needs_tombstone = BTreeSet::<TaskId>::new();
        for (task_id, node) in &dag.tasks {
            let Some(record) = latest.get(task_id) else {
                continue;
            };
            let directly_valid = record.transition == CheckpointTransition::TaskDone
                && record.task_spec_digest == scheduler_task_spec_digest(&node.spec)
                && record.dependency_checkpoints.len() == node.spec.deps.len()
                && record
                    .dependency_checkpoints
                    .iter()
                    .zip(&node.spec.deps)
                    .all(|(checkpoint, dependency)| checkpoint.task_id == *dependency)
                && recorded_artifact_paths_match(&node.spec, &record.artifacts)
                && artifact_objects_available(&self.artifact_object_directory, &record.artifacts);
            if directly_valid {
                valid.insert(task_id.clone(), record.clone());
            } else {
                invalidated.insert(task_id.clone());
                if record.transition == CheckpointTransition::TaskDone {
                    needs_tombstone.insert(task_id.clone());
                }
            }
        }

        loop {
            let invalid_downstream = valid
                .iter()
                .filter_map(|(task_id, record)| {
                    let node = &dag.tasks[task_id];
                    node.spec
                        .deps
                        .iter()
                        .zip(&record.dependency_checkpoints)
                        .any(|(dependency, checkpoint)| {
                            valid.get(dependency).is_none_or(|dependency_record| {
                                checkpoint != &dependency_checkpoint(dependency_record)
                            })
                        })
                        .then_some(task_id.clone())
                })
                .collect::<Vec<_>>();
            if invalid_downstream.is_empty() {
                break;
            }
            for task_id in invalid_downstream {
                valid.remove(&task_id);
                invalidated.insert(task_id.clone());
                needs_tombstone.insert(task_id);
            }
        }

        for task_id in &needs_tombstone {
            let spec = &dag
                .tasks
                .get(task_id)
                .expect("invalidated checkpoint task came from this DAG")
                .spec;
            let material = WalMaterial {
                schema_version: SCHEMA_VERSION,
                sequence: state.next_sequence,
                previous_hash: state.previous_hash.clone(),
                working_root: self.root.clone(),
                transition: CheckpointTransition::TaskInvalidated,
                task_id: task_id.clone(),
                task_spec_digest: scheduler_task_spec_digest(spec),
                output: String::new(),
                completion_order: state.next_sequence,
                attempts: 0,
                artifacts: Vec::new(),
                dependency_checkpoints: Vec::new(),
            };
            self.append_material(&mut state, material)?;
            state.active.remove(task_id);
        }

        let mut artifact_restore_records = valid.values().collect::<Vec<_>>();
        artifact_restore_records.sort_by_key(|record| record.completion_order);
        for record in artifact_restore_records {
            restore_artifact_set(
                &self.root,
                &self.artifact_object_directory,
                &record.artifacts,
            )?;
        }

        context.clear_for_restore();
        for node in dag.tasks.values_mut() {
            node.state = TaskState::Pending;
            node.attempts = 0;
            node.result = None;
            node.avoid_device = None;
            node.pre_reviewed = false;
        }

        let mut restored_records = valid.into_values().collect::<Vec<_>>();
        restored_records.sort_by_key(|record| record.completion_order);
        state.active = restored_records
            .iter()
            .map(|record| (record.task_id.clone(), dependency_checkpoint(record)))
            .collect();
        let mut restored = Vec::with_capacity(restored_records.len());
        for record in restored_records {
            let node = dag
                .tasks
                .get_mut(&record.task_id)
                .expect("checkpoint candidates came from this DAG");
            node.state = TaskState::Done;
            node.attempts = record.attempts;
            node.result = Some(record.output.clone());
            context.merge(&record.task_id, record.output);
            restored.push(record.task_id);
        }

        let restored_set = restored.iter().cloned().collect::<HashSet<_>>();
        let task_ids = dag.tasks.keys().cloned().collect::<Vec<_>>();
        for task_id in task_ids {
            let remaining = dag.tasks[&task_id]
                .spec
                .deps
                .iter()
                .filter(|dependency| {
                    dag.tasks
                        .get(*dependency)
                        .is_none_or(|node| !node.state.releases_dependents())
                })
                .count();
            let node = dag
                .tasks
                .get_mut(&task_id)
                .expect("task id came from this DAG");
            node.indegree_remaining = remaining;
            if !restored_set.contains(&task_id) {
                node.state = if remaining == 0 {
                    TaskState::Ready
                } else {
                    TaskState::Pending
                };
            }
        }

        Ok(SchedulerRestoreSummary {
            restored,
            invalidated: invalidated.into_iter().collect(),
        })
    }
}

pub fn scheduler_task_spec_digest(spec: &TaskSpec) -> String {
    let encoded = serde_json::to_vec(spec).expect("task specs are JSON serializable");
    sha256_digest(&encoded)
}

fn capture_owned_artifacts(
    root: &Path,
    owned_files: &[String],
) -> Result<Vec<CapturedArtifact>, SchedulerCheckpointError> {
    let mut seen = HashSet::new();
    let mut artifacts = Vec::with_capacity(owned_files.len());
    for relative in owned_files {
        if !seen.insert(relative.clone()) {
            return Err(SchedulerCheckpointError::new(format!(
                "task owns duplicate checkpoint path {relative:?}"
            )));
        }
        artifacts.push(capture_artifact(root, relative)?);
    }
    artifacts.sort_by(|left, right| left.checkpoint.path.cmp(&right.checkpoint.path));
    Ok(artifacts)
}

fn deterministic_topological_specs(
    specs: &[TaskSpec],
) -> Result<Vec<&TaskSpec>, SchedulerCheckpointError> {
    let dag = Dag::from_specs(specs.to_vec()).map_err(|error| {
        SchedulerCheckpointError::new(format!("cannot reseal invalid task DAG: {error}"))
    })?;
    let mut indegrees = dag
        .tasks
        .iter()
        .map(|(task_id, node)| (task_id.clone(), node.spec.deps.len()))
        .collect::<BTreeMap<_, _>>();
    let mut ready = indegrees
        .iter()
        .filter_map(|(task_id, indegree)| (*indegree == 0).then_some(task_id.clone()))
        .collect::<BTreeSet<_>>();
    let by_id = specs
        .iter()
        .map(|spec| (spec.id.clone(), spec))
        .collect::<BTreeMap<_, _>>();
    let mut ordered = Vec::with_capacity(specs.len());
    while let Some(task_id) = ready.pop_first() {
        ordered.push(by_id[&task_id]);
        if let Some(dependents) = dag.dependents.get(&task_id) {
            let mut dependents = dependents.clone();
            dependents.sort();
            for dependent in dependents {
                let indegree = indegrees
                    .get_mut(&dependent)
                    .expect("validated DAG dependent has an indegree");
                *indegree -= 1;
                if *indegree == 0 {
                    ready.insert(dependent);
                }
            }
        }
    }
    if ordered.len() != specs.len() {
        return Err(SchedulerCheckpointError::new(
            "cannot reseal task DAG without a complete topological order",
        ));
    }
    Ok(ordered)
}

fn exact_completion_evidence<'a>(
    specs: &[&TaskSpec],
    evidence: &'a [SchedulerCompletedTaskEvidence],
) -> Result<BTreeMap<TaskId, &'a SchedulerCompletedTaskEvidence>, SchedulerCheckpointError> {
    let expected = specs
        .iter()
        .map(|spec| spec.id.clone())
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeMap::new();
    for completion in evidence {
        if completion.task_id.trim().is_empty()
            || completion.attempts == 0
            || actual
                .insert(completion.task_id.clone(), completion)
                .is_some()
        {
            return Err(SchedulerCheckpointError::new(
                "completed DAG reseal evidence has an empty id, duplicate id, or zero attempts",
            ));
        }
    }
    if actual.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(SchedulerCheckpointError::new(
            "completed DAG reseal evidence must cover every task exactly once and no others",
        ));
    }
    Ok(actual)
}

fn dependency_checkpoint(record: &WalMaterial) -> DependencyCheckpoint {
    DependencyCheckpoint {
        task_id: record.task_id.clone(),
        task_spec_digest: record.task_spec_digest.clone(),
        completion_order: record.completion_order,
    }
}

fn validate_task_spec_for_checkpoint(spec: &TaskSpec) -> Result<(), SchedulerCheckpointError> {
    if spec.id.trim().is_empty() {
        return Err(SchedulerCheckpointError::new(
            "cannot checkpoint a task with an empty id",
        ));
    }
    let mut dependencies = HashSet::new();
    if spec
        .deps
        .iter()
        .any(|dependency| dependency.trim().is_empty() || !dependencies.insert(dependency))
    {
        return Err(SchedulerCheckpointError::new(format!(
            "task {:?} has an empty or duplicate dependency id",
            spec.id
        )));
    }
    Ok(())
}

fn capture_artifact(
    root: &Path,
    relative: &str,
) -> Result<CapturedArtifact, SchedulerCheckpointError> {
    validate_owned_file_path(relative)?;
    let path = root.join(relative);
    let mut file = open_artifact_no_follow(&path).map_err(|error| {
        SchedulerCheckpointError::io(
            &format!("cannot open completed task artifact {relative:?}"),
            error,
        )
    })?;
    let metadata = file.metadata().map_err(|error| {
        SchedulerCheckpointError::io(
            &format!("cannot inspect completed task artifact {relative:?}"),
            error,
        )
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(SchedulerCheckpointError::new(format!(
            "completed task artifact is not a regular file: {relative:?}"
        )));
    }
    let opened_path = opened_artifact_path(&file).map_err(|error| {
        SchedulerCheckpointError::io(
            &format!("cannot resolve opened task artifact {relative:?}"),
            error,
        )
    })?;
    let expected_path = root.join(relative);
    if opened_path != expected_path || !opened_path.starts_with(root) {
        return Err(SchedulerCheckpointError::new(format!(
            "task artifact resolves outside its exact owned path: {relative:?}"
        )));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        SchedulerCheckpointError::io(
            &format!("cannot read completed task artifact {relative:?}"),
            error,
        )
    })?;
    Ok(CapturedArtifact {
        checkpoint: OwnedArtifactCheckpoint {
            path: relative.to_string(),
            sha256: sha256_digest(&bytes),
            bytes: bytes.len() as u64,
            mode: artifact_mode(&metadata),
        },
        contents: bytes,
    })
}

fn artifact_object_path(
    directory: &Path,
    sha256: &str,
) -> Result<PathBuf, SchedulerCheckpointError> {
    if !canonical_digest(sha256) {
        return Err(SchedulerCheckpointError::new(
            "artifact object digest is not canonical",
        ));
    }
    Ok(directory.join(&sha256[7..]))
}

fn persist_artifact_object(
    directory: &Path,
    artifact: &CapturedArtifact,
) -> Result<(), SchedulerCheckpointError> {
    if sha256_digest(&artifact.contents) != artifact.checkpoint.sha256
        || artifact.contents.len() as u64 != artifact.checkpoint.bytes
    {
        return Err(SchedulerCheckpointError::new(
            "captured artifact bytes do not match their content address",
        ));
    }
    let destination = artifact_object_path(directory, &artifact.checkpoint.sha256)?;
    match read_artifact_object(directory, &artifact.checkpoint) {
        Ok(_) => return Ok(()),
        Err(error) if !destination.exists() => {
            let _ = error;
        }
        Err(error) => {
            return Err(SchedulerCheckpointError::new(format!(
                "scheduler checkpoint artifact object is corrupt: {error}"
            )));
        }
    }

    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(".artifact.{}.{}.tmp", std::process::id(), sequence));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| {
            SchedulerCheckpointError::io("cannot create checkpoint artifact object temp", error)
        })?;
    let result = (|| {
        file.write_all(&artifact.contents).map_err(|error| {
            SchedulerCheckpointError::io("cannot write checkpoint artifact object temp", error)
        })?;
        file.sync_all().map_err(|error| {
            SchedulerCheckpointError::io("cannot sync checkpoint artifact object temp", error)
        })?;
        drop(file);
        match std::fs::hard_link(&temporary, &destination) {
            Ok(()) => sync_directory(directory)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                read_artifact_object(directory, &artifact.checkpoint)?;
            }
            Err(error) => {
                return Err(SchedulerCheckpointError::io(
                    "cannot install checkpoint artifact object",
                    error,
                ));
            }
        }
        std::fs::remove_file(&temporary).map_err(|error| {
            SchedulerCheckpointError::io("cannot remove checkpoint artifact object temp", error)
        })?;
        sync_directory(directory)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn read_artifact_object(
    directory: &Path,
    artifact: &OwnedArtifactCheckpoint,
) -> Result<Vec<u8>, SchedulerCheckpointError> {
    let path = artifact_object_path(directory, &artifact.sha256)?;
    let mut file = open_artifact_no_follow(&path).map_err(|error| {
        SchedulerCheckpointError::io("cannot open checkpoint artifact object", error)
    })?;
    let metadata = file.metadata().map_err(|error| {
        SchedulerCheckpointError::io("cannot inspect checkpoint artifact object", error)
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(SchedulerCheckpointError::new(
            "checkpoint artifact object is not a regular file",
        ));
    }
    let opened = opened_artifact_path(&file).map_err(|error| {
        SchedulerCheckpointError::io("cannot resolve checkpoint artifact object", error)
    })?;
    if opened != path || !opened.starts_with(directory) {
        return Err(SchedulerCheckpointError::new(
            "checkpoint artifact object escaped its content-addressed directory",
        ));
    }
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).map_err(|error| {
        SchedulerCheckpointError::io("cannot read checkpoint artifact object", error)
    })?;
    if contents.len() as u64 != artifact.bytes || sha256_digest(&contents) != artifact.sha256 {
        return Err(SchedulerCheckpointError::new(
            "checkpoint artifact object bytes do not match their content address",
        ));
    }
    Ok(contents)
}

fn artifact_objects_available(directory: &Path, artifacts: &[OwnedArtifactCheckpoint]) -> bool {
    artifacts
        .iter()
        .all(|artifact| read_artifact_object(directory, artifact).is_ok())
}

fn restore_artifact_set(
    root: &Path,
    directory: &Path,
    artifacts: &[OwnedArtifactCheckpoint],
) -> Result<(), SchedulerCheckpointError> {
    let prepared = artifacts
        .iter()
        .map(|artifact| {
            validate_owned_file_path(&artifact.path)?;
            Ok((artifact, read_artifact_object(directory, artifact)?))
        })
        .collect::<Result<Vec<_>, SchedulerCheckpointError>>()?;
    for (artifact, contents) in prepared {
        if capture_artifact(root, &artifact.path)
            .is_ok_and(|current| current.checkpoint == *artifact)
        {
            continue;
        }
        restore_artifact_atomic(root, artifact, &contents)?;
    }
    Ok(())
}

fn restore_artifact_atomic(
    root: &Path,
    artifact: &OwnedArtifactCheckpoint,
    contents: &[u8],
) -> Result<(), SchedulerCheckpointError> {
    let path = root.join(&artifact.path);
    let parent = path.parent().ok_or_else(|| {
        SchedulerCheckpointError::new("checkpoint artifact destination has no parent")
    })?;
    ensure_artifact_parent(root, parent)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(SchedulerCheckpointError::new(format!(
                "checkpoint artifact destination is not a replaceable regular file: {:?}",
                artifact.path
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(SchedulerCheckpointError::io(
                "cannot inspect checkpoint artifact destination",
                error,
            ));
        }
    }

    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".checkpoint-restore.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| {
            SchedulerCheckpointError::io("cannot create checkpoint restore temp", error)
        })?;
    let result = (|| {
        file.write_all(contents).map_err(|error| {
            SchedulerCheckpointError::io("cannot write checkpoint restore temp", error)
        })?;
        set_artifact_mode(&file, artifact.mode)?;
        file.sync_all().map_err(|error| {
            SchedulerCheckpointError::io("cannot sync checkpoint restore temp", error)
        })?;
        drop(file);
        std::fs::rename(&temporary, &path).map_err(|error| {
            SchedulerCheckpointError::io("cannot install restored checkpoint artifact", error)
        })?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result?;
    let restored = capture_artifact(root, &artifact.path)?;
    if restored.checkpoint != *artifact {
        return Err(SchedulerCheckpointError::new(
            "restored checkpoint artifact failed its exact hash, length, or mode check",
        ));
    }
    Ok(())
}

fn ensure_artifact_parent(root: &Path, parent: &Path) -> Result<(), SchedulerCheckpointError> {
    let relative = parent.strip_prefix(root).map_err(|_| {
        SchedulerCheckpointError::new("checkpoint artifact parent escapes the working root")
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(SchedulerCheckpointError::new(
                "checkpoint artifact parent is not a normal relative path",
            ));
        }
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(SchedulerCheckpointError::new(
                    "checkpoint artifact parent contains a non-directory or symlink",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|error| {
                    SchedulerCheckpointError::io(
                        "cannot recreate checkpoint artifact parent",
                        error,
                    )
                })?;
                let containing = current.parent().ok_or_else(|| {
                    SchedulerCheckpointError::new(
                        "checkpoint artifact parent has no containing directory",
                    )
                })?;
                sync_directory(containing)?;
            }
            Err(error) => {
                return Err(SchedulerCheckpointError::io(
                    "cannot inspect checkpoint artifact parent",
                    error,
                ));
            }
        }
    }
    Ok(())
}

fn recorded_artifact_paths_match(spec: &TaskSpec, recorded: &[OwnedArtifactCheckpoint]) -> bool {
    let expected_paths = spec.owned_files.iter().cloned().collect::<BTreeSet<_>>();
    if expected_paths.len() != spec.owned_files.len() || recorded.len() != expected_paths.len() {
        return false;
    }
    let recorded_paths = recorded
        .iter()
        .map(|artifact| artifact.path.clone())
        .collect::<BTreeSet<_>>();
    if recorded_paths != expected_paths || recorded_paths.len() != recorded.len() {
        return false;
    }
    true
}

fn validate_owned_file_path(relative: &str) -> Result<(), SchedulerCheckpointError> {
    let relative_path = Path::new(relative);
    if relative_path.as_os_str().is_empty()
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || relative_path
            .components()
            .next()
            .is_some_and(|component| component.as_os_str() == ".swarm")
    {
        return Err(SchedulerCheckpointError::new(format!(
            "task artifact path is not a safe project-relative file: {relative:?}"
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn open_artifact_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    options.open(path)
}

#[cfg(not(unix))]
fn open_artifact_no_follow(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

#[cfg(target_os = "macos")]
fn opened_artifact_path(file: &File) -> std::io::Result<PathBuf> {
    use std::ffi::CStr;
    use std::os::fd::AsRawFd;

    let mut buffer = vec![0_i8; libc::PATH_MAX as usize];
    let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) };
    if result == -1 {
        return Err(std::io::Error::last_os_error());
    }
    let path = unsafe { CStr::from_ptr(buffer.as_ptr()) };
    Ok(PathBuf::from(path.to_string_lossy().into_owned()))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn opened_artifact_path(file: &File) -> std::io::Result<PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::canonicalize(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

#[cfg(not(unix))]
fn opened_artifact_path(file: &File) -> std::io::Result<PathBuf> {
    let _ = file;
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "opened-file path verification is unavailable on this platform",
    ))
}

#[cfg(unix)]
fn artifact_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn artifact_mode(metadata: &std::fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(unix)]
fn set_artifact_mode(file: &File, mode: u32) -> Result<(), SchedulerCheckpointError> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(mode))
        .map_err(|error| SchedulerCheckpointError::io("cannot restore artifact mode", error))
}

#[cfg(not(unix))]
fn set_artifact_mode(file: &File, mode: u32) -> Result<(), SchedulerCheckpointError> {
    let mut permissions = file
        .metadata()
        .map_err(|error| SchedulerCheckpointError::io("cannot inspect artifact mode", error))?
        .permissions();
    permissions.set_readonly(mode != 0);
    file.set_permissions(permissions)
        .map_err(|error| SchedulerCheckpointError::io("cannot restore artifact mode", error))
}

fn replay_wal(wal: &mut File, root: &Path) -> Result<ReplayState, SchedulerCheckpointError> {
    wal.seek(SeekFrom::Start(0)).map_err(|error| {
        SchedulerCheckpointError::io("cannot seek scheduler checkpoint WAL", error)
    })?;
    let mut bytes = Vec::new();
    wal.read_to_end(&mut bytes).map_err(|error| {
        SchedulerCheckpointError::io("cannot read scheduler checkpoint WAL", error)
    })?;
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        let valid_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        bytes.truncate(valid_len);
        wal.set_len(valid_len as u64).map_err(|error| {
            SchedulerCheckpointError::io(
                "cannot truncate torn scheduler checkpoint WAL tail",
                error,
            )
        })?;
        wal.sync_all().map_err(|error| {
            SchedulerCheckpointError::io("cannot sync repaired scheduler checkpoint WAL", error)
        })?;
    }

    let mut next_sequence = 0_u64;
    let mut previous_hash = GENESIS_HASH.to_string();
    let mut records = Vec::new();
    let mut prefixes = vec![(0, GENESIS_HASH.to_string())];
    if !bytes.is_empty() {
        for (index, line) in bytes[..bytes.len() - 1]
            .split(|byte| *byte == b'\n')
            .enumerate()
        {
            if line.is_empty() {
                return Err(SchedulerCheckpointError::new(format!(
                    "scheduler checkpoint WAL record {} is blank",
                    index + 1
                )));
            }
            let record: WalRecord = serde_json::from_slice(line).map_err(|error| {
                SchedulerCheckpointError::new(format!(
                    "scheduler checkpoint WAL record {} is invalid: {error}",
                    index + 1
                ))
            })?;
            if record.material.schema_version != SCHEMA_VERSION
                || record.material.sequence != next_sequence
                || record.material.completion_order != next_sequence
                || record.material.previous_hash != previous_hash
                || record.material.working_root != root
                || hash_serializable(&record.material)? != record.entry_hash
            {
                return Err(SchedulerCheckpointError::new(format!(
                    "scheduler checkpoint WAL record {} breaks its hash-linked sequence or root binding",
                    index + 1
                )));
            }
            validate_wal_material(&record.material)?;
            previous_hash = record.entry_hash;
            next_sequence = next_sequence.checked_add(1).ok_or_else(|| {
                SchedulerCheckpointError::new("checkpoint WAL sequence overflowed")
            })?;
            prefixes.push((next_sequence, previous_hash.clone()));
            records.push(record.material);
        }
    }
    wal.seek(SeekFrom::End(0)).map_err(|error| {
        SchedulerCheckpointError::io("cannot seek scheduler checkpoint WAL end", error)
    })?;
    Ok(ReplayState {
        next_sequence,
        previous_hash,
        expected_len: bytes.len() as u64,
        records,
        prefixes,
    })
}

fn validate_wal_material(material: &WalMaterial) -> Result<(), SchedulerCheckpointError> {
    if material.schema_version != SCHEMA_VERSION
        || material.completion_order != material.sequence
        || material.task_id.trim().is_empty()
        || !canonical_digest(&material.task_spec_digest)
    {
        return Err(SchedulerCheckpointError::new(
            "scheduler checkpoint WAL contains invalid task evidence",
        ));
    }
    match material.transition {
        CheckpointTransition::TaskDone => {
            if material.attempts == 0 {
                return Err(SchedulerCheckpointError::new(
                    "scheduler checkpoint Done record has zero attempts",
                ));
            }
            validate_recorded_artifacts(&material.artifacts)?;
            validate_dependency_checkpoints(material)
        }
        CheckpointTransition::TaskInvalidated => {
            if material.attempts != 0
                || !material.output.is_empty()
                || !material.artifacts.is_empty()
                || !material.dependency_checkpoints.is_empty()
            {
                return Err(SchedulerCheckpointError::new(
                    "scheduler checkpoint invalidation record contains completion evidence",
                ));
            }
            Ok(())
        }
    }
}

fn validate_dependency_checkpoints(material: &WalMaterial) -> Result<(), SchedulerCheckpointError> {
    let mut task_ids = HashSet::new();
    for checkpoint in &material.dependency_checkpoints {
        if checkpoint.task_id.trim().is_empty()
            || !task_ids.insert(checkpoint.task_id.clone())
            || !canonical_digest(&checkpoint.task_spec_digest)
            || checkpoint.completion_order >= material.completion_order
        {
            return Err(SchedulerCheckpointError::new(
                "scheduler checkpoint WAL contains invalid dependency evidence",
            ));
        }
    }
    Ok(())
}

fn validate_recorded_artifacts(
    artifacts: &[OwnedArtifactCheckpoint],
) -> Result<(), SchedulerCheckpointError> {
    let mut paths = HashSet::new();
    for artifact in artifacts {
        if artifact.path.trim().is_empty()
            || !paths.insert(artifact.path.clone())
            || !canonical_digest(&artifact.sha256)
        {
            return Err(SchedulerCheckpointError::new(
                "scheduler checkpoint WAL contains invalid artifact evidence",
            ));
        }
    }
    Ok(())
}

fn read_head(path: &Path, root: &Path) -> std::io::Result<HeadMaterial> {
    let bytes = std::fs::read(path)?;
    if !bytes.ends_with(b"\n") || bytes[..bytes.len().saturating_sub(1)].contains(&b'\n') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "scheduler checkpoint head is torn or has multiple records",
        ));
    }
    let record: HeadRecord =
        serde_json::from_slice(&bytes[..bytes.len() - 1]).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("scheduler checkpoint head is invalid: {error}"),
            )
        })?;
    let expected_hash = hash_serializable(&record.material)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    if record.material.schema_version != SCHEMA_VERSION
        || record.material.working_root != root
        || record.checkpoint_hash != expected_hash
        || (record.material.next_sequence == 0 && record.material.entry_hash != GENESIS_HASH)
        || (record.material.next_sequence > 0 && !canonical_digest(&record.material.entry_hash))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "scheduler checkpoint head hash, schema, sequence, or root is invalid",
        ));
    }
    Ok(record.material)
}

fn write_head_atomic(
    path: &Path,
    root: &Path,
    next_sequence: u64,
    entry_hash: &str,
) -> Result<(), SchedulerCheckpointError> {
    let material = HeadMaterial {
        schema_version: SCHEMA_VERSION,
        working_root: root.to_path_buf(),
        next_sequence,
        entry_hash: entry_hash.to_string(),
    };
    let record = HeadRecord {
        checkpoint_hash: hash_serializable(&material)?,
        material,
    };
    let mut encoded = serde_json::to_vec(&record).map_err(|error| {
        SchedulerCheckpointError::new(format!("cannot encode scheduler checkpoint head: {error}"))
    })?;
    encoded.push(b'\n');

    let parent = path.parent().ok_or_else(|| {
        SchedulerCheckpointError::new("scheduler checkpoint head has no parent directory")
    })?;
    let (temporary, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{HEAD_FILE}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(SchedulerCheckpointError::io(
                    "cannot create scheduler checkpoint head temp",
                    error,
                ));
            }
        }
    };
    let result = (|| {
        file.write_all(&encoded).map_err(|error| {
            SchedulerCheckpointError::io("cannot write scheduler checkpoint head temp", error)
        })?;
        file.sync_all().map_err(|error| {
            SchedulerCheckpointError::io("cannot sync scheduler checkpoint head temp", error)
        })?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|error| {
            SchedulerCheckpointError::io(
                "cannot atomically replace scheduler checkpoint head",
                error,
            )
        })?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn sync_directory(path: &Path) -> Result<(), SchedulerCheckpointError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            SchedulerCheckpointError::io("cannot sync scheduler checkpoint directory", error)
        })
}

fn ensure_control_directory(
    parent: &Path,
    path: &Path,
    label: &str,
) -> Result<(), SchedulerCheckpointError> {
    let created = match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(SchedulerCheckpointError::new(format!(
                    "{label} is not a real directory: {}",
                    path.display()
                )));
            }
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path).map_err(|error| {
                SchedulerCheckpointError::io(&format!("cannot create {label}"), error)
            })?;
            true
        }
        Err(error) => {
            return Err(SchedulerCheckpointError::io(
                &format!("cannot inspect {label}"),
                error,
            ));
        }
    };
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| SchedulerCheckpointError::io(&format!("cannot inspect {label}"), error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SchedulerCheckpointError::new(format!(
            "{label} is not a real directory: {}",
            path.display()
        )));
    }
    if created {
        sync_directory(parent)?;
    }
    Ok(())
}

fn reject_symlink_if_present(path: &Path, label: &str) -> Result<(), SchedulerCheckpointError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(SchedulerCheckpointError::new(
            format!("{label} must not be a symlink: {}", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SchedulerCheckpointError::io(
            &format!("cannot inspect {label}"),
            error,
        )),
    }
}

fn verify_linked_file(
    path: &Path,
    file: &File,
    label: &str,
) -> Result<(), SchedulerCheckpointError> {
    let open = file.metadata().map_err(|error| {
        SchedulerCheckpointError::io(&format!("cannot inspect open {label}"), error)
    })?;
    let linked = std::fs::symlink_metadata(path).map_err(|error| {
        SchedulerCheckpointError::io(&format!("cannot inspect {label} path"), error)
    })?;
    if linked.file_type().is_symlink() || !linked.is_file() {
        return Err(SchedulerCheckpointError::new(format!(
            "{label} path is not a regular file"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if open.dev() != linked.dev() || open.ino() != linked.ino() {
            return Err(SchedulerCheckpointError::new(format!(
                "{label} path was replaced"
            )));
        }
    }
    Ok(())
}

fn verify_live_file(
    path: &Path,
    file: &File,
    expected_len: u64,
) -> Result<(), SchedulerCheckpointError> {
    let open = file.metadata().map_err(|error| {
        SchedulerCheckpointError::io("cannot inspect open scheduler checkpoint WAL", error)
    })?;
    let linked = std::fs::metadata(path).map_err(|error| {
        SchedulerCheckpointError::io("cannot inspect scheduler checkpoint WAL path", error)
    })?;
    if open.len() != expected_len || linked.len() != expected_len {
        return Err(SchedulerCheckpointError::new(
            "scheduler checkpoint WAL changed outside its writer",
        ));
    }
    verify_linked_file(path, file, "scheduler checkpoint WAL")?;
    Ok(())
}

fn hash_serializable(value: &impl Serialize) -> Result<String, SchedulerCheckpointError> {
    let encoded = serde_json::to_vec(value).map_err(|error| {
        SchedulerCheckpointError::new(format!(
            "cannot encode scheduler checkpoint hash material: {error}"
        ))
    })?;
    Ok(sha256_digest(&encoded))
}

fn sha256_digest(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn canonical_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::{Difficulty, TaskSpec};
    use crate::dispatch::{DispatchError, DispatchRequest, TaskDispatcher, TaskRunOutput};
    use crate::scheduler::{DeviceCfg, Scheduler};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Arc;

    type CapturedContexts = Arc<std::sync::Mutex<Vec<(String, String)>>>;

    fn spec(id: &str, deps: &[&str], files: &[&str]) -> TaskSpec {
        TaskSpec {
            id: id.to_string(),
            description: format!("implement {id}"),
            difficulty: Difficulty::Easy,
            preferred_model: None,
            owned_files: files.iter().map(|file| file.to_string()).collect(),
            deps: deps
                .iter()
                .map(|dependency| dependency.to_string())
                .collect(),
            subsplit: Vec::new(),
            replan_authority: None,
        }
    }

    fn write_artifact(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn crash_replay_restores_output_attempts_and_releases_a_dependency() {
        let root = tempfile::tempdir().unwrap();
        write_artifact(root.path(), "src/a.rs", "pub fn a() {}\n");
        let a = spec("a", &[], &["src/a.rs"]);
        let b = spec("b", &["a"], &["src/b.rs"]);
        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        store
            .persist_done(&a, "A completed with API v1", 2)
            .unwrap();
        drop(store);

        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        let mut dag = Dag::from_specs(vec![a, b]).unwrap();
        let mut context = SharedContext::new();
        let summary = store.restore_into(&mut dag, &mut context).unwrap();

        assert_eq!(summary.restored, vec!["a"]);
        assert!(summary.invalidated.is_empty());
        assert_eq!(dag.tasks["a"].state, TaskState::Done);
        assert_eq!(dag.tasks["a"].attempts, 2);
        assert_eq!(dag.tasks["b"].state, TaskState::Ready);
        assert_eq!(dag.tasks["b"].indegree_remaining, 0);
        assert!(context
            .slice_for(&["a".to_string()])
            .contains("A completed with API v1"));
    }

    #[test]
    fn missing_and_corrupt_owned_artifacts_restore_from_content_addressed_bytes() {
        let root = tempfile::tempdir().unwrap();
        write_artifact(root.path(), "a.txt", "a-v1");
        write_artifact(root.path(), "nested/b.txt", "b-v1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                root.path().join("a.txt"),
                std::fs::Permissions::from_mode(0o750),
            )
            .unwrap();
        }
        let a = spec("a", &[], &["a.txt"]);
        let b = spec("b", &["a"], &["nested/b.txt"]);
        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        store.persist_done(&a, "a output", 1).unwrap();
        store.persist_done(&b, "b output", 1).unwrap();
        drop(store);
        write_artifact(root.path(), "a.txt", "corrupt");
        std::fs::remove_dir_all(root.path().join("nested")).unwrap();

        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        let mut dag = Dag::from_specs(vec![a, b]).unwrap();
        let mut context = SharedContext::new();
        let summary = store.restore_into(&mut dag, &mut context).unwrap();

        assert_eq!(summary.restored, vec!["a", "b"]);
        assert!(summary.invalidated.is_empty());
        assert_eq!(std::fs::read(root.path().join("a.txt")).unwrap(), b"a-v1");
        assert_eq!(
            std::fs::read(root.path().join("nested/b.txt")).unwrap(),
            b"b-v1"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(root.path().join("a.txt"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o750
            );
        }
        assert_eq!(dag.tasks["a"].state, TaskState::Done);
        assert_eq!(dag.tasks["b"].state, TaskState::Done);
    }

    #[test]
    fn invalidated_done_record_cannot_resurrect_during_a_crashed_rerun() {
        let root = tempfile::tempdir().unwrap();
        write_artifact(root.path(), "a.txt", "completed-v1");
        let a = spec("a", &[], &["a.txt"]);
        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        store.persist_done(&a, "old output", 1).unwrap();
        let artifact = capture_artifact(store.root(), "a.txt").unwrap().checkpoint;
        let object =
            artifact_object_path(&store.artifact_object_directory, &artifact.sha256).unwrap();
        std::fs::remove_file(object).unwrap();

        let mut dag = Dag::from_specs(vec![a.clone()]).unwrap();
        let mut context = SharedContext::new();
        let summary = store.restore_into(&mut dag, &mut context).unwrap();
        assert_eq!(summary.invalidated, vec!["a"]);
        drop(store);

        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        let mut dag = Dag::from_specs(vec![a]).unwrap();
        let summary = store.restore_into(&mut dag, &mut context).unwrap();

        assert!(summary.restored.is_empty());
        assert_eq!(summary.invalidated, vec!["a"]);
        assert_eq!(dag.tasks["a"].state, TaskState::Ready);
        assert!(context.completed().is_empty());
    }

    #[test]
    fn task_spec_digest_mismatch_forces_a_rerun() {
        let root = tempfile::tempdir().unwrap();
        write_artifact(root.path(), "a.txt", "unchanged artifact");
        let a = spec("a", &[], &["a.txt"]);
        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        store.persist_done(&a, "old output", 1).unwrap();
        drop(store);

        let mut changed = a;
        changed.description = "a changed exact contract".to_string();
        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        let mut dag = Dag::from_specs(vec![changed]).unwrap();
        let mut context = SharedContext::new();
        let summary = store.restore_into(&mut dag, &mut context).unwrap();

        assert!(summary.restored.is_empty());
        assert_eq!(summary.invalidated, vec!["a"]);
        assert_eq!(dag.tasks["a"].state, TaskState::Ready);
    }

    #[tokio::test]
    async fn fileless_completion_chain_restores_output_attempts_and_dependencies_without_dispatch()
    {
        let root = tempfile::tempdir().unwrap();
        let inspect = spec("inspect", &[], &[]);
        let verify = spec("integrate-verify", &["inspect"], &[]);
        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        assert!(store.can_persist_done(&inspect).unwrap());
        store
            .persist_done(&inspect, "inspection evidence", 2)
            .unwrap();
        assert!(store.can_persist_done(&verify).unwrap());
        store.persist_done(&verify, "verified", 3).unwrap();
        drop(store);

        let store = Arc::new(SchedulerCheckpointStore::open(root.path()).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let dispatcher = Arc::new(WritingDispatcher {
            root: root.path().to_path_buf(),
            calls: calls.clone(),
            contexts: None,
            sabotage_wal: None,
        });
        let scheduler = Scheduler::new(
            vec![DeviceCfg {
                id: "device".to_string(),
                model_id: "model".to_string(),
                weight: 1,
                enabled: true,
                speed_weight: 1,
                supervision: false,
            }],
            1,
        )
        .with_checkpoint_store(store);
        let report = scheduler
            .run(
                Dag::from_specs(vec![inspect, verify]).unwrap(),
                dispatcher,
                "fileless checkpoint replay".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(calls.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(report.done, vec!["inspect", "integrate-verify"]);
        assert_eq!(report.results["inspect"], "inspection evidence");
        assert_eq!(report.results["integrate-verify"], "verified");
        assert_eq!(
            report
                .tasks
                .iter()
                .find(|task| task.task_id == "inspect")
                .unwrap()
                .attempts,
            2
        );
        assert_eq!(
            report
                .tasks
                .iter()
                .find(|task| task.task_id == "integrate-verify")
                .unwrap()
                .attempts,
            3
        );
    }

    #[test]
    fn torn_tail_is_truncated_but_complete_corruption_is_rejected() {
        let torn_root = tempfile::tempdir().unwrap();
        write_artifact(torn_root.path(), "a.txt", "a");
        let store = SchedulerCheckpointStore::open(torn_root.path()).unwrap();
        store
            .persist_done(&spec("a", &[], &["a.txt"]), "done", 1)
            .unwrap();
        let torn_wal = store.wal_path.clone();
        drop(store);
        OpenOptions::new()
            .append(true)
            .open(torn_wal)
            .unwrap()
            .write_all(b"{\"torn\"")
            .unwrap();
        let recovered = SchedulerCheckpointStore::open(torn_root.path()).unwrap();
        let mut dag = Dag::from_specs(vec![spec("a", &[], &["a.txt"])]).unwrap();
        let mut context = SharedContext::new();
        let summary = recovered.restore_into(&mut dag, &mut context).unwrap();
        assert_eq!(summary.restored, vec!["a"]);
        assert!(std::fs::read(&recovered.wal_path).unwrap().ends_with(b"\n"));

        let corrupt_root = tempfile::tempdir().unwrap();
        write_artifact(corrupt_root.path(), "a.txt", "a");
        let store = SchedulerCheckpointStore::open(corrupt_root.path()).unwrap();
        store
            .persist_done(&spec("a", &[], &["a.txt"]), "done", 1)
            .unwrap();
        let corrupt_wal = store.wal_path.clone();
        drop(store);
        let mut bytes = std::fs::read(&corrupt_wal).unwrap();
        const OUTPUT_FIELD: &[u8] = b"\"output\":\"done\"";
        let position = bytes
            .windows(OUTPUT_FIELD.len())
            .position(|window| window == OUTPUT_FIELD)
            .expect("fixture output is present");
        bytes[position + b"\"output\":\"".len()] = b'g';
        std::fs::write(corrupt_wal, bytes).unwrap();
        let corrupt_error = SchedulerCheckpointStore::open(corrupt_root.path())
            .err()
            .expect("a hash-corrupt WAL must be rejected");
        assert!(corrupt_error.to_string().contains("hash-linked"));
    }

    struct WritingDispatcher {
        root: PathBuf,
        calls: Arc<AtomicUsize>,
        contexts: Option<CapturedContexts>,
        sabotage_wal: Option<PathBuf>,
    }

    #[async_trait]
    impl TaskDispatcher for WritingDispatcher {
        async fn run(&self, request: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            if let Some(contexts) = &self.contexts {
                contexts
                    .lock()
                    .unwrap()
                    .push((request.task_id.clone(), request.context_slice.clone()));
            }
            for relative in &request.owned_files {
                write_artifact(&self.root, relative, &format!("{} output", request.task_id));
            }
            if let Some(wal_path) = &self.sabotage_wal {
                OpenOptions::new()
                    .append(true)
                    .open(wal_path)
                    .unwrap()
                    .write_all(b"{}\n")
                    .unwrap();
            }
            Ok(format!("{} completed", request.task_id).into())
        }
    }

    #[tokio::test]
    async fn scheduler_replay_restores_report_output_attempts_and_dependency_context() {
        let root = tempfile::tempdir().unwrap();
        write_artifact(root.path(), "a.txt", "a-v1");
        let a = spec("a", &[], &["a.txt"]);
        let b = spec("b", &["a"], &["b.txt"]);
        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        store
            .persist_done(&a, "restored dependency output", 2)
            .unwrap();
        drop(store);

        let calls = Arc::new(AtomicUsize::new(0));
        let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let dispatcher = Arc::new(WritingDispatcher {
            root: root.path().to_path_buf(),
            calls: calls.clone(),
            contexts: Some(contexts.clone()),
            sabotage_wal: None,
        });
        let scheduler = Scheduler::new(
            vec![DeviceCfg {
                id: "device".to_string(),
                model_id: "model".to_string(),
                weight: 1,
                enabled: true,
                speed_weight: 1,
                supervision: false,
            }],
            1,
        )
        .with_checkpoint_root(root.path())
        .unwrap();

        let report = scheduler
            .run(
                Dag::from_specs(vec![a, b]).unwrap(),
                dispatcher,
                "checkpoint replay".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
        let restored = report
            .tasks
            .iter()
            .find(|task| task.task_id == "a")
            .unwrap();
        assert_eq!(restored.attempts, 2);
        assert_eq!(
            restored.output.as_deref(),
            Some("restored dependency output")
        );
        let contexts = contexts.lock().unwrap();
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].0, "b");
        assert!(contexts[0].1.contains("restored dependency output"));
    }

    #[tokio::test]
    async fn repaired_dag_reseal_restores_exact_artifacts_with_zero_dispatches() {
        let root = tempfile::tempdir().unwrap();
        let a = spec("a", &[], &["src/a.txt"]);
        let b = spec("b", &["a"], &["src/b.txt"]);
        let verify = spec("verify", &["b"], &[]);
        write_artifact(root.path(), "src/a.txt", "initial-a");
        write_artifact(root.path(), "src/b.txt", "initial-b");
        let store = SchedulerCheckpointStore::open(root.path()).unwrap();
        store.persist_done(&a, "original a output", 1).unwrap();
        store.persist_done(&b, "original b output", 2).unwrap();
        store
            .persist_done(&verify, "verification output", 4)
            .unwrap();

        write_artifact(root.path(), "src/a.txt", "repaired-a");
        write_artifact(root.path(), "src/b.txt", "repaired-b");
        let receipt = store
            .reseal_completed_dag(
                &[b.clone(), verify.clone(), a.clone()],
                &[
                    SchedulerCompletedTaskEvidence {
                        task_id: "b".to_string(),
                        output: "original b output".to_string(),
                        attempts: 2,
                    },
                    SchedulerCompletedTaskEvidence {
                        task_id: "a".to_string(),
                        output: "original a output".to_string(),
                        attempts: 1,
                    },
                    SchedulerCompletedTaskEvidence {
                        task_id: "verify".to_string(),
                        output: "verification output".to_string(),
                        attempts: 4,
                    },
                ],
            )
            .unwrap();
        assert_eq!(receipt.tasks, vec!["a", "b", "verify"]);
        assert_eq!(receipt.next_sequence - receipt.first_sequence, 3);
        {
            let state = lock(&store.state);
            let a_record = state
                .records
                .iter()
                .rev()
                .find(|record| record.task_id == "a")
                .unwrap();
            let b_record = state
                .records
                .iter()
                .rev()
                .find(|record| record.task_id == "b")
                .unwrap();
            let verify_record = state
                .records
                .iter()
                .rev()
                .find(|record| record.task_id == "verify")
                .unwrap();
            assert!(a_record.completion_order >= receipt.first_sequence);
            assert_eq!(
                b_record.dependency_checkpoints,
                vec![dependency_checkpoint(a_record)]
            );
            assert_eq!(
                verify_record.dependency_checkpoints,
                vec![dependency_checkpoint(b_record)]
            );
        }
        drop(store);

        write_artifact(root.path(), "src/a.txt", "corrupt-after-reseal");
        std::fs::remove_file(root.path().join("src/b.txt")).unwrap();
        let store = Arc::new(SchedulerCheckpointStore::open(root.path()).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let dispatcher = Arc::new(WritingDispatcher {
            root: root.path().to_path_buf(),
            calls: calls.clone(),
            contexts: None,
            sabotage_wal: None,
        });
        let scheduler = Scheduler::new(
            vec![DeviceCfg {
                id: "device".to_string(),
                model_id: "model".to_string(),
                weight: 1,
                enabled: true,
                speed_weight: 1,
                supervision: false,
            }],
            1,
        )
        .with_checkpoint_store(store);

        let report = scheduler
            .run(
                Dag::from_specs(vec![a, b, verify]).unwrap(),
                dispatcher,
                "repaired checkpoint replay".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(calls.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(
            std::fs::read(root.path().join("src/a.txt")).unwrap(),
            b"repaired-a"
        );
        assert_eq!(
            std::fs::read(root.path().join("src/b.txt")).unwrap(),
            b"repaired-b"
        );
        assert_eq!(report.done, vec!["a", "b", "verify"]);
        assert_eq!(
            report
                .tasks
                .iter()
                .find(|task| task.task_id == "a")
                .unwrap()
                .attempts,
            1
        );
        assert_eq!(
            report
                .tasks
                .iter()
                .find(|task| task.task_id == "b")
                .unwrap()
                .attempts,
            2
        );
        assert_eq!(report.results["a"], "original a output");
        assert_eq!(report.results["b"], "original b output");
        assert_eq!(report.results["verify"], "verification output");
        assert_eq!(
            report
                .tasks
                .iter()
                .find(|task| task.task_id == "verify")
                .unwrap()
                .attempts,
            4
        );
    }

    #[test]
    fn dag_reseal_rejects_missing_completion_or_artifact_evidence_before_wal_mutation() {
        let root = tempfile::tempdir().unwrap();
        let a = spec("a", &[], &["a.txt"]);
        let b = spec("b", &["a"], &["b.txt"]);
        write_artifact(root.path(), "a.txt", "a");
        write_artifact(root.path(), "b.txt", "b");
        let store = SchedulerCheckpointStore::open(root.path()).unwrap();

        let error = store
            .reseal_completed_dag(
                &[a.clone(), b.clone()],
                &[SchedulerCompletedTaskEvidence {
                    task_id: "a".to_string(),
                    output: "a output".to_string(),
                    attempts: 1,
                }],
            )
            .unwrap_err();
        assert!(error.to_string().contains("cover every task exactly once"));
        assert_eq!(lock(&store.state).next_sequence, 0);

        std::fs::remove_file(root.path().join("b.txt")).unwrap();
        let error = store
            .reseal_completed_dag(
                &[a, b],
                &[
                    SchedulerCompletedTaskEvidence {
                        task_id: "a".to_string(),
                        output: "a output".to_string(),
                        attempts: 1,
                    },
                    SchedulerCompletedTaskEvidence {
                        task_id: "b".to_string(),
                        output: "b output".to_string(),
                        attempts: 1,
                    },
                ],
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("cannot open completed task artifact"));
        assert_eq!(lock(&store.state).next_sequence, 0);
        assert_eq!(
            std::fs::read_dir(&store.artifact_object_directory)
                .unwrap()
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn persistence_failure_does_not_release_dependents() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(SchedulerCheckpointStore::open(root.path()).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let dispatcher = Arc::new(WritingDispatcher {
            root: root.path().to_path_buf(),
            calls: calls.clone(),
            contexts: None,
            sabotage_wal: Some(store.wal_path.clone()),
        });
        let dag =
            Dag::from_specs(vec![spec("a", &[], &["a.txt"]), spec("b", &["a"], &[])]).unwrap();
        let scheduler = Scheduler::new(
            vec![DeviceCfg {
                id: "device".to_string(),
                model_id: "model".to_string(),
                weight: 1,
                enabled: true,
                speed_weight: 1,
                supervision: false,
            }],
            1,
        )
        .with_checkpoint_store(store);

        let report = scheduler
            .run(dag, dispatcher, "checkpoint test".to_string())
            .await
            .unwrap();

        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
        assert!(report.done.is_empty());
        assert!(report.failed.contains(&"a".to_string()));
        assert!(report.failed.contains(&"b".to_string()));
    }
}
