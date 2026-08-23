//! The model-agnostic dispatch boundary. The scheduler only ever runs a task by calling
//! [`TaskDispatcher::run`]; the real implementation (in goose-cli) drives a Goose Agent bound to a
//! device's LM Link model id, while tests use a mock. This keeps the concurrency core testable.

use crate::dag::ReplanAuthorityReceipt;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Component, Path};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDispatchClass {
    ProviderRequired,
    DeterministicProviderFree,
}

/// One tool call a task's agent made, captured by the dispatcher for observability.
#[derive(Clone, Debug, Serialize)]
pub struct ToolCallRecord {
    pub name: String,
    /// True if the tool came from an MCP extension (vs a builtin like `developer`).
    pub is_mcp: bool,
    /// Whether the tool response was ok; `None` if no response arrived (e.g. a max-turns cutoff).
    pub ok: Option<bool>,
    /// True if this call demonstrably FETCHED AN EXTERNAL RESOURCE — a shell that ran curl/wget or a
    /// urllib/requests one-liner against an http(s) URL.
    ///
    /// Grounding used to mean `is_mcp && ok`, so a scout that read the vendor's live docs the only way
    /// available to it — `curl http://.../v1/docs` — was recorded as having looked nothing up. MEASURED on
    /// a bench run with `research_tools: {available: [], can_look_things_up: false}`: 11 curl requests
    /// reached the vendor, the scout quoted `GET /v1/payments?cursor=&limit=` verbatim in its report, and
    /// the engine still logged `grounded: 0`. Because the verbatim worker channel (`doc_facts`) is filtered
    /// on `grounded`, that channel was structurally dead on every run of this bench.
    ///
    /// This is deliberately NOT "any successful shell". The original rule exists to stop a trivial `echo`
    /// laundering a guess into a settled fact, and that concern is real; the predicate therefore requires a
    /// URL *and* a fetching program, so `echo https://x` still grounds nothing.
    pub fetched_external: bool,
}

/// A successful dispatch's result: the task output plus observability (session id + tool calls).
#[derive(Clone, Debug, Default)]
pub struct TaskRunOutput {
    pub output: String,
    pub session_id: Option<String>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub completion: TaskCompletionDisposition,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SalvageReason {
    ProgressWatchdog,
    StallExhausted,
    FinalizeSpin,
    DeterministicAccept,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredVerification {
    FullRepairRuler,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskCompletionDisposition {
    Complete,
    Salvaged {
        reason: SalvageReason,
        artifact_hashes: BTreeMap<String, String>,
        required_verification: RequiredVerification,
    },
}

impl Default for TaskCompletionDisposition {
    fn default() -> Self {
        Self::Complete
    }
}

impl TaskCompletionDisposition {
    pub fn salvaged(reason: SalvageReason, artifact_hashes: BTreeMap<String, String>) -> Self {
        Self::Salvaged {
            reason,
            artifact_hashes,
            required_verification: RequiredVerification::FullRepairRuler,
        }
    }

    pub fn is_salvaged(&self) -> bool {
        matches!(self, Self::Salvaged { .. })
    }
}

fn looks_like_test_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(lower.as_str());
    base.starts_with("test_")
        || base.ends_with("_test.py")
        || base.ends_with("_test.rs")
        || base.ends_with("_test.go")
        || base.contains(".test.")
        || base.contains(".spec.")
        || base == "conftest.py"
        || lower.contains("/tests/")
        || lower.contains("/test/")
}

fn looks_like_manifest_file(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    matches!(
        base.as_str(),
        "go.mod"
            | "go.sum"
            | "package.json"
            | "package-lock.json"
            | "cargo.toml"
            | "cargo.lock"
            | "requirements.txt"
            | "setup.py"
            | "setup.cfg"
            | "pyproject.toml"
            | "__init__.py"
            | "tsconfig.json"
            | "gemfile"
    )
}

fn artifact_sha256(path: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
        return None;
    }
    let digest = Sha256::digest(std::fs::read(path).ok()?);
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(DIGITS[(byte >> 4) as usize] as char);
        hex.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    Some(format!("sha256:{hex}"))
}

/// Captures the exact artifacts that can justify provisional dependency release. It deliberately
/// refuses tests, owns-nothing verification, manifest-only work, unsafe paths, and partial multi-file
/// output. Those cases may be retried or fail, but cannot be laundered into accepted completion.
pub fn artifact_hashes_at(root: &Path, owned_files: &[String]) -> Option<BTreeMap<String, String>> {
    if owned_files.is_empty() {
        return None;
    }

    let canonical_root = root.canonicalize().ok()?;
    let mut hashes = BTreeMap::new();
    for path in owned_files {
        let relative = Path::new(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return None;
        }
        let artifact = root.join(relative);
        if !artifact.canonicalize().ok()?.starts_with(&canonical_root) {
            return None;
        }
        hashes.insert(path.clone(), artifact_sha256(&artifact)?);
    }
    Some(hashes)
}

pub fn salvage_artifact_hashes_at(
    root: &Path,
    task_id: &str,
    owned_files: &[String],
) -> Option<BTreeMap<String, String>> {
    if task_id.to_lowercase().contains("test")
        || owned_files.iter().all(|path| looks_like_test_file(path))
        || owned_files
            .iter()
            .all(|path| looks_like_manifest_file(path))
    {
        return None;
    }
    artifact_hashes_at(root, owned_files)
}

pub fn salvage_artifact_hashes(
    task_id: &str,
    owned_files: &[String],
) -> Option<BTreeMap<String, String>> {
    salvage_artifact_hashes_at(Path::new("."), task_id, owned_files)
}

impl From<String> for TaskRunOutput {
    fn from(output: String) -> Self {
        Self {
            output,
            session_id: None,
            tool_calls: Vec::new(),
            completion: TaskCompletionDisposition::Complete,
        }
    }
}

/// One unit of work handed to a device.
#[derive(Clone, Debug)]
pub struct DispatchRequest {
    pub task_id: String,
    pub description: String,
    /// The device this task was routed to (pool id).
    pub device_id: String,
    /// The LM Link model id LM Studio routes to the device.
    pub model_id: String,
    /// The relevant slice of shared context (dependency outputs) for this task.
    pub context_slice: String,
    /// Exact files owned by this task's completed dependencies. Runtime acceptance reviews use this
    /// read-only source set; ordinary workers ignore it.
    pub dependency_files: Vec<String>,
    /// 0-based attempt number (incremented on re-dispatch).
    pub attempt: u32,
    /// The EXACT files this task must create/own (plan paths). The worker is told to write only these.
    pub owned_files: Vec<String>,
    /// The full project file manifest (every task's owned files) so the worker uses the agreed layout
    /// for imports and never invents a divergent location.
    pub all_files: Vec<String>,
    /// A corrective hint from the idle-model judge when this is a re-dispatch after the judge killed a
    /// prior attempt (e.g. "you were looping/over-reading — WRITE now"). `None` on a normal attempt.
    pub prior_hint: Option<String>,
    /// S3 i3 (GOOSE_SWARM_FILL_FAN): the task's contract-anchored parallel-fill slots, carried
    /// from TaskSpec.subsplit. Empty everywhere except a normal dispatch of a task whose spec
    /// carries the detailer's SUBSPLIT line; the dispatcher fans fillers only when the gate is
    /// on AND this has 2+ names that survive contract anchoring.
    pub subsplit: Vec<String>,
    /// True when this is a SPECULATIVE twin of an in-flight chokepoint task, raced on an idle device
    /// (GOOSE_SWARM_SPECULATE). The dispatcher must run it in an ISOLATED shadow workspace so it never
    /// writes the real owned_files; the winner's output is promoted back. `false` for every normal task.
    pub speculative: bool,
    /// The user's own answers to the clarifying questions, VERBATIM, for injection into the worker prompt.
    /// Empty when the run never asked.
    ///
    /// This field exists because there was NO channel from the user's answers to a worker at all. The engine
    /// printed `✓ keeping this plan; clarifications injected into every worker via research findings and
    /// spec` — and BOTH named channels were false. `research_findings` is only ever passed to planner-side
    /// calls (it appears ZERO times in the dispatcher's run body), and the amended spec lives in
    /// `Scheduler::goal`, whose only consumers are the replanner, the judge and the pre-reviewer. Worse, the
    /// plan is drafted BEFORE the ask, and `ask_replan` defaults off, so `description` is a pre-answer
    /// artifact too. The answers were structurally excluded from BOTH halves of the worker prompt, and only
    /// reached workers as a lossy planner-model paraphrase (pillars/contracts) — which is exactly the
    /// measured "asked for pipe-separated CSV, wrote comma-separated" failure.
    ///
    /// Empty string => the injected block is empty => byte-identical to before this field existed.
    pub user_decisions: String,
    /// GROUNDED research facts (Phase 1, Move 2 — `GOOSE_SWARM_DOC_PREFETCH`), VERBATIM, for injection into
    /// the worker prompt via the same splice pattern as `user_decisions`. These are the findings a scout
    /// actually LOOKED UP with a real research tool (never an invented/paraphrased one), routed to the worker
    /// un-paraphrased so a concrete API fact survives as a literal instead of a lossy planner paraphrase.
    ///
    /// Empty string => the injected block is empty => byte-identical to before this field existed.
    pub doc_facts: String,
    /// The task ids in this task's DAG neighborhood — its deps ∪ its consumers ∪ itself — used to scope
    /// the frozen-contract bundle to only the modules this worker actually touches (deterministic,
    /// `GOOSE_SWARM_SCOPED_CONTRACTS`). Empty => "no neighborhood info" => the consumer falls back to the
    /// full unscoped bundle, so a fix/sink/mock dispatch with no neighborhood is byte-identical to before
    /// this field existed.
    pub neighborhood: Vec<String>,
    /// Binder/compiler authority for a dynamically admitted read-only acceptance review. The
    /// dispatcher fails closed if a review-shaped task reaches it without this receipt.
    pub replan_authority: Option<ReplanAuthorityReceipt>,
}

/// Outcome of a failed dispatch. `Transient` is re-dispatched (and steered to a different device);
/// `Terminal` fails the task and its descendants.
#[derive(Clone, Debug)]
pub enum DispatchError {
    /// Recoverable — e.g. LM Studio "Model is unloaded". Re-dispatch within the attempt budget.
    Transient(String),
    /// A CONTENT failure (e.g. a syntax error in an owned file) caught by the pre-done gate. Re-dispatched
    /// like `Transient`, but the error is threaded into the retry's `prior_hint` so the fix is guided
    /// rather than a blind re-roll. Scoped to content — infra transients never carry a hint.
    ContentRetry(String),
    /// Unrecoverable for this task.
    Terminal(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::Transient(m) => write!(f, "transient: {m}"),
            DispatchError::ContentRetry(m) => write!(f, "content-retry: {m}"),
            DispatchError::Terminal(m) => write!(f, "terminal: {m}"),
        }
    }
}

impl std::error::Error for DispatchError {}

#[async_trait]
pub trait TaskDispatcher: Send + Sync {
    /// Run one task on its assigned device. Returns the task's output plus observability on success.
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError>;

    /// Promote a winning SPECULATIVE twin: copy its shadow-workspace output for `task_id` into the real
    /// project tree. Called by the scheduler when a speculative twin finishes first. Default no-op so the
    /// mock and the speculation-OFF path need no change; the real dispatcher overrides it in Phase 2.
    async fn promote_speculative(&self, _task_id: &str) {}

    /// Drop a shadow workspace for `task_id` WITHOUT promoting it — a lost/errored speculative shard, so its
    /// edits never reach the real tree and the shadow does not leak. Default no-op.
    async fn discard_speculative(&self, _task_id: &str) {}

    /// READ-ONLY correctness review of the produced `files` along ONE dimension, on a spare fleet model.
    /// The model is given the files as text with NO tools (it physically cannot write), so N of these run
    /// concurrently over one tree with no write-race. Returns advisory findings text, or None when clean /
    /// nothing to review. Default None so the mock and the fanout-OFF path are unchanged.
    async fn review_dimension(
        &self,
        _model_id: &str,
        _dim_id: &str,
        _dim_brief: &str,
        _goal: &str,
        _files: &[String],
    ) -> Option<String> {
        None
    }

    /// ADVERSARIAL VERIFY (the three-vote core): hand ONE review finding to a skeptic model — a DIFFERENT
    /// model than raised it — prompted to REFUTE it against the ACTUAL code (read-only, no tools). Returns
    /// true only if the skeptic independently CONFIRMS the finding is a real defect with HIGH confidence, i.e.
    /// it SURVIVES refutation. FAIL-CLOSED: default false, and any refute / low-confidence / parse-or-timeout
    /// failure returns false — an unverified finding can never drive a fix. Read-only => no write-race.
    async fn verify_finding(
        &self,
        _model_id: &str,
        _finding: &str,
        _goal: &str,
        _files: &[String],
    ) -> bool {
        false
    }
}
