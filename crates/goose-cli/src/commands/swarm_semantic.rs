//! Production adapters for observation-only semantic review.
//!
//! This module can seal immutable worker evidence and make one admitted provider request. It has no
//! action-delivery API: a parsed `NUDGE`, split, acceptance candidate, or other semantic action remains
//! an observation receipt owned by `goose-swarm`.

use super::swarm_provider_lifecycle::ProviderStreamProgressSnapshot;

use async_trait::async_trait;
use base64::Engine;
use goose::conversation::message::{Message, MessageContent};
use goose::providers::base::{
    collect_stream, Provider, SingleAttemptFailureProvenance, SingleAttemptStreamOutcome,
};
use goose_swarm::{
    AdmittedSemanticObservationRequest, AdmittedSemanticObservationReviewer,
    AdmittedSemanticReviewError, ArtifactExcerptSnapshot, EventSink, PhysicalFleetSnapshot,
    SemanticActivityPublisher, SemanticObservationCapture, SemanticObservationCaptureRequest,
    SemanticObservationSnapshotDraft, SemanticObservationSnapshotProducer,
    SemanticObservationSummonsSignal, SemanticTraceSnapshot, SourceRevisionKind,
    TraceStateMeasurement, VerifiedPhysicalLane, WorkRole, SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::Mutex as AsyncMutex;

use goose_provider_types::errors::ProviderError;

const RECURRENCE_WINDOW_CHARS: usize = 48;
const RECURRENCE_REACH_WINDOWS: usize = 65_536;
const RECURRENCE_HISTORY_CHARS: usize = 65_536;
const EARLIER_REASONING_MIN_DISTANCE: usize = 20_000;
const EARLIER_REASONING_MAX_DISTANCE: usize = 40_000;
const EARLIER_REASONING_EXCERPT_CHARS: usize = 2_000;
static ACTIVITY_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Flat, injective activity filename encoding shared by the stream writer and snapshot reader. Task
/// ids include owned paths (`fix::rN::a/b.rs`); distinct escape tails preserve `~`, `/`, and `\\`
/// without either path traversal or the adjacent-escape alias in the older doubling scheme.
pub(super) fn activity_digest_key(task_id: &str) -> String {
    task_id
        .replace('~', "~t")
        .replace('/', "~s")
        .replace('\\', "~b")
}

#[derive(Clone, Debug)]
struct ActivitySinkFailure {
    detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EngineActivityDigest {
    revision: u64,
    bytes: Vec<u8>,
}

pub(super) struct ActivitySinkHealth {
    working_dir: PathBuf,
    required: AtomicBool,
    failure: Mutex<Option<ActivitySinkFailure>>,
    mirror_failure: Mutex<Option<ActivitySinkFailure>>,
    digests: Mutex<HashMap<String, EngineActivityDigest>>,
    next_revision: AtomicU64,
    events: Arc<dyn EventSink>,
}

impl ActivitySinkHealth {
    pub(super) fn new(
        working_dir: impl AsRef<Path>,
        events: Arc<dyn EventSink>,
    ) -> Result<Self, String> {
        let working_dir = canonical_working_dir(working_dir.as_ref())?;
        Ok(Self {
            working_dir,
            required: AtomicBool::new(false),
            failure: Mutex::new(None),
            mirror_failure: Mutex::new(None),
            digests: Mutex::new(HashMap::new()),
            next_revision: AtomicU64::new(1),
            events,
        })
    }

    pub(super) fn activate(&self) -> Result<(), String> {
        self.ensure_not_degraded()?;
        self.required.store(true, Ordering::Release);
        let activity_dir = self.working_dir.join(".swarm").join("activity");
        let mirror_ready = match probe_activity_mirror(&activity_dir) {
            Ok(()) => true,
            Err(error) => {
                self.record_mirror_failure("activation", "probe", error);
                false
            }
        };
        self.events.write_value(serde_json::json!({
            "event": "physical_semantic_activity_sink_ready",
            "authority": "engine_memory",
            "ui_mirror": if mirror_ready { "atomic_ready" } else { "degraded" },
        }));
        Ok(())
    }

    pub(super) fn ensure_healthy(&self) -> Result<(), String> {
        if !self.required.load(Ordering::Acquire) {
            return Err("physical semantic activity sink was not activated".to_string());
        }
        self.ensure_not_degraded()
    }

    pub(super) fn write_digest(
        &self,
        path: &Path,
        contents: &[u8],
        task_id: &str,
        publisher: Option<&SemanticActivityPublisher>,
        stage: &'static str,
        required: bool,
    ) -> Result<(), String> {
        if required {
            self.ensure_healthy()?;
            let publisher = publisher.ok_or_else(|| {
                self.record_authority_failure(
                    task_id,
                    stage,
                    "physical semantic activity write has no engine-minted publisher".to_string(),
                )
            })?;
            publisher
                .validate()
                .map_err(|error| self.record_authority_failure(task_id, stage, error))?;
            if publisher.task_id() != task_id {
                return Err(self.record_authority_failure(
                    task_id,
                    stage,
                    format!(
                        "physical semantic activity publisher task {:?} does not match write task {:?}",
                        publisher.task_id(), task_id
                    ),
                ));
            }
            let expected = self.activity_path(task_id);
            if path != expected {
                return Err(self.record_authority_failure(
                    task_id,
                    stage,
                    format!(
                        "physical activity path {:?} does not match engine-owned path {:?}",
                        path, expected
                    ),
                ));
            }
            validate_authoritative_activity(contents, publisher)
                .map_err(|error| self.record_authority_failure(task_id, stage, error))?;

            let revision = self.next_revision.fetch_add(1, Ordering::AcqRel);
            unpoison(&self.digests).insert(
                publisher.publisher_id().to_string(),
                EngineActivityDigest {
                    revision,
                    bytes: contents.to_vec(),
                },
            );

            if let Err(error) = atomic_activity_write(path, contents, false) {
                self.record_mirror_failure(task_id, stage, error);
            }
            return Ok(());
        }

        atomic_activity_write(path, contents, false)
            .map_err(|error| format!("best-effort activity digest {stage} failed: {error}"))
    }

    pub(super) fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    fn activity_path(&self, task_id: &str) -> PathBuf {
        self.working_dir
            .join(".swarm")
            .join("activity")
            .join(format!("{}.json", activity_digest_key(task_id)))
    }

    fn read_digest(&self, publisher_id: &str) -> Result<Option<EngineActivityDigest>, String> {
        self.ensure_healthy()?;
        Ok(unpoison(&self.digests).get(publisher_id).cloned())
    }

    fn ensure_not_degraded(&self) -> Result<(), String> {
        match unpoison(&self.failure).as_ref() {
            Some(failure) => Err(failure.detail.clone()),
            None => Ok(()),
        }
    }

    fn record_authority_failure(
        &self,
        task_id: &str,
        stage: &'static str,
        detail: String,
    ) -> String {
        let detail =
            format!("physical semantic activity authority degraded during {stage}: {detail}");
        let first = {
            let mut failure = unpoison(&self.failure);
            if failure.is_some() {
                false
            } else {
                *failure = Some(ActivitySinkFailure {
                    detail: detail.clone(),
                });
                true
            }
        };
        if first {
            self.events.write_value(serde_json::json!({
                "event": "physical_semantic_activity_sink_degraded",
                "task_id": task_id,
                "stage": stage,
                "authority": "engine_memory",
                "provider_terminal_fabricated": false,
            }));
            eprintln!("physical semantic supervision degraded: {detail}");
        }
        detail
    }

    fn record_mirror_failure(&self, task_id: &str, stage: &'static str, error: std::io::Error) {
        let detail = format!(
            "physical semantic activity UI mirror degraded during {stage} ({:?}): {error}",
            error.kind()
        );
        let first = {
            let mut failure = unpoison(&self.mirror_failure);
            if failure.is_some() {
                false
            } else {
                *failure = Some(ActivitySinkFailure {
                    detail: detail.clone(),
                });
                true
            }
        };
        if first {
            self.events.write_value(serde_json::json!({
                "event": "physical_semantic_activity_mirror_degraded",
                "task_id": task_id,
                "stage": stage,
                "error_kind": format!("{:?}", error.kind()),
                "semantic_authority_healthy": true,
            }));
            eprintln!("{detail}");
        }
    }
}

fn probe_activity_mirror(activity_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(activity_dir)?;
    let probe = activity_dir.join(format!(
        ".semantic-activity-probe-{}-{}",
        std::process::id(),
        ACTIVITY_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let expected = b"physical-semantic-activity-probe";
    atomic_activity_write(&probe, expected, true)?;
    let observed = std::fs::read(&probe)?;
    std::fs::remove_file(&probe)?;
    if observed != expected {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "physical semantic activity mirror probe read different bytes",
        ));
    }
    Ok(())
}

fn validate_authoritative_activity(
    contents: &[u8],
    publisher: &SemanticActivityPublisher,
) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_slice(contents)
        .map_err(|error| format!("activity digest is not valid JSON: {error}"))?;
    let model = value
        .get("model")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "activity digest has no string model".to_string())?;
    if model != publisher.model_id() {
        return Err(format!(
            "activity digest model {model:?} does not match publisher model {:?}",
            publisher.model_id()
        ));
    }
    if let ActivityState::Active(digest) = classify_activity(contents)? {
        digest.validate()?;
    }
    Ok(())
}

fn canonical_working_dir(working_dir: &Path) -> Result<PathBuf, String> {
    let working_dir = std::fs::canonicalize(working_dir).map_err(|error| {
        format!(
            "cannot canonicalize semantic observation working directory {:?}: {error}",
            working_dir
        )
    })?;
    if !working_dir.is_dir() {
        return Err(format!(
            "semantic observation working directory {:?} is not a directory",
            working_dir
        ));
    }
    Ok(working_dir)
}

fn atomic_activity_write(path: &Path, contents: &[u8], sync: bool) -> std::io::Result<()> {
    use std::io::Write as _;

    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "activity digest path has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "activity digest path has no UTF-8 filename",
            )
        })?;
    let temporary = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        ACTIVITY_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        if sync {
            file.sync_all()?;
        }
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReasoningRecurrenceSnapshot {
    pub window_chars: u32,
    pub observed_windows: u64,
    pub repeated_windows: u64,
    pub repeat_share: f64,
    pub earlier_reasoning: String,
}

#[derive(Default)]
pub(super) struct ReasoningRecurrenceMeter {
    window: VecDeque<char>,
    history: VecDeque<char>,
    window_hashes: VecDeque<[u8; 32]>,
    hash_counts: HashMap<[u8; 32], usize>,
}

impl ReasoningRecurrenceMeter {
    pub(super) fn push(&mut self, reasoning_delta: &str) {
        for character in reasoning_delta.chars() {
            self.window.push_back(character);
            if self.window.len() > RECURRENCE_WINDOW_CHARS {
                self.window.pop_front();
            }

            self.history.push_back(character);
            if self.history.len() > RECURRENCE_HISTORY_CHARS {
                self.history.pop_front();
            }

            if self.window.len() != RECURRENCE_WINDOW_CHARS {
                continue;
            }
            let mut encoded = String::new();
            encoded.extend(self.window.iter());
            let hash: [u8; 32] = Sha256::digest(encoded.as_bytes()).into();
            self.window_hashes.push_back(hash);
            *self.hash_counts.entry(hash).or_default() += 1;

            if self.window_hashes.len() > RECURRENCE_REACH_WINDOWS {
                let expired = self
                    .window_hashes
                    .pop_front()
                    .expect("length was above the recurrence reach");
                match self.hash_counts.get_mut(&expired) {
                    Some(1) => {
                        self.hash_counts.remove(&expired);
                    }
                    Some(count) => *count -= 1,
                    None => unreachable!("every retained recurrence hash has a count"),
                }
            }
        }
    }

    pub(super) fn reset(&mut self) {
        self.window.clear();
        self.history.clear();
        self.window_hashes.clear();
        self.hash_counts.clear();
    }

    pub(super) fn snapshot(&self) -> ReasoningRecurrenceSnapshot {
        let observed_windows = self.window_hashes.len() as u64;
        let repeated_windows = observed_windows.saturating_sub(self.hash_counts.len() as u64);
        let repeat_share = if observed_windows == 0 {
            0.0
        } else {
            repeated_windows as f64 / observed_windows as f64
        };
        ReasoningRecurrenceSnapshot {
            window_chars: RECURRENCE_WINDOW_CHARS as u32,
            observed_windows,
            repeated_windows,
            repeat_share,
            earlier_reasoning: self.earlier_reasoning(),
        }
    }

    fn earlier_reasoning(&self) -> String {
        let history_len = self.history.len();
        if history_len <= EARLIER_REASONING_MIN_DISTANCE {
            return String::new();
        }
        let start = history_len.saturating_sub(EARLIER_REASONING_MAX_DISTANCE);
        let latest_allowed = history_len - EARLIER_REASONING_MIN_DISTANCE;
        let end = (start + EARLIER_REASONING_EXCERPT_CHARS).min(latest_allowed);
        self.history
            .iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerActivityDigest {
    tool_calls: u64,
    errors: u64,
    malformed: u64,
    recent: Vec<String>,
    last_text: String,
    calls: Vec<WorkerActivityCall>,
    reasoning: String,
    full_reasoning: String,
    thinking_chars: u64,
    last_thinking: String,
    model: String,
    reasoning_recurrence: ReasoningRecurrenceSnapshot,
    #[serde(default)]
    provider_stream: ProviderStreamProgressSnapshot,
    #[serde(default)]
    phase: Option<String>,
}

impl WorkerActivityDigest {
    fn validate(&self) -> Result<(), String> {
        if self.model.trim().is_empty() {
            return Err("activity digest model is empty".to_string());
        }
        if self.errors > self.tool_calls {
            return Err("activity digest errors exceed tool calls".to_string());
        }
        if self.malformed > self.tool_calls {
            return Err("activity digest malformed calls exceed tool calls".to_string());
        }
        let recurrence = &self.reasoning_recurrence;
        if recurrence.window_chars == 0 {
            return Err("recurrence window is zero".to_string());
        }
        if recurrence.repeated_windows > recurrence.observed_windows {
            return Err("repeated recurrence windows exceed observed windows".to_string());
        }
        if !recurrence.repeat_share.is_finite() || !(0.0..=1.0).contains(&recurrence.repeat_share) {
            return Err("recurrence repeat share is not a finite fraction".to_string());
        }
        let expected_share = if recurrence.observed_windows == 0 {
            0.0
        } else {
            recurrence.repeated_windows as f64 / recurrence.observed_windows as f64
        };
        if (recurrence.repeat_share - expected_share).abs() > f64::EPSILON * 8.0 {
            return Err("recurrence repeat share does not match its typed counts".to_string());
        }
        let provider = self.provider_stream;
        if provider.structured_output_chunks > provider.chunks {
            return Err("structured provider chunks exceed all provider chunks".to_string());
        }
        if provider.structured_output_bytes > provider.bytes {
            return Err("structured provider bytes exceed all provider bytes".to_string());
        }
        if provider.revision < provider.chunks {
            return Err("provider stream revision trails decoded chunks".to_string());
        }
        if provider.structured_output_active && provider.structured_output_chunks == 0 {
            return Err("structured provider output is active without a decoded chunk".to_string());
        }
        Ok(())
    }

    fn pending_tool_calls(&self) -> u64 {
        self.calls.iter().filter(|call| call.ok.is_none()).count() as u64
    }

    fn has_meaningful_trace(&self) -> bool {
        self.tool_calls > 0
            || self.malformed > 0
            || self.thinking_chars > 0
            || !self.recent.is_empty()
            || !self.last_text.trim().is_empty()
            || !self.reasoning.trim().is_empty()
            || !self.full_reasoning.trim().is_empty()
            || !self.last_thinking.trim().is_empty()
            || !self.calls.is_empty()
            || self.provider_stream.revision > 0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerActivityCall {
    name: String,
    summary: String,
    ok: Option<bool>,
    result: String,
    is_mcp: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OwnedArtifactState {
    Missing,
    Present(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OwnedArtifactMaterial {
    relative_path: String,
    state: OwnedArtifactState,
}

#[derive(Clone, Debug, PartialEq)]
struct StableCaptureMaterial {
    digest: WorkerActivityDigest,
    artifacts: Vec<OwnedArtifactMaterial>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActivityDigestRead {
    revision: Option<u64>,
    bytes: Vec<u8>,
}

struct CapturedTraceState {
    attempt: u32,
    measurement_hash: String,
    source_revision: u64,
}

pub struct GooseSemanticObservationSnapshotProducer {
    working_dir: PathBuf,
    activity_health: Option<Arc<ActivitySinkHealth>>,
    state: Mutex<HashMap<String, CapturedTraceState>>,
    task_lanes: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl GooseSemanticObservationSnapshotProducer {
    pub fn new(working_dir: impl AsRef<Path>) -> Result<Self, String> {
        let working_dir = canonical_working_dir(working_dir.as_ref())?;
        Ok(Self {
            working_dir,
            activity_health: None,
            state: Mutex::new(HashMap::new()),
            task_lanes: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn new_with_activity_health(
        working_dir: impl AsRef<Path>,
        activity_health: Arc<ActivitySinkHealth>,
    ) -> Result<Self, String> {
        let working_dir = canonical_working_dir(working_dir.as_ref())?;
        if working_dir != activity_health.working_dir() {
            return Err(format!(
                "semantic observation working directory {:?} does not match activity sink {:?}",
                working_dir,
                activity_health.working_dir()
            ));
        }
        Ok(Self {
            working_dir,
            activity_health: Some(activity_health),
            state: Mutex::new(HashMap::new()),
            task_lanes: Mutex::new(HashMap::new()),
        })
    }

    fn ensure_activity_healthy(&self) -> Result<(), String> {
        match &self.activity_health {
            Some(health) => health.ensure_healthy(),
            None => Ok(()),
        }
    }

    async fn stable_material(
        &self,
        request: &SemanticObservationCaptureRequest,
    ) -> Result<Option<StableCaptureMaterial>, String> {
        self.ensure_activity_healthy()?;
        let Some(first_activity) = self.read_activity(request).await? else {
            return Ok(None);
        };
        let first_digest = match classify_activity(&first_activity.bytes) {
            Ok(ActivityState::Inactive) => return Ok(None),
            Ok(ActivityState::Active(digest)) => digest,
            Err(first_error) => {
                tokio::task::yield_now().await;
                let second = self.read_activity(request).await?;
                if second.as_ref() != Some(&first_activity) {
                    return Ok(None);
                }
                return Err(first_error);
            }
        };
        let first_artifacts = self.read_owned_artifacts(request.owned_files()).await?;
        tokio::task::yield_now().await;
        let Some(second_activity) = self.read_activity(request).await? else {
            return Ok(None);
        };
        if first_activity != second_activity {
            return Ok(None);
        }
        let second_digest = match classify_activity(&second_activity.bytes)? {
            ActivityState::Inactive => return Ok(None),
            ActivityState::Active(digest) => digest,
        };
        let second_artifacts = self.read_owned_artifacts(request.owned_files()).await?;
        let first = StableCaptureMaterial {
            digest: *first_digest,
            artifacts: first_artifacts,
        };
        let second = StableCaptureMaterial {
            digest: *second_digest,
            artifacts: second_artifacts,
        };
        self.ensure_activity_healthy()?;
        Ok((first == second).then_some(first))
    }

    async fn read_activity(
        &self,
        request: &SemanticObservationCaptureRequest,
    ) -> Result<Option<ActivityDigestRead>, String> {
        if let Some(health) = &self.activity_health {
            return health
                .read_digest(request.activity_publisher().publisher_id())
                .map(|digest| {
                    digest.map(|digest| ActivityDigestRead {
                        revision: Some(digest.revision),
                        bytes: digest.bytes,
                    })
                });
        }
        let path = self
            .working_dir
            .join(".swarm")
            .join("activity")
            .join(format!("{}.json", activity_digest_key(request.task_id())));
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(Some(ActivityDigestRead {
                revision: None,
                bytes,
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("cannot read activity digest {:?}: {error}", path)),
        }
    }

    async fn read_owned_artifacts(
        &self,
        owned_files: &[String],
    ) -> Result<Vec<OwnedArtifactMaterial>, String> {
        let mut paths = owned_files.to_vec();
        paths.sort();
        paths.dedup();
        let mut artifacts = Vec::with_capacity(paths.len());
        for relative_path in paths {
            let state = self.read_owned_artifact(&relative_path).await?;
            artifacts.push(OwnedArtifactMaterial {
                relative_path,
                state,
            });
        }
        Ok(artifacts)
    }

    async fn read_owned_artifact(&self, relative_path: &str) -> Result<OwnedArtifactState, String> {
        let path = normalized_owned_path(&self.working_dir, relative_path)?;
        ensure_no_symlink_component(&self.working_dir, &path)?;
        let state = match tokio::fs::read(&path).await {
            Ok(bytes) => OwnedArtifactState::Present(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                OwnedArtifactState::Missing
            }
            Err(error) => return Err(format!("cannot read owned artifact {:?}: {error}", path)),
        };
        ensure_no_symlink_component(&self.working_dir, &path)?;
        Ok(state)
    }

    fn task_lane(&self, task_id: &str) -> Arc<AsyncMutex<()>> {
        let mut lanes = unpoison(&self.task_lanes);
        lanes
            .entry(task_id.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}

#[async_trait]
impl SemanticObservationSnapshotProducer for GooseSemanticObservationSnapshotProducer {
    async fn capture(
        &self,
        request: SemanticObservationCaptureRequest,
    ) -> Result<Option<SemanticObservationCapture>, String> {
        if request.task_id().trim().is_empty() {
            return Err("semantic capture request task id is empty".to_string());
        }
        if request.running_model_id().trim().is_empty() {
            return Err("semantic capture request running model is empty".to_string());
        }
        request.activity_publisher().validate()?;
        if request.activity_publisher().task_id() != request.task_id()
            || request.activity_publisher().attempt() != request.attempt()
            || request.activity_publisher().logical_device_id()
                != request.running_logical_device_id()
            || request.activity_publisher().model_id() != request.running_model_id()
        {
            return Err(
                "semantic capture request does not match its engine-minted activity publisher"
                    .to_string(),
            );
        }
        let publisher_id = request.activity_publisher().publisher_id().to_string();
        let lane = self.task_lane(&publisher_id);
        let _guard = lane.lock().await;
        let Some(material) = self.stable_material(&request).await? else {
            return Ok(None);
        };
        material.digest.validate()?;
        if material.digest.model != request.running_model_id() {
            return Err(format!(
                "activity digest model {:?} does not match running model {:?}",
                material.digest.model,
                request.running_model_id()
            ));
        }
        if !material.digest.has_meaningful_trace()
            && !material
                .artifacts
                .iter()
                .any(|artifact| matches!(artifact.state, OwnedArtifactState::Present(_)))
        {
            return Ok(None);
        }

        let artifact_version = artifact_version(&material.artifacts);
        let measurement_hash = measurement_hash(&request, &material.digest, &artifact_version)?;
        let mut state = unpoison(&self.state);
        if let Some(previous) = state.get(&publisher_id) {
            if previous.attempt == request.attempt()
                && previous.measurement_hash == measurement_hash
            {
                return Ok(None);
            }
        }
        let source_revision = state
            .get(&publisher_id)
            .filter(|previous| previous.attempt == request.attempt())
            .map(|previous| previous.source_revision)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| "semantic trace revision overflowed".to_string())?;

        let measurement = TraceStateMeasurement {
            measurement_hash: measurement_hash.clone(),
            tool_calls: material.digest.tool_calls,
            failed_tool_calls: material.digest.errors,
            malformed_tool_calls: material.digest.malformed,
            pending_tool_calls: material.digest.pending_tool_calls(),
            thinking_chars: material.digest.thinking_chars,
            recurrence_window_chars: material.digest.reasoning_recurrence.window_chars,
            recurrence_observed_windows: material.digest.reasoning_recurrence.observed_windows,
            recurrence_repeated_windows: material.digest.reasoning_recurrence.repeated_windows,
            recurrence_repeat_share: material.digest.reasoning_recurrence.repeat_share,
            provider_stream_revision: material.digest.provider_stream.revision,
            provider_stream_chunks: material.digest.provider_stream.chunks,
            provider_stream_bytes: material.digest.provider_stream.bytes,
            provider_structured_output_chunks: material
                .digest
                .provider_stream
                .structured_output_chunks,
            provider_structured_output_bytes: material
                .digest
                .provider_stream
                .structured_output_bytes,
            provider_last_progress_elapsed_ms: material
                .digest
                .provider_stream
                .last_progress_elapsed_ms,
            provider_structured_output_active: material
                .digest
                .provider_stream
                .structured_output_active,
            artifact_version: artifact_version.clone(),
        };
        let summons = SemanticObservationSummonsSignal::TraceStateAdvanced {
            source_id: format!("signal:trace-measurement:{measurement_hash}"),
            measurement,
            provenance: format!(
                "engine publisher {} plus two identical full owned-artifact reads; .swarm/activity/{}.json is an optional UI mirror only, and deterministic measurements grant no intervention authority",
                publisher_id,
                activity_digest_key(request.task_id())
            ),
        };
        let snapshot = SemanticObservationSnapshotDraft {
            schema_version: SEMANTIC_OBSERVATION_SNAPSHOT_SCHEMA,
            authority_scope: request
                .activity_publisher()
                .source()
                .authority_scope
                .clone(),
            phase_epoch: request.activity_publisher().source().phase_epoch,
            task_id: request.task_id().to_string(),
            attempt: request.attempt(),
            source_revision,
            contract_version: request.contract_version().to_string(),
            artifact_version,
            goal: request.goal().to_string(),
            task_contract: request.task_contract().to_string(),
            acceptance_oracle: request.acceptance_oracle().to_vec(),
            dependency_contract_versions: request.dependency_contract_versions().clone(),
            sibling_contract_versions: request.sibling_contract_versions().clone(),
            allowed_finding_routes: request.allowed_finding_routes().to_vec(),
            artifacts: artifact_snapshots(&material.artifacts),
            trace: SemanticTraceSnapshot {
                sequence: source_revision,
                recent_reasoning: trace_reasoning(&material.digest),
                recent_actions: trace_actions(&material.digest),
                prior_intervention: None,
                response_to_prior_intervention: None,
            },
            neutral_signals: vec![summons.neutral_signal()],
        }
        .seal()
        .map_err(|error| error.to_string())?;
        let capture = SemanticObservationCapture::new(snapshot, summons)?;
        state.insert(
            publisher_id,
            CapturedTraceState {
                attempt: request.attempt(),
                measurement_hash,
                source_revision,
            },
        );
        Ok(Some(capture))
    }
}

enum ActivityState {
    Inactive,
    Active(Box<WorkerActivityDigest>),
}

fn classify_activity(bytes: &[u8]) -> Result<ActivityState, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("activity digest is not valid JSON: {error}"))?;
    let phase = value.get("phase");
    match phase {
        Some(serde_json::Value::String(phase)) if phase == "processing" || phase == "done" => {
            return Ok(ActivityState::Inactive)
        }
        Some(serde_json::Value::String(phase)) => {
            return Err(format!("unknown activity digest phase {phase:?}"))
        }
        Some(_) => return Err("activity digest phase is not a string".to_string()),
        None => {}
    }
    let digest: WorkerActivityDigest = serde_json::from_value(value)
        .map_err(|error| format!("active activity digest violates its typed schema: {error}"))?;
    Ok(ActivityState::Active(Box::new(digest)))
}

fn normalized_owned_path(root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(relative_path);
    if relative_path.trim().is_empty() || candidate.is_absolute() {
        return Err(format!(
            "owned artifact path {relative_path:?} is not relative"
        ));
    }
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "owned artifact path {relative_path:?} escapes the working directory"
                ))
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(format!("owned artifact path {relative_path:?} is empty"));
    }
    Ok(root.join(normalized))
}

fn ensure_no_symlink_component(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path.strip_prefix(root).map_err(|_| {
        format!(
            "owned artifact path {:?} is outside working directory {:?}",
            path, root
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "owned artifact path {:?} traverses symlink {:?}",
                    path, current
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(format!(
                    "cannot validate owned artifact component {:?}: {error}",
                    current
                ))
            }
        }
    }
    Ok(())
}

fn artifact_version(artifacts: &[OwnedArtifactMaterial]) -> String {
    let mut hasher = Sha256::new();
    for artifact in artifacts {
        hash_sized(&mut hasher, artifact.relative_path.as_bytes());
        match &artifact.state {
            OwnedArtifactState::Missing => hasher.update([0]),
            OwnedArtifactState::Present(bytes) => {
                hasher.update([1]);
                hash_sized(&mut hasher, bytes);
            }
        }
    }
    let digest = hasher.finalize();
    sha256_label(digest.as_ref())
}

fn measurement_hash(
    request: &SemanticObservationCaptureRequest,
    digest: &WorkerActivityDigest,
    artifact_version: &str,
) -> Result<String, String> {
    #[derive(Serialize)]
    struct MeasurementIdentity<'a> {
        request: &'a SemanticObservationCaptureRequest,
        digest: &'a WorkerActivityDigest,
        artifact_version: &'a str,
    }
    let bytes = serde_json::to_vec(&MeasurementIdentity {
        request,
        digest,
        artifact_version,
    })
    .map_err(|error| format!("cannot serialize semantic measurement identity: {error}"))?;
    let digest = Sha256::digest(bytes);
    Ok(sha256_label(digest.as_ref()))
}

fn sha256_label(digest: &[u8]) -> String {
    let mut label = String::with_capacity("sha256:".len() + digest.len() * 2);
    label.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut label, "{byte:02x}").expect("writing to a String cannot fail");
    }
    label
}

fn hash_sized(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn artifact_snapshots(artifacts: &[OwnedArtifactMaterial]) -> Vec<ArtifactExcerptSnapshot> {
    artifacts
        .iter()
        .map(|artifact| ArtifactExcerptSnapshot {
            source_id: format!("artifact:{}", artifact.relative_path),
            path: artifact.relative_path.clone(),
            excerpt: match &artifact.state {
                OwnedArtifactState::Missing => "<owned artifact is missing>".to_string(),
                OwnedArtifactState::Present(bytes) => match String::from_utf8(bytes.clone()) {
                    Ok(text) => text,
                    Err(_) => format!(
                        "BINARY_BASE64:{}",
                        base64::engine::general_purpose::STANDARD.encode(bytes)
                    ),
                },
            },
            complete: true,
        })
        .collect()
}

fn trace_reasoning(digest: &WorkerActivityDigest) -> String {
    let sections = [
        (
            "EARLIER REASONING",
            digest.reasoning_recurrence.earlier_reasoning.as_str(),
        ),
        ("FULL RECENT REASONING", digest.full_reasoning.as_str()),
        ("RECENT REASONING", digest.reasoning.as_str()),
        ("RECENT THINKING", digest.last_thinking.as_str()),
        ("LAST TEXT", digest.last_text.as_str()),
    ];
    sections
        .into_iter()
        .filter(|(_, text)| !text.trim().is_empty())
        .map(|(label, text)| format!("{label}:\n{text}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn trace_actions(digest: &WorkerActivityDigest) -> Vec<String> {
    let mut actions = Vec::new();
    if digest.provider_stream.revision > 0 {
        actions.push(format!(
            "provider stream | revision={} chunks={} bytes={} structured_chunks={} structured_bytes={} structured_active={} last_progress_elapsed_ms={} payload_logged=false",
            digest.provider_stream.revision,
            digest.provider_stream.chunks,
            digest.provider_stream.bytes,
            digest.provider_stream.structured_output_chunks,
            digest.provider_stream.structured_output_bytes,
            digest.provider_stream.structured_output_active,
            digest.provider_stream.last_progress_elapsed_ms,
        ));
    }
    actions.extend(digest.calls.iter().map(|call| {
        format!(
            "{} | {} | status={} | mcp={} | {}",
            call.name,
            call.summary,
            call.ok
                .map(|ok| if ok { "ok" } else { "error" })
                .unwrap_or("pending"),
            call.is_mcp,
            call.result
        )
    }));
    actions
}

#[derive(Clone)]
pub struct GooseSemanticProviderRoute {
    provider_name: String,
    provider: Arc<dyn Provider>,
    lane: VerifiedPhysicalLane,
}

impl GooseSemanticProviderRoute {
    pub fn bind(
        provider_name: impl Into<String>,
        provider: Arc<dyn Provider>,
        lane: VerifiedPhysicalLane,
    ) -> Result<Self, String> {
        let provider_name = provider_name.into();
        if provider_name.trim().is_empty() {
            return Err("semantic provider registry name is empty".to_string());
        }
        if provider.get_name() != provider_name {
            return Err(format!(
                "semantic provider reports name {:?}, expected {:?}",
                provider.get_name(),
                provider_name
            ));
        }
        if !provider.supports_terminal_proven_single_attempt_streaming() {
            return Err(format!(
                "semantic provider {:?} has no terminal-proven single-attempt stream boundary",
                provider_name
            ));
        }
        let provider_transport_id =
            provider.transport_identity(&lane.model_id).ok_or_else(|| {
                format!(
                    "semantic provider {:?} exposes no transport identity",
                    provider_name
                )
            })?;
        if provider_transport_id != lane.provider_transport_id {
            return Err(format!(
                "semantic provider {:?} transport does not match verified lane {:?}",
                provider_name, lane.logical_device_id
            ));
        }
        Ok(Self {
            provider_name,
            provider,
            lane,
        })
    }
}

pub struct GooseAdmittedSemanticObservationReviewer {
    fleet_snapshot: PhysicalFleetSnapshot,
    routes: BTreeMap<String, GooseSemanticProviderRoute>,
    temperature: Option<f32>,
    request_params: HashMap<String, serde_json::Value>,
}

enum SemanticProviderCallError {
    BeforeStream(ProviderError),
    DuringStream(ProviderError, SingleAttemptStreamOutcome),
}

impl GooseAdmittedSemanticObservationReviewer {
    pub fn new(
        fleet_snapshot: PhysicalFleetSnapshot,
        routes: Vec<GooseSemanticProviderRoute>,
        temperature: Option<f32>,
        request_params: HashMap<String, serde_json::Value>,
    ) -> Result<Self, String> {
        let mut by_logical_device = BTreeMap::new();
        for route in routes {
            let matching_lane = fleet_snapshot
                .lanes
                .iter()
                .find(|lane| lane.logical_device_id == route.lane.logical_device_id)
                .ok_or_else(|| {
                    format!(
                        "semantic provider route {:?} is absent from fleet snapshot {:?}",
                        route.lane.logical_device_id, fleet_snapshot.snapshot_id
                    )
                })?;
            if matching_lane != &route.lane {
                return Err(format!(
                    "semantic provider route {:?} does not match its sealed fleet lane",
                    route.lane.logical_device_id
                ));
            }
            let route_id = route.lane.logical_device_id.clone();
            if by_logical_device.insert(route_id.clone(), route).is_some() {
                return Err(format!(
                    "duplicate semantic provider route for logical device {route_id:?}"
                ));
            }
        }
        if by_logical_device.is_empty() {
            return Err("semantic reviewer has no verified provider routes".to_string());
        }
        Ok(Self {
            fleet_snapshot,
            routes: by_logical_device,
            temperature,
            request_params,
        })
    }

    fn route_for(
        &self,
        request: &AdmittedSemanticObservationRequest,
    ) -> Result<&GooseSemanticProviderRoute, String> {
        let admission = &request.admission;
        if admission.role != WorkRole::SemanticJudgeObservation {
            return Err(format!(
                "admission role {:?} is not semantic observation",
                admission.role
            ));
        }
        if admission.fleet_snapshot_id != self.fleet_snapshot.snapshot_id {
            return Err(format!(
                "admission fleet {:?} does not match reviewer fleet {:?}",
                admission.fleet_snapshot_id, self.fleet_snapshot.snapshot_id
            ));
        }
        let snapshot = &request.observation.snapshot;
        if admission.source.task_id != snapshot.task_id()
            || admission.source.attempt != snapshot.attempt()
            || admission.source.revision != snapshot.source_revision()
        {
            return Err("admission trace identity does not match semantic snapshot".to_string());
        }
        match &admission.source.kind {
            SourceRevisionKind::Trace {
                trace_sequence,
                snapshot_hash,
            } if *trace_sequence == snapshot.source_revision()
                && snapshot_hash == snapshot.snapshot_hash() => {}
            _ => return Err("admission source is not the exact sealed trace revision".to_string()),
        }
        let route = self
            .routes
            .get(&admission.logical_device_id)
            .ok_or_else(|| {
                format!(
                    "no verified semantic provider is bound to admitted logical device {:?}",
                    admission.logical_device_id
                )
            })?;
        require_exact_route(admission, &route.lane)?;
        if route.provider.get_name() != route.provider_name {
            return Err(format!(
                "provider binding for {:?} drifted from {:?} to {:?}",
                admission.logical_device_id,
                route.provider_name,
                route.provider.get_name()
            ));
        }
        if route
            .provider
            .transport_identity(&request.admission.model_id)
            .as_deref()
            != Some(route.lane.provider_transport_id.as_str())
        {
            return Err(format!(
                "provider transport for {:?} drifted from its sealed physical lane",
                admission.logical_device_id
            ));
        }
        Ok(route)
    }

    fn model_config_for(
        &self,
        request: &AdmittedSemanticObservationRequest,
        route: &GooseSemanticProviderRoute,
    ) -> Result<goose_provider_types::model::ModelConfig, String> {
        let mut response_params = self.request_params.clone();
        response_params.insert(
            "response_format".to_string(),
            serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "semantic_judge_observation",
                    "strict": true,
                    "schema": request.observation.response_schema,
                }
            }),
        );
        Ok(goose::model_config::model_config_from_user_config(
            &route.provider_name,
            &request.admission.model_id,
        )
        .map_err(|error| format!("cannot build semantic reviewer model config: {error}"))?
        .with_temperature(self.temperature)
        .with_toolshim(false)
        .with_toolshim_model(None)
        .with_merged_request_params(response_params))
    }
}

#[async_trait]
impl AdmittedSemanticObservationReviewer for GooseAdmittedSemanticObservationReviewer {
    fn verify_admission(&self, request: &AdmittedSemanticObservationRequest) -> Result<(), String> {
        let route = self.route_for(request)?;
        self.model_config_for(request, route).map(|_| ())
    }

    fn eligible_logical_device_ids(&self) -> Option<Vec<String>> {
        Some(self.routes.keys().cloned().collect())
    }

    async fn review(
        &self,
        request: AdmittedSemanticObservationRequest,
    ) -> Result<String, AdmittedSemanticReviewError> {
        let route = self
            .route_for(&request)
            .map_err(AdmittedSemanticReviewError::unresolved)?;
        let model_config = self
            .model_config_for(&request, route)
            .map_err(AdmittedSemanticReviewError::unresolved)?;
        let messages = [Message::user().with_text(request.observation.user_prompt.clone())];
        let provider_request_id = request.provider_request_id.clone();
        let result = goose::session_context::with_session_id(provider_request_id, async {
            let single_attempt = route
                .provider
                .stream_once_with_terminal_proof(
                    &model_config,
                    &request.observation.system_prompt,
                    &messages,
                    &[],
                )
                .await
                .map_err(SemanticProviderCallError::BeforeStream)?;
            let terminal = single_attempt.terminal;
            match collect_stream(single_attempt.stream).await {
                Ok(message) => Ok((message, terminal.outcome())),
                Err(error) => Err(SemanticProviderCallError::DuringStream(
                    error,
                    terminal.outcome(),
                )),
            }
        })
        .await;
        let (message, _) = match result {
            Ok((message, SingleAttemptStreamOutcome::Finished)) => message,
            Ok((_, SingleAttemptStreamOutcome::Failed)) => {
                return Err(AdmittedSemanticReviewError::terminal_failure(
                    "semantic reviewer provider reported an explicit failed terminal".to_string(),
                ));
            }
            Ok((_, SingleAttemptStreamOutcome::Pending)) => {
                return Err(AdmittedSemanticReviewError::unresolved(
                    "semantic reviewer stream ended without explicit provider terminal proof"
                        .to_string(),
                ));
            }
            Err(SemanticProviderCallError::BeforeStream(error))
                if route.provider.single_attempt_failure_provenance(&error)
                    == SingleAttemptFailureProvenance::TerminalResponse =>
            {
                return Err(AdmittedSemanticReviewError::terminal_failure(format!(
                    "semantic reviewer provider returned a terminal failure: {error}"
                )));
            }
            Err(SemanticProviderCallError::DuringStream(
                error,
                SingleAttemptStreamOutcome::Finished,
            )) => {
                return Err(AdmittedSemanticReviewError::local_failure_after_terminal(
                    format!(
                        "semantic reviewer failed locally after an explicit finished provider terminal: {error}"
                    ),
                    goose_swarm::ProviderTerminalKind::Finished,
                ));
            }
            Err(SemanticProviderCallError::DuringStream(
                error,
                SingleAttemptStreamOutcome::Failed,
            )) => {
                return Err(AdmittedSemanticReviewError::local_failure_after_terminal(
                    format!(
                        "semantic reviewer failed locally after an explicit failed provider terminal: {error}"
                    ),
                    goose_swarm::ProviderTerminalKind::Failed,
                ));
            }
            Err(SemanticProviderCallError::BeforeStream(error))
            | Err(SemanticProviderCallError::DuringStream(
                error,
                SingleAttemptStreamOutcome::Pending,
            )) => {
                return Err(AdmittedSemanticReviewError::unresolved(format!(
                    "semantic reviewer provider lifecycle is unresolved: {error}"
                )));
            }
        };
        Ok(strict_text_response(&message.content).unwrap_or_else(|_| "{}".to_string()))
    }
}

fn require_exact_route(
    admission: &goose_swarm::AdmissionReceipt,
    lane: &VerifiedPhysicalLane,
) -> Result<(), String> {
    let expected = (
        lane.logical_device_id.as_str(),
        lane.model_id.as_str(),
        lane.host_id.as_str(),
        lane.model_instance_id.as_str(),
        lane.provider_transport_id.as_str(),
        lane.route_evidence_id.as_str(),
        &lane.capacity_evidence,
    );
    let actual = (
        admission.logical_device_id.as_str(),
        admission.model_id.as_str(),
        admission.physical_host_id.as_str(),
        admission.model_instance_id.as_str(),
        admission.provider_transport_id.as_str(),
        admission.route_evidence_id.as_str(),
        &admission.capacity_evidence,
    );
    if actual != expected {
        return Err(format!(
            "admission route for {:?} does not match its verified provider lane",
            admission.logical_device_id
        ));
    }
    Ok(())
}

fn strict_text_response(content: &[MessageContent]) -> Result<String, String> {
    let mut text = String::new();
    for item in content {
        match item {
            MessageContent::Text(part) => text.push_str(&part.text),
            MessageContent::Thinking(_) | MessageContent::RedactedThinking(_) => {}
            MessageContent::Image(_)
            | MessageContent::ToolRequest(_)
            | MessageContent::ToolResponse(_)
            | MessageContent::ToolConfirmationRequest(_)
            | MessageContent::ActionRequired(_)
            | MessageContent::FrontendToolRequest(_)
            | MessageContent::SystemNotification(_) => {
                return Err("semantic reviewer returned non-text action content".to_string())
            }
        }
    }
    if text.trim().is_empty() {
        return Err("semantic reviewer returned no text JSON body".to_string());
    }
    Ok(text)
}

fn unpoison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose::providers::base::{stream_from_single_message, MessageStream, ProviderUsage, Usage};
    use goose_provider_types::errors::ProviderError;
    use goose_provider_types::model::ModelConfig;
    use goose_swarm::{
        AcceptanceCriterionSnapshot, AuthorityScope, BrokeredSemanticObservationPlane, EventSink,
        HostCapacityEvidence, PhysicalAdmissionControl, SemanticObservationAdmissionPolicy,
        SemanticObservationAdmissionSubmission, SwarmEvent,
    };
    use rmcp::model::Tool;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const VERIFIED_TRANSPORT: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER_TRANSPORT: &str =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<Vec<serde_json::Value>>,
    }

    impl RecordingSink {
        fn count(&self, event: &str) -> usize {
            unpoison(&self.events)
                .iter()
                .filter(|value| value["event"] == event)
                .count()
        }
    }

    impl EventSink for RecordingSink {
        fn emit(&self, event: &SwarmEvent) {
            unpoison(&self.events).push(serde_json::to_value(event).unwrap());
        }

        fn write_value(&self, value: serde_json::Value) {
            unpoison(&self.events).push(value);
        }
    }

    #[derive(Clone, Debug)]
    struct CapturedProviderCall {
        model: String,
        request_params: Option<HashMap<String, serde_json::Value>>,
        tools: usize,
        session_id: Option<String>,
    }

    struct MockProvider {
        reply: String,
        transport_identity: Option<String>,
        stream_calls: AtomicUsize,
        stream_once_calls: AtomicUsize,
        complete_calls: AtomicUsize,
        calls: Mutex<Vec<CapturedProviderCall>>,
    }

    struct RetryOnlyProvider;

    #[async_trait]
    impl Provider for RetryOnlyProvider {
        fn get_name(&self) -> &str {
            "lmstudio"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            unreachable!("route binding must reject this provider before dispatch")
        }
    }

    impl MockProvider {
        fn new(reply: String) -> Self {
            Self {
                reply,
                transport_identity: Some(VERIFIED_TRANSPORT.to_string()),
                stream_calls: AtomicUsize::new(0),
                stream_once_calls: AtomicUsize::new(0),
                complete_calls: AtomicUsize::new(0),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn with_transport(mut self, transport_identity: Option<&str>) -> Self {
            self.transport_identity = transport_identity.map(str::to_string);
            self
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn get_name(&self) -> &str {
            "lmstudio"
        }

        fn transport_identity(&self, _model_name: &str) -> Option<String> {
            self.transport_identity.clone()
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.stream_calls.fetch_add(1, Ordering::SeqCst);
            Err(ProviderError::ExecutionError(
                "adapter must not call the retry-capable stream boundary".to_string(),
            ))
        }

        fn supports_single_attempt_streaming(&self) -> bool {
            true
        }

        fn supports_terminal_proven_single_attempt_streaming(&self) -> bool {
            true
        }

        async fn stream_once(
            &self,
            model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.stream_once_calls.fetch_add(1, Ordering::SeqCst);
            unpoison(&self.calls).push(CapturedProviderCall {
                model: model_config.model_name.clone(),
                request_params: model_config.request_params.clone(),
                tools: tools.len(),
                session_id: goose::session_context::current_session_id(),
            });
            Ok(stream_from_single_message(
                Message::assistant().with_text(self.reply.clone()),
                ProviderUsage::new("mock".to_string(), Usage::default()),
            ))
        }

        async fn stream_once_with_terminal_proof(
            &self,
            model_config: &ModelConfig,
            system: &str,
            messages: &[Message],
            tools: &[Tool],
        ) -> Result<goose::providers::base::SingleAttemptStream, ProviderError> {
            Ok(goose::providers::base::SingleAttemptStream::finished(
                self.stream_once(model_config, system, messages, tools)
                    .await?,
            ))
        }

        async fn complete(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<(Message, ProviderUsage), ProviderError> {
            self.complete_calls.fetch_add(1, Ordering::SeqCst);
            Err(ProviderError::ExecutionError(
                "adapter must bypass Provider::complete overrides".to_string(),
            ))
        }
    }

    fn capture_request() -> SemanticObservationCaptureRequest {
        capture_request_for(0, 1)
    }

    fn capture_request_for(
        attempt: u32,
        admission_sequence: u64,
    ) -> SemanticObservationCaptureRequest {
        capture_request_with_owned_files(
            attempt,
            admission_sequence,
            vec!["src/api.rs".to_string()],
        )
    }

    fn capture_request_with_owned_files(
        attempt: u32,
        admission_sequence: u64,
        owned_files: Vec<String>,
    ) -> SemanticObservationCaptureRequest {
        let capacity_evidence = HostCapacityEvidence::MeasuredProfile {
            profile_hash: "profile-worker-lane".to_string(),
            profile_key: "runtime:model:context:build".to_string(),
            max_concurrent: 1,
        };
        let activity_publisher =
            SemanticActivityPublisher::from_admission(&goose_swarm::AdmissionReceipt {
                admission_id: format!("run-a:admission:{admission_sequence:08}"),
                work_id: format!("task:build-api:attempt:{attempt}"),
                role: WorkRole::Build,
                priority: goose_swarm::WorkPriority::Implementation,
                task_rank: 7,
                source: goose_swarm::TaskVersion {
                    authority_scope: AuthorityScope::new("semantic-cli-replay", "build"),
                    phase_epoch: 0,
                    task_id: "build-api".to_string(),
                    attempt,
                    revision: u64::from(attempt) + 1,
                    kind: SourceRevisionKind::TaskAttempt,
                },
                fleet_snapshot_id: "fleet-a".to_string(),
                logical_device_id: "worker-lane".to_string(),
                model_id: "worker-model".to_string(),
                physical_host_id: "host-worker-lane".to_string(),
                model_instance_id: "instance-worker-lane".to_string(),
                provider_transport_id: VERIFIED_TRANSPORT.to_string(),
                route_evidence_id: "route-worker-lane".to_string(),
                capacity_evidence,
                queue_sequence: admission_sequence,
                admission_sequence,
            });
        SemanticObservationCaptureRequest::observation_only(
            "build-api".to_string(),
            attempt,
            7,
            "Build the sealed API contract".to_string(),
            "Implement the owned handler and prove the response".to_string(),
            owned_files,
            "contract-v1".to_string(),
            vec![AcceptanceCriterionSnapshot {
                id: "handler".to_string(),
                text: "The owned handler implements the frozen response".to_string(),
            }],
            BTreeMap::new(),
            BTreeMap::new(),
            vec!["integrate-verify".to_string()],
            "worker-lane".to_string(),
            "worker-model".to_string(),
            activity_publisher,
        )
        .expect("test capture request matches its admitted publisher")
    }

    fn recurrence() -> ReasoningRecurrenceSnapshot {
        ReasoningRecurrenceSnapshot {
            window_chars: 48,
            observed_windows: 100,
            repeated_windows: 25,
            repeat_share: 0.25,
            earlier_reasoning: "earlier endpoint analysis".to_string(),
        }
    }

    fn active_digest() -> serde_json::Value {
        serde_json::json!({
            "tool_calls": 1,
            "errors": 0,
            "malformed": 0,
            "recent": ["developer__text_editor ok"],
            "last_text": "I wrote the handler and will run its contract check.",
            "calls": [{
                "name": "developer__text_editor",
                "summary": "write src/api.rs",
                "ok": true,
                "result": "wrote file",
                "is_mcp": true
            }],
            "reasoning": "The handler now covers the required response.",
            "full_reasoning": "I mapped the frozen response and wrote the exact handler.",
            "thinking_chars": 180,
            "last_thinking": "Next I will run the contract check.",
            "model": "worker-model",
            "reasoning_recurrence": recurrence(),
        })
    }

    fn provider_only_digest(revision: u64, bytes: u64) -> serde_json::Value {
        serde_json::json!({
            "tool_calls": 0,
            "errors": 0,
            "malformed": 0,
            "recent": [],
            "last_text": "",
            "calls": [],
            "reasoning": "",
            "full_reasoning": "",
            "thinking_chars": 0,
            "last_thinking": "",
            "model": "worker-model",
            "reasoning_recurrence": {
                "window_chars": 48,
                "observed_windows": 0,
                "repeated_windows": 0,
                "repeat_share": 0.0,
                "earlier_reasoning": ""
            },
            "provider_stream": {
                "revision": revision,
                "chunks": revision,
                "bytes": bytes,
                "structured_output_chunks": revision,
                "structured_output_bytes": bytes,
                "last_progress_elapsed_ms": revision * 100,
                "structured_output_active": true
            }
        })
    }

    fn write_activity(root: &Path, task_id: &str, value: serde_json::Value) {
        let activity = root.join(".swarm/activity");
        std::fs::create_dir_all(&activity).unwrap();
        std::fs::write(
            activity.join(format!("{}.json", activity_digest_key(task_id))),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }

    fn lane(logical_device_id: &str) -> VerifiedPhysicalLane {
        VerifiedPhysicalLane {
            logical_device_id: logical_device_id.to_string(),
            model_id: "judge-model".to_string(),
            host_id: format!("host-{logical_device_id}"),
            model_instance_id: format!("instance-{logical_device_id}"),
            provider_transport_id: VERIFIED_TRANSPORT.to_string(),
            advertised_instance_capacity: 1,
            routing_weight: 1,
            capacity_evidence: HostCapacityEvidence::MeasuredProfile {
                profile_hash: format!("profile-{logical_device_id}"),
                profile_key: "runtime:model:context:semantic-observation".to_string(),
                max_concurrent: 1,
            },
            route_evidence_id: format!("route-{logical_device_id}"),
        }
    }

    #[test]
    fn provider_route_rejects_a_retry_only_provider_before_admission() {
        let error = GooseSemanticProviderRoute::bind(
            "lmstudio",
            Arc::new(RetryOnlyProvider),
            lane("judge-lane"),
        )
        .err()
        .expect("retry-only provider must be rejected");
        assert!(error.contains("single-attempt stream boundary"));
    }

    #[test]
    fn provider_route_requires_the_sealed_transport_endpoint() {
        for provider in [
            MockProvider::new("unused".to_string()).with_transport(None),
            MockProvider::new("unused".to_string()).with_transport(Some(OTHER_TRANSPORT)),
        ] {
            let error = GooseSemanticProviderRoute::bind(
                "lmstudio",
                Arc::new(provider),
                lane("judge-lane"),
            )
            .err()
            .expect("unproved transport must be rejected");
            assert!(error.contains("transport"));
        }
    }

    #[test]
    fn physical_activity_sink_probes_and_atomically_replaces_digest() {
        let root = tempfile::tempdir().unwrap();
        let events = Arc::new(RecordingSink::default());
        let health = ActivitySinkHealth::new(root.path(), events.clone()).unwrap();
        health.activate().unwrap();

        let request = capture_request();
        let path = health.activity_path(request.task_id());
        health
            .write_digest(
                &path,
                &serde_json::to_vec(&active_digest()).unwrap(),
                request.task_id(),
                Some(request.activity_publisher()),
                "test",
                true,
            )
            .unwrap();
        let mut second = active_digest();
        second["last_text"] = serde_json::json!("second authoritative revision");
        let second = serde_json::to_vec(&second).unwrap();
        health
            .write_digest(
                &path,
                &second,
                request.task_id(),
                Some(request.activity_publisher()),
                "test",
                true,
            )
            .unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), second);
        assert_eq!(events.count("physical_semantic_activity_sink_ready"), 1);
        assert!(std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")));
    }

    #[tokio::test]
    async fn unavailable_ui_mirror_does_not_disable_engine_activity_authority() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".swarm"), b"blocks the optional mirror").unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/api.rs"), "pub fn api() {}\n").unwrap();
        let request = capture_request();
        let events = Arc::new(RecordingSink::default());
        let health = Arc::new(ActivitySinkHealth::new(root.path(), events.clone()).unwrap());

        health.activate().unwrap();
        health
            .write_digest(
                &health.activity_path(request.task_id()),
                &serde_json::to_vec(&active_digest()).unwrap(),
                request.task_id(),
                Some(request.activity_publisher()),
                "stream",
                true,
            )
            .unwrap();
        let producer =
            GooseSemanticObservationSnapshotProducer::new_with_activity_health(root.path(), health)
                .unwrap();

        assert!(producer.capture(request).await.unwrap().is_some());
        assert_eq!(events.count("physical_semantic_activity_sink_ready"), 1);
        assert_eq!(
            events.count("physical_semantic_activity_mirror_degraded"),
            1
        );
        assert_eq!(events.count("physical_semantic_activity_sink_degraded"), 0);
    }

    #[tokio::test]
    async fn physical_producer_uses_authoritative_digest_when_ui_mirror_degrades() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/api.rs"), "pub fn api() {}\n").unwrap();
        let request = capture_request();
        let events = Arc::new(RecordingSink::default());
        let health = Arc::new(ActivitySinkHealth::new(root.path(), events.clone()).unwrap());
        health.activate().unwrap();
        let activity_path = health.activity_path(request.task_id());
        health
            .write_digest(
                &activity_path,
                &serde_json::to_vec(&active_digest()).unwrap(),
                request.task_id(),
                Some(request.activity_publisher()),
                "seed",
                true,
            )
            .unwrap();
        let producer = GooseSemanticObservationSnapshotProducer::new_with_activity_health(
            root.path(),
            health.clone(),
        )
        .unwrap();

        std::fs::write(
            &activity_path,
            serde_json::to_vec(&serde_json::json!({
                "model": "worker-model",
                "phase": "done"
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(producer.capture(request.clone()).await.unwrap().is_some());

        std::fs::remove_file(&activity_path).unwrap();
        std::fs::remove_dir(activity_path.parent().unwrap()).unwrap();
        std::fs::write(activity_path.parent().unwrap(), b"block").unwrap();
        let mut advanced = active_digest();
        advanced["last_text"] = serde_json::json!("authoritative memory advanced");
        health
            .write_digest(
                &activity_path,
                &serde_json::to_vec(&advanced).unwrap(),
                request.task_id(),
                Some(request.activity_publisher()),
                "stream",
                true,
            )
            .unwrap();
        health
            .write_digest(
                &activity_path,
                &serde_json::to_vec(&advanced).unwrap(),
                request.task_id(),
                Some(request.activity_publisher()),
                "final",
                true,
            )
            .unwrap();

        assert!(producer.capture(request).await.unwrap().is_some());
        assert_eq!(
            events.count("physical_semantic_activity_mirror_degraded"),
            1
        );
        assert_eq!(events.count("physical_semantic_activity_sink_degraded"), 0);
    }

    #[tokio::test]
    async fn malformed_authoritative_digest_latches_and_blocks_later_provider_turns() {
        let root = tempfile::tempdir().unwrap();
        let request = capture_request();
        let events = Arc::new(RecordingSink::default());
        let health = Arc::new(ActivitySinkHealth::new(root.path(), events.clone()).unwrap());
        health.activate().unwrap();
        let activity_path = health.activity_path(request.task_id());

        let first = health
            .write_digest(
                &activity_path,
                b"not-json",
                request.task_id(),
                Some(request.activity_publisher()),
                "stream",
                true,
            )
            .unwrap_err();
        assert!(first.contains("not valid JSON"));
        let later = health
            .write_digest(
                &activity_path,
                &serde_json::to_vec(&active_digest()).unwrap(),
                request.task_id(),
                Some(request.activity_publisher()),
                "final",
                true,
            )
            .unwrap_err();
        assert!(later.contains("authority degraded"));

        let producer =
            GooseSemanticObservationSnapshotProducer::new_with_activity_health(root.path(), health)
                .unwrap();
        assert!(producer
            .capture(request)
            .await
            .unwrap_err()
            .contains("authority degraded"));
        assert_eq!(events.count("physical_semantic_activity_sink_degraded"), 1);
    }

    #[tokio::test]
    async fn retry_publisher_cannot_consume_an_earlier_attempt_digest() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/api.rs"), "pub fn api() {}\n").unwrap();
        let first = capture_request_for(0, 1);
        let retry = capture_request_for(1, 2);
        let health = Arc::new(
            ActivitySinkHealth::new(root.path(), Arc::new(RecordingSink::default())).unwrap(),
        );
        health.activate().unwrap();
        let activity_path = health.activity_path(first.task_id());
        health
            .write_digest(
                &activity_path,
                &serde_json::to_vec(&active_digest()).unwrap(),
                first.task_id(),
                Some(first.activity_publisher()),
                "stream",
                true,
            )
            .unwrap();
        let producer = GooseSemanticObservationSnapshotProducer::new_with_activity_health(
            root.path(),
            health.clone(),
        )
        .unwrap();

        assert!(producer.capture(retry.clone()).await.unwrap().is_none());
        health
            .write_digest(
                &activity_path,
                &serde_json::to_vec(&active_digest()).unwrap(),
                retry.task_id(),
                Some(retry.activity_publisher()),
                "stream",
                true,
            )
            .unwrap();
        assert!(producer.capture(retry).await.unwrap().is_some());
    }

    #[test]
    fn physical_activity_authority_rejects_digest_model_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let request = capture_request();
        let events = Arc::new(RecordingSink::default());
        let health = ActivitySinkHealth::new(root.path(), events.clone()).unwrap();
        health.activate().unwrap();
        let path = health.activity_path(request.task_id());
        let mut wrong = active_digest();
        wrong["model"] = serde_json::json!("other-model");
        let error = health
            .write_digest(
                &path,
                &serde_json::to_vec(&wrong).unwrap(),
                request.task_id(),
                Some(request.activity_publisher()),
                "stream",
                true,
            )
            .unwrap_err();
        assert!(error.contains("does not match publisher model"));
        assert_eq!(events.count("physical_semantic_activity_sink_degraded"), 1);
    }

    #[tokio::test]
    async fn producer_seals_one_revision_for_identical_concurrent_state_and_advances_on_change() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/api.rs"), "pub fn api() {}\n").unwrap();
        let request = capture_request();
        write_activity(root.path(), request.task_id(), active_digest());
        let producer =
            Arc::new(GooseSemanticObservationSnapshotProducer::new(root.path()).unwrap());

        let (left, right) = tokio::join!(
            producer.capture(request.clone()),
            producer.capture(request.clone())
        );
        let (left, right) = (left.unwrap(), right.unwrap());
        assert_ne!(
            left.is_some(),
            right.is_some(),
            "identical concurrent polls must mint exactly one capture"
        );
        let captured = left.or(right).unwrap();
        assert_eq!(
            captured.snapshot().payload().artifacts[0].excerpt,
            "pub fn api() {}\n"
        );
        assert!(captured.snapshot().payload().artifacts[0].complete);

        std::fs::write(
            root.path().join("src/api.rs"),
            "pub fn api() { println!(\"advanced\"); }\n",
        )
        .unwrap();
        let advanced = producer.capture(request).await.unwrap().unwrap();
        assert_eq!(advanced.snapshot().source_revision(), 2);
        assert_ne!(
            advanced.snapshot().snapshot_hash(),
            captured.snapshot().snapshot_hash()
        );
    }

    #[tokio::test]
    async fn provider_progress_alone_advances_a_payload_free_semantic_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let request = capture_request();
        let producer = GooseSemanticObservationSnapshotProducer::new(root.path()).unwrap();

        write_activity(
            root.path(),
            request.task_id(),
            provider_only_digest(4, 16_384),
        );
        let first = producer.capture(request.clone()).await.unwrap().unwrap();
        let SemanticObservationSummonsSignal::TraceStateAdvanced { measurement, .. } =
            first.summons();
        assert_eq!(measurement.provider_stream_revision, 4);
        assert_eq!(measurement.provider_structured_output_bytes, 16_384);
        assert!(measurement.provider_structured_output_active);
        assert!(first
            .snapshot()
            .payload()
            .trace
            .recent_actions
            .iter()
            .any(|action| action.contains("payload_logged=false")));

        write_activity(
            root.path(),
            request.task_id(),
            provider_only_digest(5, 20_480),
        );
        let advanced = producer.capture(request).await.unwrap().unwrap();
        assert_eq!(advanced.snapshot().source_revision(), 2);
        assert_ne!(
            advanced.snapshot().snapshot_hash(),
            first.snapshot().snapshot_hash()
        );
    }

    #[tokio::test]
    async fn producer_skips_inactive_state_and_rejects_partial_or_escaped_evidence() {
        let root = tempfile::tempdir().unwrap();
        let request = capture_request();
        let producer = GooseSemanticObservationSnapshotProducer::new(root.path()).unwrap();
        write_activity(
            root.path(),
            request.task_id(),
            serde_json::json!({"model": "worker-model", "phase": "processing"}),
        );
        assert!(producer.capture(request.clone()).await.unwrap().is_none());

        write_activity(
            root.path(),
            request.task_id(),
            serde_json::json!({"model": "worker-model"}),
        );
        assert!(producer.capture(request.clone()).await.is_err());

        let escaped = capture_request_with_owned_files(
            request.attempt(),
            1,
            vec!["../outside.rs".to_string()],
        );
        write_activity(root.path(), escaped.task_id(), active_digest());
        assert!(producer.capture(escaped).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn producer_rejects_owned_file_symlink_even_when_target_exists() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(outside.path().join("secret.rs"), "secret").unwrap();
        symlink(
            outside.path().join("secret.rs"),
            root.path().join("src/api.rs"),
        )
        .unwrap();
        let request = capture_request();
        write_activity(root.path(), request.task_id(), active_digest());
        let producer = GooseSemanticObservationSnapshotProducer::new(root.path()).unwrap();
        assert!(producer.capture(request).await.is_err());
    }

    #[test]
    fn recurrence_meter_is_chunk_invariant_and_counts_repeats_over_the_long_window() {
        let cycle = "For the buckets endpoint I map every Berlin day and status before writing. ";
        let stream = cycle.repeat(900);
        let mut whole = ReasoningRecurrenceMeter::default();
        whole.push(&stream);
        let mut chunked = ReasoningRecurrenceMeter::default();
        for chunk in stream.as_bytes().chunks(137) {
            chunked.push(std::str::from_utf8(chunk).unwrap());
        }
        let whole = whole.snapshot();
        assert_eq!(whole, chunked.snapshot());
        assert!(whole.observed_windows > 40_000);
        assert!(whole.repeated_windows > 30_000);
        assert!((0.0..=1.0).contains(&whole.repeat_share));
        assert!(!whole.earlier_reasoning.is_empty());
    }

    #[test]
    fn recurrence_meter_replays_the_exact_f924_capture_with_the_correct_denominator() {
        let repository_fixture = include_str!("fixtures/f924-looping-detail-call.txt");
        let capture = repository_fixture
            .strip_suffix('\n')
            .unwrap_or(repository_fixture);
        assert_eq!(capture.len(), 9_304);
        assert_eq!(
            sha256_label(Sha256::digest(capture.as_bytes()).as_ref()),
            "sha256:e16c2aeecb9a847bc75b33a1194111046f53093cb915b63a7723fe44021196b3"
        );
        let mut meter = ReasoningRecurrenceMeter::default();
        meter.push(capture);
        let snapshot = meter.snapshot();
        assert!(
            (snapshot.repeat_share - 0.4033).abs() < 0.0001,
            "{snapshot:?}"
        );
        let withdrawn_denominator = snapshot.observed_windows - snapshot.repeated_windows;
        let withdrawn_share = snapshot.repeated_windows as f64 / withdrawn_denominator as f64;
        assert!((withdrawn_share - 0.6758).abs() < 0.0001);
        assert_ne!(snapshot.repeat_share, withdrawn_share);
    }

    #[test]
    fn recurrence_meter_slow_advancing_reasoning_does_not_invent_duplicate_windows() {
        let mut meter = ReasoningRecurrenceMeter::default();
        for index in 0..2_000 {
            meter.push(&format!(
                "Hypothesis {index:04} checks invariant {:016x} against requirement {:016x}. ",
                index * 7 + 3,
                index * 13 + 5
            ));
        }
        let snapshot = meter.snapshot();
        assert!(snapshot.repeat_share < 0.05, "{snapshot:?}");
        meter.reset();
        assert_eq!(meter.snapshot().observed_windows, 0);
    }

    #[tokio::test]
    async fn admitted_adapter_makes_one_stream_call_with_exact_lifecycle_and_schema() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/api.rs"), "pub fn api() {}\n").unwrap();
        let capture_request = capture_request();
        write_activity(root.path(), capture_request.task_id(), active_digest());
        let producer = GooseSemanticObservationSnapshotProducer::new(root.path()).unwrap();
        let capture = producer.capture(capture_request).await.unwrap().unwrap();
        let snapshot_hash = capture.snapshot().snapshot_hash().to_string();
        let reply = serde_json::json!({
            "protocol": goose_swarm::SEMANTIC_OBSERVATION_PROTOCOL,
            "snapshot_hash": snapshot_hash,
            "observation": {
                "action": "CONTINUE",
                "summary": "The trace and artifact advanced.",
                "evidence": [{
                    "source_id": format!("trace:{}", capture.snapshot().source_revision()),
                    "observation": "The sealed trace records a concrete write."
                }]
            }
        })
        .to_string();
        let provider = Arc::new(MockProvider::new(reply));
        let judge_lane = lane("judge-lane");
        let mut worker_lane = lane("worker-lane");
        worker_lane.model_id = "worker-model".to_string();
        let fleet =
            PhysicalFleetSnapshot::new("fleet-v1", vec![judge_lane.clone(), worker_lane]).unwrap();
        let route =
            GooseSemanticProviderRoute::bind("lmstudio", provider.clone(), judge_lane.clone())
                .unwrap();
        let reviewer = Arc::new(
            GooseAdmittedSemanticObservationReviewer::new(
                fleet.clone(),
                vec![route],
                Some(0.2),
                HashMap::from([("top_p".to_string(), serde_json::json!(0.8))]),
            )
            .unwrap(),
        );
        let sink = Arc::new(RecordingSink::default());
        let control = PhysicalAdmissionControl::new("semantic-cli", fleet, sink.clone()).unwrap();
        let plane = BrokeredSemanticObservationPlane::new(control, sink.clone()).unwrap();
        let submission = plane
            .submit_if_idle(
                capture.into_snapshot(),
                SemanticObservationAdmissionPolicy {
                    task_rank: 7,
                    eligible_logical_device_ids: vec!["judge-lane".to_string()],
                    preferred_model_id: Some("judge-model".to_string()),
                    excluded_logical_device_id: Some("worker-lane".to_string()),
                },
                reviewer,
            )
            .await
            .unwrap()
            .unwrap();
        let handle = match submission {
            SemanticObservationAdmissionSubmission::Started(handle) => handle,
            SemanticObservationAdmissionSubmission::Rejected(_) => panic!("review was rejected"),
        };
        handle.wait().await.unwrap();

        assert_eq!(provider.stream_once_calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.stream_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.complete_calls.load(Ordering::SeqCst), 0);
        let calls = unpoison(&provider.calls);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].model, "judge-model");
        assert_eq!(calls[0].tools, 0);
        let permitted_provider_request_id = {
            let events = unpoison(&sink.events);
            events
                .iter()
                .find(|value| value["event"] == "broker_provider_request_permitted")
                .and_then(|value| value["receipt"]["key"]["provider_request_id"].as_str())
                .unwrap()
                .to_string()
        };
        assert_eq!(
            calls[0].session_id.as_deref(),
            Some(permitted_provider_request_id.as_str())
        );
        let response_format = &calls[0].request_params.as_ref().unwrap()["response_format"];
        assert_eq!(response_format["type"], "json_schema");
        assert_eq!(response_format["json_schema"]["strict"], true);
        assert_eq!(sink.count("broker_provider_request_permitted"), 1);
        assert_eq!(sink.count("broker_provider_terminal_observed"), 1);
        assert_eq!(sink.count("broker_work_local_completed"), 1);
        assert_eq!(sink.count("broker_admission_released"), 1);
        assert_eq!(sink.count("broker_provider_not_started"), 0);
    }

    #[tokio::test]
    async fn mismatched_provider_route_fails_before_stream_and_records_not_started() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/api.rs"), "pub fn api() {}\n").unwrap();
        let capture_request = capture_request();
        write_activity(root.path(), capture_request.task_id(), active_digest());
        let producer = GooseSemanticObservationSnapshotProducer::new(root.path()).unwrap();
        let capture = producer.capture(capture_request).await.unwrap().unwrap();
        let provider = Arc::new(MockProvider::new("unused".to_string()));

        let admitted_lane = lane("judge-lane");
        let control_fleet =
            PhysicalFleetSnapshot::new("control-fleet", vec![admitted_lane.clone()]).unwrap();
        let reviewer_fleet =
            PhysicalFleetSnapshot::new("reviewer-fleet", vec![admitted_lane.clone()]).unwrap();
        let reviewer = Arc::new(
            GooseAdmittedSemanticObservationReviewer::new(
                reviewer_fleet,
                vec![
                    GooseSemanticProviderRoute::bind("lmstudio", provider.clone(), admitted_lane)
                        .unwrap(),
                ],
                None,
                HashMap::new(),
            )
            .unwrap(),
        );
        let sink = Arc::new(RecordingSink::default());
        let control =
            PhysicalAdmissionControl::new("route-drift", control_fleet, sink.clone()).unwrap();
        let plane = BrokeredSemanticObservationPlane::new(control, sink.clone()).unwrap();
        let submission = plane
            .submit_if_idle(
                capture.into_snapshot(),
                SemanticObservationAdmissionPolicy {
                    task_rank: 1,
                    eligible_logical_device_ids: vec!["judge-lane".to_string()],
                    preferred_model_id: None,
                    excluded_logical_device_id: None,
                },
                reviewer,
            )
            .await
            .unwrap()
            .unwrap();
        match submission {
            SemanticObservationAdmissionSubmission::Started(handle) => {
                handle.wait().await.unwrap();
            }
            SemanticObservationAdmissionSubmission::Rejected(_) => panic!("unexpected rejection"),
        }
        assert_eq!(provider.stream_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.stream_once_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.complete_calls.load(Ordering::SeqCst), 0);
        assert_eq!(sink.count("broker_provider_not_started"), 1);
        assert_eq!(sink.count("broker_provider_request_permitted"), 0);
        assert_eq!(sink.count("broker_provider_terminal_observed"), 0);
        assert_eq!(sink.count("broker_admission_released"), 1);
    }
}
