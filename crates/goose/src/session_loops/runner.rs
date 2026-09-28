//! The loop runner (design §5.1–§5.3, slice L2a): one per goose process. It owns each loop's clock
//! and every decision; the window's renderer only fires the ticks it offers.
//!
//! The flow of one loop, as built:
//!
//! 1. **Claim** only on intent — `start`, `control{resume}`, `control{tickNow}` — a compare-and-set
//!    of `owner` inside one `update_extension_state` transaction, so two goose processes racing
//!    produce exactly one owner. `loops/get` and `loops/list` never reach the runner.
//! 2. **Wait** on the record's WALL time `next_tick.at` (a cadence says only when a tick STARTS;
//!    nothing here cuts a tick or a check — gate 5). The wait is re-evaluated, never "caught up", on
//!    events: a record change, a tick ending, its reviewers ending, a user turn ending, a door
//!    opening, `loops/ready`, a needs-you resolution, `loops/wake`.
//! 3. **Hold** a due tick while a user turn runs in this process (`waiting_turn`, the turn's chat
//!    named), while the previous tick's end-of-turn reviewers still run, or — Mac-wide
//!    (`mac_wide.rs`) — while a person's reply in any goose window holds the way the tick would use
//!    or stop (`WayHeld`), or holds the way the previous tick yielded to it on.
//! 4. **Offer** it: mint the message id, write `offer`, send `loops/tickDue` on every door. The
//!    offer stands until `on_prompt` accepts it (`accept_offer` → `OfferReservation::started`); a
//!    refusal is recorded and re-sent on `loops/ready` — never on a timer.
//! 5. **Run**: while the tick's turn runs, a person's reply on the tick's OWN way — in this window
//!    or any other goose process on this Mac — yields it (v1b, `mac_wide.rs`): the runner sets the
//!    cause cell to `yield` and cancels the run's own token. The check before the offer runs again
//!    as the tick starts, so a hold that began between the offer and the start yields it there.
//! 6. **End**: record `ended_at`, `wrote`, `served`, `tokens`, and a question the tick left open;
//!    run the check when the verdict asks for one (`check.rs`); then `rules::decide_after_tick`.
//! 7. **Release**: when this process's last door closes, every loop it owns reads "goose was
//!    closed" with `owner = None`; a door that opens again (a reload) restores what it released.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use goose_sdk_types::custom_notifications::{LoopsChangedNotification, LoopsTickDueNotification};
use goose_sdk_types::custom_requests::{
    LoopCheckRun, LoopControlAction, LoopEdit, LoopNextReason, LoopNextTick, LoopOffer, LoopOwner,
    LoopRecord, LoopRefusal, LoopRefusalCode, LoopRefuseReason, LoopStatus, LoopStatusReason,
    LoopTickOrigin, LoopTickOutcome, LoopTickRecord, LoopTokenDelta, LoopsTickRefusedRequest,
    NodeServedTurnDto,
};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::check::{self, CheckEnd, CheckSpec, STOPPED_BY_YOU};
use super::mac_wide::{self, Held, MacWide};
use super::owner::{self, ProcessTable, SystemProcesses};
use super::prompt::{self, AskedResolution, PromptFacts};
use super::record::{self, fmt_time, parse_time};
use super::rules::{
    self, AskedItem, CancelCause, NextTickDecision, OwnerProof, TickEnd, TickFacts,
};
use super::seam::{self, LoopRunner};
use crate::conversation::message::Message;
use crate::needs_you::{NeedsYouItem, NeedsYouState, NeedsYouStatus};
use crate::nodes::seam::PersonHold;
use crate::session::extension_data::ExtensionState;
use crate::session::SessionManager;
use crate::token_counter::TokenCounter;
use crate::turn_priority::{self, TurnPriority};

// ---------------------------------------------------------------------------------------------
// What the runner is given
// ---------------------------------------------------------------------------------------------

/// Where the runner reads the time and waits for it. Time is a VALUE the runner is handed, so the
/// tests feed it; the system clock waits on tokio's timer, which does not count a Mac's sleep —
/// `loops/wake` re-reads the wall clock after one (§5.4).
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn sleep_until(&self, at: DateTime<Utc>) -> BoxFuture<'static, ()>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn sleep_until(&self, at: DateTime<Utc>) -> BoxFuture<'static, ()> {
        let wait = (at - Utc::now())
            .to_std()
            .unwrap_or(std::time::Duration::ZERO);
        Box::pin(tokio::time::sleep(wait))
    }
}

/// One window's connection (L2b registers one per ACP connection; the tests supply a fake).
pub trait TickDoor: Send + Sync {
    fn tick_due(&self, due: &LoopsTickDueNotification);
    fn changed(&self, changed: &LoopsChangedNotification);
}

/// The PATH a check runs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckPath {
    /// The one goose inherited.
    Inherited,
    /// The user's login shell's, as the shell tool resolves it (goosed started by the app inherits
    /// a minimal PATH).
    LoginShell,
}

pub struct RunnerDeps {
    pub sessions: Arc<SessionManager>,
    pub clock: Arc<dyn Clock>,
    pub processes: Arc<dyn ProcessTable>,
    /// This process as an owner.
    pub me: LoopOwner,
    pub turns: &'static TurnPriority,
    /// This Mac's replies, as the node loader records them (the Mac-wide half, §5.3 v1b).
    pub mac: Arc<dyn MacWide>,
    /// `<data_dir>/loops`: each check's full output lands in `<loopId>/check-<n>.log`.
    pub logs_dir: PathBuf,
    pub check_path: CheckPath,
}

/// What `on_prompt` hands the runner when a tick starts (§5.2 step 4): the run's own cancel token
/// and cause cell — the runner's ONLY handle on the tick — and the session's context window, which
/// the check's output tail is a share of.
pub struct TickTicket {
    pub cancel: CancellationToken,
    pub cause: Arc<OnceLock<CancelCause>>,
    pub context_window_tokens: usize,
}

/// `_meta.goose.loopTick` of a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickMeta {
    pub loop_id: String,
    pub n: u32,
    pub message_id: String,
}

/// Read `goose.loopTick = {loopId, n, messageId}` from a prompt's `_meta`; `None` for any prompt
/// that does not carry all three.
pub fn tick_meta(meta: &serde_json::Map<String, Value>) -> Option<TickMeta> {
    let tick = meta.get("goose")?.get("loopTick")?;
    Some(TickMeta {
        loop_id: tick.get("loopId")?.as_str()?.to_string(),
        n: u32::try_from(tick.get("n")?.as_u64()?).ok()?,
        message_id: tick.get("messageId")?.as_str()?.to_string(),
    })
}

// ---------------------------------------------------------------------------------------------
// The runner's memory: only what the record cannot hold
// ---------------------------------------------------------------------------------------------

struct Running {
    loop_id: String,
    n: u32,
    started_at: DateTime<Utc>,
    cancel: CancellationToken,
    cause: Arc<OnceLock<CancelCause>>,
    context_window_tokens: usize,
    tokens_before: Option<(u64, u64, u64)>,
    watcher: tokio::task::AbortHandle,
}

/// What a release changed, so a door that opens again (a reload) can restore it.
struct Released {
    closed_at: String,
    status: LoopStatus,
    reason: Option<LoopStatusReason>,
    next_tick: Option<LoopNextTick>,
    offer: Option<LoopOffer>,
}

#[derive(Default)]
struct Mem {
    loop_id: String,
    /// The offer `on_prompt` accepted and has not yet started (§5.2's reservation).
    reserved: Option<String>,
    running: Option<Running>,
    /// The tick has ended and its record, check and decision are being written.
    finishing: bool,
    checking: Option<CancellationToken>,
    reviewers_pending: bool,
    released: Option<Released>,
    /// A `submit_failed` refusal gets no `loops/ready` from the window: the runner re-offers on
    /// its own events (a user turn ending, a door opening).
    reoffer_on_event: bool,
}

#[derive(Default)]
struct State {
    loops: HashMap<String, Mem>,
    doors: Vec<(u64, Arc<dyn TickDoor>)>,
    next_door: u64,
    idle_waiter: bool,
    /// The person's replies a held tick waits on, one waiter each.
    mac_waiters: std::collections::HashSet<String>,
}

enum Event {
    Evaluate,
    DoorOpened,
    DoorClosed,
    TickEnded {
        session: String,
        running: Running,
        end: TickEnd,
    },
    NeedsYouResolved {
        session: String,
        item: String,
    },
    UserTurnEnded {
        session: String,
    },
}

struct Inner {
    deps: RunnerDeps,
    state: Mutex<State>,
    /// Serialises the runner's own read-modify-writes with the memory beside them. Never held
    /// across a tick or a check.
    op: tokio::sync::Mutex<()>,
    events: mpsc::UnboundedSender<Event>,
    tokens: tokio::sync::OnceCell<Result<Arc<TokenCounter>, String>>,
}

#[derive(Clone)]
pub struct Runner {
    inner: Arc<Inner>,
}

/// Why a record write did not happen.
#[derive(Debug)]
enum Skip {
    NoLoop,
    NotOurs,
    Idle,
    /// Nothing to write; the loop next needs looking at at this time.
    Unchanged(DateTime<Utc>),
    Refused(LoopRefusal),
    Store(String),
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Skip {}

fn refused(reason: impl Into<String>) -> Skip {
    Skip::Refused(LoopRefusal {
        code: LoopRefusalCode::Refused,
        reason: reason.into(),
    })
}

fn to_refusal(skip: Skip) -> LoopRefusal {
    match skip {
        Skip::Refused(refusal) => refusal,
        Skip::NoLoop | Skip::NotOurs | Skip::Idle | Skip::Unchanged(_) => LoopRefusal {
            code: LoopRefusalCode::NoLoop,
            reason: "No loop in this chat.".to_string(),
        },
        Skip::Store(error) => LoopRefusal {
            code: LoopRefusalCode::RecordUnreadable,
            reason: format!("The loop record could not be read: {error}"),
        },
    }
}

fn origin_of(reason: &LoopNextReason) -> LoopTickOrigin {
    match reason {
        LoopNextReason::First => LoopTickOrigin::First,
        LoopNextReason::Cadence | LoopNextReason::Overdue => LoopTickOrigin::Cadence,
        LoopNextReason::SelfPaced { .. } => LoopTickOrigin::SelfPaced,
        LoopNextReason::BackToBack => LoopTickOrigin::BackToBack,
        LoopNextReason::AfterYourTurn => LoopTickOrigin::AfterYourTurn,
        LoopNextReason::AfterYourAnswer => LoopTickOrigin::AfterYourAnswer,
        LoopNextReason::OnWake => LoopTickOrigin::OnWake,
        LoopNextReason::Resume => LoopTickOrigin::Resume,
        LoopNextReason::Now => LoopTickOrigin::Now,
    }
}

fn next_now(now: DateTime<Utc>, reason: LoopNextReason) -> LoopNextTick {
    LoopNextTick {
        at: fmt_time(now),
        reason,
    }
}

fn last_n(record: &LoopRecord) -> u32 {
    record.ticks.last().map_or(0, |t| t.n)
}

/// A status a control wrote while a tick ran; the tick's end records its outcome under it.
fn held_by_control(status: LoopStatus) -> bool {
    matches!(status, LoopStatus::Paused | LoopStatus::Ended)
}

fn usage_totals(session: &crate::session::Session) -> Option<(u64, u64, u64)> {
    let usage = &session.accumulated_usage;
    let n = |v: Option<i32>| v.and_then(|v| u64::try_from(v).ok());
    Some((
        n(usage.input_tokens)?,
        n(usage.output_tokens)?,
        n(usage.total_tokens)?,
    ))
}

fn needs_you_items(session: &crate::session::Session) -> Result<Vec<NeedsYouItem>, String> {
    session
        .extension_data
        .get_extension_state(NeedsYouState::EXTENSION_NAME, NeedsYouState::VERSION)
        .map(|value| NeedsYouState::from_value(value).map(|s| s.items))
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(|e| e.to_string())
}

fn local_utc_offset_minutes() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

// ---------------------------------------------------------------------------------------------
// The handles `on_prompt` holds
// ---------------------------------------------------------------------------------------------

/// A window's door, open until this guard drops (L2b holds it across the connection's serving
/// future, so a dead connection is noticed by its own end — not by a send error or a clock).
pub struct DoorGuard {
    runner: Weak<Inner>,
    id: u64,
}

impl Drop for DoorGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.runner.upgrade() {
            lock(&inner.state).doors.retain(|(id, _)| *id != self.id);
            let _ = inner.events.send(Event::DoorClosed);
        }
    }
}

/// The offer `on_prompt` accepted (§5.2): it confirms with `started`, and any exit before that
/// drops it, which returns the offer to open (re-sent on `loops/ready`).
pub struct OfferReservation {
    runner: Runner,
    session: String,
    loop_id: String,
    n: u32,
    message_id: String,
    armed: bool,
}

impl OfferReservation {
    /// The id `on_prompt` stamps on the tick's message.
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    /// `tick_started` (§5.2 step 4): the tick's record is written before this returns. `Err` = the
    /// loop changed under the offer (paused, stopped, replaced); the prompt is not a tick.
    pub async fn started(mut self, ticket: TickTicket) -> Result<TickRun, String> {
        self.armed = false;
        let (runner, session) = (self.runner.clone(), self.session.clone());
        runner
            .tick_started(&session, &self.loop_id, self.n, &self.message_id, ticket)
            .await
    }
}

impl Drop for OfferReservation {
    fn drop(&mut self) {
        if self.armed {
            let mut state = lock(&self.runner.inner.state);
            if let Some(mem) = state.loops.get_mut(&self.session) {
                if mem.reserved.as_deref() == Some(self.message_id.as_str()) {
                    mem.reserved = None;
                }
            }
        }
    }
}

/// A tick in flight. `ended` is called exactly once at the exits that clear the active run; a
/// `TickRun` dropped without it records the tick `failed` ("tick_lost"), never nothing.
pub struct TickRun {
    runner: Runner,
    session: String,
    n: u32,
    ended: bool,
}

impl TickRun {
    pub fn n(&self) -> u32 {
        self.n
    }

    /// `tick_ended` (§5.2 step 6). Returns the slot the tick's end-of-turn reviewers go in.
    pub fn ended(mut self, end: TickEnd) -> ReviewSlot {
        self.ended = true;
        self.runner.tick_ended(&self.session, self.n, end, true);
        ReviewSlot {
            runner: self.runner.clone(),
            session: self.session.clone(),
            armed: true,
        }
    }
}

impl Drop for TickRun {
    fn drop(&mut self) {
        if !self.ended {
            self.runner.tick_ended(
                &self.session,
                self.n,
                TickEnd::Errored {
                    error_class: "tick_lost".to_string(),
                    error: "the tick's turn ended without telling the loop how".to_string(),
                },
                false,
            );
        }
    }
}

/// `tick_reviewed` (§5.2 step 7): no tick starts beside the previous tick's reviewers. Hand it the
/// reviewers' join handles; dropping it unfilled says none were spawned.
pub struct ReviewSlot {
    runner: Runner,
    session: String,
    armed: bool,
}

impl ReviewSlot {
    pub fn reviewers<T: Send + 'static>(mut self, handles: Vec<tokio::task::JoinHandle<T>>) {
        self.armed = false;
        let (runner, session) = (self.runner.clone(), self.session.clone());
        tokio::spawn(async move {
            for handle in handles {
                if let Err(error) = handle.await {
                    tracing::warn!(%error, "loop: a tick's reviewer ended abnormally");
                }
            }
            runner.reviewed(&session);
        });
    }
}

impl Drop for ReviewSlot {
    fn drop(&mut self) {
        if self.armed {
            self.runner.reviewed(&self.session);
        }
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().expect("loop runner state poisoned")
}

// ---------------------------------------------------------------------------------------------
// The runner
// ---------------------------------------------------------------------------------------------

impl Runner {
    /// A runner and its driver task (needs a tokio runtime).
    pub fn new(deps: RunnerDeps) -> Self {
        let (events, rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            deps,
            state: Mutex::new(State::default()),
            op: tokio::sync::Mutex::new(()),
            events,
            tokens: tokio::sync::OnceCell::new(),
        });
        tokio::spawn(drive(Arc::downgrade(&inner), rx));
        Self { inner }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.inner.state)
    }

    fn now(&self) -> DateTime<Utc> {
        self.inner.deps.clock.now()
    }

    fn me(&self) -> LoopOwner {
        self.inner.deps.me
    }

    fn send(&self, event: Event) {
        let _ = self.inner.events.send(event);
    }

    fn poke(&self) {
        self.send(Event::Evaluate);
    }

    pub fn register_door(&self, door: Arc<dyn TickDoor>) -> DoorGuard {
        let id = {
            let mut state = self.state();
            state.next_door += 1;
            let id = state.next_door;
            state.doors.push((id, door));
            id
        };
        self.send(Event::DoorOpened);
        DoorGuard {
            runner: Arc::downgrade(&self.inner),
            id,
        }
    }

    /// A user turn in `session` ended (§5.2 step 8): the tick that waited for a needs-you answer's
    /// turn can follow it.
    pub fn user_turn_ended(&self, session_id: &str) {
        self.send(Event::UserTurnEnded {
            session: session_id.to_string(),
        });
    }

    async fn write<T>(
        &self,
        session_id: &str,
        modify: impl FnOnce(Option<LoopRecord>) -> Result<(LoopRecord, T), Skip>,
    ) -> Result<(LoopRecord, T), Skip> {
        let written = record::update(&self.inner.deps.sessions, session_id, |current| {
            let (rec, out) = modify(current).map_err(anyhow::Error::new)?;
            Ok((rec.clone(), (rec, out)))
        })
        .await;
        written.map_err(|e| match e.downcast::<Skip>() {
            Ok(skip) => skip,
            Err(e) => Skip::Store(e.to_string()),
        })
    }

    /// A decision that most often changes nothing — `evaluate` runs for every owned loop after
    /// every event, `ready_inner` for every loop when a door opens — is taken first on a READ of
    /// the record (the store is WAL: a read never waits on a writer), and only a decision that
    /// changes the record takes the write transaction, where it is taken again on the record as
    /// it is then. A decision that writes nothing is thereby linearized at its read; before, it
    /// waited on any other writer of the store (another goose process, a person's turn end) while
    /// holding `op`, and every other door of the runner — the tick's `accept_offer` — waited on
    /// `op` behind a transaction that would write nothing (Q-286).
    async fn decide_then_write<T>(
        &self,
        session_id: &str,
        decide: impl Fn(Option<LoopRecord>) -> Result<(LoopRecord, T), Skip>,
    ) -> Result<(LoopRecord, T), Skip> {
        let current = record::read(&self.inner.deps.sessions, session_id)
            .await
            .map_err(|e| Skip::Store(e.to_string()))?
            .map_err(Skip::Store)?;
        decide(current)?;
        self.write(session_id, decide).await
    }

    fn emit(&self, session_id: &str, rec: &LoopRecord) {
        let changed = LoopsChangedNotification {
            session_id: session_id.to_string(),
            record: rec.clone(),
        };
        let doors: Vec<_> = self.state().doors.iter().map(|(_, d)| d.clone()).collect();
        for door in doors {
            door.changed(&changed);
        }
    }

    fn claim_check(&self, rec: &LoopRecord) -> Result<(), Skip> {
        match &rec.owner {
            None => Ok(()),
            Some(owner) => {
                match owner::prove(owner, &self.me(), self.inner.deps.processes.as_ref()) {
                    OwnerProof::ThisProcess | OwnerProof::Gone { .. } => Ok(()),
                    OwnerProof::Live => Err(refused(
                        "This loop runs in another goose window — use the controls there.",
                    )),
                    OwnerProof::Unproven { why } => Err(refused(format!(
                    "Another goose window may run this loop ({why}); goose will not take it over."
                ))),
                }
            }
        }
    }

    fn ensure_mem(&self, session_id: &str, loop_id: &str) {
        let mut state = self.state();
        let mem = state.loops.entry(session_id.to_string()).or_default();
        mem.loop_id = loop_id.to_string();
        mem.released = None;
    }

    fn stop_running(&self, session_id: &str, cause: CancelCause) {
        let state = self.state();
        if let Some(mem) = state.loops.get(session_id) {
            if let Some(running) = &mem.running {
                let _ = running.cause.set(cause);
                running.cancel.cancel();
            }
        }
    }

    // --- the wait and the offer ---------------------------------------------------------------

    async fn evaluate_all(&self) -> Option<DateTime<Utc>> {
        let sessions: Vec<String> = self.state().loops.keys().cloned().collect();
        let mut earliest: Option<DateTime<Utc>> = None;
        for session in sessions {
            if let Some(at) = self.evaluate(&session).await {
                earliest = Some(earliest.map_or(at, |e| e.min(at)));
            }
        }
        earliest
    }

    fn ensure_idle_waiter(&self) {
        {
            let mut state = self.state();
            if state.idle_waiter {
                return;
            }
            state.idle_waiter = true;
        }
        let runner = Arc::downgrade(&self.inner);
        let turns = self.inner.deps.turns;
        tokio::spawn(async move {
            turns.wait_no_user_turn().await;
            if let Some(inner) = runner.upgrade() {
                let runner = Runner { inner };
                runner.state().idle_waiter = false;
                runner.user_turns_idle().await;
                runner.poke();
            }
        });
    }

    /// Re-offer the refusals the window will not clear itself (`submit_failed`).
    async fn user_turns_idle(&self) {
        let pending: Vec<String> = self
            .state()
            .loops
            .iter()
            .filter(|(_, m)| m.reoffer_on_event)
            .map(|(s, _)| s.clone())
            .collect();
        for session in pending {
            if let Err(refusal) = self.ready_inner(&session).await {
                tracing::warn!(session, reason = %refusal.reason, "loop: re-offer failed");
            }
        }
    }

    async fn holder_reason(
        &self,
        running: &turn_priority::RunningTurns,
    ) -> Option<LoopStatusReason> {
        let session_id = running.sessions.first()?.clone();
        let chat = mac_wide::chat_name(&self.inner.deps.sessions, &session_id).await;
        Some(LoopStatusReason::UserTurn { session_id, chat })
    }

    /// The Mac-wide check before an offer (§5.3 v1b, `mac_wide::offer_check`), read only for a
    /// loop that is due or held now. A check that cannot read whose replies hold the engine is
    /// named loudly and holds nothing: the tick's own lease then meets the loader, which refuses
    /// a load on the same unreadable record by name.
    async fn held_before_offer(
        &self,
        session_id: &str,
        loop_id: &str,
        now: DateTime<Utc>,
    ) -> Option<Held> {
        let rec = match record::read(&self.inner.deps.sessions, session_id).await {
            Ok(Ok(Some(rec))) if rec.id == loop_id => rec,
            _ => return None,
        };
        let due = match rec.status {
            LoopStatus::Waiting => {
                rec.offer.is_none()
                    && rec
                        .next_tick
                        .as_ref()
                        .and_then(|next| parse_time(&next.at).ok())
                        .is_some_and(|at| at <= now)
            }
            LoopStatus::WaitingTurn => rec.offer.is_none(),
            _ => false,
        };
        if !due {
            return None;
        }
        self.check_mac_wide(session_id, &rec).await
    }

    async fn check_mac_wide(&self, session_id: &str, rec: &LoopRecord) -> Option<Held> {
        let deps = &self.inner.deps;
        match mac_wide::offer_check(deps.mac.as_ref(), &deps.sessions, session_id, rec).await {
            Ok(held) => held,
            Err(error) => {
                tracing::error!(session_id, %error, "loop: whether a person's reply holds this Mac's engine is unknown; the tick is not held for it");
                None
            }
        }
    }

    /// Look again when the person's reply a held tick waits on ends — one waiter per reply.
    fn ensure_mac_waiter(&self, person: PersonHold, since: u64) {
        let key = format!("{}|{:?}", person.session, person.ends);
        if !self.state().mac_waiters.insert(key.clone()) {
            return;
        }
        let ended = self.inner.deps.mac.person_ended(&person, since);
        let runner = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            ended.await;
            if let Some(inner) = runner.upgrade() {
                let runner = Runner { inner };
                runner.state().mac_waiters.remove(&key);
                runner.poke();
            }
        });
    }

    /// The yield's cause for a person's reply: its chat, and the way the two shared.
    async fn yield_to_person(&self, person: &PersonHold) -> CancelCause {
        CancelCause::Yield {
            to_session: person.session.clone(),
            to_chat: mac_wide::chat_name(&self.inner.deps.sessions, &person.session).await,
            way: Some(person.way.clone()),
        }
    }

    /// Decide what one owned loop does now; the time it next needs looking at, if any.
    async fn evaluate(&self, session_id: &str) -> Option<DateTime<Utc>> {
        let _op = self.inner.op.lock().await;
        let (loop_id, reviewers_pending) = {
            let state = self.state();
            let mem = state.loops.get(session_id)?;
            if mem.reserved.is_some()
                || mem.running.is_some()
                || mem.finishing
                || mem.released.is_some()
            {
                return None;
            }
            (mem.loop_id.clone(), mem.reviewers_pending)
        };
        let now = self.now();
        let turns = self.inner.deps.turns.running();
        let turn_runs = turns.running > 0;
        let holder = if turn_runs {
            self.ensure_idle_waiter();
            self.holder_reason(&turns).await
        } else {
            None
        };
        let held = if turn_runs {
            None
        } else {
            self.held_before_offer(session_id, &loop_id, now).await
        };
        let me = self.me();

        enum Step {
            Sleep(DateTime<Utc>),
            Hold,
            Offer,
        }
        let written = self
            .decide_then_write(session_id, |current| {
                let mut rec = current.ok_or(Skip::NoLoop)?;
                if rec.id != loop_id || rec.owner != Some(me) {
                    return Err(Skip::NotOurs);
                }
                let next = match (rec.status, &rec.status_reason) {
                    (LoopStatus::Waiting, _) => rec.next_tick.clone().ok_or(Skip::Idle)?,
                    (LoopStatus::WaitingTurn, Some(LoopStatusReason::Reviewers { .. }))
                        if !reviewers_pending =>
                    {
                        match rules::next_tick(&rec, now, false).map_err(Skip::Store)? {
                            NextTickDecision::At { next } => next,
                            NextTickDecision::WaitingYou { reason } => {
                                rec.status = LoopStatus::WaitingYou;
                                rec.status_reason = Some(reason);
                                return Ok((rec, Step::Hold));
                            }
                            NextTickDecision::AfterReviewers { .. } => return Err(Skip::Idle),
                        }
                    }
                    (LoopStatus::WaitingTurn, Some(LoopStatusReason::UserTurn { .. }))
                        if !turn_runs =>
                    {
                        next_now(now, LoopNextReason::AfterYourTurn)
                    }
                    (LoopStatus::WaitingTurn, Some(LoopStatusReason::WayHeld { .. })) => {
                        next_now(now, LoopNextReason::AfterYourTurn)
                    }
                    _ => return Err(Skip::Idle),
                };
                let at = parse_time(&next.at).map_err(Skip::Store)?;
                if at > now {
                    if rec.status == LoopStatus::Waiting && rec.next_tick.as_ref() == Some(&next) {
                        return Err(Skip::Unchanged(at));
                    }
                    rec.status = LoopStatus::Waiting;
                    rec.status_reason = None;
                    rec.next_tick = Some(next);
                    return Ok((rec, Step::Sleep(at)));
                }
                if rec.status == LoopStatus::Waiting && rec.offer.is_some() {
                    // Offered already: it is re-sent on a door opening and on `loops/ready`, never
                    // on another look at the clock.
                    return Err(Skip::Idle);
                }
                if reviewers_pending {
                    rec.status = LoopStatus::WaitingTurn;
                    rec.status_reason = Some(LoopStatusReason::Reviewers { n: last_n(&rec) });
                    rec.next_tick = None;
                    return Ok((rec, Step::Hold));
                }
                if turn_runs {
                    // A turn that did not name its chat (`turn_priority::user_turn`) still holds
                    // the tick; with nothing to name, the written status stays as it is.
                    let reason = holder.clone().ok_or(Skip::Idle)?;
                    rec.status = LoopStatus::WaitingTurn;
                    rec.status_reason = Some(reason);
                    rec.next_tick = None;
                    return Ok((rec, Step::Hold));
                }
                if let Some(held) = &held {
                    if rec.status == LoopStatus::WaitingTurn
                        && rec.status_reason.as_ref() == Some(&held.reason)
                        && rec.next_tick.is_none()
                    {
                        return Err(Skip::Idle);
                    }
                    rec.status = LoopStatus::WaitingTurn;
                    rec.status_reason = Some(held.reason.clone());
                    rec.next_tick = None;
                    return Ok((rec, Step::Hold));
                }
                rec.status = LoopStatus::Waiting;
                rec.status_reason = None;
                rec.next_tick = Some(next);
                if rec.offer.is_none() {
                    let n = last_n(&rec) + 1;
                    let uuid = Uuid::new_v4().simple().to_string();
                    rec.offer = Some(LoopOffer {
                        n,
                        message_id: rules::tick_id(&rec.id, n, &uuid),
                        offered_at: fmt_time(now),
                        refused: None,
                    });
                }
                Ok((rec, Step::Offer))
            })
            .await;
        if let Some(held) = held {
            self.ensure_mac_waiter(held.person, held.since);
        }
        match written {
            Ok((rec, step)) => {
                self.emit(session_id, &rec);
                match step {
                    Step::Sleep(at) => Some(at),
                    Step::Hold => None,
                    Step::Offer => {
                        self.send_offer(session_id, &rec).await;
                        None
                    }
                }
            }
            Err(Skip::Unchanged(at)) => Some(at),
            Err(Skip::NoLoop) | Err(Skip::NotOurs) => {
                self.state().loops.remove(session_id);
                None
            }
            Err(Skip::Idle) => {
                if turn_runs && holder.is_none() {
                    tracing::warn!(
                        session_id,
                        "loop: a user turn that named no chat holds the tick (on_prompt should open turn_priority::user_turn_in)"
                    );
                }
                None
            }
            Err(Skip::Refused(refusal)) => {
                tracing::warn!(session_id, reason = %refusal.reason, "loop: not offered");
                None
            }
            Err(Skip::Store(error)) => {
                tracing::error!(session_id, %error, "loop: the record could not be evaluated");
                None
            }
        }
    }

    /// Send the record's open offer on every door (§5.1).
    async fn send_offer(&self, session_id: &str, rec: &LoopRecord) {
        let Some(offer) = &rec.offer else { return };
        let session = match self
            .inner
            .deps
            .sessions
            .get_session(session_id, false)
            .await
        {
            Ok(session) => session,
            Err(error) => {
                tracing::error!(session_id, %error, "loop: the chat of an offer could not be read");
                return;
            }
        };
        let (finished, cut_short) = prompt::last_finished(rec);
        let asked_resolution = match finished.and_then(|t| t.outcome.as_ref()) {
            Some(LoopTickOutcome::Asked { item_id, .. }) => Some(
                match needs_you_items(&session)
                    .ok()
                    .and_then(|items| items.into_iter().find(|i| &i.id == item_id))
                {
                    Some(item) => match item.status {
                        NeedsYouStatus::Answered => match item.answer {
                            Some(answer) => AskedResolution::Answered { answer },
                            None => AskedResolution::Open,
                        },
                        NeedsYouStatus::Dismissed => AskedResolution::Dismissed,
                        NeedsYouStatus::Open => AskedResolution::Open,
                    },
                    None => AskedResolution::Open,
                },
            ),
            _ => None,
        };
        let mut chat_names = BTreeMap::new();
        for tick in cut_short {
            let Some(LoopTickOutcome::Yielded { to_session, .. }) = &tick.outcome else {
                continue;
            };
            if to_session.is_empty() || chat_names.contains_key(to_session) {
                continue;
            }
            match self
                .inner
                .deps
                .sessions
                .get_session(to_session, false)
                .await
            {
                Ok(chat) => {
                    chat_names.insert(to_session.clone(), chat.name);
                }
                Err(error) => tracing::warn!(
                    to_session,
                    %error,
                    "loop: the chat a tick yielded to could not be read; the prompt names it as it was at the yield"
                ),
            }
        }
        let facts = PromptFacts {
            working_dir: session.working_dir.to_string_lossy().to_string(),
            asked_resolution,
            utc_offset_minutes: local_utc_offset_minutes(),
            chat_names,
        };
        let text = match prompt::tick_prompt(rec, offer.n, &facts) {
            Ok(text) => text,
            Err(error) => {
                self.pause_on_engine_error(
                    session_id,
                    offer.n,
                    format!("goose could not write tick {}'s prompt: {error}", offer.n),
                )
                .await;
                return;
            }
        };
        let due = LoopsTickDueNotification {
            session_id: session_id.to_string(),
            loop_id: rec.id.clone(),
            n: offer.n,
            message_id: offer.message_id.clone(),
            prompt: text,
        };
        let doors: Vec<_> = self.state().doors.iter().map(|(_, d)| d.clone()).collect();
        for door in doors {
            door.tick_due(&due);
        }
    }

    /// A rule of the runner itself failed (a record it cannot decide on). The loop pauses and the
    /// sentence carries the error; the contract has no reason of its own for this, so it reads as
    /// "Blocked — goose could not …".
    async fn pause_on_engine_error(&self, session_id: &str, n: u32, error: String) {
        tracing::error!(session_id, n, %error, "loop: paused on the runner's own error");
        let written = self
            .write(session_id, |current| {
                let mut rec = current.ok_or(Skip::NoLoop)?;
                rec.status = LoopStatus::Paused;
                rec.status_reason = Some(LoopStatusReason::Blocked {
                    n,
                    blocked_on: error,
                });
                rec.offer = None;
                rec.next_tick = None;
                Ok((rec, ()))
            })
            .await;
        if let Ok((rec, ())) = written {
            self.emit(session_id, &rec);
        }
    }

    // --- the tick ------------------------------------------------------------------------------

    /// `on_prompt`'s match (§5.2): `Some` only when the prompt's `loopTick` is this chat's open
    /// offer, not yet taken. A forged or stale meta gets `None` — an ordinary user prompt.
    pub async fn accept_offer(
        &self,
        session_id: &str,
        meta: &TickMeta,
    ) -> Option<OfferReservation> {
        let _op = self.inner.op.lock().await;
        {
            let state = self.state();
            let mem = state.loops.get(session_id)?;
            if mem.reserved.is_some() || mem.running.is_some() || mem.finishing {
                return None;
            }
        }
        let rec = record::read(&self.inner.deps.sessions, session_id)
            .await
            .ok()?
            .ok()??;
        let offer = rec.offer.as_ref()?;
        let open = rec.id == meta.loop_id
            && offer.n == meta.n
            && offer.message_id == meta.message_id
            && rec.owner == Some(self.me())
            && matches!(rec.status, LoopStatus::Waiting | LoopStatus::WaitingTurn);
        if !open {
            return None;
        }
        let mut state = self.state();
        let mem = state.loops.get_mut(session_id)?;
        mem.reserved = Some(meta.message_id.clone());
        mem.reoffer_on_event = false;
        Some(OfferReservation {
            runner: self.clone(),
            session: session_id.to_string(),
            loop_id: rec.id,
            n: offer.n,
            message_id: offer.message_id.clone(),
            armed: true,
        })
    }

    async fn tick_started(
        &self,
        session_id: &str,
        loop_id: &str,
        n: u32,
        message_id: &str,
        ticket: TickTicket,
    ) -> Result<TickRun, String> {
        let _op = self.inner.op.lock().await;
        let clear_reservation = |runner: &Runner| {
            if let Some(mem) = runner.state().loops.get_mut(session_id) {
                if mem.reserved.as_deref() == Some(message_id) {
                    mem.reserved = None;
                }
            }
        };
        let now = self.now();
        let tokens_before = match self
            .inner
            .deps
            .sessions
            .get_session(session_id, false)
            .await
        {
            Ok(session) => usage_totals(&session),
            Err(error) => {
                clear_reservation(self);
                return Err(format!("the chat could not be read: {error}"));
            }
        };
        // The check before the offer, again (§5.3): a person's reply that took the way between the
        // offer and this start yields the tick below, before it reaches the model.
        let held_at_start = match record::read(&self.inner.deps.sessions, session_id).await {
            Ok(Ok(Some(rec))) if rec.id == loop_id => self.check_mac_wide(session_id, &rec).await,
            _ => None,
        };
        let me = self.me();
        let written = self
            .write(session_id, |current| {
                let mut rec = current.ok_or(Skip::NoLoop)?;
                let offer = rec.offer.clone().ok_or(Skip::Idle)?;
                if rec.id != loop_id
                    || rec.owner != Some(me)
                    || offer.message_id != message_id
                    || !matches!(rec.status, LoopStatus::Waiting | LoopStatus::WaitingTurn)
                {
                    return Err(Skip::Idle);
                }
                let origin = rec
                    .next_tick
                    .as_ref()
                    .map(|next| origin_of(&next.reason))
                    .ok_or_else(|| refused(format!("tick {n}'s offer names no reason")))?;
                rec.ticks.push(LoopTickRecord {
                    n,
                    origin,
                    started_at: fmt_time(now),
                    ended_at: None,
                    first_message_id: message_id.to_string(),
                    report: None,
                    outcome: None,
                    wrote: Vec::new(),
                    check: None,
                    served: None,
                    tokens: None,
                });
                rec.offer = None;
                rec.next_tick = None;
                rec.status = LoopStatus::Running;
                rec.status_reason = None;
                Ok((rec, ()))
            })
            .await;
        let rec = match written {
            Ok((rec, ())) => rec,
            Err(skip) => {
                clear_reservation(self);
                return Err(match skip {
                    Skip::Store(error) => format!("the loop record could not be written: {error}"),
                    Skip::Refused(refusal) => refusal.reason,
                    _ => "the loop changed under the offer".to_string(),
                });
            }
        };
        // What held the offer, holding again between the offer and this start — a user turn in
        // this process, or a person's reply the Mac-wide check reads — yields the tick here,
        // before the prompt goes on: decided now, not by a task racing `on_prompt`, so the tick
        // never reaches the model and `on_prompt` settles it on its way in.
        let turns = self.inner.deps.turns.running();
        let yield_now = match (turns.running, held_at_start) {
            (0, None) => None,
            (0, Some(held)) => Some(self.yield_to_person(&held.person).await),
            _ => Some(
                self.yield_cause(session_id, n, turns.sessions.first().cloned())
                    .await,
            ),
        };
        if let Some(cause) = yield_now {
            let _ = ticket.cause.set(cause);
            ticket.cancel.cancel();
        }
        let watcher = self.spawn_yield_watcher(session_id, n);
        {
            let mut state = self.state();
            let mem = state.loops.entry(session_id.to_string()).or_default();
            mem.reserved = None;
            mem.running = Some(Running {
                loop_id: loop_id.to_string(),
                n,
                started_at: now,
                cancel: ticket.cancel,
                cause: ticket.cause,
                context_window_tokens: ticket.context_window_tokens,
                tokens_before,
                watcher,
            });
        }
        self.emit(session_id, &rec);
        Ok(TickRun {
            runner: self.clone(),
            session: session_id.to_string(),
            n,
            ended: false,
        })
    }

    /// v1b (§5.3, L2c): while the tick runs, only a person's reply on the tick's OWN way — in this
    /// window or any other goose process on this Mac — yields it (`mac_wide::person_on_tick_way`).
    /// A user turn on a cloud node, on another way, or a loop's tick never does.
    fn spawn_yield_watcher(&self, session_id: &str, n: u32) -> tokio::task::AbortHandle {
        let mac = self.inner.deps.mac.clone();
        let runner = Arc::downgrade(&self.inner);
        let session = session_id.to_string();
        tokio::spawn(async move {
            let person = mac_wide::person_on_tick_way(mac, session.clone()).await;
            if let Some(inner) = runner.upgrade() {
                let runner = Runner { inner };
                let cause = runner.yield_to_person(&person).await;
                runner.yield_tick(&session, n, cause);
            }
        })
        .abort_handle()
    }

    /// The yield's cause for a user turn of this process: its chat, by id and name.
    async fn yield_cause(&self, session_id: &str, n: u32, to: Option<String>) -> CancelCause {
        let (to_session, to_chat) = match to {
            Some(to) => {
                let chat = mac_wide::chat_name(&self.inner.deps.sessions, &to).await;
                (to, chat)
            }
            None => {
                tracing::warn!(
                    session_id,
                    n,
                    "loop: a tick yields to a user turn that named no chat (on_prompt should open turn_priority::user_turn_in)"
                );
                (String::new(), String::new())
            }
        };
        CancelCause::Yield {
            to_session,
            to_chat,
            way: None,
        }
    }

    fn yield_tick(&self, session_id: &str, n: u32, cause: CancelCause) {
        let state = self.state();
        let Some(running) = state.loops.get(session_id).and_then(|m| m.running.as_ref()) else {
            return;
        };
        if running.n == n {
            let _ = running.cause.set(cause);
            running.cancel.cancel();
        }
    }

    fn tick_ended(&self, session_id: &str, n: u32, end: TickEnd, reviewers_follow: bool) {
        let running = {
            let mut state = self.state();
            let Some(mem) = state.loops.get_mut(session_id) else {
                return;
            };
            if mem.running.as_ref().map(|r| r.n) != Some(n) {
                return;
            }
            let running = mem.running.take().expect("checked above");
            running.watcher.abort();
            mem.finishing = true;
            mem.reviewers_pending = reviewers_follow;
            running
        };
        self.send(Event::TickEnded {
            session: session_id.to_string(),
            running,
            end,
        });
    }

    fn reviewed(&self, session_id: &str) {
        if let Some(mem) = self.state().loops.get_mut(session_id) {
            mem.reviewers_pending = false;
        }
        self.poke();
    }

    async fn token_counter(&self) -> Result<Arc<TokenCounter>, String> {
        self.inner
            .tokens
            .get_or_init(|| async {
                crate::token_counter::create_token_counter()
                    .await
                    .map(Arc::new)
            })
            .await
            .clone()
    }

    async fn check_path(&self) -> Option<String> {
        match self.inner.deps.check_path {
            CheckPath::Inherited => None,
            CheckPath::LoginShell => login_shell_path().await,
        }
    }

    /// Everything after a tick's turn (§4.6): its readings, its check, the decision.
    async fn finish_tick(&self, session_id: String, running: Running, end: TickEnd) {
        let end = match end {
            TickEnd::Cancelled { cause } => TickEnd::Cancelled {
                cause: running.cause.get().cloned().or(cause),
            },
            other => other,
        };
        let finished = self.finish_tick_inner(&session_id, &running, end).await;
        let release = {
            let mut state = self.state();
            if let Some(mem) = state.loops.get_mut(&session_id) {
                mem.finishing = false;
                mem.checking = None;
            }
            state.doors.is_empty()
        };
        if let Err(error) = finished {
            tracing::error!(session_id, n = running.n, %error, "loop: the tick's end was not recorded");
        }
        if release {
            self.release(&session_id).await;
        }
        self.user_turns_idle().await;
        self.poke();
    }

    async fn finish_tick_inner(
        &self,
        session_id: &str,
        running: &Running,
        end: TickEnd,
    ) -> Result<(), String> {
        let now = self.now();
        let sessions = &self.inner.deps.sessions;
        let rec = match record::read(sessions, session_id)
            .await
            .map_err(|e| e.to_string())?
        {
            Ok(Some(rec)) if rec.id == running.loop_id => rec,
            // The loop was replaced or removed while its tick ran: nothing of it is left to record.
            Ok(_) => return Ok(()),
            Err(error) => return Err(error),
        };
        let session = sessions
            .get_session(session_id, true)
            .await
            .map_err(|e| e.to_string())?;
        let working_dir = session.working_dir.to_string_lossy().to_string();
        let marker = rec
            .ticks
            .iter()
            .find(|t| t.n == running.n)
            .map(|t| t.first_message_id.clone())
            .ok_or_else(|| format!("tick {} is not in the record", running.n))?;
        let messages: &[Message] = session
            .conversation
            .as_ref()
            .map_or(&[][..], |c| c.messages().as_slice());
        let wrote = match messages
            .iter()
            .position(|m| m.id.as_deref() == Some(marker.as_str()))
        {
            Some(start) => rules::wrote(&messages[start..], &rec.state_file, &working_dir),
            None => {
                tracing::warn!(
                    session_id,
                    marker,
                    "loop: the tick's marker message is not in the conversation; its writes cannot be listed"
                );
                Vec::new()
            }
        };
        let started_at = parse_time(&fmt_time(running.started_at))?;
        let asked = match needs_you_items(&session) {
            Ok(items) => items
                .into_iter()
                .filter(|i| i.status == NeedsYouStatus::Open && i.created_at >= started_at)
                .max_by_key(|i| i.created_at)
                .map(|i| AskedItem {
                    item_id: i.id,
                    question: i.question,
                }),
            Err(error) => {
                tracing::warn!(session_id, %error, "loop: the needs-you store could not be read");
                None
            }
        };
        let served: Option<NodeServedTurnDto> = match crate::nodes::served::last(
            sessions, session_id,
        )
        .await
        {
            Ok(turn) => turn.filter(|t| {
                i64::try_from(t.at_ms).is_ok_and(|at| at >= started_at.timestamp_millis())
            }),
            Err(error) => {
                tracing::warn!(session_id, %error, "loop: the served-turn record could not be read");
                None
            }
        };
        let tokens = match (running.tokens_before, usage_totals(&session)) {
            (Some((i0, o0, t0)), Some((i1, o1, t1))) => Some(LoopTokenDelta {
                input: i1.saturating_sub(i0),
                output: o1.saturating_sub(o0),
                total: t1.saturating_sub(t0),
            }),
            _ => None,
        };
        let log_path = self
            .inner
            .deps
            .logs_dir
            .join(&running.loop_id)
            .join(format!("check-{}.log", running.n));

        // 1. The tick's end and its readings; the check's start when one runs.
        let n = running.n;
        let (rec, check_command) = {
            let _op = self.inner.op.lock().await;
            let (end, asked, log_path) = (end.clone(), asked.clone(), log_path.clone());
            let written = self
                .write(session_id, |current| {
                    let mut rec = current.ok_or(Skip::NoLoop)?;
                    if rec.id != running.loop_id {
                        return Err(Skip::NotOurs);
                    }
                    let command = rec.check.clone();
                    let held = held_by_control(rec.status);
                    let tick = rec
                        .ticks
                        .iter_mut()
                        .find(|t| t.n == n)
                        .ok_or(Skip::NoLoop)?;
                    tick.ended_at = Some(fmt_time(now));
                    tick.wrote = wrote;
                    tick.served = served;
                    tick.tokens = tokens;
                    let runs = command.filter(|_| {
                        rules::check_runs_after(&end, tick.report.as_ref(), asked.is_some())
                    });
                    if let Some(command) = &runs {
                        tick.check = Some(LoopCheckRun {
                            command: command.clone(),
                            started_at: fmt_time(now),
                            ended_at: None,
                            ran: true,
                            exit: None,
                            output_tail: String::new(),
                            log_path: Some(log_path.to_string_lossy().to_string()),
                            error: None,
                        });
                        if !held {
                            rec.status = LoopStatus::Checking;
                            rec.status_reason = None;
                        }
                    }
                    Ok((rec, runs))
                })
                .await;
            match written {
                Ok(written) => written,
                Err(Skip::NoLoop) | Err(Skip::NotOurs) => return Ok(()),
                Err(skip) => return Err(skip.to_string()),
            }
        };
        self.emit(session_id, &rec);

        // 2. The check, to its exit status (no timeout; the user's Stop check is the only stop).
        let check_run = match check_command {
            None => None,
            Some(command) => {
                let stop = CancellationToken::new();
                if let Some(mem) = self.state().loops.get_mut(session_id) {
                    mem.checking = Some(stop.clone());
                }
                let spec = CheckSpec {
                    command: command.clone(),
                    working_dir: session.working_dir.clone(),
                    log_path: log_path.clone(),
                    path_env: self.check_path().await,
                };
                let outcome = check::run(&spec, stop).await;
                let ended_at = fmt_time(self.now());
                let log = log_path.to_string_lossy().to_string();
                let tail = match &outcome.output {
                    Ok(text) => match self.token_counter().await {
                        Ok(counter) => rules::output_tail(text, running.context_window_tokens, |s| {
                            counter.count_tokens(s)
                        })
                        .to_string(),
                        Err(error) => format!(
                            "(goose could not count this output's tokens to cut its tail: {error}; the whole output is in {log})"
                        ),
                    },
                    Err(error) => format!("({error})"),
                };
                let started_at = rec
                    .ticks
                    .iter()
                    .find(|t| t.n == n)
                    .and_then(|t| t.check.as_ref())
                    .map_or_else(|| fmt_time(now), |c| c.started_at.clone());
                Some(match outcome.end {
                    CheckEnd::Exited { code } => LoopCheckRun {
                        command,
                        started_at,
                        ended_at: Some(ended_at),
                        ran: true,
                        exit: code,
                        output_tail: tail,
                        log_path: Some(log),
                        error: None,
                    },
                    CheckEnd::CouldNotRun { error } => LoopCheckRun {
                        command,
                        started_at,
                        ended_at: Some(ended_at),
                        ran: false,
                        exit: None,
                        output_tail: String::new(),
                        log_path: Some(log),
                        error: Some(error),
                    },
                    CheckEnd::Stopped => LoopCheckRun {
                        command,
                        started_at,
                        ended_at: Some(ended_at),
                        ran: false,
                        exit: None,
                        output_tail: tail,
                        log_path: Some(log),
                        error: Some(STOPPED_BY_YOU.to_string()),
                    },
                })
            }
        };

        // 3. The decision (§4.6), over the record as it is now.
        let _op = self.inner.op.lock().await;
        let reviewers_pending = self
            .state()
            .loops
            .get(session_id)
            .is_some_and(|m| m.reviewers_pending);
        let now = self.now();
        let written = self
            .write(session_id, |current| {
                let mut rec = current.ok_or(Skip::NoLoop)?;
                if rec.id != running.loop_id {
                    return Err(Skip::NotOurs);
                }
                let at = rec
                    .ticks
                    .iter()
                    .position(|t| t.n == n)
                    .ok_or(Skip::NoLoop)?;
                if let Some(run) = check_run {
                    rec.ticks[at].check = Some(run);
                }
                let facts = TickFacts {
                    end,
                    check: rec.ticks[at].check.clone(),
                    asked,
                    reviewers_pending,
                    now: fmt_time(now),
                };
                match rules::decide_after_tick(&rec, &facts) {
                    Ok(decision) => {
                        rec.ticks[at].outcome = Some(decision.outcome);
                        if held_by_control(rec.status) && decision.status != LoopStatus::Ended {
                            if let Some(LoopStatusReason::FinishingElsewhere { n }) =
                                rec.status_reason
                            {
                                rec.status_reason = Some(LoopStatusReason::ByYou { after_tick: n });
                            }
                        } else {
                            rec.status = decision.status;
                            rec.status_reason = decision.reason;
                            rec.next_tick = decision.next_tick;
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "loop: no decision after the tick");
                        rec.status = LoopStatus::Paused;
                        rec.status_reason = Some(LoopStatusReason::Blocked {
                            n,
                            blocked_on: format!(
                                "goose could not decide what follows tick {n}: {error}"
                            ),
                        });
                        rec.next_tick = None;
                    }
                }
                if rec.status == LoopStatus::Ended && rec.ended_at.is_none() {
                    rec.ended_at = Some(fmt_time(now));
                }
                rec.offer = None;
                Ok((rec, ()))
            })
            .await;
        match written {
            Ok((rec, ())) => {
                self.emit(session_id, &rec);
                Ok(())
            }
            Err(Skip::NoLoop) | Err(Skip::NotOurs) => Ok(()),
            Err(skip) => Err(skip.to_string()),
        }
    }

    // --- release and restore ---------------------------------------------------------------------

    /// This process's last door closed: the loop reads "goose was closed" and nobody owns it. A loop
    /// with a tick or a check in flight is released when that ends.
    async fn release(&self, session_id: &str) {
        let _op = self.inner.op.lock().await;
        {
            let state = self.state();
            if !state.doors.is_empty() {
                return;
            }
            let Some(mem) = state.loops.get(session_id) else {
                return;
            };
            if mem.running.is_some()
                || mem.finishing
                || mem.reserved.is_some()
                || mem.released.is_some()
            {
                return;
            }
        }
        let now = fmt_time(self.now());
        let me = self.me();
        let written = self
            .write(session_id, |current| {
                let mut rec = current.ok_or(Skip::NoLoop)?;
                if rec.owner != Some(me) || rec.status == LoopStatus::Ended {
                    return Err(Skip::NotOurs);
                }
                let saved = Released {
                    closed_at: now.clone(),
                    status: rec.status,
                    reason: rec.status_reason.clone(),
                    next_tick: rec.next_tick.clone(),
                    offer: rec.offer.clone(),
                };
                rec.owner = None;
                if matches!(rec.status, LoopStatus::Waiting | LoopStatus::WaitingTurn) {
                    rec.status = LoopStatus::Paused;
                    rec.status_reason = Some(LoopStatusReason::Closed {
                        closed_at: Some(now),
                    });
                    rec.offer = None;
                }
                Ok((rec, saved))
            })
            .await;
        match written {
            Ok((_, saved)) => {
                if let Some(mem) = self.state().loops.get_mut(session_id) {
                    mem.released = Some(saved);
                }
            }
            Err(Skip::NoLoop) | Err(Skip::NotOurs) => {
                self.state().loops.remove(session_id);
            }
            Err(skip) => tracing::error!(session_id, %skip, "loop: not released"),
        }
    }

    async fn release_all(&self) {
        let sessions: Vec<String> = self.state().loops.keys().cloned().collect();
        for session in sessions {
            self.release(&session).await;
        }
    }

    /// A door opened: restore what the last door's closing released (a reload), then re-send every
    /// open offer on the doors (§5.1).
    async fn door_opened(&self) {
        let released: Vec<(String, String)> = self
            .state()
            .loops
            .iter()
            .filter(|(_, m)| m.released.is_some())
            .map(|(s, m)| (s.clone(), m.loop_id.clone()))
            .collect();
        for (session, loop_id) in released {
            let _op = self.inner.op.lock().await;
            let Some(saved) = self
                .state()
                .loops
                .get_mut(&session)
                .and_then(|m| m.released.take())
            else {
                continue;
            };
            let me = self.me();
            let written = self
                .write(&session, |current| {
                    let mut rec = current.ok_or(Skip::NoLoop)?;
                    let untouched = rec.id == loop_id
                        && rec.owner.is_none()
                        && if matches!(saved.status, LoopStatus::Waiting | LoopStatus::WaitingTurn)
                        {
                            rec.status == LoopStatus::Paused
                                && rec.status_reason
                                    == Some(LoopStatusReason::Closed {
                                        closed_at: Some(saved.closed_at.clone()),
                                    })
                        } else {
                            rec.status == saved.status
                        };
                    if !untouched {
                        return Err(Skip::NotOurs);
                    }
                    rec.owner = Some(me);
                    rec.status = saved.status;
                    rec.status_reason = saved.reason;
                    rec.next_tick = saved.next_tick;
                    rec.offer = saved.offer;
                    Ok((rec, ()))
                })
                .await;
            match written {
                Ok((rec, ())) => self.emit(&session, &rec),
                Err(_) => {
                    self.state().loops.remove(&session);
                }
            }
        }
        let sessions: Vec<String> = self.state().loops.keys().cloned().collect();
        for session in sessions {
            if let Err(refusal) = self.ready_inner(&session).await {
                tracing::warn!(session, reason = %refusal.reason, "loop: not re-offered on the new door");
            }
        }
    }

    // --- events from the chat --------------------------------------------------------------------

    async fn needs_you_answered_or_dismissed(&self, session_id: &str, item_id: &str) {
        let _op = self.inner.op.lock().await;
        let session = match self
            .inner
            .deps
            .sessions
            .get_session(session_id, false)
            .await
        {
            Ok(session) => session,
            Err(error) => {
                tracing::warn!(session_id, %error, "loop: the chat of a resolved question could not be read");
                return;
            }
        };
        let status = match needs_you_items(&session) {
            Ok(items) => items
                .into_iter()
                .find(|i| i.id == item_id)
                .map(|i| i.status),
            Err(error) => {
                tracing::warn!(session_id, %error, "loop: the needs-you store could not be read");
                return;
            }
        };
        let Some(status) = status else { return };
        let now = self.now();
        let written = self
            .write(session_id, |current| {
                let mut rec = current.ok_or(Skip::NoLoop)?;
                let n = match (&rec.status, &rec.status_reason) {
                    (
                        LoopStatus::NeedsYou,
                        Some(LoopStatusReason::Asked {
                            n, item_id: asked, ..
                        }),
                    ) if asked == item_id => *n,
                    _ => return Err(Skip::Idle),
                };
                match status {
                    NeedsYouStatus::Answered => {
                        rec.status_reason = Some(LoopStatusReason::AnswerRunning { n });
                    }
                    NeedsYouStatus::Dismissed => {
                        rec.status = LoopStatus::Waiting;
                        rec.status_reason = None;
                        rec.next_tick = Some(next_now(now, LoopNextReason::AfterYourAnswer));
                    }
                    NeedsYouStatus::Open => return Err(Skip::Idle),
                }
                Ok((rec, ()))
            })
            .await;
        if let Ok((rec, ())) = written {
            self.emit(session_id, &rec);
        }
    }

    async fn answer_turn_ended(&self, session_id: &str) {
        // Every person's turn in every chat ends here. Only a loop waiting on its answer's turn
        // moves, so the chat is READ first (a WAL read never waits on a writer) and the write
        // transaction — which would hold the runner's lock while another writer holds the store —
        // is taken only for such a loop; the write re-checks under it.
        match record::read(&self.inner.deps.sessions, session_id).await {
            Ok(Ok(Some(rec)))
                if matches!(
                    (&rec.status, &rec.status_reason),
                    (
                        LoopStatus::NeedsYou,
                        Some(LoopStatusReason::AnswerRunning { .. })
                    )
                ) => {}
            Ok(Ok(_)) => return,
            Ok(Err(error)) => {
                tracing::warn!(session_id, %error, "loop: the record of a chat whose turn ended could not be read");
                return;
            }
            Err(error) => {
                tracing::warn!(session_id, %error, "loop: the chat whose turn ended could not be read");
                return;
            }
        }
        let _op = self.inner.op.lock().await;
        let now = self.now();
        let written = self
            .write(session_id, |current| {
                let mut rec = current.ok_or(Skip::NoLoop)?;
                if !matches!(
                    (&rec.status, &rec.status_reason),
                    (
                        LoopStatus::NeedsYou,
                        Some(LoopStatusReason::AnswerRunning { .. })
                    )
                ) {
                    return Err(Skip::Idle);
                }
                rec.status = LoopStatus::Waiting;
                rec.status_reason = None;
                rec.next_tick = Some(next_now(now, LoopNextReason::AfterYourAnswer));
                Ok((rec, ()))
            })
            .await;
        if let Ok((rec, ())) = written {
            self.emit(session_id, &rec);
        }
    }

    // --- the renderer's answers ------------------------------------------------------------------

    async fn ready_inner(&self, session_id: &str) -> Result<bool, LoopRefusal> {
        let _op = self.inner.op.lock().await;
        {
            let mut state = self.state();
            let Some(mem) = state.loops.get_mut(session_id) else {
                return Ok(false);
            };
            if mem.reserved.is_some() || mem.running.is_some() || mem.finishing {
                return Ok(false);
            }
            mem.reoffer_on_event = false;
        }
        let me = self.me();
        let written = self
            .decide_then_write(session_id, |current| {
                let mut rec = current.ok_or(Skip::NoLoop)?;
                if rec.owner != Some(me) {
                    return Err(Skip::NotOurs);
                }
                let offer = rec.offer.as_mut().ok_or(Skip::Idle)?;
                offer.refused = None;
                if matches!(
                    (&rec.status, &rec.status_reason),
                    (
                        LoopStatus::WaitingTurn,
                        Some(LoopStatusReason::Refused { .. })
                    )
                ) {
                    rec.status = LoopStatus::Waiting;
                    rec.status_reason = None;
                }
                Ok((rec, ()))
            })
            .await;
        match written {
            Ok((rec, ())) => {
                self.emit(session_id, &rec);
                self.send_offer(session_id, &rec).await;
                Ok(true)
            }
            Err(Skip::Idle) | Err(Skip::NoLoop) | Err(Skip::NotOurs) => Ok(false),
            Err(skip) => Err(to_refusal(skip)),
        }
    }
}

async fn handle(runner: &Runner, event: Event) {
    match event {
        Event::Evaluate => {}
        Event::DoorOpened => runner.door_opened().await,
        Event::DoorClosed => {
            if runner.state().doors.is_empty() {
                runner.release_all().await;
            }
        }
        Event::TickEnded {
            session,
            running,
            end,
        } => {
            let runner = runner.clone();
            tokio::spawn(async move { runner.finish_tick(session, running, end).await });
        }
        Event::NeedsYouResolved { session, item } => {
            runner
                .needs_you_answered_or_dismissed(&session, &item)
                .await
        }
        Event::UserTurnEnded { session } => runner.answer_turn_ended(&session).await,
    }
}

/// The runner's one waiting place: the earliest wall time any owned loop needs looking at, or the
/// next event — whichever comes first. Nothing polls.
async fn drive(inner: Weak<Inner>, mut events: mpsc::UnboundedReceiver<Event>) {
    loop {
        let Some(strong) = inner.upgrade() else {
            return;
        };
        let runner = Runner { inner: strong };
        let deadline = runner.evaluate_all().await;
        let sleep = deadline.map(|at| runner.inner.deps.clock.sleep_until(at));
        drop(runner);
        let event = match sleep {
            Some(sleep) => tokio::select! {
                event = events.recv() => event,
                _ = sleep => Some(Event::Evaluate),
            },
            None => events.recv().await,
        };
        let Some(event) = event else { return };
        let Some(strong) = inner.upgrade() else {
            return;
        };
        handle(&Runner { inner: strong }, event).await;
    }
}

#[cfg(not(windows))]
async fn login_shell_path() -> Option<String> {
    static PATH: tokio::sync::OnceCell<Option<String>> = tokio::sync::OnceCell::const_new();
    PATH.get_or_init(|| async {
        let resolved = tokio::task::spawn_blocking(
            crate::agents::platform_extensions::developer::shell::resolve_login_shell_path,
        )
        .await
        .ok()
        .flatten();
        if resolved.is_none() {
            tracing::warn!(
                "loop: the login shell's PATH could not be read; checks run with goose's own PATH"
            );
        }
        resolved
    })
    .await
    .clone()
}

#[cfg(windows)]
async fn login_shell_path() -> Option<String> {
    None
}

// ---------------------------------------------------------------------------------------------
// The seam's side (L0's `LoopRunner`)
// ---------------------------------------------------------------------------------------------

#[async_trait]
impl LoopRunner for Runner {
    async fn start(&self, session_id: &str, start: LoopEdit) -> Result<LoopRecord, LoopRefusal> {
        let _op = self.inner.op.lock().await;
        let now = self.now();
        let me = self.me();
        let written = self
            .write(session_id, |current| {
                if let Some(old) = &current {
                    if old.status != LoopStatus::Ended {
                        self.claim_check(old)?;
                    }
                }
                let mut rec = record::new_record(record::new_loop_id(), start, now);
                rec.owner = Some(me);
                rec.next_tick = Some(next_now(now, LoopNextReason::First));
                Ok((rec, ()))
            })
            .await;
        let rec = written.map_err(to_refusal)?.0;
        // A tick or check of the loop this one replaces stops here; its end records nothing.
        self.stop_running(session_id, CancelCause::LoopStopped);
        {
            let mut state = self.state();
            let mem = state.loops.entry(session_id.to_string()).or_default();
            mem.loop_id = rec.id.clone();
            mem.released = None;
            mem.reoffer_on_event = false;
            if let Some(stop) = mem.checking.take() {
                stop.cancel();
            }
        }
        self.emit(session_id, &rec);
        self.poke();
        Ok(rec)
    }

    async fn update(&self, session_id: &str, edit: LoopEdit) -> Result<LoopRecord, LoopRefusal> {
        let _op = self.inner.op.lock().await;
        let now = self.now();
        let reviewers_pending = self
            .state()
            .loops
            .get(session_id)
            .is_some_and(|m| m.reviewers_pending);
        let written = self
            .write(session_id, |current| {
                let mut rec = current.ok_or(Skip::NoLoop)?;
                if rec.status == LoopStatus::Ended {
                    return Err(refused("The loop has ended — start a new one."));
                }
                rec.goal = edit.goal;
                rec.template = edit.template;
                rec.steps = edit.steps;
                rec.cadence = edit.cadence;
                rec.state_file = edit.state_file;
                rec.check = edit.check;
                rec.stop_after_ticks = edit.stop_after_ticks;
                let rescheduled = rec.status == LoopStatus::Waiting
                    && rec.offer.is_none()
                    && rec.next_tick.as_ref().is_some_and(|next| {
                        matches!(
                            next.reason,
                            LoopNextReason::Cadence
                                | LoopNextReason::Overdue
                                | LoopNextReason::SelfPaced { .. }
                                | LoopNextReason::BackToBack
                        )
                    });
                if rescheduled {
                    match rules::next_tick(&rec, now, reviewers_pending) {
                        Ok(NextTickDecision::At { next }) => rec.next_tick = Some(next),
                        Ok(NextTickDecision::AfterReviewers { n }) => {
                            rec.status = LoopStatus::WaitingTurn;
                            rec.status_reason = Some(LoopStatusReason::Reviewers { n });
                            rec.next_tick = None;
                        }
                        Ok(NextTickDecision::WaitingYou { reason }) => {
                            rec.status = LoopStatus::WaitingYou;
                            rec.status_reason = Some(reason);
                            rec.next_tick = None;
                        }
                        Err(error) => return Err(refused(error)),
                    }
                }
                Ok((rec, ()))
            })
            .await;
        let rec = written.map_err(to_refusal)?.0;
        self.emit(session_id, &rec);
        self.poke();
        Ok(rec)
    }

    async fn control(
        &self,
        session_id: &str,
        action: LoopControlAction,
    ) -> Result<LoopRecord, LoopRefusal> {
        let _op = self.inner.op.lock().await;
        let now = self.now();
        let me = self.me();
        let (running_here, check_here) = {
            let state = self.state();
            let mem = state.loops.get(session_id);
            (
                mem.is_some_and(|m| m.running.is_some() || m.finishing || m.reserved.is_some()),
                mem.and_then(|m| m.checking.clone()),
            )
        };
        let written = match action {
            LoopControlAction::Pause => {
                self.write(session_id, |current| {
                    let mut rec = current.ok_or(Skip::NoLoop)?;
                    if rec.status == LoopStatus::Ended {
                        return Err(refused("The loop has ended."));
                    }
                    let n = last_n(&rec);
                    let elsewhere = matches!(
                        rec.owner.as_ref().map(|o| owner::prove(
                            o,
                            &me,
                            self.inner.deps.processes.as_ref()
                        )),
                        Some(OwnerProof::Live)
                    );
                    rec.status_reason = Some(
                        if elsewhere
                            && matches!(rec.status, LoopStatus::Running | LoopStatus::Checking)
                        {
                            LoopStatusReason::FinishingElsewhere { n }
                        } else {
                            LoopStatusReason::ByYou { after_tick: n }
                        },
                    );
                    rec.status = LoopStatus::Paused;
                    rec.offer = None;
                    rec.next_tick = None;
                    Ok((rec, ()))
                })
                .await
            }
            LoopControlAction::Stop => {
                self.write(session_id, |current| {
                    let mut rec = current.ok_or(Skip::NoLoop)?;
                    if rec.status == LoopStatus::Ended {
                        return Ok((rec, ()));
                    }
                    rec.status_reason = Some(LoopStatusReason::StoppedByYou { n: last_n(&rec) });
                    rec.status = LoopStatus::Ended;
                    rec.ended_at = Some(fmt_time(now));
                    rec.offer = None;
                    rec.next_tick = None;
                    Ok((rec, ()))
                })
                .await
            }
            LoopControlAction::Resume => {
                self.write(session_id, |current| {
                    let mut rec = current.ok_or(Skip::NoLoop)?;
                    if rec.status == LoopStatus::Ended {
                        return Err(refused("The loop has ended — start a new one."));
                    }
                    self.claim_check(&rec)?;
                    let was_mine = rec.owner == Some(me);
                    rec.owner = Some(me);
                    let keeps = matches!(rec.status, LoopStatus::NeedsYou | LoopStatus::WaitingYou)
                        || (was_mine
                            && matches!(
                                rec.status,
                                LoopStatus::Running
                                    | LoopStatus::Checking
                                    | LoopStatus::Waiting
                                    | LoopStatus::WaitingTurn
                            ));
                    if !keeps {
                        rec.status = LoopStatus::Waiting;
                        rec.status_reason = None;
                        rec.offer = None;
                        rec.next_tick = Some(next_now(now, LoopNextReason::Resume));
                    }
                    Ok((rec, ()))
                })
                .await
            }
            LoopControlAction::TickNow => {
                self.write(session_id, |current| {
                    let mut rec = current.ok_or(Skip::NoLoop)?;
                    if rec.status == LoopStatus::Ended {
                        return Err(refused("The loop has ended — start a new one."));
                    }
                    if running_here
                        || matches!(rec.status, LoopStatus::Running | LoopStatus::Checking)
                    {
                        return Err(refused("A tick is already running"));
                    }
                    self.claim_check(&rec)?;
                    rec.owner = Some(me);
                    rec.status = LoopStatus::Waiting;
                    rec.status_reason = None;
                    match rec.offer.as_mut() {
                        Some(offer) => offer.refused = None,
                        None => rec.next_tick = Some(next_now(now, LoopNextReason::Now)),
                    }
                    Ok((rec, ()))
                })
                .await
            }
            LoopControlAction::StopCheck => match &check_here {
                Some(_) => record::read(&self.inner.deps.sessions, session_id)
                    .await
                    .map_err(|e| Skip::Store(e.to_string()))
                    .and_then(|r| r.map_err(Skip::Store))
                    .and_then(|r| r.ok_or(Skip::NoLoop))
                    .map(|rec| (rec, ())),
                None => Err(refused("No check is running in this window.")),
            },
        };
        let rec = written.map_err(to_refusal)?.0;
        match action {
            LoopControlAction::Stop => {
                self.stop_running(session_id, CancelCause::LoopStopped);
                if let Some(stop) = check_here {
                    stop.cancel();
                }
            }
            LoopControlAction::StopCheck => {
                if let Some(stop) = check_here {
                    stop.cancel();
                }
            }
            LoopControlAction::Resume | LoopControlAction::TickNow => {
                self.ensure_mem(session_id, &rec.id);
            }
            LoopControlAction::Pause => {}
        }
        self.emit(session_id, &rec);
        if action == LoopControlAction::TickNow && rec.offer.is_some() {
            drop(_op);
            self.ready_inner(session_id).await?;
        }
        self.poke();
        Ok(rec)
    }

    async fn tick_refused(&self, refusal: LoopsTickRefusedRequest) -> Result<(), LoopRefusal> {
        let _op = self.inner.op.lock().await;
        let session_id = refusal.session_id.clone();
        let submit_failed = matches!(refusal.reason, LoopRefuseReason::SubmitFailed { .. });
        let written = self
            .write(&session_id, |current| {
                let mut rec = current.ok_or(Skip::NoLoop)?;
                if rec.id != refusal.loop_id {
                    return Err(Skip::Idle);
                }
                let offer = rec.offer.as_mut().ok_or(Skip::Idle)?;
                if offer.n != refusal.n {
                    return Err(Skip::Idle);
                }
                offer.refused = Some(refusal.reason.clone());
                rec.status = LoopStatus::WaitingTurn;
                rec.status_reason = Some(LoopStatusReason::Refused {
                    refused: refusal.reason,
                });
                Ok((rec, ()))
            })
            .await;
        match written {
            Ok((rec, ())) => {
                if submit_failed {
                    if let Some(mem) = self.state().loops.get_mut(&session_id) {
                        mem.reoffer_on_event = true;
                    }
                    if self.inner.deps.turns.running().running > 0 {
                        self.ensure_idle_waiter();
                    }
                }
                self.emit(&session_id, &rec);
                Ok(())
            }
            // A refusal of an offer no longer open — the tick started and failed before any reply,
            // which the window cannot tell from a refusal — changes nothing.
            Err(Skip::Idle) | Err(Skip::NoLoop) | Err(Skip::NotOurs) => Ok(()),
            Err(skip) => Err(to_refusal(skip)),
        }
    }

    async fn ready(&self, session_id: &str) -> Result<bool, LoopRefusal> {
        self.ready_inner(session_id).await
    }

    async fn wake(&self) -> Result<u32, LoopRefusal> {
        let _op = self.inner.op.lock().await;
        let now = self.now();
        let me = self.me();
        let sessions: Vec<String> = self.state().loops.keys().cloned().collect();
        let mut rearmed = 0;
        for session in sessions {
            let written = self
                .write(&session, |current| {
                    let mut rec = current.ok_or(Skip::NoLoop)?;
                    if rec.owner != Some(me)
                        || rec.status != LoopStatus::Waiting
                        || rec.offer.is_some()
                    {
                        return Err(Skip::Idle);
                    }
                    let next = rec.next_tick.as_mut().ok_or(Skip::Idle)?;
                    let at = parse_time(&next.at).map_err(Skip::Store)?;
                    if at > now {
                        return Err(Skip::Idle);
                    }
                    next.reason = LoopNextReason::OnWake;
                    Ok((rec, ()))
                })
                .await;
            if let Ok((rec, ())) = written {
                rearmed += 1;
                self.emit(&session, &rec);
            }
        }
        self.poke();
        Ok(rearmed)
    }

    fn prove_owner(&self, owner: &LoopOwner) -> OwnerProof {
        owner::prove(owner, &self.me(), self.inner.deps.processes.as_ref())
    }

    fn needs_you_resolved(&self, session_id: &str, item_id: &str) {
        self.send(Event::NeedsYouResolved {
            session: session_id.to_string(),
            item: item_id.to_string(),
        });
    }
}

// ---------------------------------------------------------------------------------------------
// This process's runner
// ---------------------------------------------------------------------------------------------

static RUNNER: OnceLock<Runner> = OnceLock::new();

/// Build this process's runner and install it behind L0's seam (L2b calls this once goosed's
/// runtime runs). Idempotent.
pub fn install() -> Result<Runner, String> {
    if let Some(runner) = RUNNER.get() {
        return Ok(runner.clone());
    }
    let processes: Arc<dyn ProcessTable> = Arc::new(SystemProcesses);
    let me = owner::this_process(processes.as_ref())?;
    let runner = RUNNER
        .get_or_init(|| {
            let sessions = Arc::new(SessionManager::instance());
            Runner::new(RunnerDeps {
                sessions: sessions.clone(),
                clock: Arc::new(SystemClock),
                processes,
                me,
                turns: turn_priority::global(),
                mac: Arc::new(mac_wide::Holders::installed(sessions)),
                logs_dir: crate::config::paths::Paths::in_data_dir("loops"),
                check_path: CheckPath::LoginShell,
            })
        })
        .clone();
    seam::install_runner(Arc::new(runner.clone()));
    Ok(runner)
}

pub fn installed() -> Option<Runner> {
    RUNNER.get().cloned()
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
