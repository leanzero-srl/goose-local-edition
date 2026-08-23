//! The deterministic weighted work-queue scheduler.
//!
//! One central dispatch loop owns the [`Dag`]; per-device capacity is `weight` (max concurrent
//! in-flight tasks on that device). Each loop pass claims as many ready tasks as devices have free
//! capacity (work-stealing: a ready task prefers its planner-suggested device but falls back to any
//! free one), spawns their dispatch futures, then waits on a [`Notify`] that completions fire. A
//! task is locked (state `Claimed`, its files held) while in flight, so it is never double-claimed
//! and two tasks owning the same file never run concurrently. Completions relax dependents
//! (unlocking the DAG), merge output into the shared context, and free device capacity.

use crate::broker::{
    AdmissionReceipt, PhysicalExecutionAuthority, SourceRevisionKind, TaskVersion, WorkOpportunity,
};
use crate::context::SharedContext;
use crate::control_plane::{
    BrokeredTaskDispatcher, PhysicalAdmissionControl, PhysicalDispatchAuthority,
    ProviderLifecycleDispatcher,
};
use crate::dag::{Dag, Difficulty, TaskId, TaskSpec, TaskState};
use crate::dispatch::{
    DispatchError, DispatchRequest, TaskDispatcher, TaskRunOutput, ToolCallRecord,
};
use crate::event::{EventSink, NullSink, SwarmEvent};
use crate::judge::{
    Judge, JudgeConfig, JudgeOutcome, JudgeRequest, PreReviewRequest, PreReviewer, Verdict,
};
use crate::repair::{
    mint_provisional_task_receipt, ProvisionalTaskReceipt, SalvageReason, TaskCompletionDisposition,
};
use crate::replan::{ReplanContext, Replanner};
use crate::semantic_control::{
    semantic_observation_task_version, AdmittedSemanticObservationReviewer,
    BrokeredSemanticObservationPlane, SemanticObservationAdmissionPolicy,
    SemanticObservationAdmissionSubmission,
};
use crate::semantic_observation::AcceptanceCriterionSnapshot;
use crate::semantic_runtime::{
    EngineSemanticTaskAuthority, SemanticActivityPublisher, SemanticObservationCaptureRequest,
    SemanticObservationSnapshotProducer, SemanticTraceRevision,
};
use anyhow::{bail, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex, Notify};

/// GOOSE_SWARM_SPLIT_INHERIT_SPEC (default ON): give a split CHILD the parent's full implementation spec,
/// scoped to the child's own files — instead of the ~40-char label it gets today.
///
/// MEASURED (loop-04): PLAN spent 48.4 min (40% of the whole run) writing a 2038-char implementation-ready
/// spec for `data-model-persistence` (three SPM targets, Swift 6 mode, sqlite3 system library, `@Observable
/// class NoteStore: Sendable`, an undo stack). The judge then split it, and every child's ENTIRE task
/// statement became `"(split of data-model-persistence) note-store"` — 43 characters. The spec the run had
/// just paid 40% of its wall-clock to produce was thrown away at the moment of use, and the shipped app
/// showed it: 221 LOC against an ~800-1200 spec, a plain JSON store where the plan demanded SQLite.
///
/// The splitter is default-ON on the desktop path, so this fires on real runs.
///
/// The default was flipped on after the file-scoped child path was reviewed. Handing a child the parent's
/// whole spec risks it writing its SIBLINGS' files, so `child_description` leads with the hard scope header;
/// an explicit 0/off/false/no remains the rollback.
/// The ONE resolution of GOOSE_SWARM_SINK_REVIEW. Both halves of the mechanism — this crate's
/// producer and goose-cli's drain — must read the same answer, or the run reports a lever it is not
/// running.
pub fn sink_review_enabled() -> bool {
    std::env::var("GOOSE_SWARM_SINK_REVIEW")
        .map(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

/// F779: the tail idle-fill (GOOSE_SWARM_TAIL_REVIEW). DEFAULT ON — read-only, cannot corrupt,
/// and it IS the ratio lever: when the DAG tail leaves nodes idle (a long test task, an e2e
/// shard, the sink), the free devices run read-only dimension review instead of sitting idle
/// while the busy node grinds. Set GOOSE_SWARM_TAIL_REVIEW=0 to restore the pre-F779 silence.
pub fn tail_review_enabled() -> bool {
    std::env::var("GOOSE_SWARM_TAIL_REVIEW")
        .map(|v| {
            !matches!(
                v.trim().to_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            )
        })
        .unwrap_or(true)
}

/// F790-3: the operator-question channel (GOOSE_SWARM_QA). DEFAULT ON — it is read-only, costs
/// nothing while the inbox is empty, and exists precisely so the operator can ask the run
/// questions while it works. Set 0/off to silence it.
pub fn qa_enabled() -> bool {
    std::env::var("GOOSE_SWARM_QA")
        .map(|v| {
            !matches!(
                v.trim().to_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            )
        })
        .unwrap_or(true)
}

/// The ONE resolution of GOOSE_SWARM_TESTGEN (S7): idle slots generate contract-derived tests.
/// Default OFF — an arm, not a silent flip. Shared with goose-cli's dispatcher for the same
/// reason as sink_review_enabled above: two halves reading different answers is the measured
/// failure mode.
pub fn testgen_enabled() -> bool {
    std::env::var("GOOSE_SWARM_TESTGEN")
        .map(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

pub fn split_inherit_spec_enabled() -> bool {
    // DEFAULT ON (2026-08-16 review). The deterministic no-first-write split hands each child a
    // file list; without inheritance its ONLY instruction is "(split of <parent>)" — a child with
    // no statement builds nothing (the F457 lesson: split children buy +0.036 WITH a 43-char
    // statement; a stub statement is worse than the parent's full spec). Opt-out stays via env=0.
    !matches!(
        std::env::var("GOOSE_SWARM_SPLIT_INHERIT_SPEC")
            .unwrap_or_default()
            .trim()
            .to_lowercase()
            .as_str(),
        "0" | "off" | "false" | "no"
    )
}

/// The task statement a split child receives.
///
/// OFF (today's behaviour, byte-identical): `"(split of <parent>) <child-id>"`.
/// ON: a file-scope header naming exactly the files this child owns and stating that the siblings belong to
/// other workers, followed by the parent's FULL spec as shared context. Pure string work — no model call, no
/// new judgement, no dep semantics touched. The header comes FIRST so the scope is read before the spec that
/// describes files the child must not touch.
fn child_description(
    parent_id: &str,
    parent_desc: &str,
    child: &crate::judge::ChildSpec,
    inherit_spec: bool,
) -> String {
    if !inherit_spec || parent_desc.trim().is_empty() {
        return format!("(split of {parent_id}) {}", child.id);
    }
    format!(
        "This task is one PART of a larger subtask (`{parent_id}`) that was split across workers.\n\n\
         YOU OWN ONLY THESE FILES — create/edit these and NOTHING else:\n{}\n\n\
         The other files named in the spec below belong to OTHER workers on this same plan and are being \
         written right now in parallel. Do NOT create them, do NOT edit them, and do NOT wait for them.\n\n\
         The FULL spec of the original subtask follows. Implement ONLY the parts that describe the files you \
         own; treat the rest as context for how your files must fit together.\n\n{parent_desc}",
        child
            .files
            .iter()
            .map(|f| format!("- {f}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// GOOSE_SWARM_SALVAGE_SPIN (default ON): when a NON-TEST task terminal-fails via finalize-spin (Verdict::
/// Looping), preserve complete artifacts as Salvaged instead of Failed. Looping only fires once an owned file was written, so the
/// worker DID produce output — discarding it also fails its dependents (esp. the integrate-verify sink), which
/// reports a WORKING app as FAILED (observed UNIQ9: the entry spun on its final fix -> integrate-verify blocked
/// -> run FAILED though the app runs). Salvaging lets integrate-verify be the real gate. Off with 0/off/false/no.
pub fn salvage_spin_enabled() -> bool {
    std::env::var("GOOSE_SWARM_SALVAGE_SPIN")
        .map(|v| {
            !matches!(
                v.trim().to_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            )
        })
        .unwrap_or(true)
}

/// GOOSE_SWARM_SALVAGE_REQUIRE_CRITICAL (#134, default OFF = byte-identical `.any()`): when a stalled/spinning
/// task used the legacy salvage path, require its CRITICAL owned files. Retained as a config compatibility
/// reader; typed provisional receipts now always require every owned artifact and do not consult this flag.
pub fn salvage_require_critical() -> bool {
    std::env::var("GOOSE_SWARM_SALVAGE_REQUIRE_CRITICAL")
        .map(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

/// Content fingerprint of a task's owned files, for the progress-gated kill rule. Absent
/// files hash as a marker so created-vs-missing is itself movement.
fn owned_files_fingerprint(owned_files: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in owned_files {
        f.hash(&mut h);
        match std::fs::read(f) {
            Ok(bytes) => bytes.hash(&mut h),
            Err(_) => "ABSENT".hash(&mut h),
        }
    }
    h.finish()
}

/// The corrective note an INFRA transient earns, if any. Extracted so it is unit-testable without a
/// live scheduler run, like `should_degrade_on_stall`.
///
/// Infra transients deliberately carry no hint: a "model unloaded" retry means nothing happened, and
/// a stale note would mislead the worker. A MID-STREAM BODY DROP is the exception, and it is the one
/// that costs the most. MEASURED, current generation: every sink discard is a dropped HTTP body —
/// `baseline-n3-r0` lost TWO attempts to it (1383s and 941s) and `baseline-n1-r0` one, while
/// `baseline-n3-r1`, whose sink dropped nothing, ran a 480s join and posted the best execute
/// occupancy on record (0.8256 against a 0.55-0.67 band).
///
/// The worker was mid-generation when the socket died, so its earlier tool calls ALREADY LANDED — the
/// files it wrote or edited are on disk. But `run_agent_in` always starts a fresh conversation (there
/// is no prior-session parameter; resume exists only at RUN level), so the retry begins with no memory
/// of any of it and re-derives everything from nothing. The work survives; the understanding does not.
/// A hint is the only channel that crosses that boundary.
fn transient_retry_hint(msg: &str) -> Option<String> {
    if msg.contains("mid-stream body drop") {
        return Some(
            "Your previous attempt was cut off mid-generation by a dropped connection — not by anything \
             you did wrong, and not because the work was rejected. ANY FILE YOU HAD ALREADY WRITTEN OR \
             EDITED IS STILL ON DISK. Do NOT start over from scratch: first READ the current state of the \
             files involved, KEEP what is already correct, and continue from where that left off. \
             Re-deriving work that is already done is the most expensive thing you can do here."
                .to_string(),
        );
    }
    // THE STALL CLASS RETRIES WARM. Measured (r5/r6 wall attribution): stalled-then-cold-retried
    // attempts were the single largest wall sink in BOTH runs — 58.3 min in r5, 104.9 min in r6 by
    // minute 85 — because a cold retry re-derives everything, hits the same wall, and stalls the
    // same way (the class survived temp 0.2, so it is task shape, not sampling luck). The retry
    // carries three things the cold attempt lacked: the partial work is on disk, the specific
    // pathology that killed attempt N (threaded verbatim from the kill site), and the instruction
    // to act before deliberating — 13/15 measured stalls ended reasoning cleanly and never issued
    // a tool call.
    //
    // The marker is the SHARED PREFIX of all five kill-site strings, not the "no productive
    // progress" suffix family: r7's first live stall retried COLD because the idle watchdog's
    // own message ("agent stalled — no progress for Ns (no token/tool activity)") — the DOMINANT
    // variant in the measured 420s loops — does not carry that suffix. Caught live under the
    // kill-on-divergence watch; the prefix has exactly five producers, all stalls, verified by
    // grep across both crates.
    if msg.contains("agent stalled") {
        return Some(format!(
            "Your previous attempt on this task was stopped because it stalled: {msg}. \
             ANY FILE IT ALREADY WROTE OR EDITED IS STILL ON DISK. If your owned file(s) \
             already exist, READ them first, KEEP what is correct, and FINISH the work — do \
             not regenerate it from scratch. ACT FIRST: your very first response must be a tool call (read or \
             write), never deliberation — the previous attempt died deliberating without acting. \
             If the whole job is too big for one pass, write it in sections across MULTIPLE \
             smaller tool calls, completing the most load-bearing piece first."
        ));
    }
    None
}

/// Is an UNACTED judge verdict still worth carrying to the task's next attempt?
///
/// `apply_judge_outcome` can only re-dispatch while the per-task intervention cap holds, and can only
/// terminal-fail on a DETERMINISTIC verdict. Once a task has spent its cap, a model verdict reaches no
/// acting branch at all — it is logged as `observed` and dropped. MEASURED: one test task drew thirteen
/// consecutive cap-exhausted verdicts naming a literal syntax error (`from Non` for `from None`) and a
/// wrong mock target, roughly one a minute; every one was discarded, and the attempt that eventually
/// replaced it was started by `worker_timeout` — a timer — carrying the stale hint from its last kill.
///
/// Keeping the hint changes no control flow. It is the difference between a timer replacing a worker
/// blind and replacing it with the run's freshest diagnosis in hand.
fn observed_hint_worth_keeping(still_live: bool, verdict: Verdict, hint: &str) -> bool {
    // `still_live` matters: a verdict on an attempt that has already ended describes a worker whose
    // replacement is a different question, and `is_problem()` excludes Ok/Accept, whose hint is empty
    // anyway.
    still_live && verdict.is_problem() && !hint.trim().is_empty()
}

/// Does this dispatch deserve the FASTEST free node?
///
/// `pick_device` already routes HARD tasks to the quickest host, on the reasoning stated there —
/// identical models differ only in host speed, so putting the heaviest work on the fastest node
/// shortens the critical path. A task on its THIRD attempt has earned exactly the same treatment and
/// was not getting it.
///
/// MEASURED. `test-api-error-handling` went to the fast worksmacstudio node at 44.0 min, was killed
/// `over_reading` at 51.2, went to the fast local-mihai node, was killed `over_reading` again at
/// 60.3 — and its third dispatch landed on `mac-gabee`, the SLOWEST node, where it then ran for 29
/// minutes drawing nothing but `ok` verdicts while both faster nodes reported READY. It was the last
/// task in the run, so the whole run waited on the slowest host.
///
/// ⚠️ THE BAN IS NOT THE CULPRIT AND I NEARLY "FIXED" IT. `avoid_device` holds ONE device and is
/// overwritten each kill, so the third attempt had both gabee AND worksmacstudio available. It went
/// to gabee because `test-meridian-resilience` was dispatched in the SAME instant and took the fast
/// node, leaving one slot. Nothing was malfunctioning; the ranking simply had no reason to prefer the
/// twice-killed task over the fresh one.
///
/// That is the reason to rank by attempts: a task on attempt 2+ has already consumed two attempts of
/// wall-clock, everything downstream is blocked behind it, and it is empirically the run's tail risk.
/// A fresh task competing for the same slot is not. The bar is 2 rather than 1 because single
/// retries are common and cheap, and this targets only the case that was actually measured to hurt.
/// The DAG must still have real work left before the replanner is allowed to invent more.
///
/// Dynamic-replan exists to fill idle capacity in the MIDDLE of a run. Near the end its arithmetic
/// inverts: a task injected when almost everything is done has nothing left to overlap with, so it
/// does not fill a gap — it BECOMES the tail, and the run waits on work nobody asked for.
///
/// MEASURED across every 3-node cell in the corpus that replanned, 3 for 3, the LAST task to
/// complete — the one that sets the run's length — is a replanner-added bonus task:
///
///   n3-r2  injected at 50.2m with  3 of 21 mandatory left (14%)  ->  18.3 min of bonus tail
///   n3-r3  injected at 50.7m with  2 of 18 mandatory left (11%)  ->  26.8 min of bonus tail
///
/// n3-r3 settles it: its last MANDATORY task finished at 48.8m and the replanner injected three
/// tasks at 50.7m which ran until 75.7m. The run was already done. The engine made it 55% longer
/// with work the planner never asked for.
///
/// AND IT IS NODE-COUNT-SPECIFIC, which is why it matters here. The gate requires
/// `idle_capacity() >= 2`; a 1-node run never has it, so it never injects bonus work and never grows
/// a bonus tail. Spare nodes are the precondition — so adding nodes does not merely fail to help, it
/// arms the mechanism that makes the run longer.
///
/// This is the same principle the gate ALREADY applies via `sink_in_flight`: when only the join
/// remains, do not replan. That rule was one case short — "only the join remains" and "almost
/// nothing remains" are the same situation, and only the first was covered.
///
/// THE BAR IS A FRACTION, NOT A COUNT, and the first version of this got that wrong. An absolute
/// "more than 3 tasks left" reproduced both measured cases correctly and DISABLED DYNAMIC-REPLAN
/// ENTIRELY FOR SMALL DAGS — `idle_triggers_replan_and_fills_nodes` builds a 2-task DAG where one
/// task runs long, and 1-of-2 remaining is the mid-run case the feature exists for, not a tail. The
/// harm is "nothing left to overlap with", which is inherently relative to the plan's size. Two
/// pre-existing tests failed and are the reason this reads as it does.
///
/// A quarter of the plan still outstanding clears both measured harms (14% and 11%) while leaving
/// mid-run injection untouched. Deliberately conservative: it can only ever refuse a replan, so it
/// cannot make a run longer.
fn replan_has_enough_dag_left(mandatory_incomplete: usize, mandatory_total: usize) -> bool {
    mandatory_total == 0 || mandatory_incomplete * 4 >= mandatory_total
}

fn project_relative_runtime_path(path: &str) -> bool {
    !path.is_empty()
        && path.trim() == path
        && !path.contains('\\')
        && !path.starts_with('/')
        && !path
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':' && path.as_bytes()[0].is_ascii_alphabetic())
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."))
}

/// Runtime replanning is not a second raw planner. Only a binder/compiler receipt can admit a
/// read-only acceptance review, and every authority fact must survive verbatim into its payload.
fn validate_runtime_replan_specs(dag: &Dag, specs: &[TaskSpec]) -> Result<()> {
    for spec in specs {
        let receipt = spec.replan_authority.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "dynamic task `{}` has no binder/compiler authority receipt",
                spec.id
            )
        })?;
        if !spec.id.starts_with("replan-review::") {
            bail!(
                "dynamic task `{}` is not a runtime acceptance review",
                spec.id
            );
        }
        if !spec.owned_files.is_empty() || !spec.subsplit.is_empty() {
            bail!(
                "dynamic review `{}` attempted write ownership or a build split",
                spec.id
            );
        }
        if spec.deps != [receipt.source_task_id.clone()] {
            bail!(
                "dynamic review `{}` is not bound exclusively to source task `{}`",
                spec.id,
                receipt.source_task_id
            );
        }
        let source = dag.tasks.get(&receipt.source_task_id).ok_or_else(|| {
            anyhow::anyhow!(
                "dynamic review `{}` references missing source task `{}`",
                spec.id,
                receipt.source_task_id
            )
        })?;
        if source.state != TaskState::Done {
            bail!(
                "dynamic review `{}` source task `{}` is not completed",
                spec.id,
                receipt.source_task_id
            );
        }
        let mut source_files = source.spec.owned_files.clone();
        source_files.sort();
        source_files.dedup();
        let mut receipt_files = receipt.source_files.clone();
        receipt_files.sort();
        receipt_files.dedup();
        if receipt_files.is_empty()
            || receipt_files != source_files
            || receipt_files
                .iter()
                .any(|path| !project_relative_runtime_path(path))
        {
            bail!(
                "dynamic review `{}` source-file receipt does not match its completed dependency",
                spec.id
            );
        }
        if receipt.binding_task_id.trim().is_empty()
            || receipt.slice_id.trim().is_empty()
            || receipt.objective.trim().is_empty()
            || receipt.requirements.is_empty()
            || receipt.acceptance_evidence.is_empty()
        {
            bail!(
                "dynamic review `{}` has an incomplete authority receipt",
                spec.id
            );
        }
        let mut fact_ids = HashSet::new();
        for fact in receipt
            .requirements
            .iter()
            .chain(receipt.evidence.iter())
            .chain(receipt.interfaces.iter())
        {
            if fact.id.trim().is_empty()
                || fact.text.trim().is_empty()
                || !fact_ids.insert(fact.id.as_str())
                || !spec.description.contains(&fact.id)
                || !spec.description.contains(&fact.text)
            {
                bail!(
                    "dynamic review `{}` lost or duplicated authority fact `{}`",
                    spec.id,
                    fact.id
                );
            }
        }
        if !spec.description.contains(&receipt.objective)
            || receipt
                .source_files
                .iter()
                .chain(receipt.acceptance_evidence.iter())
                .any(|fact| fact.trim().is_empty() || !spec.description.contains(fact))
        {
            bail!(
                "dynamic review `{}` payload erased authoritative paths, objective, or acceptance evidence",
                spec.id
            );
        }
    }
    Ok(())
}

/// A2: the ranking key for a HARD task's device choice — min() wins. IDLE beats busier (first
/// element), weight decides among equally-loaded devices (second), a first-dispatch timing accident
/// never outranks the operator's weights (speed is third, among equal weights only). The measured
/// defect this pins against: weight-absolute ordering stacked hard tasks two-deep on the fastest
/// host while a whole node idled, and concurrent generations on one Apple host degrade each other
/// (queue-time monotonic in concurrency, 7.06 SE).
fn hard_device_key(
    in_flight: u32,
    weight_rank: u32,
    speed: u64,
    weighted_load: u64,
    idx: usize,
) -> (u64, u64, u64, u64, usize) {
    (
        in_flight as u64,
        weight_rank as u64,
        speed,
        weighted_load,
        idx,
    )
}

fn dispatch_prefers_fastest_node(is_hard: bool, attempts: u32) -> bool {
    is_hard || attempts >= 2
}

fn provisional_task_receipt(
    spec: &TaskSpec,
    attempt: u32,
    reason: SalvageReason,
) -> Option<ProvisionalTaskReceipt> {
    mint_provisional_task_receipt(
        Path::new("."),
        &spec.id,
        attempt,
        &task_contract_version(spec),
        reason,
        &spec.owned_files,
    )
}

fn stall_salvage_receipt(
    enabled: bool,
    is_content: bool,
    spec: &TaskSpec,
    attempt: u32,
) -> Option<ProvisionalTaskReceipt> {
    if !enabled || is_content {
        return None;
    }
    provisional_task_receipt(spec, attempt, SalvageReason::StallExhausted)
}

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
    /// F779 i3: a SUPERVISION device carries read-only idle work only (judge, pre/tail-review,
    /// testgen) — never build dispatch, speculation twins, or replan-injected work — and is
    /// invisible to every node-count reader (worker_count, fleet_models/slots, occupancy, planner
    /// sizing all count build devices). This is how a capped run (the n1 arm) borrows its excluded
    /// idle machines for quality work without changing what it BUILDS.
    pub supervision: bool,
}

#[derive(Debug, Serialize)]
pub struct RunReport {
    pub done: Vec<TaskId>,
    /// Tasks whose complete artifact bytes survived but still require the full repair ruler.
    pub salvaged: Vec<TaskId>,
    pub failed: Vec<TaskId>,
    /// Ids of opportunistic/replanner-added (bonus) tasks — their failure must NOT fail the run.
    pub bonus: Vec<TaskId>,
    /// Owned files carried by every DONE or SALVAGED task in the FINAL dag — including files added by
    /// replan/split after the caller's pre-run snapshot. Salvaged files remain in verification scope;
    /// `done`, `salvaged`, and the typed receipt remain separate truth in this report.
    pub planned_files: Vec<String>,
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
    /// `done` | `salvaged` | `failed` | `incomplete`.
    pub status: String,
    /// Compatibility projection of `completion`.
    pub salvaged: bool,
    pub completion: Option<TaskCompletionDisposition>,
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
    /// True when this task owns NO files (e.g. the injected `integrate-verify` model-judge sink). Such a
    /// task's failure is a MODEL self-report, never a deterministic engine event — the hard completion gate
    /// must exclude it from the green-blocking set so a judge's dissent can never veto a good app (C1).
    pub owns_nothing: bool,
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
/// Releases ONE idle-job slot when an idle-job task ends — INCLUDING on panic, so a panicking judge or
/// pre-reviewer can never leak a slot and starve future idle work. Always decrements `idle_jobs`; for a
/// judge job it also clears `judge_running` so a panicked judge does not wedge the single-judge invariant.
/// Drop is synchronous, so it spawns a tiny task to update the count under the async State lock (only if a
/// runtime is still current — during shutdown the count no longer matters).
struct IdleSlotGuard {
    state: Arc<Mutex<State>>,
    is_judge: bool,
    /// The device index this idle-job CLAIMED (bumped in_flight on), so a worker dispatch + the next idle-job
    /// see it as busy and never stack a 2nd call on the same node (the "+1 QUEUED on one node, another idle"
    /// bug). `None` when the fleet was saturated so no idle device could be claimed (deterministic-only judge).
    claimed_device: Option<usize>,
    /// A3: the scheduler loop's wakeup. Releasing a claimed slot without notifying left the freed
    /// slot unusable until the next 15s tick — ~41-61 releases per run (CONFIRMED), each a dead slot
    /// window while ready work waited. Notified AFTER the decrement lands, so the woken pass sees
    /// the slot free. None (tests) => release exactly as before.
    notify: Option<Arc<Notify>>,
}

impl Drop for IdleSlotGuard {
    fn drop(&mut self) {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let st = self.state.clone();
            let is_judge = self.is_judge;
            let claimed = self.claimed_device;
            let notify = self.notify.clone();
            handle.spawn(async move {
                {
                    let mut s = st.lock().await;
                    s.idle_jobs = s.idle_jobs.saturating_sub(1);
                    if is_judge {
                        s.judge_running = false;
                    }
                    if let Some(dev) = claimed {
                        if s.devices[dev].in_flight > 0 {
                            s.devices[dev].in_flight -= 1;
                        }
                    }
                }
                // F779/F778: wake the loop ONLY when this release actually freed a DEVICE — a
                // device-less idle job frees nothing dispatchable, so notifying just re-wakes the
                // loop into an immediate re-pick of the same task: the measured ~40/sec
                // judge_observed/skipped spin. Intervention has its own explicit notify; the 15s
                // tick still fires the device-less judge, just not at CPU speed.
                //
                // F893: and never notify for a JUDGE release even WITH a claimed device — the
                // claimed-device variant of the same spin. Measured live (fleet sb-6 run, fix
                // round): idle fleet -> judge claims a device -> the dedup skip returns in
                // microseconds (no LLM call) -> this release notified -> immediate re-pick of the
                // same unchanged task, 333 observe/skip/verdict cycles in five minutes. The judge
                // job's own `intervened` notify covers the one case where waking the loop buys
                // anything; everything else re-evaluates on the tick.
                if claimed.is_some() && !is_judge {
                    if let Some(n) = notify {
                        n.notify_one();
                    }
                }
            });
        }
    }
}

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

enum SchedulerDispatcher {
    Legacy(Arc<dyn TaskDispatcher>),
    Physical {
        control: PhysicalAdmissionControl,
        dispatcher: Arc<dyn ProviderLifecycleDispatcher>,
        execution: PhysicalExecutionAuthority,
    },
}

#[derive(Clone)]
struct SemanticObservationConfig {
    producer: Arc<dyn SemanticObservationSnapshotProducer>,
    reviewer: Arc<dyn AdmittedSemanticObservationReviewer>,
}

#[derive(Clone)]
struct SchedulerSemanticObservationRuntime {
    control: PhysicalAdmissionControl,
    plane: BrokeredSemanticObservationPlane,
    producer: Arc<dyn SemanticObservationSnapshotProducer>,
    reviewer: Arc<dyn AdmittedSemanticObservationReviewer>,
    sink: Arc<dyn EventSink>,
}

struct ScheduledSemanticObservationCaptureRequest {
    request: SemanticObservationCaptureRequest,
    task_authority: EngineSemanticTaskAuthority,
}

enum SemanticObservationAttemptDisposition {
    NoCapture,
    Deferred,
    Consumed,
}

struct SemanticObservationAttemptResult {
    task_id: TaskId,
    revision: Option<SemanticTraceRevision>,
    disposition: SemanticObservationAttemptDisposition,
}

impl SchedulerSemanticObservationRuntime {
    async fn available_provider_slots(&self) -> usize {
        self.control
            .physical_occupancy()
            .await
            .into_iter()
            .map(|host| {
                host.capacity
                    .saturating_sub(host.provider_turn_permits_held) as usize
            })
            .sum()
    }

    async fn observe(
        &self,
        scheduled: ScheduledSemanticObservationCaptureRequest,
        last_consumed: Option<SemanticTraceRevision>,
        last_summoned: Option<SemanticTraceRevision>,
    ) -> SemanticObservationAttemptResult {
        let ScheduledSemanticObservationCaptureRequest {
            request,
            task_authority,
        } = scheduled;
        let task_id = request.task_id.clone();
        let attempt = request.attempt;
        let task_evidence = match self
            .plane
            .register_scheduler_task_evidence(task_authority, &request)
        {
            Ok(evidence) => evidence,
            Err(error) => {
                self.capture_failed(
                    &task_id,
                    attempt,
                    format!("semantic scheduler task authority was rejected: {error}"),
                );
                return SemanticObservationAttemptResult {
                    task_id,
                    revision: None,
                    disposition: SemanticObservationAttemptDisposition::NoCapture,
                };
            }
        };
        if let Err(reason) = request.activity_publisher.validate() {
            self.capture_failed(
                &task_id,
                attempt,
                format!("semantic capture request has invalid activity authority: {reason}"),
            );
            return SemanticObservationAttemptResult {
                task_id,
                revision: None,
                disposition: SemanticObservationAttemptDisposition::NoCapture,
            };
        }
        let admitted_source = &request.activity_publisher.source;
        if request.task_id != admitted_source.task_id || request.attempt != admitted_source.attempt
        {
            self.capture_failed(
                &task_id,
                attempt,
                "semantic capture request task/attempt diverges from its admitted source authority"
                    .to_string(),
            );
            return SemanticObservationAttemptResult {
                task_id,
                revision: None,
                disposition: SemanticObservationAttemptDisposition::NoCapture,
            };
        }
        let capture = match self.producer.capture(request.clone()).await {
            Ok(Some(capture)) => capture,
            Ok(None) => {
                return SemanticObservationAttemptResult {
                    task_id,
                    revision: None,
                    disposition: SemanticObservationAttemptDisposition::NoCapture,
                }
            }
            Err(reason) => {
                self.capture_failed(&task_id, attempt, reason);
                return SemanticObservationAttemptResult {
                    task_id,
                    revision: None,
                    disposition: SemanticObservationAttemptDisposition::NoCapture,
                };
            }
        };
        let revision = capture.revision();
        if revision.authority_scope != admitted_source.authority_scope
            || revision.phase_epoch != admitted_source.phase_epoch
            || revision.task_id != request.task_id
            || revision.attempt != request.attempt
        {
            self.capture_failed(
                &request.task_id,
                request.attempt,
                format!(
                    "snapshot authority {:?}/epoch {} task `{}` attempt {} does not match admitted authority {:?}/epoch {} task `{}` attempt {}",
                    revision.authority_scope,
                    revision.phase_epoch,
                    revision.task_id,
                    revision.attempt,
                    admitted_source.authority_scope,
                    admitted_source.phase_epoch,
                    admitted_source.task_id,
                    admitted_source.attempt,
                ),
            );
            return SemanticObservationAttemptResult {
                task_id,
                revision: None,
                disposition: SemanticObservationAttemptDisposition::NoCapture,
            };
        }
        if let Err(reason) =
            self.plane
                .publish_scheduler_activity(&task_evidence, &request, &capture)
        {
            self.capture_failed(
                &request.task_id,
                request.attempt,
                format!("semantic activity publication was rejected: {reason}"),
            );
            return SemanticObservationAttemptResult {
                task_id,
                revision: Some(revision),
                disposition: SemanticObservationAttemptDisposition::NoCapture,
            };
        }
        if let Some(last) = &last_consumed {
            if revision.source_revision < last.source_revision
                || (revision.source_revision == last.source_revision
                    && revision.snapshot_hash == last.snapshot_hash
                    && revision.attempt == last.attempt)
            {
                return SemanticObservationAttemptResult {
                    task_id,
                    revision: Some(revision),
                    disposition: SemanticObservationAttemptDisposition::NoCapture,
                };
            }
            if revision.source_revision == last.source_revision {
                self.capture_failed(
                    &request.task_id,
                    request.attempt,
                    format!(
                        "trace revision {} changed immutable identity from `{}` to `{}`",
                        revision.source_revision, last.snapshot_hash, revision.snapshot_hash
                    ),
                );
                return SemanticObservationAttemptResult {
                    task_id,
                    revision: Some(revision),
                    disposition: SemanticObservationAttemptDisposition::NoCapture,
                };
            }
        }

        if last_summoned.as_ref() != Some(&revision) {
            self.sink.emit(&SwarmEvent::SemanticObservationSummoned {
                source: semantic_observation_task_version(capture.snapshot()),
                signal: capture.summons().neutral_signal(),
            });
        }

        let mut eligible_logical_device_ids: Vec<String> = self
            .control
            .verified_lanes()
            .await
            .into_iter()
            .filter(|lane| lane.logical_device_id != request.running_logical_device_id)
            .map(|lane| lane.logical_device_id)
            .collect();
        eligible_logical_device_ids.sort();
        eligible_logical_device_ids.dedup();
        if let Some(mut reviewer_routes) = self.reviewer.eligible_logical_device_ids() {
            reviewer_routes.sort();
            reviewer_routes.dedup();
            eligible_logical_device_ids.retain(|logical_device_id| {
                reviewer_routes.binary_search(logical_device_id).is_ok()
            });
        }
        if eligible_logical_device_ids.is_empty() {
            self.sink.emit(&SwarmEvent::SemanticObservationDeferred {
                task_id: revision.task_id.clone(),
                attempt: revision.attempt,
                source_revision: revision.source_revision,
                snapshot_hash: revision.snapshot_hash.clone(),
                reason: "no_verified_alternate_provider_route".to_string(),
            });
            return SemanticObservationAttemptResult {
                task_id,
                revision: Some(revision),
                disposition: SemanticObservationAttemptDisposition::Deferred,
            };
        }

        let submission = self
            .plane
            .submit_if_idle(
                capture.into_snapshot(),
                SemanticObservationAdmissionPolicy {
                    task_rank: request.task_rank,
                    eligible_logical_device_ids,
                    preferred_model_id: None,
                    excluded_logical_device_id: Some(request.running_logical_device_id),
                },
                self.reviewer.clone(),
            )
            .await;
        match submission {
            Ok(None) => SemanticObservationAttemptResult {
                task_id,
                revision: Some(revision),
                disposition: SemanticObservationAttemptDisposition::Deferred,
            },
            Ok(Some(SemanticObservationAdmissionSubmission::Rejected(_))) => {
                SemanticObservationAttemptResult {
                    task_id,
                    revision: Some(revision),
                    disposition: SemanticObservationAttemptDisposition::Consumed,
                }
            }
            Ok(Some(SemanticObservationAdmissionSubmission::Started(handle))) => {
                if let Err(error) = handle.wait().await {
                    self.capture_failed(&request.task_id, request.attempt, error.to_string());
                }
                SemanticObservationAttemptResult {
                    task_id,
                    revision: Some(revision),
                    disposition: SemanticObservationAttemptDisposition::Consumed,
                }
            }
            Err(error) => {
                self.capture_failed(&request.task_id, request.attempt, error.to_string());
                SemanticObservationAttemptResult {
                    task_id,
                    revision: Some(revision),
                    disposition: SemanticObservationAttemptDisposition::Consumed,
                }
            }
        }
    }

    fn capture_failed(&self, task_id: &str, attempt: u32, reason: String) {
        self.sink
            .emit(&SwarmEvent::SemanticObservationCaptureFailed {
                task_id: task_id.to_string(),
                attempt,
                reason,
            });
    }
}

/// Global cap on SPECULATIVE twins per run (GOOSE_SWARM_SPECULATE) — a long serial chokepoint cannot burn
/// unbounded compute racing twins. Generous: it is a last-resort idle-fill, not a hot path.
const SPECULATION_CAP: u32 = 8;

/// S7: total generated-test jobs per run. Each lands at most one new pytest file; 3 files of
/// 3-5 functions is the design's own ask, and a cap keeps a long idle tail from burning
/// unbounded generations on one app.
const TESTGEN_CAP: u32 = 3;

/// F779: total tail-review jobs per run. "Generous" was written when this mechanism emitted no
/// event and nobody could see what it cost — the ceiling was reasoned about as noise/CPU because
/// the reviews are read-only.
///
/// MEASURED the first run that instrumented it (run 6): 27 calls, 12,382s — **3.4 hours of fleet
/// time in one run** — for 7 findings, five of the calls sitting at the 900s timeout having found
/// nothing. It is dispatched into EVERY free slot on EVERY scheduler tick, so a long tail
/// multiplies it, and it takes the idle slot ahead of `testgen`, whose output IS consumed. The
/// per-call cap (240s, swarm.rs) bounds one review; this bounds the run. 8 calls rotates the
/// dimension set roughly twice, which is where every finding in the measured run came from —
/// the productive dimension (`wiring`) found 5 defects in under 3 minutes each, all early.
const TAIL_REVIEW_CAP: u32 = 8;

struct State {
    dag: Dag,
    ready: BinaryHeap<Ranked>,
    devices: Vec<DeviceRt>,
    held_files: HashSet<String>,
    held_by: HashMap<TaskId, Vec<String>>,
    claimed_device: HashMap<TaskId, usize>,
    physical_activity_publishers: HashMap<TaskId, SemanticActivityPublisher>,
    dispatched_per_device: HashMap<String, u32>,
    ctx: SharedContext,
    max_attempts: u32,
    degrade_on_stall: bool,
    sink: Arc<dyn EventSink>,
    attempt_started_at: HashMap<TaskId, Instant>,
    attempt_log: HashMap<TaskId, Vec<AttemptRecord>>,
    task_session: HashMap<TaskId, Option<String>>,
    task_tool_calls: HashMap<TaskId, Vec<ToolCallRecord>>,
    /// (device_id, model_id) of each task's most recent attempt.
    task_final_device: HashMap<TaskId, (String, String)>,
    /// The user goal (passed to the replanner) + how many replan rounds have run.
    goal: String,
    /// The user's VERBATIM answers to the clarifying questions, for the worker prompt. Empty when the run
    /// never asked. Unlike `goal` — which reaches only the replanner/judge/pre-reviewer — this is handed to
    /// every DispatchRequest, because there was previously no path from an answer to a worker at all.
    user_decisions: String,
    /// GROUNDED research facts (Phase 1, Move 2), VERBATIM, handed to every DispatchRequest alongside
    /// `user_decisions`. Empty when DOC_PREFETCH is off => the worker prompt is byte-identical.
    doc_facts: String,
    replans_done: u32,
    /// How many tasks were still incomplete the last time the replanner answered with NOTHING.
    ///
    /// An empty answer used to burn the entire budget (`replans_done = max_replans`), which turned
    /// "no more work is needed right now" into "never ask again". MEASURED on a live 3-node run: the
    /// replan was asked at +50min with 9 of 18 tasks done, correctly declined because half the DAG was
    /// still queued, and was thereby disabled for good — so at +68min, with ONE task in flight, two
    /// nodes idle and idle_capacity()==5, the one mechanism built to fill them was off.
    ///
    /// The replanner's answer is a function of the DAG state when it was asked, so it is cached
    /// against that state rather than forever: it may be asked again once STRICTLY FEWER tasks remain,
    /// which is the only situation in which it could honestly give a different answer.
    replan_declined_at_incomplete: Option<usize>,
    /// Ids of replanner-added (bonus) tasks — failures here are non-fatal to the run.
    bonus_ids: HashSet<TaskId>,
    /// Observed per-device speed: device index -> (total completed ms, count). Used to route the
    /// hardest tasks (incl. integrate-verify) to the proven-fastest node on an identical-model fleet.
    device_speed: HashMap<usize, (u64, u32)>,
    /// Judge support — empty/false unless a judge is attached. `abort_handles` lets the judge kill a
    /// stuck worker's future; `prior_hints` carries the judge's corrective note onto the re-dispatch;
    /// `interventions` caps kills per task; `judge_running` keeps at most one judge in flight at a time
    /// (never two judging the same worker); `idle_jobs` counts ALL running idle jobs (the judge + any
    /// pre-reviews) so up to `idle_capacity()` run CONCURRENTLY — one per free node — instead of the old
    /// single shared slot that let the judge starve pre-review and left a second idle node asleep.
    abort_handles: HashMap<TaskId, tokio::task::AbortHandle>,
    /// Owned-file fingerprint recorded at each judge kill. A SECOND kill is allowed only if the
    /// files moved since the previous one — a restart that repeats a no-progress attempt costs a
    /// full task restart (measured 4-25 min) and converges on nothing the deterministic
    /// backstops would not handle better.
    kill_tree_hash: HashMap<TaskId, u64>,
    prior_hints: HashMap<TaskId, String>,
    /// Every corrective note the judge has produced this run, in order, and NEVER consumed.
    ///
    /// `prior_hints` is keyed by task and REMOVED on the next dispatch of that task, so a judge
    /// finding survives exactly one re-dispatch and then vanishes. That is right for guiding a retry
    /// and wrong for everything else — most of all for the SINK, which is told "you are the ONLY task
    /// permitted to edit files here" and whose entire job is fixing what upstream found.
    ///
    /// MEASURED: the judge caught a real defect mid-run — "EXPECTED_SORTED_IDS has wrong order,
    /// pay_005 at +01:00 converts to 07:00Z (earliest), not pay_002" — handed it to `test-meridian`,
    /// and it was consumed. The sink then spent roughly 20 of its 30 minutes REDISCOVERING that same
    /// bug: six overlapping `sed` reads of test_meridian.py and a hand-written python one-liner
    /// recomputing the very sort the judge had already worked out. The information existed; nothing
    /// carried it to the one task that could act on it.
    judge_notes: Vec<(TaskId, String)>,
    interventions: HashMap<TaskId, u32>,
    /// Omni-judge aborts per task. Counted SEPARATELY from `interventions` on purpose: that map also caps
    /// how many times the deterministic judge may act on a task (max_interventions_per_task), and spending
    /// that budget on a model's reasoning-loop abort would leave a genuinely stuck task with no
    /// deterministic supervisor at the point it needs one most.
    omni_aborts: HashMap<TaskId, u32>,
    /// Attempts lost to a TRANSPORT fault (mid-stream body drop) rather than to the task.
    /// MEASURED (qwen3.8 r1): one LAN-flaky node dropped streams repeatedly and three modules —
    /// api, frontend-viz, frontend-page-cli — burned all four attempts on it and were LOST, two of
    /// them never recovered. A socket that dies mid-generation says nothing about the work, exactly
    /// like a judge kill, so it must not consume the task's failure budget.
    transport_drops: HashMap<TaskId, u32>,
    /// Split generation per task: 0 for original tasks, parent+1 for children injected by a split. Feeds
    /// JudgeRequest.split_count so the judge caps splitting at once (a split-child is never re-split).
    split_generation: HashMap<TaskId, u32>,
    judge_running: bool,
    /// Which node is running the CURRENT judge. A single Option is sufficient and correct because
    /// `judge_running` makes the judge single-flight — `judge_observed` and `judge_verdict` counts
    /// match exactly (103/103, 72/72, 64/64, 43/43) across every archived run, and they never
    /// interleave. If that invariant is ever relaxed this must become a per-task map.
    judge_node: Option<String>,
    task_completion: HashMap<String, TaskCompletionDisposition>,
    idle_jobs: u32,
    /// SINK IDLE-FILL (GOOSE_SWARM_SINK_REVIEW): rotating review-dimension index for idle nodes during the
    /// sink, so successive idle reviews cover different angles.
    sink_review_dim: usize,
    /// When each task was last judged, so an OK ("observed") task is NOT re-judged every 15s tick for its
    /// whole life — that fired ~4 wasted model calls/min on a single long worker, which LM Studio piled onto
    /// a busy node (one node "+1 QUEUED" while another sat idle). A re-judge waits `JUDGE_REJUDGE_COOLDOWN`.
    last_judged: HashMap<TaskId, Instant>,
    /// SPECULATIVE EXECUTION (GOOSE_SWARM_SPECULATE, default-OFF). When a node would otherwise sit idle at a
    /// serial dependency chokepoint, a TWIN of the in-flight task is raced on the idle device (first-to-finish
    /// wins). The twin runs in a shadow workspace (dispatcher side) so it never touches `held_files` — only
    /// the PRIMARY ever holds the real owned files. These maps track the twin's OWN device claim, keyed by
    /// the task id; `speculating` marks a task that currently has a twin. All empty unless the flag is on, so
    /// the validated path is byte-identical.
    spec_device: HashMap<TaskId, usize>,
    spec_started_at: HashMap<TaskId, Instant>,
    spec_abort: HashMap<TaskId, tokio::task::AbortHandle>,
    speculating: HashSet<TaskId>,
    spec_count: u32,
    /// S7 (GOOSE_SWARM_TESTGEN): generated-test jobs fired this run, capped at TESTGEN_CAP.
    testgen_count: u32,
    /// F883/E8: set for the repair-round scheduler run — disables testgen idle-fill (its landed
    /// files write the REAL tree, which a fix round must never touch except via a graded promote).
    fix_round: bool,
    /// F779: tail-review jobs fired this run (capped at TAIL_REVIEW_CAP) + its rotating dimension.
    tail_review_count: u32,
    tail_review_dim: usize,
    /// Physical mode queues every task-derived opportunity centrally; logical device weights do
    /// not admit work or emit route/timing facts before the broker selects a verified lane.
    physical_admission: bool,
    physical_execution: Option<PhysicalExecutionAuthority>,
}

struct SchedulerPhysicalAuthority {
    state: Arc<Mutex<State>>,
    notify: Arc<Notify>,
}

#[async_trait::async_trait]
impl PhysicalDispatchAuthority for SchedulerPhysicalAuthority {
    async fn opportunity(&self, req: &DispatchRequest) -> Result<WorkOpportunity, DispatchError> {
        let state = self.state.lock().await;
        let node = state.dag.tasks.get(&req.task_id).ok_or_else(|| {
            DispatchError::Terminal(format!(
                "physical dispatch cannot find claimed task `{}`",
                req.task_id
            ))
        })?;
        if node.state != TaskState::Claimed || node.attempts != req.attempt {
            return Err(DispatchError::Terminal(format!(
                "physical dispatch task `{}` is no longer claimed at attempt {}",
                req.task_id, req.attempt
            )));
        }
        let mut eligible: Vec<String> = state
            .devices
            .iter()
            .filter(|device| device.cfg.enabled && !device.cfg.supervision)
            .map(|device| device.cfg.id.clone())
            .collect();
        eligible.sort();
        let excluded = node.avoid_device.as_ref().filter(|avoid| {
            eligible.len() > 1
                && eligible
                    .iter()
                    .any(|device| device.as_str() != avoid.as_str())
        });
        let execution = state.physical_execution.as_ref().ok_or_else(|| {
            DispatchError::Terminal(
                "physical dispatch has no explicit run/phase execution authority".to_string(),
            )
        })?;
        let role = execution.role;
        let source = TaskVersion {
            authority_scope: execution.scope.clone(),
            phase_epoch: execution.phase_epoch,
            task_id: req.task_id.clone(),
            attempt: req.attempt,
            revision: u64::from(req.attempt) + 1,
            kind: SourceRevisionKind::TaskAttempt,
        };
        Ok(WorkOpportunity {
            work_id: format!(
                "{}:epoch:{}:attempt:{}",
                source.authority_key(),
                source.phase_epoch,
                source.attempt
            ),
            role,
            priority: role.priority(),
            task_rank: node.fan_out as u64,
            source,
            eligible_logical_device_ids: eligible,
            preferred_model_id: node.spec.preferred_model.clone(),
            excluded_logical_device_id: excluded.cloned(),
        })
    }

    async fn route_admitted(
        &self,
        req: &mut DispatchRequest,
        admission: &AdmissionReceipt,
    ) -> Result<(), DispatchError> {
        let mut state = self.state.lock().await;
        let device_index = state
            .devices
            .iter()
            .position(|device| {
                device.cfg.enabled
                    && !device.cfg.supervision
                    && device.cfg.id == admission.logical_device_id
                    && device.cfg.model_id == admission.model_id
            })
            .ok_or_else(|| {
                DispatchError::Terminal(format!(
                    "broker admitted unconfigured route `{}`/`{}`",
                    admission.logical_device_id, admission.model_id
                ))
            })?;
        let node = state.dag.tasks.get(&req.task_id).ok_or_else(|| {
            DispatchError::Terminal(format!("broker admitted missing task `{}`", req.task_id))
        })?;
        if node.state != TaskState::Claimed || node.attempts != req.attempt {
            return Err(DispatchError::Terminal(format!(
                "broker admitted stale task `{}` attempt {}",
                req.task_id, req.attempt
            )));
        }
        if admission.source.task_id != req.task_id || admission.source.attempt != req.attempt {
            return Err(DispatchError::Terminal(format!(
                "broker admission source does not match task `{}` attempt {}",
                req.task_id, req.attempt
            )));
        }
        if state.claimed_device.contains_key(&req.task_id) {
            return Err(DispatchError::Terminal(format!(
                "task `{}` received more than one physical route",
                req.task_id
            )));
        }

        req.device_id = admission.logical_device_id.clone();
        req.model_id = admission.model_id.clone();
        let activity_publisher = SemanticActivityPublisher::from_admission(admission);
        activity_publisher.validate().map_err(|reason| {
            DispatchError::Terminal(format!(
                "broker minted invalid semantic activity publisher: {reason}"
            ))
        })?;
        req.activity_publisher = Some(activity_publisher.clone());
        state.devices[device_index].in_flight += 1;
        state
            .claimed_device
            .insert(req.task_id.clone(), device_index);
        state
            .physical_activity_publishers
            .insert(req.task_id.clone(), activity_publisher);
        *state
            .dispatched_per_device
            .entry(req.device_id.clone())
            .or_default() += 1;
        state
            .attempt_started_at
            .insert(req.task_id.clone(), Instant::now());
        state.task_final_device.insert(
            req.task_id.clone(),
            (req.device_id.clone(), req.model_id.clone()),
        );
        let node = &state.dag.tasks[&req.task_id];
        state.sink.emit(&SwarmEvent::TaskDispatched {
            task_id: req.task_id.clone(),
            device: req.device_id.clone(),
            model: req.model_id.clone(),
            attempt: req.attempt,
            deps: node.spec.deps.clone(),
            owned_files: req.owned_files.clone(),
            context_slice_len: req.context_slice.len(),
            description_chars: req.description.len(),
            difficulty: match node.spec.difficulty {
                Difficulty::Hard => "hard".to_string(),
                _ => "easy".to_string(),
            },
        });
        drop(state);
        self.notify.notify_one();
        Ok(())
    }
}

impl State {
    fn all_terminal(&self) -> bool {
        self.dag.tasks.values().all(|n| n.state.is_terminal())
    }

    /// Tasks not yet terminal. The replan re-arm keys off this: a decline is only stale once the DAG
    /// has actually shrunk.
    fn incomplete_count(&self) -> usize {
        self.dag
            .tasks
            .values()
            .filter(|n| !n.state.is_terminal())
            .count()
    }

    /// The worker session this task ran in, if one was recorded.
    ///
    /// Every FAILURE emit site hard-coded `session_id: None`, so a failed task's full trace — every
    /// tool request and response in the sessions DB — was unjoinable, which is precisely the task you
    /// most want to read. The map was already there and already populated on dispatch; line ~1944
    /// performs this exact lookup for another event. Same class of defect as the missing `error`:
    /// the engine had the value and the event dropped it.
    fn task_session_id(&self, tid: &str) -> Option<String> {
        self.task_session.get(tid).cloned().flatten()
    }

    /// The reason the task's LAST attempt ended, or `None` if it succeeded.
    ///
    /// One helper rather than six inline expressions, because six copies of a rule is how the
    /// dispatch paths drifted apart before (`pick_device` learned speed-weight routing and the repair
    /// path did not). Every `TaskCompleted` reads this, so a successful task naturally reports `None`
    /// — the winning attempt carries no error — and a failure reports the string the engine already
    /// had and used to discard.
    fn last_attempt_error(&self, tid: &str) -> Option<String> {
        self.attempt_log
            .get(tid)
            .and_then(|a| a.last())
            .and_then(|r| r.error.clone())
    }

    fn total_in_flight(&self) -> u32 {
        self.devices.iter().map(|d| d.in_flight).sum()
    }

    /// In-flight on BUILD devices only. The stuck-bail and the judge's nothing-running gate must
    /// key on this: a supervision device grinding a review while the DAG is blocked would
    /// otherwise mask the stall (the reader's constraint — a masked stall never bails).
    fn build_in_flight(&self) -> u32 {
        self.devices
            .iter()
            .filter(|d| !d.cfg.supervision)
            .map(|d| d.in_flight)
            .sum()
    }

    fn physical_dispatch_pending(&self) -> bool {
        self.physical_admission
            && self.dag.tasks.iter().any(|(task_id, node)| {
                node.state == TaskState::Claimed && !self.claimed_device.contains_key(task_id)
            })
    }

    /// Any enabled supervision device with a free slot? The A3 last-slot yield protects BUILD
    /// slots; when the free device is a supervision one the yield must not veto the pick (the
    /// preference in least_loaded_free_device guarantees that device is the one claimed).
    fn has_free_supervision_device(&self) -> bool {
        self.devices
            .iter()
            .any(|d| d.cfg.enabled && d.cfg.supervision && d.in_flight < d.cfg.weight)
    }

    /// Free BUILD worker slots across enabled devices (how much parallel build work could start
    /// right now). Supervision devices are excluded: they can never take build work, and counting
    /// them would (a) open the replan gate (>=2) on a 1-node run and (b) defeat the A3 last-slot
    /// yield — both node-count semantics, both build-only by contract.
    fn idle_capacity(&self) -> u32 {
        self.devices
            .iter()
            .filter(|d| d.cfg.enabled && !d.cfg.supervision)
            .map(|d| d.cfg.weight.saturating_sub(d.in_flight))
            .sum()
    }

    /// K1: is the integrate-verify SINK the (only) in-flight task? Dynamic-replan is suppressed in this
    /// window — the sink verifies-by-running the whole tree and owns NO files, so a bonus task completing
    /// here could land UNVERIFIED code AFTER the sink's PASS. Before the sink starts (its deps are every
    /// other task, so it runs alone at the end) other tasks are still in flight and replan is fine; this
    /// only guards the exact sink-race window.
    /// Mandatory (planned) work still outstanding — bonus tasks excluded.
    ///
    /// `incomplete_count` counts the whole DAG, and the DAG grows every time the replanner adds a
    /// task. So once bonus work is in flight, the very tasks that make the tail long also make the
    /// DAG look busy, and any gate reading `incomplete_count` sees a healthy run right up to the end.
    fn mandatory_incomplete(&self) -> usize {
        self.dag
            .tasks
            .iter()
            .filter(|(id, n)| !self.bonus_ids.contains(*id) && !n.state.is_terminal())
            .count()
    }

    /// Every planned (non-bonus) task, terminal or not — the denominator for the replan gate.
    fn mandatory_total(&self) -> usize {
        self.dag
            .tasks
            .keys()
            .filter(|id| !self.bonus_ids.contains(*id))
            .count()
    }

    fn sink_in_flight(&self) -> bool {
        self.dag
            .tasks
            .iter()
            .any(|(id, n)| n.state == TaskState::Claimed && id.as_str() == "integrate-verify")
    }

    /// B2: any replanner-added task not yet terminal? The sink's claim gate keys on this — bonus
    /// work must land BEFORE the join certifies the tree, or it ships unverified after the PASS.
    fn bonus_incomplete(&self) -> bool {
        self.bonus_ids.iter().any(|id| {
            self.dag
                .tasks
                .get(id)
                .is_some_and(|n| !n.state.is_terminal())
        })
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

    fn dag_specs_snapshot(&self) -> Vec<TaskSpec> {
        let mut specs: Vec<TaskSpec> = self
            .dag
            .tasks
            .values()
            .map(|node| node.spec.clone())
            .collect();
        specs.sort_by(|left, right| left.id.cmp(&right.id));
        specs
    }

    fn semantic_observation_capture_requests(
        &self,
        already_capturing: &HashSet<TaskId>,
        available_provider_slots: usize,
    ) -> Vec<ScheduledSemanticObservationCaptureRequest> {
        if available_provider_slots == 0 {
            return Vec::new();
        }
        let mut task_ids: Vec<TaskId> = self
            .dag
            .tasks
            .iter()
            .filter(|(task_id, node)| {
                node.state == TaskState::Claimed
                    && self.claimed_device.contains_key(*task_id)
                    && !already_capturing.contains(*task_id)
            })
            .map(|(task_id, _)| task_id.clone())
            .collect();
        task_ids.sort_by(|left, right| {
            self.attempt_started_at
                .get(left)
                .cmp(&self.attempt_started_at.get(right))
                .then_with(|| left.cmp(right))
        });
        task_ids
            .into_iter()
            .take(available_provider_slots)
            .filter_map(|task_id| self.semantic_observation_capture_request(&task_id))
            .collect()
    }

    fn semantic_observation_capture_request(
        &self,
        task_id: &str,
    ) -> Option<ScheduledSemanticObservationCaptureRequest> {
        let node = self.dag.tasks.get(task_id)?;
        let device_index = *self.claimed_device.get(task_id)?;
        let running_device = self.devices.get(device_index)?;
        let contract_version = task_contract_version(&node.spec);

        let dependency_contract_versions = node
            .spec
            .deps
            .iter()
            .filter_map(|dependency_id| {
                self.dag.tasks.get(dependency_id).map(|dependency| {
                    (
                        dependency_id.clone(),
                        task_contract_version(&dependency.spec),
                    )
                })
            })
            .collect::<BTreeMap<_, _>>();

        let mut sibling_ids = BTreeSet::new();
        for dependency_id in &node.spec.deps {
            if let Some(dependents) = self.dag.dependents.get(dependency_id) {
                sibling_ids.extend(
                    dependents
                        .iter()
                        .filter(|sibling_id| sibling_id.as_str() != task_id)
                        .cloned(),
                );
            }
        }
        let sibling_contract_versions = sibling_ids
            .into_iter()
            .filter_map(|sibling_id| {
                self.dag
                    .tasks
                    .get(&sibling_id)
                    .map(|sibling| (sibling_id, task_contract_version(&sibling.spec)))
            })
            .collect::<BTreeMap<_, _>>();

        let mut allowed_finding_routes = self
            .dag
            .dependents
            .get(task_id)
            .cloned()
            .unwrap_or_default();
        allowed_finding_routes.sort();
        allowed_finding_routes.dedup();

        let request = SemanticObservationCaptureRequest::from_scheduler_state(
            task_id.to_string(),
            node.attempts,
            node.fan_out as u64,
            self.goal.clone(),
            node.spec.description.clone(),
            node.spec.owned_files.clone(),
            contract_version,
            task_acceptance_oracle(&node.spec),
            dependency_contract_versions,
            sibling_contract_versions,
            allowed_finding_routes,
            running_device.cfg.id.clone(),
            running_device.cfg.model_id.clone(),
            self.physical_activity_publishers.get(task_id)?.clone(),
        )
        .ok()?;
        let task_authority =
            EngineSemanticTaskAuthority::mint_from_scheduler_state(&request).ok()?;
        Some(ScheduledSemanticObservationCaptureRequest {
            request,
            task_authority,
        })
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
        // Build dispatch NEVER lands on a supervision device — that is the capped-out set
        // GOOSE_SWARM_MAX_NODES excluded from building; borrowing it is for read-only work only.
        let free: Vec<usize> = self
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.cfg.enabled
                    && !d.cfg.supervision
                    && (self.physical_admission || d.in_flight < d.cfg.weight)
            })
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
        // If avoiding the failed device leaves nothing, WAIT for a different slot instead of
        // rebinding — as long as the fleet has any other build device at all. The fallback used
        // to hand the retry straight back to the device that just killed it, because the kill
        // frees exactly that slot while the rest of the fleet is mid-generation. MEASURED (r8):
        // web-viz stalled at 420s of zero token activity on the slowest node FOUR times, every
        // retry re-landing on it — the same starvation each attempt, and the caller already
        // re-queues a None cleanly (`leftover` -> Ready), so waiting is deadlock-free: another
        // node's completion re-opens dispatch. A single-build-device fleet keeps the fallback.
        let pool = if allowed.is_empty() {
            let another_build_device_exists = n.avoid_device.is_some()
                && self.devices.iter().any(|d| {
                    d.cfg.enabled
                        && !d.cfg.supervision
                        && n.avoid_device.as_deref() != Some(d.cfg.id.as_str())
                });
            if another_build_device_exists {
                return None;
            }
            free
        } else {
            allowed
        };
        if self.physical_admission {
            return pool.into_iter().min();
        }
        // Spread work across the fleet: the LEAST-LOADED device wins, so idle nodes get work before
        // any node doubles up; ties break toward the planner's preferred model, then by index for
        // determinism. (Honoring preferred_model first would pile every same-model task on one device
        // and leave the rest of the fleet idle — the opposite of what a swarm is for.)
        let pm = n.spec.preferred_model.as_deref();
        // A HARD task (the heaviest work, incl. integrate-verify) prefers the FASTEST free node: identical
        // models differ only in host speed, so the critical path shrinks if the big tasks land on the
        // quickest node. Load (in_flight) stays primary, so this never over-concentrates.
        // A REPEATEDLY-RETRIED task is tail risk and gets the same fast-node preference as a hard one
        // — see `dispatch_prefers_fastest_node`. Measured: the run's last task, twice killed, landed
        // on the slowest host for 29 minutes while both faster nodes sat READY.
        let hard = dispatch_prefers_fastest_node(
            matches!(n.spec.difficulty, Difficulty::Hard),
            n.attempts,
        );
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
            // ORDERING DEPENDS ON THE TASK, and this is the whole point of having weights.
            //
            // For a HARD task, an IDLE device beats a BUSIER faster one, and weight decides among
            // equally-loaded devices (A2). Weight used to be ABSOLUTE primary here, and the measured
            // consequence was hard tasks stacking two-deep on the highest-weighted host while a whole
            // node sat idle — and two concurrent generations on one Apple host degrade each other
            // (detail time is QUEUE time, monotonic in concurrency at 7.06 SE, F623; the operator has
            // fixed 2-per-node as the hard ceiling for the same reason). A shared "fast" slot is not
            // faster than a whole "slow" one at this fleet's weight spread, and the critical path is
            // set by exactly these tasks.
            //
            // What the old comment defended still holds one level down: among devices at EQUAL load,
            // weight is decisive and is never overridden by a first-dispatch timing accident —
            // observed ms/task breaks ties only among equally-weighted hosts.
            //
            // For everything else load stays primary, which is what spreads ordinary work across the
            // fleet and keeps idle nodes busy.
            //
            // Operator directive (2026-08-17): the highest-speed-weight host must be the unit that
            // gets the MOST tasks. Ordinary placement used to break equal-load ties by preferred
            // model then INDEX — and index order is discovery order, so on the operator's fleet
            // every tie went to the slowest host (measured on a full app run: gabee 73 dispatches,
            // workhorse 42, the exact inverse of `speed_weights: {gabee:1, local:2,
            // worksmacstudio:3}`). Weight now ranks directly after load in BOTH branches: the
            // fastest host wins every tie, and load-primary still guarantees it is never stacked
            // while a sibling idles.
            let weight_rank = u32::MAX - d.cfg.speed_weight.max(1);
            if hard {
                hard_device_key(d.in_flight, weight_rank, speed, weighted_load, i)
            } else {
                (
                    d.in_flight as u64,
                    weight_rank as u64,
                    prefers_rank as u64,
                    weighted_load,
                    i,
                )
            }
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
            // B2: the sink waits for every replanner-added task, as a CLAIM-TIME gate. splice_specs
            // never rewires integrate-verify's plan-time deps, and the K1 suppression only covers the
            // sink's Claimed window — so bonus tasks raced the join and OUTLIVED it (measured: 1,263s
            // of post-sink solo tail, both bonus tasks failed, their test files written into a tree
            // the sink had already certified). Mutating deps/indegree instead is INERT once the sink
            // is Ready (pick_assignments gates on state, relax_dependents swallows the extra count) —
            // this predicate is pure DAG-state evidence, needs no bookkeeping, and clears itself:
            // a bonus task's terminal state (Done OR Failed) unblocks the claim, so a failed bonus
            // stays non-fatal by construction. The leftover re-push keeps the sink Ready meanwhile.
            if tid.as_str() == "integrate-verify" && self.bonus_incomplete() {
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
        let mut dependency_files: Vec<String> = deps
            .iter()
            .filter_map(|dep| self.dag.tasks.get(dep))
            .flat_map(|node| node.spec.owned_files.iter().cloned())
            .collect();
        dependency_files.sort();
        dependency_files.dedup();
        let neighborhood = self.neighborhood_of(&tid, &deps);
        let slice = if tid == "integrate-verify" {
            let (runtime_reviews, ordinary): (Vec<TaskId>, Vec<TaskId>) =
                deps.iter().cloned().partition(|dep| {
                    self.dag
                        .tasks
                        .get(dep)
                        .is_some_and(|node| node.spec.replan_authority.is_some())
                });
            [
                self.ctx.slice_for_unbounded(&runtime_reviews),
                self.ctx.slice_for(&ordinary),
            ]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
        } else {
            self.ctx.slice_for(&deps)
        };
        let (files, description, attempt, subsplit, replan_authority) = {
            let n = self.dag.tasks.get_mut(&tid).unwrap();
            n.state = TaskState::Claimed;
            (
                n.spec.owned_files.clone(),
                n.spec.description.clone(),
                n.attempts,
                n.spec.subsplit.clone(),
                n.spec.replan_authority.clone(),
            )
        };
        for f in &files {
            self.held_files.insert(f.clone());
        }
        let device_id = self.devices[dev].cfg.id.clone();
        let model_id = self.devices[dev].cfg.model_id.clone();
        if !self.physical_admission {
            self.devices[dev].in_flight += 1;
            self.claimed_device.insert(tid.clone(), dev);
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
                description_chars: description.len(),
                difficulty: match self.dag.tasks[&tid].spec.difficulty {
                    Difficulty::Hard => "hard".to_string(),
                    _ => "easy".to_string(),
                },
            });
        }
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
        let mut prior_hint = self.prior_hints.remove(&tid);
        // THE SINK INHERITS EVERY JUDGE FINDING, because it is the only task that can act on them and
        // the judge is a source of findings it otherwise cannot see. A task that owns no files and
        // joins the graph is the sink; ordinary workers keep the existing one-shot behaviour.
        if owned_files.is_empty() && replan_authority.is_none() && !self.judge_notes.is_empty() {
            let notes = self
                .judge_notes
                .iter()
                .map(|(t, h)| format!("- [{t}] {h}"))
                .collect::<Vec<_>>()
                .join("\n");
            let block = format!(
                "WHAT THE SUPERVISOR ALREADY FOUND while these tasks ran — each was reported to the \
                 worker at the time, but you are the only task that can still act on it. Treat these \
                 as leads you do NOT need to rediscover:\n{notes}"
            );
            prior_hint = Some(match prior_hint {
                Some(existing) => format!("{existing}\n\n{block}"),
                None => block,
            });
        }
        out.push(Assignment {
            task_id: tid.clone(),
            request: DispatchRequest {
                task_id: tid,
                description,
                device_id,
                model_id,
                context_slice: slice,
                dependency_files,
                attempt,
                owned_files,
                all_files,
                prior_hint,
                subsplit,
                speculative: false,
                // The user's own words reach a worker for the first time here. Every other channel the
                // engine claimed (research_findings, the amended spec) is planner-side only.
                user_decisions: self.user_decisions.clone(),
                doc_facts: self.doc_facts.clone(),
                neighborhood,
                replan_authority,
                activity_publisher: None,
            },
        });
    }

    /// The DAG neighborhood of `tid`: its deps ∪ its consumers (reverse edges) ∪ itself, deduped. Used to
    /// scope the frozen-contract bundle to only the modules a worker touches.
    fn neighborhood_of(&self, tid: &str, deps: &[TaskId]) -> Vec<String> {
        let mut n: Vec<String> = deps.to_vec();
        if let Some(consumers) = self.dag.dependents.get(tid) {
            n.extend(consumers.iter().cloned());
        }
        n.push(tid.to_string());
        n.sort();
        n.dedup();
        n
    }

    /// Relax every dependent of a just-finished task: drop its indegree and promote it to Ready at zero.
    /// MUST run for BOTH a normal success AND a finalize-spin salvage (both leave the task Done) — otherwise
    /// a salvaged task leaves its dependents Pending forever, so the CLI/integrate-verify sink never
    /// dispatches and the run ends `scheduler_stuck`. Observed on expense/tmpl: a working library or a
    /// spun-but-written CLI shipped with the entry/verify tasks never run.
    fn relax_dependents(&mut self, tid: &str) {
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

    fn complete(&mut self, tid: &str, attempt: u32, res: Result<TaskRunOutput, DispatchError>) {
        // Ignore a completion from an attempt the judge already superseded (killed + re-dispatched):
        // its device and file holds were released when the judge intervened, so this stale future must
        // not touch the newer attempt's bookkeeping. `attempts` advances on every kill/retry, so a
        // mismatch uniquely identifies a dead attempt.
        if self.dag.tasks.get(tid).map(|n| n.attempts) != Some(attempt) {
            return;
        }
        // SPECULATIVE first-wins: if the task is no longer Claimed, the other instance (primary or twin) of
        // this attempt already accepted it -> this completion is the loser; do nothing (each instance's own
        // device was released by its own path). With speculation OFF no twin exists and the task is always
        // Claimed here, so this guard never triggers (byte-identical).
        if self.dag.tasks.get(tid).map(|n| n.state) != Some(TaskState::Claimed) {
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
                let TaskRunOutput {
                    output,
                    session_id,
                    tool_calls,
                    salvaged,
                } = run;
                self.task_session
                    .insert(tid.to_string(), session_id.clone());
                self.task_tool_calls
                    .insert(tid.to_string(), tool_calls.clone());
                let provisional = salvaged
                    .then(|| {
                        let node = self.dag.tasks.get(tid)?;
                        provisional_task_receipt(
                            &node.spec,
                            attempt,
                            SalvageReason::ProgressWatchdog,
                        )
                    })
                    .flatten();
                if salvaged && provisional.is_none() {
                    let error =
                        "dispatcher requested salvage without complete eligible artifact evidence"
                            .to_string();
                    self.attempt_log
                        .entry(tid.to_string())
                        .or_default()
                        .push(AttemptRecord {
                            device: dev_id.clone(),
                            model: model_id.clone(),
                            outcome: "invalid_salvage".to_string(),
                            error: Some(error.clone()),
                            elapsed_ms,
                        });
                    self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Failed;
                    self.fail_descendants(tid);
                    self.sink.emit(&SwarmEvent::TaskCompleted {
                        task_id: tid.to_string(),
                        salvaged: false,
                        completion: None,
                        status: "failed".to_string(),
                        device: dev_id,
                        model: model_id,
                        attempts: self.attempt_log[tid].len() as u32,
                        elapsed_ms,
                        session_id,
                        error: Some(error),
                        tool_calls,
                    });
                } else {
                    if let Some(dev) = released_dev {
                        let entry = self.device_speed.entry(dev).or_insert((0, 0));
                        entry.0 += elapsed_ms;
                        entry.1 += 1;
                    }
                    let completion = provisional
                        .map(TaskCompletionDisposition::Salvaged)
                        .unwrap_or(TaskCompletionDisposition::Complete);
                    let is_salvaged = completion.is_salvaged();
                    self.task_completion
                        .insert(tid.to_string(), completion.clone());
                    self.attempt_log
                        .entry(tid.to_string())
                        .or_default()
                        .push(AttemptRecord {
                            device: dev_id.clone(),
                            model: model_id.clone(),
                            outcome: if is_salvaged { "salvaged" } else { "ok" }.to_string(),
                            error: None,
                            elapsed_ms,
                        });
                    {
                        let node = self.dag.tasks.get_mut(tid).unwrap();
                        node.state = if is_salvaged {
                            TaskState::Salvaged
                        } else {
                            TaskState::Done
                        };
                        node.result = Some(output.clone());
                        node.avoid_device = None;
                    }
                    self.ctx.merge(tid, output);
                    self.sink.emit(&SwarmEvent::TaskCompleted {
                        task_id: tid.to_string(),
                        salvaged: is_salvaged,
                        completion: Some(completion),
                        status: if is_salvaged { "salvaged" } else { "done" }.to_string(),
                        device: dev_id,
                        model: model_id,
                        attempts: self.attempt_log[tid].len() as u32,
                        elapsed_ms,
                        session_id,
                        error: self.last_attempt_error(tid),
                        tool_calls,
                    });
                    self.relax_dependents(tid);
                }
                // SPECULATIVE abort-loser: this PRIMARY won -> abort + release any twin still racing this
                // task. (When the TWIN won, resolve_speculation cleared `speculating` BEFORE calling
                // complete(), so this is a no-op there.) Off by default -> the maps are empty -> no-op.
                if self.speculating.remove(tid) {
                    if let Some(h) = self.spec_abort.remove(tid) {
                        h.abort();
                    }
                    if let Some(dev) = self.spec_device.remove(tid) {
                        if self.devices[dev].in_flight > 0 {
                            self.devices[dev].in_flight -= 1;
                        }
                    }
                    self.spec_started_at.remove(tid);
                }
            }
            Err(e @ (DispatchError::Transient(_) | DispatchError::ContentRetry(_))) => {
                // A CONTENT failure (pre-done syntax gate) is re-dispatched exactly like a Transient, but its
                // error is threaded into the retry's prior_hint so the fix is guided. Infra transients are not.
                let (msg, is_content) = match e {
                    DispatchError::Transient(m) => (m, false),
                    DispatchError::ContentRetry(m) => (m, true),
                    DispatchError::Terminal(_) => unreachable!(),
                };
                self.attempt_log
                    .entry(tid.to_string())
                    .or_default()
                    .push(AttemptRecord {
                        device: dev_id.clone(),
                        model: model_id.clone(),
                        outcome: if is_content {
                            "content_retry"
                        } else {
                            "transient"
                        }
                        .to_string(),
                        error: Some(msg.clone()),
                        elapsed_ms,
                    });
                // An OMNI-JUDGE abort is supervision too: a model read the call's own reasoning and stopped
                // it. It arrives as a plain Transient with no intervention increment, so without this it
                // burned the task's retry budget — the exact cost the judge-kill exclusion below exists to
                // avoid. It bites hardest on a `verify::` task, which owns no files: the progress-watchdog
                // salvage path is disabled for those, so every omni abort was a pure budget burn pushing an
                // otherwise-healthy verify toward Failed.
                if msg.contains("the judge read this call's own reasoning") {
                    *self.omni_aborts.entry(tid.to_string()).or_insert(0) += 1;
                }
                let exhausted = {
                    // Judge kills advance n.attempts (for the epoch guard) but are SUPERVISORY, not task
                    // failures — and the judge can be wrong (a borderline over-read). Don't let a judge
                    // intervention burn the transient-retry budget: exclude it from the exhaustion count.
                    let judge_kills = self.interventions.get(tid).copied().unwrap_or(0)
                        + self.omni_aborts.get(tid).copied().unwrap_or(0);
                    // A mid-stream body drop is the NETWORK failing, not the task. Counting it
                    // toward exhaustion let a flaky node delete finished-quality modules from the
                    // build (r1: three tasks, two never recovered). Excluded like a judge kill;
                    // bounded because a permanently dead link still exhausts the wall, not the
                    // budget, and the run's own gates still report the missing deliverable.
                    if msg.contains("stream decode error") || msg.contains("mid-stream body drop") {
                        *self.transport_drops.entry(tid.to_string()).or_insert(0) += 1;
                    }
                    let transport = self.transport_drops.get(tid).copied().unwrap_or(0);
                    let n = self.dag.tasks.get_mut(tid).unwrap();
                    n.attempts += 1;
                    n.attempts
                        .saturating_sub(judge_kills)
                        .saturating_sub(transport)
                        >= self.max_attempts
                };
                if exhausted {
                    let provisional = self.dag.tasks.get(tid).and_then(|node| {
                        stall_salvage_receipt(
                            self.degrade_on_stall,
                            is_content,
                            &node.spec,
                            attempt,
                        )
                    });
                    let attempts = self.attempt_log[tid].len() as u32;
                    if let Some(receipt) = provisional {
                        let completion = TaskCompletionDisposition::Salvaged(receipt);
                        self.task_completion
                            .insert(tid.to_string(), completion.clone());
                        self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Salvaged;
                        self.sink.emit(&SwarmEvent::JudgeVerdict {
                            task_id: tid.to_string(),
                            device: dev_id.clone().unwrap_or_default(),
                            judge_node: self.judge_node.clone().unwrap_or_default(),
                            verdict: "degraded_stall".to_string(),
                            confidence: 1.0,
                            hint: "stall-exhausted with every assigned artifact preserved; full ruler still required"
                                .to_string(),
                            action: "degraded".to_string(),
                            deterministic: true,
                        });
                        self.relax_dependents(tid);
                        let ended_because = self.last_attempt_error(tid);
                        self.sink.emit(&SwarmEvent::TaskCompleted {
                            task_id: tid.to_string(),
                            salvaged: true,
                            completion: Some(completion),
                            status: "salvaged".to_string(),
                            device: dev_id,
                            model: model_id,
                            attempts,
                            elapsed_ms,
                            session_id: self.task_session_id(tid),
                            error: ended_because,
                            tool_calls: Vec::new(),
                        });
                    } else {
                        self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Failed;
                        self.fail_descendants(tid);
                        let ended_because = self.last_attempt_error(tid);
                        self.sink.emit(&SwarmEvent::TaskCompleted {
                            task_id: tid.to_string(),
                            salvaged: false,
                            completion: None,
                            status: "failed".to_string(),
                            device: dev_id,
                            model: model_id,
                            attempts,
                            elapsed_ms,
                            session_id: self.task_session_id(tid),
                            error: ended_because,
                            tool_calls: Vec::new(),
                        });
                    }
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
                    // Guided retry: thread the content error into the next attempt's prior_hint (surfaced to
                    // the worker as a SUPERVISOR NOTE). Infra transients carry no hint — a stale content note
                    // on a "model unloaded" retry would mislead the worker.
                    if is_content {
                        self.prior_hints.insert(tid.to_string(), msg.clone());
                        self.judge_notes.push((tid.to_string(), msg.clone()));
                    } else if let Some(hint) = transient_retry_hint(&msg) {
                        // The one infra transient that HAS something to say: see `transient_retry_hint`.
                        self.prior_hints.insert(tid.to_string(), hint);
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
                let ended_because = self.last_attempt_error(tid);
                self.sink.emit(&SwarmEvent::TaskCompleted {
                    task_id: tid.to_string(),
                    salvaged: false,
                    completion: None,
                    status: "failed".to_string(),
                    device: dev_id,
                    model: model_id,
                    attempts,
                    elapsed_ms,
                    session_id: self.task_session_id(tid),
                    error: ended_because,
                    tool_calls: Vec::new(),
                });
            }
        }
    }

    /// Choose an in-flight worker for the judge to inspect: the longest-running Claimed task that is at
    /// least `min_age_secs` old and under its intervention cap, to be judged on a currently-idle device.
    /// Returns the request + the attempt inspected, and marks a judge running (at most one at a time).
    /// A3: the device an IDLE JOB (judge / pre-review / sink-review / twin) claims — the
    /// LEAST-LOADED free device, never the first free one. `.position()` stacked a review as a
    /// second concurrent generation beside a busy worker while another node sat physically idle
    /// (CONFIRMED live), and concurrent generations on one Apple host degrade each other (F623).
    /// Ties break by index for determinism, matching the old behavior on an evenly-loaded fleet.
    fn least_loaded_free_device(&self) -> Option<usize> {
        // Supervision devices sort FIRST (false < true): idle work lands on the borrowed machines
        // before it ever claims a build slot, so on a capped run the lone build node keeps
        // building while the excluded machines carry the reviews.
        self.devices
            .iter()
            .enumerate()
            .filter(|(_, d)| d.cfg.enabled && d.in_flight < d.cfg.weight)
            .min_by_key(|(i, d)| (!d.cfg.supervision, d.in_flight, *i))
            .map(|(i, _)| i)
    }

    fn pick_judge_target(
        &mut self,
        cfg: &JudgeConfig,
    ) -> Option<(JudgeRequest, u32, Option<usize>)> {
        // The LLM review wants an idle device; the deterministic checks (won't-compile / no-output /
        // wrote-then-stale) need no model at all. CLAIM an idle device's slot for the review (so a worker +
        // the next idle-job never stack on it), but fall through with no claim + an empty model_id so the
        // deterministic verdicts still fire when every node is busy (saturated) — a stuck worker must not go
        // unjudged. The actual claim (in_flight bump) happens at the end, only if a task is selected.
        let claimed_device = self.least_loaded_free_device();
        let judge_model_id = claimed_device
            .map(|i| self.devices[i].cfg.model_id.clone())
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
            // Re-judge cooldown: an already-judged task is not re-inspected until JUDGE_REJUDGE_COOLDOWN_SECS
            // has passed, so an OK long worker is not re-judged every tick (the wasted-call/queue-on-busy-node
            // problem). Applies ONLY to UNDER-CAP re-judging — a cap-exhausted stuck task is NEVER cooled down,
            // so its terminal-fail stays prompt. The first judge is gated only by min_age_secs above.
            if !at_cap
                && self
                    .last_judged
                    .get(tid)
                    .map(|t| t.elapsed().as_secs() < cfg.rejudge_cooldown_secs)
                    .unwrap_or(false)
            {
                continue;
            }
            // Skip RE-judging an owns-NOTHING task (the integrate-verify sink). Every deterministic
            // judge gate is disarmed for it (over-read/finalize-spin/broken-code all require owned
            // files, judge.rs:292/311/332), and its LLM verdict is always a non-actionable "ok", so a
            // re-judge catches nothing yet steals an idle node from sink-review. Judge it ONCE (first
            // pass, for observability) then leave it to worker_timeout as the hard-stall backstop.
            // …that rationale held while NO verdict could fire for an owns-nothing task. One can now:
            // the Accept branch for a join that has acted and then gone quiet (judge.rs). Judging it
            // once and never again would make that branch unreachable, because a first pass early in
            // the join is always too young for it. So the skip now applies only while the task IS too
            // young for that branch — the `rejudge_cooldown_secs` check above still throttles the rest,
            // so this cannot spin re-judges at a sink-review node.
            if n.spec.owned_files.is_empty()
                && self.last_judged.contains_key(tid)
                && elapsed < cfg.min_age_secs.max(420)
            {
                continue;
            }
            let slot = if at_cap {
                &mut best_terminal
            } else {
                &mut best
            };
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
            attempt: self.dag.tasks.get(&tid).map(|n| n.attempts).unwrap_or(0),
        };
        self.judge_running = true;
        // Record the JUDGING node before the call, not after: the verdict emit reads
        // `task_final_device`, which is the judged worker. `None` here is meaningful — it is the
        // deterministic-only path where no device was claimed and no inference was spent.
        self.judge_node = claimed_device.map(|i| self.devices[i].cfg.model_id.clone());
        self.idle_jobs += 1;
        self.last_judged.insert(tid.clone(), Instant::now());
        // Claim the idle device's slot now that the judge is actually firing, so a worker dispatch (which
        // sorts by in_flight) + the next idle-job avoid this node. Released by the IdleSlotGuard.
        if let Some(i) = claimed_device {
            self.devices[i].in_flight += 1;
        }
        Some((req, attempt, claimed_device))
    }

    /// M5: pick a COMPLETED-but-unreviewed task (that owns files) for an idle-node correctness pre-review,
    /// claiming one idle-job slot (does NOT take the single-judge flag, so it runs concurrently with the
    /// judge on a different free node). Returns the request, or None if no idle device is free, nothing is
    /// reviewable, or all idle slots are taken. Marks the task pre_reviewed up front so it is picked at most
    /// once even while the review is in flight.
    fn pick_prereview_request(&mut self) -> Option<(PreReviewRequest, usize)> {
        let claimed_device = self.least_loaded_free_device()?;
        let reviewer_model_id = self.devices[claimed_device].cfg.model_id.clone();
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
        self.idle_jobs += 1;
        // Claim the idle device's slot so a worker dispatch + the next idle-job avoid this node. Released by
        // the IdleSlotGuard.
        self.devices[claimed_device].in_flight += 1;
        Some((
            PreReviewRequest {
                task_id: tid,
                description,
                owned_files,
                goal: self.goal.clone(),
                reviewer_model_id,
            },
            claimed_device,
        ))
    }

    /// SINK IDLE-FILL (GOOSE_SWARM_SINK_REVIEW): while the integrate-verify SINK runs SOLO and pre-review is
    /// exhausted, claim a genuinely-free device (never the sink's — it is at weight) for a READ-ONLY
    /// whole-tree dimension review, rotating the dimension. Returns (model_id, dim_index, goal, device).
    /// None unless the flag is on AND the sink is in flight AND a device is free (mirrors pick_prereview's
    /// claim so it never oversubscribes). Released by the IdleSlotGuard.
    fn pick_sink_review(&mut self) -> Option<(String, usize, String, usize)> {
        // ONE default, shared with the consumer. These two halves disagreed: this producer defaulted
        // OFF while run_swarm's drain and `levers_resolved` both defaulted ON — so every run REPORTED
        // sink_review enabled, the queue was never filled, `prewarmed` was always empty and the event
        // never fired. Measured as a real zero across three runs before the cause was found, and an
        // operator auditing levers would have read `sink_review: true` and believed it.
        //
        // This is the mechanism that exists to fill the biggest idle window there is: the SINK owns
        // 100% of the solo time in 2 of 3 measured runs (543-1045s with two nodes idle). It has never
        // run once.
        //
        // The default stays OFF — the truthful one, matching every measurement taken so far — so
        // baseline does not shift underneath the campaign. Turning it on is an ARM, not a silent flip.
        if !sink_review_enabled() || !self.sink_in_flight() {
            return None;
        }
        let claimed_device = self.least_loaded_free_device()?;
        let model_id = self.devices[claimed_device].cfg.model_id.clone();
        let dim = self.sink_review_dim;
        self.sink_review_dim = self.sink_review_dim.wrapping_add(1);
        self.idle_jobs += 1;
        self.devices[claimed_device].in_flight += 1;
        Some((model_id, dim, self.goal.clone(), claimed_device))
    }

    /// F779: claim an idle device for one READ-ONLY dimension review during the DAG TAIL. Unlike
    /// pick_sink_review this is NOT gated on sink_in_flight — it fires whenever `ready` is empty
    /// (no dispatchable build work waiting) AND a device is genuinely free AND the run has
    /// started dispatching (there is something built to review). This is the answer to the
    /// measured idle-tail waste: a long test task or e2e shard grinding on one node while the
    /// others sit idle now gets those idle nodes doing quality work. Read-only (the consumer runs
    /// the reviewer with no write tools), so N run concurrently over the tree with no race and no
    /// possible corruption. Mirrors pick_sink_review's claim discipline (idle_jobs + in_flight,
    /// released by the IdleSlotGuard) and its rotating dimension.
    fn pick_tail_review(&mut self) -> Option<(String, usize, String, usize)> {
        if !tail_review_enabled() || self.tail_review_count >= TAIL_REVIEW_CAP {
            return None;
        }
        // The tail: nothing dispatchable is waiting, and at least one task has been dispatched
        // (so there is produced code to review). During the active build `ready` is rarely empty;
        // when it is AND a node is free, that node would otherwise idle.
        if !self.ready.is_empty() || self.dispatched_per_device.is_empty() {
            return None;
        }
        let claimed_device = self.least_loaded_free_device()?;
        let model_id = self.devices[claimed_device].cfg.model_id.clone();
        let dim = self.tail_review_dim;
        self.tail_review_dim = self.tail_review_dim.wrapping_add(1);
        self.tail_review_count += 1;
        self.idle_jobs += 1;
        self.devices[claimed_device].in_flight += 1;
        Some((model_id, dim, self.goal.clone(), claimed_device))
    }

    /// F790-3: one-string run state for the Q&A answerer — the judge's perspective, cheaply.
    fn run_state_brief(&self) -> String {
        let mut done: Vec<&str> = Vec::new();
        let mut salvaged: Vec<&str> = Vec::new();
        let mut running: Vec<&str> = Vec::new();
        let mut pending = 0usize;
        let mut failed: Vec<&str> = Vec::new();
        for (id, n) in &self.dag.tasks {
            match n.state {
                TaskState::Done => done.push(id),
                TaskState::Salvaged => salvaged.push(id),
                TaskState::Claimed => running.push(id),
                TaskState::Failed => failed.push(id),
                _ => pending += 1,
            }
        }
        done.sort();
        salvaged.sort();
        running.sort();
        failed.sort();
        format!(
            "done ({}): {}\nsalvaged/full-ruler-pending ({}): {}\nrunning ({}): {}\npending: {}\nfailed ({}): {}",
            done.len(),
            done.join(", "),
            salvaged.len(),
            salvaged.join(", "),
            running.len(),
            running.join(", "),
            pending,
            failed.len(),
            if failed.is_empty() {
                "none".to_string()
            } else {
                failed.join(", ")
            },
        )
    }

    /// F790-3: claim a device for one operator answer. Mirrors the idle-fill claim discipline;
    /// the supervision-first preference in least_loaded_free_device means a borrowed node
    /// answers when one exists.
    fn pick_qa(&mut self) -> Option<(String, String, usize)> {
        let claimed_device = self.least_loaded_free_device()?;
        let model_id = self.devices[claimed_device].cfg.model_id.clone();
        let brief = self.run_state_brief();
        self.idle_jobs += 1;
        self.devices[claimed_device].in_flight += 1;
        Some((model_id, brief, claimed_device))
    }

    /// S7 (GOOSE_SWARM_TESTGEN): claim an idle device for one contract-derived test-generation
    /// job. Fires only when at least one task is DONE — before that there is no agreed contract
    /// worth testing against and the fan needs every slot. Mirrors pick_prereview's claim
    /// discipline exactly (idle_jobs + in_flight, released by the IdleSlotGuard); the seq is the
    /// landed filename's suffix, so replicates of a run produce the same names.
    fn pick_testgen(&mut self) -> Option<(String, u32, usize)> {
        if self.fix_round || !testgen_enabled() || self.testgen_count >= TESTGEN_CAP {
            return None;
        }
        if !self.dag.tasks.values().any(|n| n.state == TaskState::Done) {
            return None;
        }
        let claimed_device = self.least_loaded_free_device()?;
        let model_id = self.devices[claimed_device].cfg.model_id.clone();
        let seq = self.testgen_count;
        self.testgen_count += 1;
        self.idle_jobs += 1;
        self.devices[claimed_device].in_flight += 1;
        Some((model_id, seq, claimed_device))
    }

    /// SPECULATIVE EXECUTION: pick a TWIN to race on an idle device. Choose the longest-running Claimed task
    /// that is NOT already being speculated and whose PRIMARY is on a DIFFERENT device than the idle one (so
    /// the twin truly runs on a free node — 1 task per node). Builds the same DispatchRequest the primary got
    /// but `speculative: true`, and claims the twin's OWN device slot + spec_* maps WITHOUT touching
    /// held_files / claimed_device / the task's Claimed state (only the primary holds the real files).
    fn pick_speculation_target(&mut self) -> Option<(DispatchRequest, usize)> {
        // A twin is BUILD work (it can be promoted into the real tree) — never on a supervision
        // device. Inline build-only variant of least_loaded_free_device.
        let dev = self
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| d.cfg.enabled && !d.cfg.supervision && d.in_flight < d.cfg.weight)
            // Same tie rule as pick_device: at equal load the FASTEST host wins, never the
            // first index (which on the real fleet is the slowest host — the class the
            // operator directive fixed in ordinary placement; a twin exists to BEAT the
            // primary, so it wants the fast node even more).
            .min_by_key(|(i, d)| (d.in_flight, u32::MAX - d.cfg.speed_weight.max(1), *i))
            .map(|(i, _)| i)?;
        let mut best: Option<(TaskId, u64)> = None;
        for (tid, n) in &self.dag.tasks {
            if n.state != TaskState::Claimed || self.speculating.contains(tid) {
                continue;
            }
            // FAIL-CLOSE: never speculate a task that owns NO files (e.g. the injected integrate-verify
            // sink). A twin of such a task has nothing to promote, so a "win" would abort the primary and
            // commit a text-only merge while the twin's whole-tree edits stay stranded in its shadow —
            // dropping the integrator's files from the real tree. Only file-owning tasks are safe to
            // speculate. (This bounds the blast radius of the still-default-OFF speculation path; the full
            // promote/verify/join fix is tracked separately.)
            if n.spec.owned_files.is_empty() {
                continue;
            }
            if self.claimed_device.get(tid) == Some(&dev) {
                continue; // the twin must run on a DIFFERENT device than the primary
            }
            let elapsed = self
                .attempt_started_at
                .get(tid)
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            if best.as_ref().map(|(_, e)| elapsed > *e).unwrap_or(true) {
                best = Some((tid.clone(), elapsed));
            }
        }
        let (tid, _elapsed) = best?;
        let deps = self.dag.tasks[&tid].spec.deps.clone();
        let mut dependency_files: Vec<String> = deps
            .iter()
            .filter_map(|dep| self.dag.tasks.get(dep))
            .flat_map(|node| node.spec.owned_files.iter().cloned())
            .collect();
        dependency_files.sort();
        dependency_files.dedup();
        let replan_authority = self.dag.tasks[&tid].spec.replan_authority.clone();
        let neighborhood = self.neighborhood_of(&tid, &deps);
        let slice = self.ctx.slice_for(&deps);
        let (owned_files, description, attempt) = {
            let n = &self.dag.tasks[&tid];
            (
                n.spec.owned_files.clone(),
                n.spec.description.clone(),
                n.attempts,
            )
        };
        let device_id = self.devices[dev].cfg.id.clone();
        let model_id = self.devices[dev].cfg.model_id.clone();
        let mut all_files: Vec<String> = self
            .dag
            .tasks
            .values()
            .flat_map(|n| n.spec.owned_files.iter().cloned())
            .collect();
        all_files.sort();
        all_files.dedup();
        self.devices[dev].in_flight += 1;
        self.spec_device.insert(tid.clone(), dev);
        self.spec_started_at.insert(tid.clone(), Instant::now());
        self.speculating.insert(tid.clone());
        self.spec_count += 1;
        let req = DispatchRequest {
            task_id: tid,
            description,
            device_id,
            model_id,
            context_slice: slice,
            dependency_files,
            attempt,
            owned_files,
            all_files,
            prior_hint: None,
            subsplit: Vec::new(),
            speculative: true,
            user_decisions: self.user_decisions.clone(),
            doc_facts: self.doc_facts.clone(),
            neighborhood,
            replan_authority,
            activity_publisher: None,
        };
        Some((req, dev))
    }

    /// Resolve a SPECULATIVE twin's completion. Releases the twin's OWN device + clears its spec_* maps
    /// (idempotent with the primary-win abort path). Then FIRST-WINS: if the task is no longer Claimed the
    /// PRIMARY already won -> the twin lost, nothing more to do. Otherwise the twin WON: on Ok, abort the
    /// primary's future and route the twin's output through `complete()` (which releases the primary's device
    /// + file hold and does Done/merge/relax); on Err, leave the primary running.
    fn resolve_speculation(
        &mut self,
        tid: &str,
        attempt: u32,
        res: Result<TaskRunOutput, DispatchError>,
    ) {
        if let Some(dev) = self.spec_device.remove(tid) {
            if self.devices[dev].in_flight > 0 {
                self.devices[dev].in_flight -= 1;
            }
        }
        self.spec_started_at.remove(tid);
        self.spec_abort.remove(tid);
        self.speculating.remove(tid);
        // The twin only wins if the task is STILL Claimed AND on the SAME attempt. The attempt check is
        // essential when a judge is also on: the judge can re-dispatch this task (bumping n.attempts) while a
        // twin of the OLD attempt is still running — without it, the stale twin would abort the healthy new
        // primary and, because complete()'s attempt guard then rejects the stale call, leak its device.
        // (Mirrors complete()'s and apply_judge_outcome()'s attempt guards.)
        let still_live = self
            .dag
            .tasks
            .get(tid)
            .map(|n| n.state == TaskState::Claimed && n.attempts == attempt)
            .unwrap_or(false);
        if !still_live {
            // The twin lost. Recorded rather than dropped: "speculation ran and the primary won" and
            // "speculation never ran" are opposite facts about whether an idle node bought anything,
            // and until now a run could not distinguish them.
            self.sink.emit(&SwarmEvent::Speculated {
                task_id: tid.to_string(),
                attempt,
                winner: "primary".to_string(),
            });
            return; // primary already won, OR the judge re-dispatched (attempt advanced) — the twin lost
        }
        if res.is_ok() {
            if let Some(h) = self.abort_handles.get(tid) {
                h.abort();
            }
            self.sink.emit(&SwarmEvent::Speculated {
                task_id: tid.to_string(),
                attempt,
                winner: "twin".to_string(),
            });
            self.complete(tid, attempt, res);
        } else {
            // A twin that ERRORED bought nothing and cost a device; that is the case worth seeing.
            self.sink.emit(&SwarmEvent::Speculated {
                task_id: tid.to_string(),
                attempt,
                winner: "twin_failed".to_string(),
            });
        }
        // On a twin Err: the primary keeps running; the twin's own device was already released above.
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
        // Captured ONCE, up front, because every branch below removes `attempt_started_at` before it
        // emits. All five emits in this function used to hard-code `elapsed_ms: 0` while this very
        // number sat in scope — and `finish()` sums `per_device.busy_ms += a.elapsed_ms` over the whole
        // attempt history, so EVERY judge-terminated attempt contributed zero node-seconds to its
        // device, and a task whose LAST attempt was judge-accepted reported zero for itself. MEASURED:
        // a task that ran 80.2 minutes across five attempts is recorded as taking no time at all on
        // three of them. `busy_ms` is the engine's own answer to how busy each node was, which is the
        // question the whole node-scaling goal turns on, and a judge kill is the commonest restart
        // there is.
        let elapsed_ms = self
            .attempt_started_at
            .get(tid)
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let elapsed = self
            .attempt_started_at
            .get(tid)
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        // A deterministic accept can preserve completed bytes without paying for a cold restart, but it
        // is not a full repair-ruler verdict. It therefore releases dependents as Salvaged, never Done.
        let deterministic_accept =
            (still_live && outcome.verdict == Verdict::Accept && outcome.deterministic)
                .then(|| {
                    self.dag.tasks.get(tid).and_then(|node| {
                        provisional_task_receipt(
                            &node.spec,
                            attempt,
                            SalvageReason::DeterministicAccept,
                        )
                    })
                })
                .flatten();
        if let Some(receipt) = deterministic_accept {
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
            self.sink.emit(&SwarmEvent::JudgeVerdict {
                task_id: tid.to_string(),
                device: device.clone().unwrap_or_default(),
                judge_node: self.judge_node.clone().unwrap_or_default(),
                verdict: "accept".to_string(),
                confidence: outcome.confidence,
                hint: outcome.hint.clone(),
                action: "accepted".to_string(),
                deterministic: outcome.deterministic,
            });
            self.attempt_log
                .entry(tid.to_string())
                .or_default()
                .push(AttemptRecord {
                    device: device.clone(),
                    model: model.clone(),
                    outcome: "judge_accepted_provisional".to_string(),
                    error: None,
                    elapsed_ms,
                });
            let completion = TaskCompletionDisposition::Salvaged(receipt);
            self.task_completion
                .insert(tid.to_string(), completion.clone());
            self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Salvaged;
            self.relax_dependents(tid);
            let attempts = self.attempt_log[tid].len() as u32;
            let ended_because = self.last_attempt_error(tid);
            self.sink.emit(&SwarmEvent::TaskCompleted {
                task_id: tid.to_string(),
                salvaged: true,
                completion: Some(completion),
                status: "salvaged".to_string(),
                device,
                model,
                attempts,
                elapsed_ms,
                session_id: self.task_session_id(tid),
                error: ended_because,
                tool_calls: Vec::new(),
            });
            return true;
        }
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
        // TERMINAL-FAIL REQUIRES A DETERMINISTIC VERDICT. `actionable` gates on
        // `outcome.confidence >= cfg.intervene_confidence`, but the judge MODEL produces that confidence —
        // so without this term a model OPINION decides an irreversible failure. MEASURED: nf-ts-cadence's
        // integrate-verify went `over_reading -> re_dispatch, re_dispatch, FAILED` at confidence 0.90 from
        // the LLM path; under fan-verify integrate-verify depends on every verify::<M>, so that single model
        // opinion turned the whole run red. The standing rule is that only a DETERMINISTIC engine event may
        // create or kill a verdict. A model verdict keeps its full STEERING power (re_dispatch with a hint,
        // below) — it simply may no longer be the thing that fails a task; that task now runs to its own
        // deterministic backstop (worker_timeout / the spiral + repeat breakers) instead.
        let terminal = actionable
            && outcome.deterministic
            && cfg.max_interventions_per_task > 0
            && interv >= cfg.max_interventions_per_task
            && elapsed >= cfg.terminal_min_secs;
        let redispatch = actionable && interv < cfg.max_interventions_per_task;
        // FIX ROUNDS: the judge OBSERVES but never intervenes. A kill or split inside a fix
        // round drops the shard's future before grade_promotion_preview can land its verified
        // edit — the promotion dies with the worker — and judge splits are what kept rounds
        // alive past the completion cap (F890; measured: one round was 31.9 wall-minutes of
        // re-dispatch/split churn that landed nothing). Hints still flow as observations; the
        // round's own deterministic gates stay the only authority on shard outcomes.
        let (is_split, terminal, redispatch) = if self.fix_round {
            (false, false, false)
        } else {
            (is_split, terminal, redispatch)
        };
        // PROGRESS-GATED KILLS: a second re_dispatch is allowed only if the task's owned files
        // moved since the previous kill. A no-progress attempt that gets killed and restarted
        // repeats the same doomed generation (each restart measured 4-25 min of a node); with
        // the kill withheld, the attempt runs to its own deterministic backstop instead, and
        // the judge's hint is still stored via the observed path below. File-less tasks
        // (verify::, e2e shards) keep the old behavior — there is nothing to fingerprint.
        let mut kill_withheld = false;
        let redispatch = if redispatch {
            let files = self
                .dag
                .tasks
                .get(tid)
                .map(|n| n.spec.owned_files.clone())
                .unwrap_or_default();
            if files.is_empty() {
                true
            } else {
                // Caveat (review): a speculation twin writes its SHADOW, so real-tree
                // fingerprints cannot see its progress — the gate degrades such a task to
                // observe-only rather than ever corrupting state.
                let fp = owned_files_fingerprint(&files);
                if self.kill_tree_hash.get(tid) == Some(&fp) {
                    kill_withheld = true;
                    false
                } else {
                    self.kill_tree_hash.insert(tid.to_string(), fp);
                    true
                }
            }
        } else {
            false
        };
        // SPLIT is handled FIRST so the emitted event reflects the ACTUAL outcome: apply_split validates the
        // proposal and returns false (no-op, worker keeps running) if it is malformed — in that case the
        // event must report "observed", not a "split" that never happened.
        if is_split {
            let children = outcome.proposed_split.clone().unwrap_or_default();
            let applied = self.apply_split(tid, &children);
            self.sink.emit(&SwarmEvent::JudgeVerdict {
                task_id: tid.to_string(),
                device: device.clone().unwrap_or_default(),
                judge_node: self.judge_node.clone().unwrap_or_default(),
                verdict: outcome.verdict.as_str().to_string(),
                confidence: outcome.confidence,
                hint: outcome.hint.clone(),
                action: if applied { "split" } else { "observed" }.to_string(),
                deterministic: outcome.deterministic,
            });
            return applied;
        }
        let action = if terminal {
            "failed"
        } else if redispatch {
            "re_dispatch"
        } else if kill_withheld {
            // The progress gate withheld a kill — distinguishable from an ordinary observe so
            // the event stream can count how often the gate fires (instrument, don't note).
            "kill_withheld"
        } else {
            "observed"
        };
        self.sink.emit(&SwarmEvent::JudgeVerdict {
            task_id: tid.to_string(),
            device: device.clone().unwrap_or_default(),
            judge_node: self.judge_node.clone().unwrap_or_default(),
            verdict: outcome.verdict.as_str().to_string(),
            confidence: outcome.confidence,
            hint: outcome.hint.clone(),
            action: action.to_string(),
            deterministic: outcome.deterministic,
        });
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
            let salvage_receipt = (salvage_spin_enabled()
                && matches!(outcome.verdict, Verdict::Looping))
            .then(|| {
                self.dag.tasks.get(tid).and_then(|node| {
                    provisional_task_receipt(&node.spec, attempt, SalvageReason::FinalizeSpin)
                })
            })
            .flatten();
            let salvage = salvage_receipt.is_some();
            let (outcome_label, error_text, state, status) = if salvage {
                (
                    "salvaged_spin",
                    "finalize-spin preserved every assigned artifact; full ruler still required"
                        .to_string(),
                    TaskState::Salvaged,
                    "salvaged",
                )
            } else {
                (
                    "judge_failed",
                    outcome.verdict.as_str().to_string(),
                    TaskState::Failed,
                    "failed",
                )
            };
            if salvage {
                self.sink.emit(&SwarmEvent::JudgeVerdict {
                    task_id: tid.to_string(),
                    device: device.clone().unwrap_or_default(),
                    judge_node: self.judge_node.clone().unwrap_or_default(),
                    verdict: "salvaged_spin".to_string(),
                    confidence: 1.0,
                    hint: error_text.clone(),
                    action: "salvaged".to_string(),
                    // Engine bookkeeping on a terminal Looping, not a fresh judge call.
                    deterministic: true,
                });
            }
            self.attempt_log
                .entry(tid.to_string())
                .or_default()
                .push(AttemptRecord {
                    device: device.clone(),
                    model: model.clone(),
                    outcome: outcome_label.to_string(),
                    error: Some(error_text),
                    elapsed_ms,
                });
            self.dag.tasks.get_mut(tid).unwrap().state = state;
            if salvage {
                let completion = TaskCompletionDisposition::Salvaged(
                    salvage_receipt.expect("salvage receipt checked above"),
                );
                self.task_completion.insert(tid.to_string(), completion);
                self.relax_dependents(tid);
            } else {
                self.fail_descendants(tid);
            }
            let attempts = self.attempt_log[tid].len() as u32;
            let ended_because = self.last_attempt_error(tid);
            self.sink.emit(&SwarmEvent::TaskCompleted {
                task_id: tid.to_string(),
                salvaged: salvage,
                completion: self.task_completion.get(tid).cloned(),
                status: status.to_string(),
                device,
                model,
                attempts,
                elapsed_ms,
                session_id: self.task_session_id(tid),
                error: ended_because,
                tool_calls: Vec::new(),
            });
            return true;
        }
        if !redispatch {
            // A problem the judge SAW but could not act on is still the freshest and most specific
            // thing the run knows about this task. It used to be discarded: `prior_hints` is written
            // only on the re_dispatch path below, so once the intervention cap is spent every further
            // verdict is pure logging. MEASURED: a test task drew thirteen consecutive cap-exhausted
            // verdicts naming a literal syntax error (`from Non` for `from None`) and a wrong mock
            // target, all `observed`, and the attempt that replaced it — started by worker_timeout,
            // a timer — carried the stale hint from its last kill instead. Storing here kills, fails
            // and re-queues nothing; it only makes the NEXT dispatch of this task an informed one,
            // whatever ends the current attempt.
            if observed_hint_worth_keeping(still_live, outcome.verdict, &outcome.hint) {
                self.prior_hints.insert(tid.to_string(), outcome.hint);
            }
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
        self.judge_notes
            .push((tid.to_string(), outcome.hint.clone()));
        self.prior_hints.insert(tid.to_string(), outcome.hint);
        self.attempt_log
            .entry(tid.to_string())
            .or_default()
            .push(AttemptRecord {
                device,
                model,
                outcome: "judge_killed".to_string(),
                error: Some(outcome.verdict.as_str().to_string()),
                elapsed_ms,
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
        let (orig_files, orig_deps, orig_diff, orig_model, orig_desc) =
            match self.dag.tasks.get(tid) {
                Some(n) => (
                    n.spec
                        .owned_files
                        .iter()
                        .cloned()
                        .collect::<std::collections::BTreeSet<String>>(),
                    n.spec.deps.clone(),
                    n.spec.difficulty,
                    n.spec.preferred_model.clone(),
                    n.spec.description.clone(),
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
        // Reject a self-dep or any cycle among siblings BEFORE aborting the worker. Otherwise such a
        // proposal passes here but fails splice_specs' Kahn check AFTER the abort, hitting the destructive
        // Err arm — which would cascade-FAIL a healthy worker and break the documented no-op contract.
        if children
            .iter()
            .any(|c| c.depends_on.iter().any(|d| d == &c.id))
        {
            return false;
        }
        {
            // Kahn topological drain over the children's sibling-dep edges; a non-empty remainder = a cycle.
            let mut indeg: std::collections::HashMap<&str, usize> =
                children.iter().map(|c| (c.id.as_str(), 0usize)).collect();
            for c in children {
                for d in &c.depends_on {
                    *indeg.get_mut(c.id.as_str()).unwrap() += 1;
                    let _ = d;
                }
            }
            let mut queue: Vec<&str> = indeg
                .iter()
                .filter(|(_, &n)| n == 0)
                .map(|(&k, _)| k)
                .collect();
            let mut drained = 0usize;
            while let Some(node) = queue.pop() {
                drained += 1;
                for c in children {
                    if c.depends_on.iter().any(|d| d == node) {
                        let e = indeg.get_mut(c.id.as_str()).unwrap();
                        *e -= 1;
                        if *e == 0 {
                            queue.push(c.id.as_str());
                        }
                    }
                }
            }
            if drained != children.len() {
                return false; // cycle among siblings — leave the worker running (no-op)
            }
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
        let inherit_spec = split_inherit_spec_enabled();
        let child_id_list: Vec<TaskId> = children.iter().map(|c| c.id.clone()).collect();
        let specs: Vec<crate::dag::TaskSpec> = children
            .iter()
            .map(|c| {
                let mut deps = orig_deps.clone();
                deps.extend(c.depends_on.iter().cloned());
                crate::dag::TaskSpec {
                    id: c.id.clone(),
                    description: child_description(tid, &orig_desc, c, inherit_spec),
                    difficulty: orig_diff,
                    preferred_model: orig_model.clone(),
                    owned_files: c.files.clone(),
                    deps,
                    subsplit: Vec::new(),
                    replan_authority: None,
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
            // The split shell is superseded by its children — mark it reviewed so the idle pre-reviewer
            // (M5) never picks this phantom (Done + owns the union files) and reviews a partial file set.
            n.pre_reviewed = true;
        }
        // ---- enqueue the children that are immediately ready ----
        for id in newly_ready {
            let fan_out = self.dag.tasks[&id].fan_out;
            self.ready.push(Ranked { fan_out, id });
        }
        // A split is the mechanism by which spare nodes get more work to do, and until now it changed
        // the DAG silently. Three real runs could not be asked whether a split ever happened, because
        // the only trace was child task ids appearing in later dispatches — indistinguishable from a
        // plan that named them. Emitted at the success return ONLY, so the event means "a split was
        // applied", never "one was considered".
        self.sink.emit(&SwarmEvent::TaskSplit {
            task_id: tid.to_string(),
            children: children.iter().map(|c| c.id.clone()).collect(),
        });
        true
    }

    /// A failed task can never produce output, so its (transitive) dependents can never run —
    /// mark them Failed so the run terminates instead of deadlocking on blocked tasks.
    fn fail_descendants(&mut self, tid: &str) {
        // (parent, dependent) pairs so a relaxed verifier's hint names its DIRECT failed
        // dependency, not the BFS root (review: a→m→v used to tell v that 'a' failed).
        let mut q: VecDeque<(TaskId, TaskId)> = self
            .dag
            .dependents
            .get(tid)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|d| (tid.to_string(), d))
            .collect();
        while let Some((parent, d)) = q.pop_front() {
            let n = self.dag.tasks.get_mut(&d).unwrap();
            if n.state.is_terminal() {
                continue;
            }
            // RELAX-THROUGH-FAILURE for verification-shaped dependents (wall-time hunt,
            // verified): a dependent that OWNS NO FILES verifies or integrates — it writes
            // nothing, so running it against the tree that exists is strictly more informative
            // than cascading Failed. Measured: 2 of 3 sb-6 runs shipped apps that never bind a
            // port because the module failure killed the verify fan with it, so the boot defect
            // was discovered by the SCORER after three hours instead of by the run's own gate
            // during it. The failed dependency is threaded into prior_hints so the verifier
            // names what it is verifying around; write-owning dependents still fail exactly as
            // before.
            if n.spec.owned_files.is_empty() {
                let hint = format!(
                    "dependency '{parent}' FAILED — verify the tree that exists and report what \
                     its absence breaks"
                );
                let deps_relaxed = {
                    if n.indegree_remaining > 0 {
                        n.indegree_remaining -= 1;
                    }
                    n.indegree_remaining == 0 && n.state == TaskState::Pending
                };
                if deps_relaxed {
                    let nd = self.dag.tasks.get_mut(&d).unwrap();
                    nd.state = TaskState::Ready;
                    let fan_out = nd.fan_out;
                    self.ready.push(Ranked {
                        fan_out,
                        id: d.clone(),
                    });
                }
                match self.prior_hints.entry(d.clone()) {
                    std::collections::hash_map::Entry::Occupied(mut e) => {
                        let v = e.get_mut();
                        // Converging failed paths must not repeat the same sentence.
                        if !v.contains(&hint) {
                            v.push_str("; ");
                            v.push_str(&hint);
                        }
                    }
                    std::collections::hash_map::Entry::Vacant(e) => {
                        e.insert(hint);
                    }
                }
                continue;
            }
            n.state = TaskState::Failed;
            for dd in self.dag.dependents.get(&d).cloned().unwrap_or_default() {
                q.push_back((d.clone(), dd));
            }
        }
    }

    fn build_report(&self) -> RunReport {
        let mut done = Vec::new();
        let mut salvaged = Vec::new();
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
                TaskState::Salvaged => {
                    salvaged.push(id.clone());
                    "salvaged"
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
            let completion = match n.state {
                TaskState::Done => Some(
                    self.task_completion
                        .get(id)
                        .cloned()
                        .unwrap_or(TaskCompletionDisposition::Complete),
                ),
                TaskState::Salvaged => Some(
                    self.task_completion
                        .get(id)
                        .cloned()
                        .expect("salvaged task must retain its engine receipt"),
                ),
                _ => None,
            };

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
                salvaged: completion
                    .as_ref()
                    .is_some_and(TaskCompletionDisposition::is_salvaged),
                completion,
                device,
                model,
                attempts: history.len() as u32,
                attempt_history: history,
                elapsed_ms,
                session_id,
                tool_calls,
                output: n.result.clone(),
                owns_nothing: n.spec.owned_files.is_empty(),
            });
        }
        for (d, c) in &self.dispatched_per_device {
            per_device.entry(d.clone()).or_default().dispatched = *c;
        }
        done.sort();
        salvaged.sort();
        failed.sort();
        tasks.sort_by(|a, b| a.task_id.cmp(&b.task_id));
        let mut bonus: Vec<TaskId> = self.bonus_ids.iter().cloned().collect();
        bonus.sort();
        let mut planned_files: Vec<String> = {
            let mut set = std::collections::BTreeSet::new();
            for n in self.dag.tasks.values() {
                if n.state.releases_dependents() {
                    for f in &n.spec.owned_files {
                        set.insert(f.clone());
                    }
                }
            }
            set.into_iter().collect()
        };
        planned_files.sort();
        RunReport {
            done,
            salvaged,
            failed,
            bonus,
            results,
            context_json: self.ctx.to_json(),
            dispatched_per_device: self.dispatched_per_device.clone(),
            tasks,
            per_device,
            planned_files,
        }
    }
}

fn task_contract_version(spec: &TaskSpec) -> String {
    let canonical = serde_json::to_vec(spec).expect("task specs are JSON serializable");
    let digest = Sha256::digest(canonical);
    let mut version = String::with_capacity(digest.len() * 2 + 7);
    version.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(version, "{byte:02x}");
    }
    version
}

fn task_acceptance_oracle(spec: &TaskSpec) -> Vec<AcceptanceCriterionSnapshot> {
    if let Some(authority) = &spec.replan_authority {
        if !authority.requirements.is_empty() {
            return authority
                .requirements
                .iter()
                .map(|requirement| AcceptanceCriterionSnapshot {
                    id: requirement.id.clone(),
                    text: requirement.text.clone(),
                })
                .collect();
        }
        if !authority.acceptance_evidence.is_empty() {
            return authority
                .acceptance_evidence
                .iter()
                .enumerate()
                .map(|(index, evidence)| AcceptanceCriterionSnapshot {
                    id: format!("acceptance-evidence-{index}"),
                    text: evidence.clone(),
                })
                .collect();
        }
    }
    if spec.description.trim().is_empty() {
        Vec::new()
    } else {
        vec![AcceptanceCriterionSnapshot {
            id: "task-contract".to_string(),
            text: spec.description.clone(),
        }]
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
    speculation_enabled: bool,
    /// When set, the scheduler HOLDS at task boundaries while this file exists (the in-process pause).
    /// None (default) -> pause is inert and the loop is byte-identical to before.
    pause_file: Option<std::path::PathBuf>,
    /// GROUNDED research facts (Phase 1, Move 2), VERBATIM, handed to every DispatchRequest for injection into
    /// the worker prompt — the same channel as `user_decisions`. Empty (default) -> the worker prompt is
    /// byte-identical. Set via `with_doc_facts` so `run_with_decisions`' signature is unchanged.
    doc_facts: String,
    /// GOOSE_SWARM_DEGRADE_ON_STALL (#134/#132, default OFF): when a task exhausts its transient-retry budget
    /// (a mid-generation model hang) but its CRITICAL owned file is already on disk, mark it Done(degraded) +
    /// relax dependents instead of fail_descendants — so a single hung core task does not kill the capstone.
    /// integrate-verify then gates the degraded file honestly (build + R1 missing-deliverable). false =>
    /// the exhausted arm is byte-identical (fail_descendants).
    degrade_on_stall: bool,
    /// F883/E8: marks this scheduler as a repair-round run — testgen idle-fill is disabled there.
    fix_round: bool,
    /// Typed, immutable, observation-only semantic review. It is consulted only by the physical
    /// scheduler because the ordinary dispatcher cannot produce provider lifecycle receipts.
    semantic_observation: Option<SemanticObservationConfig>,
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
            speculation_enabled: false,
            pause_file: None,
            doc_facts: String::new(),
            degrade_on_stall: false,
            fix_round: false,
            semantic_observation: None,
        }
    }

    /// Attach the GROUNDED research facts (Phase 1, Move 2) that each worker gets VERBATIM. Empty (default)
    /// => the worker prompt is byte-identical, so callers that don't opt into DOC_PREFETCH are unchanged.
    pub fn with_doc_facts(mut self, doc_facts: String) -> Self {
        self.doc_facts = doc_facts;
        self
    }

    /// Attach an idle-model judge: when a node would otherwise sit idle while tasks are still in
    /// flight, it inspects a busy worker and may kill + re-dispatch one that is looping, over-reading,
    /// or producing broken code. OFF by default — with no judge attached the scheduler is unchanged.
    /// F779 i3: append SUPERVISION devices — machines the build pool excluded (the
    /// GOOSE_SWARM_MAX_NODES tail) that carry read-only idle work only. Forced supervision=true
    /// regardless of input; an entry whose model_id collides with an existing device (worker or
    /// pushed planner) is DROPPED, not bailed — the model is already reachable, and a capped run
    /// must not die because its borrowed node duplicates one it kept.
    pub fn with_supervision_devices(mut self, cfgs: Vec<DeviceCfg>) -> Self {
        for mut c in cfgs {
            if self.devices.iter().any(|d| d.model_id == c.model_id) {
                continue;
            }
            c.supervision = true;
            self.devices.push(c);
        }
        self
    }

    pub fn with_judge(mut self, judge: Arc<dyn Judge>, cfg: JudgeConfig) -> Self {
        self.judge = Some(judge);
        self.judge_cfg = cfg;
        self
    }

    /// Attach the Engine 5 observation-only semantic path. A measured trace snapshot can consume a
    /// genuinely idle verified provider route, but its result cannot mutate, stop, accept, split, or
    /// reschedule build work. This path is available only through `run_with_physical_admission`.
    pub fn with_semantic_observation(
        mut self,
        producer: Arc<dyn SemanticObservationSnapshotProducer>,
        reviewer: Arc<dyn AdmittedSemanticObservationReviewer>,
    ) -> Self {
        self.semantic_observation = Some(SemanticObservationConfig { producer, reviewer });
        self
    }

    /// Attach an idle-node PRE-REVIEWER (M5): when a node would otherwise idle and NO in-flight worker
    /// needs judging, it correctness-checks a COMPLETED-but-unreviewed task's output and records findings
    /// for integrate-verify. OFF by default — with none attached the scheduler is unchanged.
    pub fn with_pre_reviewer(mut self, pre_reviewer: Arc<dyn PreReviewer>) -> Self {
        self.pre_reviewer = Some(pre_reviewer);
        self
    }

    /// Enable SPECULATIVE EXECUTION (GOOSE_SWARM_SPECULATE): when a node would otherwise idle at a serial
    /// chokepoint (no ready task, no pre-review work) a TWIN of the longest-running in-flight task is raced
    /// on the idle device, first-to-finish wins. OFF by default — with it off no twin is ever spawned and
    /// the scheduler is byte-identical. The twin spawns ONLY on a genuinely idle device (1 task per node).
    /// F883/E8: a FIX-ROUND scheduler run must not fill idle slots with test GENERATION. The
    /// testgen path writes landed files to the REAL tree, and a fix round's whole discipline is
    /// that nothing reaches the real tree except a graded promote — a generated test landing
    /// mid-wave shifts every in-flight shard's baseline under it, and the per-round seq reset
    /// can overwrite the main run's landed generated tests. Pre-review idle-fill stays: read-only.
    pub fn for_fix_round(mut self) -> Self {
        self.fix_round = true;
        self
    }

    pub fn with_speculation(mut self) -> Self {
        self.speculation_enabled = true;
        self
    }

    /// Enable DEGRADE-ON-STALL (GOOSE_SWARM_DEGRADE_ON_STALL, #134/#132): at transient-retry exhaustion, if the
    /// stalled task already wrote its critical owned file, mark it Done(degraded) + relax dependents instead of
    /// failing the whole subtree. OFF by default — with it off the exhausted arm is byte-identical
    /// (fail_descendants). integrate-verify gates the degraded file honestly downstream.
    pub fn with_degrade_on_stall(mut self) -> Self {
        self.degrade_on_stall = true;
        self
    }

    /// Attach an event sink for structured observability (goose-cli writes JSONL through it).
    pub fn with_sink(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.sink = sink;
        self
    }

    /// Enable the in-process PAUSE hold: while `path` exists the scheduler holds at task boundaries
    /// (claims no new ready task; in-flight tasks finish). Deleting the file resumes with zero re-runs.
    /// Not called -> `pause_file` stays None -> pause is inert and the loop is byte-identical.
    pub fn with_pause_file(mut self, path: std::path::PathBuf) -> Self {
        self.pause_file = Some(path);
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
        self.run_with_decisions(dag, dispatcher, goal, String::new())
            .await
    }

    /// Opt-in physical execution. This is a separate entry point because an ordinary
    /// `TaskDispatcher` cannot supply provider request/terminal receipts. Legacy auxiliary paths
    /// are rejected rather than allowed to bypass the common queue; they can be reintroduced only
    /// as typed lifecycle-capable opportunities. The ordinary `run` path is unchanged.
    pub async fn run_with_physical_admission(
        &self,
        dag: Dag,
        dispatcher: Arc<dyn ProviderLifecycleDispatcher>,
        control: PhysicalAdmissionControl,
        execution: PhysicalExecutionAuthority,
        goal: String,
        user_decisions: String,
    ) -> Result<RunReport> {
        execution.validate().map_err(anyhow::Error::msg)?;
        if self.judge.is_some() {
            bail!(
                "physical admission cannot enable the legacy judge path: it bypasses provider lifecycle and can abort admitted work; Engine 5 must submit trace-bound observation-only judge work"
            );
        }
        if self.pre_reviewer.is_some() {
            bail!(
                "physical admission cannot enable the legacy pre-review/QA/tail/testgen paths: they bypass the common lifecycle queue"
            );
        }
        if self.replanner.is_some() {
            bail!(
                "physical admission cannot enable the legacy idle-capacity replanner: work must be task-derived and contract-bound before queueing"
            );
        }
        if self.speculation_enabled {
            bail!(
                "physical admission cannot enable speculative twins because first-wins requires killing an admitted request"
            );
        }
        if self.devices.iter().any(|device| device.supervision) {
            bail!(
                "physical admission has no permanent supervision lane; auxiliary evidence competes in the common queue"
            );
        }
        if dag
            .tasks
            .values()
            .any(|node| node.spec.replan_authority.is_some())
        {
            bail!(
                "physical admission requires runtime reviews to enter as contract-versioned auxiliary opportunities, not build dispatches"
            );
        }
        let physical_snapshot = control.snapshot().await;
        for device in self.devices.iter().filter(|device| device.enabled) {
            let lane = physical_snapshot
                .lanes
                .iter()
                .find(|lane| lane.logical_device_id == device.id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "physical admission has no verified lane for enabled device `{}`",
                        device.id
                    )
                })?;
            if lane.model_id != device.model_id {
                bail!(
                    "physical lane `{}` routes model `{}`, but the scheduler routes `{}`",
                    device.id,
                    lane.model_id,
                    device.model_id
                );
            }
        }
        if !control.uses_sink(&self.sink) {
            bail!("physical admission and scheduler must share one authoritative event sink");
        }
        self.sink.emit(&SwarmEvent::PhysicalFleetSnapshotObserved {
            snapshot: physical_snapshot,
            enforcement: "active".to_string(),
            provider_lifecycle_available: true,
        });
        let report = self
            .run_internal(
                dag,
                SchedulerDispatcher::Physical {
                    control: control.clone(),
                    dispatcher,
                    execution,
                },
                goal,
                user_decisions,
            )
            .await;
        // Local task completion cannot make a run terminal while a correlated provider terminal is
        // missing. There is intentionally no elapsed-time escape hatch here.
        control.wait_until_drained().await?;
        report
    }

    /// `run`, plus the user's verbatim clarify answers to hand to every worker. `run` delegates here with
    /// an empty string, so every existing caller and test is byte-identical.
    pub async fn run_with_decisions(
        &self,
        dag: Dag,
        dispatcher: Arc<dyn TaskDispatcher>,
        goal: String,
        user_decisions: String,
    ) -> Result<RunReport> {
        if self.semantic_observation.is_some() {
            bail!(
                "semantic observation requires physical admission and exact provider lifecycle receipts"
            );
        }
        self.run_internal(
            dag,
            SchedulerDispatcher::Legacy(dispatcher),
            goal,
            user_decisions,
        )
        .await
    }

    async fn run_internal(
        &self,
        dag: Dag,
        dispatcher: SchedulerDispatcher,
        goal: String,
        user_decisions: String,
    ) -> Result<RunReport> {
        if !self.devices.iter().any(|d| d.enabled && !d.supervision) {
            bail!("no enabled BUILD devices in the pool");
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

        let physical_execution = match &dispatcher {
            SchedulerDispatcher::Legacy(_) => None,
            SchedulerDispatcher::Physical { execution, .. } => Some(execution.clone()),
        };
        let physical_admission = physical_execution.is_some();
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
            physical_activity_publishers: HashMap::new(),
            dispatched_per_device: HashMap::new(),
            ctx: SharedContext::new(),
            max_attempts: self.max_attempts,
            degrade_on_stall: self.degrade_on_stall,
            sink: self.sink.clone(),
            attempt_started_at: HashMap::new(),
            attempt_log: HashMap::new(),
            task_session: HashMap::new(),
            task_tool_calls: HashMap::new(),
            task_final_device: HashMap::new(),
            goal,
            user_decisions,
            doc_facts: self.doc_facts.clone(),
            replans_done: 0,
            replan_declined_at_incomplete: None,
            bonus_ids: HashSet::new(),
            device_speed: HashMap::new(),
            abort_handles: HashMap::new(),
            kill_tree_hash: HashMap::new(),
            prior_hints: HashMap::new(),
            judge_notes: Vec::new(),
            interventions: HashMap::new(),
            omni_aborts: HashMap::new(),
            transport_drops: HashMap::new(),
            split_generation: HashMap::new(),
            judge_running: false,
            judge_node: None,
            task_completion: HashMap::new(),
            idle_jobs: 0,
            sink_review_dim: 0,
            last_judged: HashMap::new(),
            spec_device: HashMap::new(),
            spec_started_at: HashMap::new(),
            spec_abort: HashMap::new(),
            speculating: HashSet::new(),
            spec_count: 0,
            testgen_count: 0,
            fix_round: self.fix_round,
            tail_review_count: 0,
            tail_review_dim: 0,
            physical_admission,
            physical_execution,
        }));
        let notify = Arc::new(Notify::new());
        let mut semantic_runtime = None;
        let (dispatcher, physical_dispatcher): (
            Arc<dyn TaskDispatcher>,
            Option<Arc<BrokeredTaskDispatcher>>,
        ) = match dispatcher {
            SchedulerDispatcher::Legacy(dispatcher) => (dispatcher, None),
            SchedulerDispatcher::Physical {
                control,
                dispatcher,
                execution: _,
            } => {
                if let Some(config) = &self.semantic_observation {
                    semantic_runtime = Some(SchedulerSemanticObservationRuntime {
                        control: control.clone(),
                        plane: BrokeredSemanticObservationPlane::new(
                            control.clone(),
                            self.sink.clone(),
                        )?,
                        producer: config.producer.clone(),
                        reviewer: config.reviewer.clone(),
                        sink: self.sink.clone(),
                    });
                }
                let brokered = Arc::new(BrokeredTaskDispatcher::new(
                    control,
                    dispatcher,
                    Arc::new(SchedulerPhysicalAuthority {
                        state: state.clone(),
                        notify: notify.clone(),
                    }),
                ));
                (brokered.clone(), Some(brokered))
            }
        };
        let (semantic_completion_tx, mut semantic_completion_rx) =
            mpsc::unbounded_channel::<SemanticObservationAttemptResult>();
        let mut semantic_captures_in_flight = HashSet::new();
        let mut semantic_last_consumed: HashMap<TaskId, SemanticTraceRevision> = HashMap::new();
        let mut semantic_last_summoned: HashMap<TaskId, SemanticTraceRevision> = HashMap::new();
        // Edge-detect pause transitions so run_paused/run_unpaused is emitted once per transition, not per tick.
        let mut was_paused = false;

        loop {
            while let Ok(result) = semantic_completion_rx.try_recv() {
                semantic_captures_in_flight.remove(&result.task_id);
                if let Some(revision) = result.revision {
                    semantic_last_summoned.insert(result.task_id.clone(), revision.clone());
                    if matches!(
                        result.disposition,
                        SemanticObservationAttemptDisposition::Consumed
                    ) {
                        semantic_last_consumed.insert(result.task_id, revision);
                    }
                }
            }
            // In-process PAUSE hold: while the sentinel exists, claim NO new ready task. Already-spawned
            // in-flight futures (below) run to completion — the hold is BETWEEN tasks, so it can never
            // corrupt a half-written file. Cheap Path::exists per wake; inert when pause_file is None.
            let paused = self.pause_file.as_ref().is_some_and(|p| p.exists());
            if paused != was_paused {
                let s = state.lock().await;
                s.sink.emit(if paused {
                    &SwarmEvent::RunPaused
                } else {
                    &SwarmEvent::RunUnpaused
                });
                was_paused = paused;
            }
            let assignments = if paused {
                Vec::new()
            } else {
                state.lock().await.pick_assignments()
            };
            let dispatched_now = !assignments.is_empty();
            for a in assignments {
                if let Some(physical) = &physical_dispatcher {
                    // Preserve the scheduler's exact fan-out/id ranking at the broker boundary.
                    // Each opportunity is queued before its task future can be polled.
                    physical.prepare(&a.request).await;
                }
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
                // Register the abort handle when a judge OR speculation is on, so the loser can be killed.
                // Neither -> the map stays empty and the default path is byte-identical to before.
                if self.judge.is_some() || self.speculation_enabled {
                    state
                        .lock()
                        .await
                        .abort_handles
                        .insert(task_id, jh.abort_handle());
                }
            }

            {
                let s = state.lock().await;
                if s.all_terminal() && semantic_captures_in_flight.is_empty() {
                    return Ok(s.build_report());
                }
                if !paused
                    && !dispatched_now
                    && !s.all_terminal()
                    && s.build_in_flight() == 0
                    && !s.physical_dispatch_pending()
                {
                    // A completion can make work Ready after this loop pass picked assignments but
                    // before it acquires this lock. Retry immediately instead of misreporting that
                    // ready work as a deadlock. A genuinely blocked Ready set still falls through.
                    let became_assignable = s.ready.iter().any(|ranked| {
                        s.dag.tasks[&ranked.id].state == TaskState::Ready
                            && !s.files_conflict(&ranked.id)
                            && s.pick_device(&ranked.id).is_some()
                    });
                    if became_assignable {
                        continue;
                    }
                    // Nothing assignable and nothing running, but not all terminal: the remaining
                    // tasks are permanently blocked (deps failed, or a file deadlock).
                    // The `!paused` guard is LOAD-BEARING: while held we intentionally claim nothing and can
                    // drain to zero in-flight — without this guard that state trips the stuck-bail and turns
                    // Pause into an accidental terminate. Held + drained must idle until the sentinel clears.
                    let remaining = s
                        .dag
                        .tasks
                        .values()
                        .filter(|n| !n.state.is_terminal())
                        .count();
                    s.sink.emit(&SwarmEvent::SchedulerStuck { remaining });
                    bail!(
                        "scheduler stuck: {remaining} task(s) cannot proceed (blocked by failed deps or file holds)"
                    );
                }
            }
            if let Some(runtime) = semantic_runtime.as_ref().filter(|_| !paused) {
                let available_provider_slots = runtime.available_provider_slots().await;
                let requests = {
                    let s = state.lock().await;
                    if s.all_terminal() {
                        Vec::new()
                    } else {
                        s.semantic_observation_capture_requests(
                            &semantic_captures_in_flight,
                            available_provider_slots,
                        )
                    }
                };
                for scheduled in requests {
                    let task_id = scheduled.request.task_id.clone();
                    if !semantic_captures_in_flight.insert(task_id.clone()) {
                        continue;
                    }
                    let observation_runtime = runtime.clone();
                    let panic_runtime = runtime.clone();
                    let last_consumed = semantic_last_consumed.get(&task_id).cloned();
                    let last_summoned = semantic_last_summoned.get(&task_id).cloned();
                    let completion = semantic_completion_tx.clone();
                    let semantic_notify = notify.clone();
                    tokio::spawn(async move {
                        let attempt = scheduled.request.attempt;
                        let observed = tokio::spawn(async move {
                            observation_runtime
                                .observe(scheduled, last_consumed, last_summoned)
                                .await
                        })
                        .await;
                        let result = match observed {
                            Ok(result) => result,
                            Err(error) => {
                                panic_runtime.capture_failed(
                                    &task_id,
                                    attempt,
                                    format!("snapshot producer task ended unexpectedly: {error}"),
                                );
                                SemanticObservationAttemptResult {
                                    task_id,
                                    revision: None,
                                    disposition: SemanticObservationAttemptDisposition::NoCapture,
                                }
                            }
                        };
                        let consumed = matches!(
                            &result.disposition,
                            SemanticObservationAttemptDisposition::Consumed
                        );
                        let _ = completion.send(result);
                        if consumed {
                            semantic_notify.notify_one();
                        }
                    });
                }
            }
            // Dynamic replan: workers idle while a task is still in flight (e.g. a slow tail) — ask the
            // planner for more parallel work to fill them. Gated on in_flight > 0, so it is mutually
            // exclusive with the stuck-bail above (which needs in_flight == 0). The state lock is
            // released across the async planner call; completions fire meanwhile and splice_specs
            // re-validates against the now-current DAG.
            if !paused && self.replanner.is_some() {
                let ctx = {
                    let mut s = state.lock().await;
                    if !dispatched_now
                        && s.total_in_flight() > 0
                        && s.ready.is_empty()
                        && s.idle_capacity() >= 2
                        && s.replans_done < self.max_replans
                        && !s.sink_in_flight()
                        // Near the end, an injected task has nothing to overlap with and simply
                        // becomes the tail — see `replan_has_enough_dag_left`.
                        && replan_has_enough_dag_left(s.mandatory_incomplete(), s.mandatory_total())
                        // A previous EMPTY answer is cached against the DAG size that produced it.
                        // Re-ask only when strictly fewer tasks remain — the one change that can make
                        // the replanner answer differently — so the tail gets its ask without the
                        // planner being pestered at an unchanged state.
                        && s
                            .replan_declined_at_incomplete
                            .is_none_or(|prev| s.incomplete_count() < prev)
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
                        let dag = s.dag_specs_snapshot();
                        s.sink.emit(&SwarmEvent::Replanned {
                            round,
                            added: Vec::new(),
                            tasks: Vec::new(),
                            dag,
                            stopped: true,
                        });
                        // REFUND the round and remember the state instead of burning the budget. An
                        // empty answer costs one planner call and says nothing about a DAG that has
                        // since shrunk; consuming the whole budget for it is what left two nodes idle
                        // through an 18-minute single-task tail with the replanner switched off.
                        s.replans_done = s.replans_done.saturating_sub(1);
                        s.replan_declined_at_incomplete = Some(s.incomplete_count());
                    } else {
                        // Replanner-added tasks are OPPORTUNISTIC (idle-fill) — record them as bonus so a
                        // bonus failure cannot fail an otherwise-complete run (run success = core plan).
                        if let Err(error) = validate_runtime_replan_specs(&s.dag, &specs) {
                            let dag = s.dag_specs_snapshot();
                            s.sink.write_value(serde_json::json!({
                                "event": "dynamic_replan_rejected",
                                "round": round,
                                "reason": error.to_string(),
                                "candidate_count": specs.len(),
                            }));
                            s.sink.emit(&SwarmEvent::Replanned {
                                round,
                                added: Vec::new(),
                                tasks: Vec::new(),
                                dag,
                                stopped: true,
                            });
                            s.replans_done = self.max_replans;
                            continue;
                        }
                        let spliced_ids: Vec<TaskId> =
                            specs.iter().map(|sp| sp.id.clone()).collect();
                        let mut admitted_tasks = specs.clone();
                        admitted_tasks.sort_by(|left, right| left.id.cmp(&right.id));
                        match s.dag.splice_specs_before(specs, "integrate-verify") {
                            Ok(new_ready) => {
                                // `added` must be what was ADDED, not what happened to become READY.
                                // A spliced task whose deps are not yet satisfied is in the DAG and
                                // will run, but it is not in `new_ready` — so reporting new_ready
                                // under-counts, and can report ZERO for a successful replan.
                                //
                                // MEASURED: a live run emitted `Replanned { added: [], stopped: false }`
                                // while `test-api-edge-cases` and `test-store-integrity` were both
                                // spliced and later dispatched. `stopped: false` with an empty `added`
                                // is a contradiction — the empty case takes the other branch — and it
                                // made a plan-vs-execution review report two legitimately-added tasks
                                // as UNPLANNED DRIFT. An event that cannot be reconciled with the
                                // dispatch log turns a correct mechanism into a false alarm.
                                let added = spliced_ids.clone();
                                s.bonus_ids.extend(spliced_ids);
                                for id in new_ready {
                                    let fan_out = s.dag.tasks[&id].fan_out;
                                    s.ready.push(Ranked { fan_out, id });
                                }
                                let dag = s.dag_specs_snapshot();
                                s.sink.emit(&SwarmEvent::Replanned {
                                    round,
                                    added,
                                    tasks: admitted_tasks,
                                    dag,
                                    stopped: false,
                                });
                                drop(s);
                                continue;
                            }
                            Err(error) => {
                                let dag = s.dag_specs_snapshot();
                                s.sink.write_value(serde_json::json!({
                                    "event": "dynamic_replan_rejected",
                                    "round": round,
                                    "reason": error.to_string(),
                                    "candidate_count": admitted_tasks.len(),
                                }));
                                s.sink.emit(&SwarmEvent::Replanned {
                                    round,
                                    added: Vec::new(),
                                    tasks: Vec::new(),
                                    dag,
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
            if let Some(judge) = self.judge.as_ref().filter(|_| !paused) {
                let target = {
                    let mut s = state.lock().await;
                    // The judge is NOT capacity-bounded: it must fire even on a SATURATED fleet to kill a
                    // stuck worker and free a slot (that is unblocking, not idle-node work). It still counts
                    // toward idle_jobs so pre-review (below) knows one slot is taken.
                    if s.judge_running || s.build_in_flight() == 0 {
                        None
                    } else {
                        s.pick_judge_target(&self.judge_cfg)
                    }
                };
                if let Some((req, attempt, claimed_device)) = target {
                    let tid = req.task_id.clone();
                    let judge = judge.clone();
                    let st = state.clone();
                    let nt = notify.clone();
                    let cfg = self.judge_cfg;
                    tokio::spawn(async move {
                        // The IdleSlotGuard is the SOLE releaser of the idle_jobs slot AND the claimed device
                        // slot — decrement-ONCE on BOTH normal and panic exit. A counter must not be
                        // double-decremented the way the old idempotent bool harmlessly could (that
                        // undercounts and oversubscribes the fleet). We still clear judge_running on the hot
                        // path so the next tick can re-judge immediately; the guard also clears it as the
                        // panic backstop.
                        let _slot = IdleSlotGuard {
                            state: st.clone(),
                            is_judge: true,
                            claimed_device,
                            notify: Some(nt.clone()),
                        };
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
            // F790-3 Q&A (GOOSE_SWARM_QA, default ON) — DELIBERATELY AHEAD of pre-review: an
            // operator question is rare, one turn, and HUMAN-BLOCKING, while pre-review is
            // continuous background. MEASURED (F795): behind pre-review, a live question
            // starved 65+ minutes while reviews won nine freed slots in a row. An operator
            // question in the inbox is
            // answered on an idle node with the run's own state as context. One at a time (the
            // in-flight set inside the dispatcher dedups), read-only for the build, and the
            // has_pending_question check keeps the empty-inbox cost at one fs metadata call.
            if let Some(pr) = self.pre_reviewer.as_ref().filter(|_| !paused) {
                if qa_enabled() && pr.has_pending_question() {
                    let pick = {
                        let mut s = state.lock().await;
                        if !s.ready.is_empty()
                            && s.idle_capacity() <= 1
                            && !s.has_free_supervision_device()
                        {
                            None
                        } else {
                            s.pick_qa()
                        }
                    };
                    if let Some((model_id, brief, claimed_device)) = pick {
                        let pr = pr.clone();
                        let st = state.clone();
                        let nt = notify.clone();
                        let goal = { state.lock().await.goal.clone() };
                        tokio::spawn(async move {
                            let _slot = IdleSlotGuard {
                                state: st.clone(),
                                is_judge: false,
                                claimed_device: Some(claimed_device),
                                notify: Some(nt),
                            };
                            pr.answer_user_question(&model_id, &goal, &brief).await;
                        });
                    }
                }
            }
            // M5: put any STILL-idle node (beyond the one the judge took) on a correctness PRE-REVIEW of a
            // completed-but-unreviewed task (findings feed integrate-verify). Judge + pre-review now run
            // CONCURRENTLY, bounded by idle_capacity() so each free node gets one idle job and none is
            // oversubscribed; multiple pre-reviews can run at once (each on a distinct task, marked
            // pre_reviewed up front). Off unless a pre-reviewer is attached; None when all idle slots taken.
            if let Some(pr) = &self.pre_reviewer {
                let req = {
                    let mut s = state.lock().await;
                    // Idle-jobs now CLAIM a device (bump in_flight), so idle_capacity() already reflects them
                    // — fire a pre-review iff a device is genuinely free. (The old `idle_jobs >= idle_capacity`
                    // double-counted once claiming was added, blocking the concurrent pre-review.)
                    //
                    // A3: AND never the LAST free slot while ready work is waiting on capacity.
                    // pick_assignments ran first this pass, so a non-empty ready set here means tasks
                    // exist that could not be placed — a review claiming the final slot would make the
                    // fleet supervise instead of build at exactly the moment building is possible. With
                    // 2+ slots free (or nothing waiting) the review proceeds as before.
                    if !s.has_free_supervision_device()
                        && (s.idle_capacity() == 0
                            || (!s.ready.is_empty() && s.idle_capacity() <= 1))
                    {
                        None
                    } else {
                        s.pick_prereview_request()
                    }
                };
                if let Some((req, claimed_device)) = req {
                    let pr = pr.clone();
                    let st = state.clone();
                    let nt = notify.clone();
                    tokio::spawn(async move {
                        // The IdleSlotGuard is the SOLE releaser of this idle_jobs slot AND the claimed device
                        // slot — decrement-ONCE on both normal and panic exit (is_judge=false leaves
                        // judge_running untouched). Do NOT also decrement explicitly here: that double-counts
                        // the slot and oversubscribes.
                        let _slot = IdleSlotGuard {
                            state: st.clone(),
                            is_judge: false,
                            claimed_device: Some(claimed_device),
                            notify: Some(nt),
                        };
                        let tid = req.task_id.clone();
                        let dev = req.reviewer_model_id.clone();
                        let started = std::time::Instant::now();
                        let out = pr.pre_review(req).await;
                        // Emit so idle-node utilization is OBSERVABLE in the jsonl (it was previously invisible
                        // — a pre-review only left a file when it found ISSUES, so "ran + OK" looked like "never
                        // ran"). One quick sync emit under the lock, same as the judge's verdict emit.
                        st.lock().await.sink.emit(&SwarmEvent::PreReview {
                            task_id: tid,
                            device: dev,
                            had_findings: out.had_findings,
                            secs: started.elapsed().as_secs_f64(),
                        });
                    });
                }
            }
            // SINK IDLE-FILL (GOOSE_SWARM_SINK_REVIEW): when the integrate-verify SINK runs solo and
            // pre-review is exhausted, put an otherwise-idle node on a READ-ONLY whole-tree dimension review.
            // Findings accumulate in the dispatcher; run_swarm drains + re-verifies them after the sink. The
            // IdleSlotGuard releases the claimed device. Off by default (pick_sink_review returns None).
            if let Some(pr) = self.pre_reviewer.as_ref().filter(|_| !paused) {
                // Fill ALL currently-free nodes this tick (not one) — pick_sink_review claims a device each
                // call and returns None once none is free, so this saturates the idle nodes during the sink
                // instead of leaving them idle between the ~15s tick and a ~90s review finishing.
                loop {
                    let pick = {
                        let mut s = state.lock().await;
                        // A3: same last-slot yield as pre-review — inert during a normal sink window
                        // (ready is empty by construction) but load-bearing if a replan injects work
                        // mid-sink.
                        if !s.ready.is_empty()
                            && s.idle_capacity() <= 1
                            && !s.has_free_supervision_device()
                        {
                            None
                        } else {
                            s.pick_sink_review()
                        }
                    };
                    let Some((model_id, dim, goal, claimed_device)) = pick else {
                        break;
                    };
                    let pr = pr.clone();
                    let st = state.clone();
                    let nt = notify.clone();
                    tokio::spawn(async move {
                        let _slot = IdleSlotGuard {
                            state: st.clone(),
                            is_judge: false,
                            claimed_device: Some(claimed_device),
                            notify: Some(nt),
                        };
                        pr.idle_dimension_review(&model_id, &goal, dim).await;
                    });
                }
            }
            // F779 TAIL IDLE-FILL (GOOSE_SWARM_TAIL_REVIEW, default ON): the answer to the measured
            // idle-tail waste. Unlike sink-review this is NOT sink-gated — whenever `ready` is empty
            // and a node is free (a long test task or e2e shard grinding solo while the others idle),
            // the free nodes run READ-ONLY dimension review. Saturates ALL free nodes each tick, same
            // as sink-review. Read-only, so it never races the busy node's writes and cannot corrupt.
            if let Some(pr) = self.pre_reviewer.as_ref().filter(|_| !paused) {
                loop {
                    let pick = {
                        let mut s = state.lock().await;
                        // A3 last-slot yield: never take the final free slot while dispatchable work
                        // waits (inert on a real tail where `ready` is empty by construction).
                        if !s.ready.is_empty()
                            && s.idle_capacity() <= 1
                            && !s.has_free_supervision_device()
                        {
                            None
                        } else {
                            s.pick_tail_review()
                        }
                    };
                    let Some((model_id, dim, goal, claimed_device)) = pick else {
                        break;
                    };
                    let pr = pr.clone();
                    let st = state.clone();
                    let nt = notify.clone();
                    tokio::spawn(async move {
                        let _slot = IdleSlotGuard {
                            state: st.clone(),
                            is_judge: false,
                            claimed_device: Some(claimed_device),
                            notify: Some(nt),
                        };
                        pr.idle_dimension_review(&model_id, &goal, dim).await;
                    });
                }
            }
            // S7 TESTGEN (GOOSE_SWARM_TESTGEN): when a node is STILL idle after pre-review and
            // sink-review got first refusal, spend it generating contract-derived tests — the one
            // idle job with ZERO merge surface (new files, pytest-collected). Same last-slot yield
            // as its siblings; the IdleSlotGuard releases the claim. Default OFF -> pick_testgen
            // returns None and this block is byte-identical to absent.
            if let Some(pr) = self.pre_reviewer.as_ref().filter(|_| !paused) {
                let pick = {
                    let mut s = state.lock().await;
                    if !s.has_free_supervision_device()
                        && (s.idle_capacity() == 0
                            || (!s.ready.is_empty() && s.idle_capacity() <= 1))
                    {
                        None
                    } else {
                        s.pick_testgen()
                    }
                };
                if let Some((model_id, seq, claimed_device)) = pick {
                    let pr = pr.clone();
                    let st = state.clone();
                    let nt = notify.clone();
                    let goal = { state.lock().await.goal.clone() };
                    tokio::spawn(async move {
                        let _slot = IdleSlotGuard {
                            state: st,
                            is_judge: false,
                            claimed_device: Some(claimed_device),
                            notify: Some(nt),
                        };
                        pr.generate_tests(&model_id, &goal, seq).await;
                    });
                }
            }
            // SPECULATIVE EXECUTION: when speculation is ON and a node is STILL idle (runs AFTER pre-review,
            // so pre-review gets first refusal of the idle slot), race a TWIN of the longest-running in-flight
            // task on a free device — first-to-finish wins. Gated on spare capacity beyond the running idle
            // jobs (so it never oversubscribes) and no ready work. OFF by default -> the block is skipped and
            // pick_speculation_target / spec_* are never touched (byte-identical).
            if !paused && self.speculation_enabled {
                let target = {
                    let mut s = state.lock().await;
                    // Bounds: no ready work, spare capacity beyond the running idle jobs, and a global cap on
                    // total speculative spawns per run (so a long chokepoint can't burn unbounded compute).
                    if !s.ready.is_empty()
                        || s.idle_capacity() == 0
                        || s.spec_count >= SPECULATION_CAP
                    {
                        None
                    } else {
                        s.pick_speculation_target()
                    }
                };
                if let Some((req, _dev)) = target {
                    let dispatcher = dispatcher.clone();
                    let task_state = state.clone();
                    let notify = notify.clone();
                    let attempt = req.attempt;
                    let tid = req.task_id.clone();
                    let tid_spawn = tid.clone();
                    let jh = tokio::spawn(async move {
                        let res = dispatcher.run(req).await;
                        {
                            let mut s = task_state.lock().await;
                            s.resolve_speculation(&tid_spawn, attempt, res);
                        }
                        notify.notify_one();
                    });
                    state.lock().await.spec_abort.insert(tid, jh.abort_handle());
                }
            }
            // Wake on a completion, or — when a judge is attached — at least every 15s, so it can
            // inspect a worker that crosses a threshold BETWEEN completions (a lone stuck worker produces
            // no completion to wake on). A short tick means the behavioral over-read signal (many actions,
            // zero output) and the terminal-fail decision act within ~15s of tripping, not minutes.
            // tokio::Notify stores one permit, so a completion that fires before this await is not lost.
            // With no judge this is an effectively-infinite wait: byte-identical to before.
            let tick = if paused {
                // While held, re-poll the pause sentinel ~every 2s so Resume is detected promptly even when
                // there are no in-flight completions left to wake the loop.
                std::time::Duration::from_secs(2)
            } else if self.judge.is_some()
                || self.pre_reviewer.is_some()
                || self.speculation_enabled
                || semantic_runtime.is_some()
                // The REPLANNER is an idle-node mechanism too, and it was missing from this list. Its
                // trigger is "nodes idle while a task is still in flight", which by construction produces
                // NO completion to wake on — so without a tick the one window it exists for is never
                // re-examined, and the run waits out the tail with the check unevaluated. It only worked
                // at all because a judge happened to be attached and was lending it a heartbeat.
                || self.replanner.is_some()
            {
                std::time::Duration::from_secs(15)
            } else {
                std::time::Duration::from_secs(86_400)
            };
            let _ = tokio::time::timeout(tick, notify.notified()).await;
        }
    }
}

#[cfg(test)]
mod salvage_tests {
    use super::*;

    /// The split used to hand a child a ~40-char label as its ENTIRE task statement, discarding the
    /// implementation spec PLAN had just spent 40% of the run's wall-clock writing (loop-04: a 2038-char
    /// spec -> "(split of data-model-persistence) note-store", 43 chars). These pin both arms.
    #[test]
    fn tail_review_gate_defaults_on_and_respects_the_env() {
        // F779: read-only, IS the ratio lever -> default ON (unlike sink_review/testgen).
        // The env var is process-global; set/clear around the assertions.
        std::env::remove_var("GOOSE_SWARM_TAIL_REVIEW");
        assert!(tail_review_enabled(), "default is ON");
        std::env::set_var("GOOSE_SWARM_TAIL_REVIEW", "0");
        assert!(!tail_review_enabled(), "0 turns it off");
        std::env::set_var("GOOSE_SWARM_TAIL_REVIEW", "off");
        assert!(!tail_review_enabled());
        std::env::set_var("GOOSE_SWARM_TAIL_REVIEW", "1");
        assert!(tail_review_enabled());
        std::env::remove_var("GOOSE_SWARM_TAIL_REVIEW");
    }

    #[test]
    fn split_child_description_off_is_byte_identical() {
        let child = crate::judge::ChildSpec {
            id: "note-store".into(),
            files: vec!["Sources/NotesLibrary/NoteStore.swift".into()],
            depends_on: vec![],
        };
        // OFF -> exactly today's string, unchanged.
        assert_eq!(
            child_description(
                "data-model-persistence",
                "a 2038-char spec...",
                &child,
                false
            ),
            "(split of data-model-persistence) note-store"
        );
        // ON but the parent had no spec -> nothing to inherit, fall back to the label.
        assert_eq!(
            child_description("data-model-persistence", "   ", &child, true),
            "(split of data-model-persistence) note-store"
        );
    }

    #[test]
    fn split_child_description_on_scopes_files_then_carries_the_spec() {
        let child = crate::judge::ChildSpec {
            id: "note-store".into(),
            files: vec![
                "Sources/NotesLibrary/NoteStore.swift".into(),
                "Sources/NotesLibrary/Note.swift".into(),
            ],
            depends_on: vec![],
        };
        let parent_spec =
            "**Package.swift**: three targets. **NoteStore.swift**: @Observable class.";
        let d = child_description("data-model-persistence", parent_spec, &child, true);

        // The parent's real spec survives — that is the whole point.
        assert!(
            d.contains(parent_spec),
            "the child must receive the parent's spec"
        );
        // Every owned file is named explicitly.
        assert!(d.contains("- Sources/NotesLibrary/NoteStore.swift"));
        assert!(d.contains("- Sources/NotesLibrary/Note.swift"));
        // The scope guard comes BEFORE the spec, so the child reads its limits before reading about files
        // it must not touch (the risk this lever introduces).
        let scope_at = d
            .find("YOU OWN ONLY THESE FILES")
            .expect("scope header present");
        let spec_at = d.find(parent_spec).expect("spec present");
        assert!(
            scope_at < spec_at,
            "the file-scope header must precede the parent spec"
        );
        assert!(d.contains("belong to OTHER workers"));
        // And it is vastly more than the 43-char label it replaces.
        assert!(
            d.len() > 200,
            "expected a real task statement, got {} chars",
            d.len()
        );
    }

    #[test]
    fn salvage_off_values_parse() {
        // Parse mirror of salvage_spin_enabled: unset -> ON; explicit off-values -> OFF.
        let off = |v: &str| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            )
        };
        assert!(off("0") && off("off") && off("FALSE") && off(" no "));
        assert!(!off("1") && !off("true") && !off("anything"));
    }

    fn provisional_fixture(name: &str, bytes: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::Builder::new()
            .prefix("provisional-")
            .tempdir_in(".")
            .unwrap();
        let file = dir.path().join(name);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, bytes).unwrap();
        let root = std::path::Path::new(".").canonicalize().unwrap();
        let relative = file
            .canonicalize()
            .unwrap()
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        (dir, relative)
    }

    /// A dropped body and a stall are the ONLY infra transients that earn a hint, and the distinction
    /// is the whole point: "model unloaded" means nothing happened, so a note would mislead; a
    /// mid-stream drop or a stall means the worker RAN — anything it wrote before dying is on disk
    /// (possibly nothing, for a thinking-only stall — the hint's read instruction is conditional).
    #[test]
    fn only_a_dropped_body_earns_a_retry_hint() {
        let h =
            transient_retry_hint("stream decode error (mid-stream body drop) on integrate-verify")
                .expect("a dropped body must carry a hint");
        assert!(
            h.contains("STILL ON DISK") && h.contains("Do NOT start over"),
            "the hint must tell the fresh conversation that prior work survived: {h}"
        );
        // Every other infra transient stays silent.
        for quiet in [
            "model is not loaded",
            "connection reset",
            "529 Overloaded",
            "invalid model identifier",
            "",
        ] {
            assert!(
                transient_retry_hint(quiet).is_none(),
                "{quiet:?} must not carry a hint — nothing was produced to preserve"
            );
        }
    }

    /// The stall class retries WARM: the hint must carry the specific pathology that killed the
    /// previous attempt (so the retry can avoid it), the on-disk fact, and the act-first directive.
    /// All four kill-site variants share the "no productive progress" marker; that marker — not any
    /// variant's wording — is the contract this test pins.
    #[test]
    fn a_stall_earns_a_warm_retry_hint_carrying_its_pathology() {
        for stall in [
            "agent stalled — no productive progress: reasoning spiral, 61000 thinking chars",
            "agent stalled — no productive progress: repeated the identical tool call 14x",
            "agent stalled — no productive progress (tool/output/text) for 900s while streaming reasoning only",
            "agent stalled — no productive progress: the judge read this call's own reasoning",
            // The idle watchdog's variant — the DOMINANT one in the measured 420s loops, and the
            // one the first predicate missed (r7 kill-on-divergence catch).
            "agent stalled — no progress for 420s (no token/tool activity)",
        ] {
            let h = transient_retry_hint(stall).expect("a stall must carry a warm hint");
            assert!(
                h.contains(stall),
                "the hint must thread the specific stall pathology verbatim: {h}"
            );
            assert!(
                h.contains("STILL ON DISK") && h.contains("ACT FIRST"),
                "the hint must carry the on-disk fact and the act-first directive: {h}"
            );
        }
    }

    #[test]
    fn stall_salvage_is_engine_minted_and_strictly_gated() {
        let (_dir, main) = provisional_fixture("src/main.go", "package main\nfunc main(){}\n");
        let spec = TaskSpec {
            id: "cli-entry".into(),
            description: "implement the CLI entry point".into(),
            difficulty: Difficulty::Easy,
            preferred_model: None,
            owned_files: vec![main.clone()],
            deps: Vec::new(),
            subsplit: Vec::new(),
            replan_authority: None,
        };

        assert!(stall_salvage_receipt(true, false, &spec, 2).is_some());
        assert!(stall_salvage_receipt(false, false, &spec, 2).is_none());
        assert!(stall_salvage_receipt(true, true, &spec, 2).is_none());

        let mut missing = spec.clone();
        missing.owned_files.push("missing.go".into());
        assert!(stall_salvage_receipt(true, false, &missing, 2).is_none());

        let mut owns_nothing = spec.clone();
        owns_nothing.owned_files.clear();
        assert!(stall_salvage_receipt(true, false, &owns_nothing, 2).is_none());

        let (_tests, test_file) =
            provisional_fixture("tests/test_cli.py", "def test_cli(): pass\n");
        let test_spec = TaskSpec {
            id: "integration-test".into(),
            description: "verify the CLI".into(),
            difficulty: Difficulty::Easy,
            preferred_model: None,
            owned_files: vec![test_file],
            deps: Vec::new(),
            subsplit: Vec::new(),
            replan_authority: None,
        };
        assert!(stall_salvage_receipt(true, false, &test_spec, 2).is_none());

        let receipt = stall_salvage_receipt(true, false, &spec, 2).unwrap();
        assert_eq!(receipt.task_id(), "cli-entry");
        assert_eq!(receipt.reason(), SalvageReason::StallExhausted);
        assert_eq!(receipt.artifacts().len(), 1);
        assert!(receipt.artifacts().contains_key(&main));
    }

    /// THE SINK IS THE TASK THIS EXISTS FOR, AND IT WAS THE ONE TASK EXCLUDED.
    ///
    /// `integrate-verify` owns no files, so `critical_owned_files_written` fell through to `any()` over
    /// an empty slice — false — and the join could never degrade. Measured consequence: a transient
    /// `stream decode error (mid-stream body drop)` re-dispatched the entire join to another node and
    /// restarted it from zero, twice, costing 15.3 min on one cell and 44.3 min (29.5% of its wall) on
    /// another. Killing the longest, most fleet-blocking task in the run because a socket hiccuped
    /// discards every command already run and every fix already written.
    /// A supervisor that has run out of levers has not run out of information.
    ///
    /// The cap-exhausted verdicts that motivated this were `spec_drift` and `broken_code` — the two
    /// carrying the actual diagnosis — interleaved with `ok` verdicts whose hint is empty. Keeping the
    /// empty one would OVERWRITE the useful hint with nothing, which is worse than never keeping any,
    /// so the empty-hint case is asserted explicitly rather than left to the `is_problem()` filter.
    /// A twice-killed task is tail risk and must compete for the fastest node like a hard one.
    ///
    /// Measured live: the run's last task, killed `over_reading` twice, took its third dispatch on
    /// the slowest host and ran there 29 minutes while both faster nodes reported READY. Nothing was
    /// broken — the ranking simply had no reason to prefer a twice-killed task over a fresh one
    /// competing for the same slot in the same instant.
    #[test]
    fn a_repeatedly_retried_task_competes_for_the_fastest_node() {
        // A fresh EASY task must NOT claim the fast-node preference — that would hand it to
        // everything and the preference would stop meaning anything.
        assert!(!dispatch_prefers_fastest_node(false, 0));
        // One retry is common and cheap; the bar is deliberately above it.
        assert!(!dispatch_prefers_fastest_node(false, 1));
        // The third dispatch — exactly the measured case.
        assert!(dispatch_prefers_fastest_node(false, 2));
        assert!(dispatch_prefers_fastest_node(false, 5));
        // HARD keeps the preference it always had, on its first dispatch and every later one.
        assert!(dispatch_prefers_fastest_node(true, 0));
        assert!(dispatch_prefers_fastest_node(true, 3));
    }

    #[test]
    fn a_hard_task_prefers_an_idle_node_over_a_busier_faster_one() {
        // Both directions of A2. weight_rank inverts speed_weight (lower rank = faster host).
        let fast_busy = hard_device_key(1, u32::MAX - 3, 0, 0, 0);
        let slow_idle = hard_device_key(0, u32::MAX - 1, 0, 0, 1);
        assert!(
            slow_idle < fast_busy,
            "an idle device must win over a busier higher-weighted one — stacking two hard \
             generations on one Apple host degrades both (F623)"
        );
        // Among equally-loaded devices the operator's weight stays decisive — the intent the old
        // absolute ordering was defending, preserved one tier down.
        let fast_idle = hard_device_key(0, u32::MAX - 3, 0, 0, 0);
        let slow_idle = hard_device_key(0, u32::MAX - 1, 0, 0, 1);
        assert!(fast_idle < slow_idle, "equal load: higher weight wins");
        // And a timing sample still cannot outrank a configured weight at equal load.
        let fast_idle_slow_sample = hard_device_key(0, u32::MAX - 3, 9_999, 0, 0);
        assert!(fast_idle_slow_sample < slow_idle);
    }

    #[test]
    fn a_verdict_the_judge_could_not_act_on_still_keeps_its_diagnosis() {
        let real = "SyntaxError: `from Non` should be `from None`";
        for v in [
            Verdict::BrokenCode,
            Verdict::SpecDrift,
            Verdict::OverReading,
        ] {
            assert!(
                observed_hint_worth_keeping(true, v, real),
                "{v:?} named a real defect the judge could not act on — the next attempt must hear it"
            );
        }
        // An `ok` verdict must never clobber a kept diagnosis, whether or not it carries text.
        assert!(!observed_hint_worth_keeping(true, Verdict::Ok, ""));
        assert!(!observed_hint_worth_keeping(
            true,
            Verdict::Ok,
            "looks fine"
        ));
        assert!(!observed_hint_worth_keeping(true, Verdict::Accept, "done"));
        // Nor may a problem verdict with nothing to say replace one that had something.
        assert!(!observed_hint_worth_keeping(
            true,
            Verdict::BrokenCode,
            "   "
        ));
        // A verdict about an attempt that has already ended is not about the worker being replaced.
        assert!(!observed_hint_worth_keeping(
            false,
            Verdict::BrokenCode,
            real
        ));
    }

    #[test]
    fn replan_does_not_invent_work_for_a_dag_that_is_already_finishing() {
        // The two measured harmful injections must now be refused: n3-r2 at 3-of-21 (18.3 min of
        // bonus tail) and n3-r3 at 2-of-18 (26.8 min, on a run whose mandatory work was ALREADY done).
        assert!(
            !replan_has_enough_dag_left(3, 21),
            "n3-r2's injection must be refused"
        );
        assert!(
            !replan_has_enough_dag_left(2, 18),
            "n3-r3's injection must be refused"
        );

        // MID-RUN INJECTION IS THE FEATURE AND MUST SURVIVE. The first version of this gate used an
        // absolute count and silently disabled dynamic-replan for every small DAG — 1-of-2 remaining
        // is mid-run, not a tail, and two pre-existing tests caught it.
        assert!(
            replan_has_enough_dag_left(1, 2),
            "a 2-task DAG with one left is MID-RUN"
        );
        assert!(replan_has_enough_dag_left(2, 4));
        assert!(replan_has_enough_dag_left(9, 20));

        // Degenerate input must not divide the world by zero or refuse forever.
        assert!(replan_has_enough_dag_left(0, 0));

        // Monotone in the work remaining: more left can never be a weaker reason to replan.
        for n in 0..25usize {
            if replan_has_enough_dag_left(n, 24) {
                assert!(
                    replan_has_enough_dag_left(n + 1, 24),
                    "{n} armed the replan but {} did not — the gate is not monotone",
                    n + 1
                );
            }
        }
    }
}
