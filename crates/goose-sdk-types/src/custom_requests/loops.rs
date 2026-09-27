//! Session loops (design: local-edition/mlx/quality/DESIGN-SESSION-LOOPS.md §4, §9 L0).
//!
//! A session loop belongs to one chat: the user's goal, a cadence, a state file and stop rules,
//! and a tick ledger. These types ARE the shape of the record goosed keeps in the session's
//! `extension_data["loop.v0"]` and of every `loops/*` ACP method and notification; the rules over
//! them live in `goose::session_loops`, and the desktop's `components/loops/model.ts` re-exports
//! the generated mirror. The whole contract is closed here (L0): later slices (the runner, the
//! report tool, the renderer door, the rail) fill bodies behind it and never reopen this file.
//!
//! Times are RFC 3339 in UTC with whole seconds (`2026-09-27T22:01:00Z`).

use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::NodeServedTurnDto;

/// The starting template the user picked in the Start dialog (§7.4).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoopTemplateId {
    /// Software quality loop: discover → critique → fix → prove.
    Quality,
    /// Until a check passes.
    UntilCheck,
    /// Watch and act.
    Watch,
    /// The goal alone.
    #[default]
    Blank,
}

/// When the next tick STARTS (§4.3). A cadence never cuts a tick or a check.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LoopCadence {
    /// Fixed: the time between tick starts, in the one cadence grammar (`<n>s|m|h`).
    Every { every: String },
    /// "When goose decides": each tick's report names its next delay and why.
    #[default]
    SelfPaced,
    /// "Right after each tick": as soon as the tick just ended is recorded, checked and reviewed.
    BackToBack,
}

/// The loop's status (§4.7). `elsewhere` is never written: it is derived at read time for a loop
/// whose owner is another live goose process.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    /// A tick's turn is in flight.
    Running,
    /// The check command runs after a tick.
    Checking,
    /// The next tick is scheduled.
    #[default]
    Waiting,
    /// A tick is due but must wait (a user turn, this chat's own turn or queued message, the
    /// previous tick's reviewers, a way a user reply holds).
    WaitingTurn,
    /// A self-paced tick named no delay goose can read.
    WaitingYou,
    /// A tick asked the user and the question is open, or its answer's turn still runs.
    NeedsYou,
    /// By the user, by a self-pause rule, or because the owning goose was closed.
    Paused,
    /// A stop condition held.
    Ended,
    /// Another live goose window runs this loop's clock.
    Elsewhere,
}

/// What a refused tick offer said (§5.1): the renderer could not submit the tick now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LoopRefuseReason {
    /// This chat already has a turn in flight.
    TurnRunning,
    /// The composer's queue for this chat is not empty.
    QueuedMessage,
    /// A stop of this chat's previous answer is still settling.
    PendingCancel,
    /// The session could not be loaded into the window.
    LoadFailed { error: String },
    /// goose refused the tick's message.
    SubmitFailed { error: String },
}

/// Why the loop has the status it has; every variant is a sentence the user reads (§4.6, §8.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LoopStatusReason {
    // paused
    /// Pause, from the rail or `/loop pause`.
    ByYou { after_tick: u32 },
    /// The user stopped the running tick (composer Stop, Escape, an interruption word).
    YouStoppedTick { n: u32 },
    /// The tick reported `blocked`.
    Blocked { n: u32, blocked_on: String },
    /// The check could not run (spawn error, a missing shell, stopped by the user). Never read as
    /// a failed check.
    CheckCouldNotRun { n: u32, error: String },
    /// Ticks `prev` and `n` failed with the same error class.
    SameFailureTwice { prev: u32, n: u32, error: String },
    /// Ticks `prev` and `n` both ended without a loop report.
    NoReportTwice { prev: u32, n: u32 },
    /// Tick `n` reported progress, made no write or edit outside the state file, and named the
    /// same next step as tick `prev`.
    Stalled { prev: u32, n: u32 },
    /// The goose that ran the clock is gone. `closedAt` is set when that goose released the loop
    /// itself (its last window closed); absent when it was proven gone afterwards (its time is not
    /// known, and is never guessed).
    Closed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        closed_at: Option<String>,
    },
    /// Paused from this window while tick `n` still finishes in the window that runs the loop.
    FinishingElsewhere { n: u32 },
    // ended
    /// The check passed after a tick whose verdict was done or progress.
    GoalMet { n: u32, check: String },
    /// No check is set and the tick reported done (the model's own claim, labelled as such).
    ReportedDone { n: u32 },
    /// The user's own tick count was reached.
    ReachedCount { k: u32 },
    /// Stop loop, from the rail or `/loop stop`.
    StoppedByYou { n: u32 },
    // waiting_turn
    /// A user turn runs in this goose.
    UserTurn { session_id: String, chat: String },
    /// The renderer refused the offer.
    Refused { refused: LoopRefuseReason },
    /// Tick `n`'s end-of-turn reviewers still run on the same model.
    Reviewers { n: u32 },
    /// A user reply holds the way this tick would swap (the Mac-wide half).
    WayHeld {
        node: String,
        chat: String,
        target: String,
    },
    // waiting_you
    /// Tick `n` named no delay.
    NoDelay { n: u32 },
    /// Tick `n` named a delay outside the cadence grammar.
    BadDelay { n: u32, given: String },
    // needs_you
    /// Tick `n` asked the user and the question is open.
    Asked {
        n: u32,
        item_id: String,
        question: String,
    },
    /// The user answered tick `n`'s question and that answer's turn runs.
    AnswerRunning { n: u32 },
}

/// Why the next tick is at the time it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LoopNextReason {
    /// The loop's first tick.
    First,
    /// The previous start plus the cadence.
    Cadence,
    /// The previous tick overran the cadence: as soon as it is recorded.
    Overdue,
    /// The delay the previous tick named, and its reason verbatim (absent when it gave none).
    SelfPaced {
        interval: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    BackToBack,
    AfterYourTurn,
    AfterYourAnswer,
    OnWake,
    Resume,
    /// "Run a tick now".
    Now,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopNextTick {
    pub at: String,
    pub reason: LoopNextReason,
}

/// What started a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoopTickOrigin {
    First,
    Cadence,
    Now,
    SelfPaced,
    BackToBack,
    AfterYourTurn,
    AfterYourAnswer,
    OnWake,
    Resume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoopVerdict {
    Progress,
    Done,
    Blocked,
}

/// What the tick's `loop_report` call said, verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopReport {
    pub verdict: LoopVerdict,
    /// What this tick did, with its evidence (command output, file:line).
    pub summary: String,
    /// The one concrete next step.
    pub next_step: String,
    /// Self-paced only: `<n>s|m|h`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_in: Option<String>,
    /// Why that delay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_reason: Option<String>,
    /// Verdict `blocked`: what only the user can decide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_on: Option<String>,
}

/// How a tick ended (§4.2). Absent on the tick in flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LoopTickOutcome {
    Progress,
    Done,
    Blocked,
    /// The tick asked the user (`ask_user` ends the turn), decided by the runner from the
    /// session's needs-you store, never by the model.
    Asked {
        item_id: String,
        question: String,
    },
    /// The turn errored.
    Failed { error_class: String, error: String },
    /// The turn ended without calling `loop_report`.
    NoReport,
    /// The tick was cancelled for a user reply. `way` is set once the Mac-wide half knows it.
    Yielded {
        to_session: String,
        to_chat: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        way: Option<String>,
    },
    /// The user stopped the tick or the loop.
    StoppedByYou,
}

/// One run of the check command after a tick. `ran: false` with `error` = it could not run
/// (never a failed check).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopCheckRun {
    pub command: String,
    pub started_at: String,
    /// Absent while the check runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub ran: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    /// The longest suffix of the output within 1/64 of the session's context window (§4.4).
    #[serde(default)]
    pub output_tail: String,
    /// The full output, in goose's data dir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Session token totals after − before the tick: a measurement, shown, never a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopTokenDelta {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopTickRecord {
    /// 1 = the loop's first tick.
    pub n: u32,
    pub origin: LoopTickOrigin,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    /// The tick's prompt message (`looptick_<loopId>_<n>_<uuid>`); the next tick's marker ends
    /// its range.
    pub first_message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<LoopReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<LoopTickOutcome>,
    /// Paths goose wrote or edited in the tick (`write`/`edit` diffs), the state file excluded.
    /// A shell command that changed files is not listed.
    #[serde(default)]
    pub wrote: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<LoopCheckRun>,
    /// The `nodes.served` record of the tick's lease, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served: Option<NodeServedTurnDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<LoopTokenDelta>,
}

/// A due tick the runner offered the renderer (§5.1); cleared when `on_prompt` accepts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopOffer {
    pub n: u32,
    pub message_id: String,
    pub offered_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<LoopRefuseReason>,
}

/// Which goose process runs the loop's clock (§5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopOwner {
    pub goosed_pid: u32,
    /// The process's start, unix seconds: with the pid, what proves it is still the owner.
    pub goosed_started_at: u64,
    /// goosed's parent (the app) at the claim: a goosed reparented away from it is an orphan.
    pub app_pid: u32,
}

/// The loop record, `extension_data["loop.v0"]` (§4.2). Written only through the session
/// manager's per-key read-modify-write.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopRecord {
    /// `lp_<8 hex>`, stable for the loop's life.
    pub id: String,
    /// The user's words, verbatim.
    pub goal: String,
    pub template: LoopTemplateId,
    /// The template's step text as the user left it, slots included.
    #[serde(default)]
    pub steps: String,
    pub cadence: LoopCadence,
    /// Relative to the session's working dir.
    pub state_file: String,
    /// A shell command run in the working dir after each tick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    /// The user's own tick count; absent by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_after_ticks: Option<u32>,
    pub status: LoopStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<LoopStatusReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_tick: Option<LoopNextTick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offer: Option<LoopOffer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<LoopOwner>,
    pub created_at: String,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub ticks: Vec<LoopTickRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoopRefusalCode {
    /// No loop runner in this goose process.
    RunnerAbsent,
    /// The `loop_report` extension cannot be synced in this goose process.
    ExtensionSyncAbsent,
    /// The chat builds with the swarm.
    SwarmBuild,
    EmptyGoal,
    BadCadence,
    EmptyStateFile,
    StateFileOutside,
    UnknownSlot,
    CheckRequired,
    BadStopAfter,
    NoLoop,
    /// The loop record could not be read.
    RecordUnreadable,
    /// The runner refused (its words).
    Refused,
}

/// A named refusal: nothing was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopRefusal {
    pub code: LoopRefusalCode,
    pub reason: String,
}

/// The loop of one chat. A PURE read: never claims the clock, never writes.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/loops/get", response = LoopsGetResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsGetRequest {
    pub session_id: String,
}

/// `loop` absent AND `error` absent = this chat has no loop. An unreadable record answers
/// `error`, never an absent loop.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsGetResponse {
    #[serde(rename = "loop", default, skip_serializing_if = "Option::is_none")]
    pub record: Option<LoopRecord>,
    /// The status as read now: `paused{closed}` when the owner is proven gone, `elsewhere` when
    /// another live goose runs it, else the written status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_status: Option<LoopStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_reason: Option<LoopStatusReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Start (or replace) this chat's loop; the first tick runs now. Claims the clock.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/loops/start", response = LoopsChangeResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsStartRequest {
    pub session_id: String,
    pub goal: String,
    pub template: LoopTemplateId,
    #[serde(default)]
    pub steps: String,
    pub cadence: LoopCadence,
    pub state_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_after_ticks: Option<u32>,
}

/// The loop after a change, or the named refusal (nothing written).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsChangeResponse {
    #[serde(rename = "loop", default, skip_serializing_if = "Option::is_none")]
    pub record: Option<LoopRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<LoopRefusal>,
}

/// Every field the Edit dialog shows, as the user left it (the whole editable set, so clearing
/// the check or the tick count is a value, not an absence).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopEdit {
    pub goal: String,
    pub template: LoopTemplateId,
    #[serde(default)]
    pub steps: String,
    pub cadence: LoopCadence,
    pub state_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_after_ticks: Option<u32>,
}

/// Edit the loop. Editing does not start a tick.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/loops/update", response = LoopsChangeResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsUpdateRequest {
    pub session_id: String,
    pub patch: LoopEdit,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LoopControlAction {
    #[default]
    Pause,
    Resume,
    Stop,
    TickNow,
    StopCheck,
}

/// Pause / Resume / Stop loop / Run a tick now / Stop check. `resume` and `tickNow` claim the
/// clock; the others write the record without claiming.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/loops/control", response = LoopsChangeResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsControlRequest {
    pub session_id: String,
    pub action: LoopControlAction,
}

/// The renderer could not submit an offered tick now; it sends `loops/ready` when that clears.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/loops/tickRefused", response = LoopsTickRefusedResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsTickRefusedRequest {
    pub session_id: String,
    pub loop_id: String,
    pub n: u32,
    pub reason: LoopRefuseReason,
}

impl Default for LoopsTickRefusedRequest {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            loop_id: String::new(),
            n: 0,
            reason: LoopRefuseReason::TurnRunning,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsTickRefusedResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<LoopRefusal>,
}

/// What refused an offer for this chat has cleared (the store's attempt ended, the queue
/// emptied): re-send the open offer.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/loops/ready", response = LoopsReadyResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsReadyRequest {
    pub session_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsReadyResponse {
    pub reoffered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<LoopRefusal>,
}

/// The Mac woke: re-read the wall clock (one tick if any were due, never a burst).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/loops/wake", response = LoopsWakeResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsWakeRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsWakeResponse {
    pub rearmed: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<LoopRefusal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopTemplateDto {
    pub id: LoopTemplateId,
    pub name: String,
    pub description: String,
    /// The steps with their slots, as the dialog prefills them.
    pub steps: String,
    /// The slot names the steps use (`state_file`, `check`, …).
    pub slots: Vec<String>,
    pub suggested_cadence: LoopCadence,
    /// The template cannot start without a check command.
    pub needs_check: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/loops/templates", response = LoopsTemplatesResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsTemplatesRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsTemplatesResponse {
    pub templates: Vec<LoopTemplateDto>,
}

/// One chat's loop, as the lists show it. Exactly one of `status` / `error`: an unreadable record
/// is named, never skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopSummaryDto {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<LoopStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_tick_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Every chat's loop (ended ones included). A PURE read.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/loops/list", response = LoopsListResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsListRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LoopsListResponse {
    pub loops: Vec<LoopSummaryDto>,
}
