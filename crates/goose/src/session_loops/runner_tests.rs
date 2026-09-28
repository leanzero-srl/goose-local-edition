//! The runner (L2a) over a real session store, with a fake door, a fake clock fed as values and a
//! fake process table — plus the parts only real processes can prove: the owner proof by pid,
//! start time and parent, the two-process claim race, and the check's exit semantics.

use std::sync::Mutex as StdMutex;

use chrono::Duration;
use goose_sdk_types::custom_requests::{
    LoopCadence, LoopReport, LoopTemplateId, LoopVerdict, LoopsTickRefusedRequest,
};
use tokio::sync::watch;

use super::*;
use crate::needs_you::{self, NewQuestion, Resolution};
use crate::session::SessionType;

const ME: LoopOwner = LoopOwner {
    goosed_pid: 4242,
    goosed_started_at: 1,
    app_pid: 4241,
};

// ---------------------------------------------------------------------------------------------
// The bed
// ---------------------------------------------------------------------------------------------

/// Wall time and tokio's timer, apart: a Mac that sleeps moves the wall and not the timer.
struct FakeClock {
    wall: watch::Sender<DateTime<Utc>>,
    timer: watch::Sender<DateTime<Utc>>,
}

impl FakeClock {
    fn new() -> Arc<Self> {
        let now = parse_time(&fmt_time(Utc::now())).unwrap();
        Arc::new(Self {
            wall: watch::channel(now).0,
            timer: watch::channel(now).0,
        })
    }

    fn advance(&self, by: Duration) {
        self.wall.send_modify(|t| *t += by);
        self.timer.send_modify(|t| *t += by);
    }

    fn sleep_through(&self, by: Duration) {
        self.wall.send_modify(|t| *t += by);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> {
        *self.wall.borrow()
    }

    fn sleep_until(&self, at: DateTime<Utc>) -> BoxFuture<'static, ()> {
        let mut timer = self.timer.subscribe();
        Box::pin(async move {
            let _ = timer.wait_for(|t| *t >= at).await;
        })
    }
}

#[derive(Default)]
struct FakeDoor {
    dues: StdMutex<Vec<LoopsTickDueNotification>>,
}

impl FakeDoor {
    fn dues(&self) -> Vec<LoopsTickDueNotification> {
        self.dues.lock().unwrap().clone()
    }
}

impl TickDoor for FakeDoor {
    fn tick_due(&self, due: &LoopsTickDueNotification) {
        self.dues.lock().unwrap().push(due.clone());
    }

    fn changed(&self, _changed: &LoopsChangedNotification) {}
}

/// pid → (alive, parent); an unknown pid is dead.
#[derive(Default)]
struct FakeProcesses {
    known: HashMap<u32, (u64, u32)>,
}

impl ProcessTable for FakeProcesses {
    fn alive(&self, pid: u32, started_at: u64) -> Result<(), String> {
        match self.known.get(&pid) {
            Some((started, _)) if *started == started_at => Ok(()),
            Some(_) => Err(format!("pid {pid} is now another process")),
            None => Err(format!("no process has pid {pid}")),
        }
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        self.known.get(&pid).map(|(_, parent)| *parent)
    }
}

struct Bed {
    dir: tempfile::TempDir,
    sessions: Arc<SessionManager>,
    session: String,
    other: String,
    clock: Arc<FakeClock>,
    door: Arc<FakeDoor>,
    guard: Option<DoorGuard>,
    runner: Runner,
    turns: &'static TurnPriority,
}

async fn bed_with(processes: FakeProcesses) -> Bed {
    let dir = tempfile::tempdir().unwrap();
    let sessions = Arc::new(SessionManager::new(dir.path().join("data")));
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let create = |name: &str| {
        let (sessions, work, name) = (sessions.clone(), work.clone(), name.to_string());
        async move {
            sessions
                .create_session(
                    work,
                    name,
                    SessionType::User,
                    crate::config::GooseMode::default(),
                )
                .await
                .unwrap()
                .id
        }
    };
    let session = create("loop chat").await;
    let other = create("Kickoff notes").await;
    let clock = FakeClock::new();
    let turns: &'static TurnPriority = Box::leak(Box::new(TurnPriority::new()));
    let runner = Runner::new(RunnerDeps {
        sessions: sessions.clone(),
        clock: clock.clone(),
        processes: Arc::new(processes),
        me: ME,
        turns,
        logs_dir: dir.path().join("logs"),
        check_path: CheckPath::Inherited,
    });
    let door = Arc::new(FakeDoor::default());
    let guard = Some(runner.register_door(door.clone()));
    Bed {
        dir,
        sessions,
        session,
        other,
        clock,
        door,
        guard,
        runner,
        turns,
    }
}

async fn bed() -> Bed {
    bed_with(FakeProcesses::default()).await
}

fn edit(cadence: LoopCadence, check: Option<&str>) -> LoopEdit {
    LoopEdit {
        goal: "Make every test pass".into(),
        template: LoopTemplateId::Blank,
        steps: String::new(),
        cadence,
        state_file: ".goose/loops/x/NOW.md".into(),
        check: check.map(str::to_string),
        stop_after_ticks: None,
    }
}

fn every(text: &str) -> LoopCadence {
    LoopCadence::Every { every: text.into() }
}

fn report(verdict: LoopVerdict, next_step: &str, next_in: Option<&str>) -> LoopReport {
    LoopReport {
        verdict,
        summary: "Added the svc- accounts; scripts/generate_users.js:40".into(),
        next_step: next_step.into(),
        next_in: next_in.map(str::to_string),
        next_reason: next_in.map(|_| "the build takes that long".to_string()),
        blocked_on: None,
    }
}

fn meta(due: &LoopsTickDueNotification) -> TickMeta {
    TickMeta {
        loop_id: due.loop_id.clone(),
        n: due.n,
        message_id: due.message_id.clone(),
    }
}

async fn settle() {
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
}

impl Bed {
    async fn start(&self, edit: LoopEdit) -> LoopRecord {
        self.runner.start(&self.session, edit).await.unwrap()
    }

    async fn record(&self) -> LoopRecord {
        record::read(&self.sessions, &self.session)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }

    async fn until(&self, what: &str, holds: impl Fn(&LoopRecord) -> bool) -> LoopRecord {
        for _ in 0..1000 {
            let rec = self.record().await;
            if holds(&rec) {
                return rec;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("never: {what}\n{:#?}", self.record().await);
    }

    async fn due(&self, count: usize) -> LoopsTickDueNotification {
        due_on(&self.door, count).await
    }

    async fn begin(&self, due: &LoopsTickDueNotification) -> (TickRun, CancellationToken) {
        let reservation = self
            .runner
            .accept_offer(&self.session, &meta(due))
            .await
            .expect("the open offer is accepted");
        let cancel = CancellationToken::new();
        let run = reservation
            .started(TickTicket {
                cancel: cancel.clone(),
                cause: Arc::new(OnceLock::new()),
                context_window_tokens: 262_144,
            })
            .await
            .unwrap();
        (run, cancel)
    }

    /// What L3's `loop_report` writes: the report on the running tick.
    async fn report(&self, report: LoopReport) {
        record::update(&self.sessions, &self.session, |rec| {
            let mut rec = rec.unwrap();
            rec.ticks.last_mut().unwrap().report = Some(report);
            Ok((rec, ()))
        })
        .await
        .unwrap();
    }

    /// One whole tick: accepted, reported, ended with no reviewers.
    async fn tick(&self, due: &LoopsTickDueNotification, report: Option<LoopReport>) {
        let (run, _) = self.begin(due).await;
        if let Some(report) = report {
            self.report(report).await;
        }
        drop(run.ended(TickEnd::Completed));
    }
}

async fn due_on(door: &FakeDoor, count: usize) -> LoopsTickDueNotification {
    for _ in 0..1000 {
        let dues = door.dues();
        if dues.len() >= count {
            return dues[count - 1].clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("offer {count} never came: {:#?}", door.dues());
}

// ---------------------------------------------------------------------------------------------
// The offer protocol
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_first_tick_is_offered_on_start_and_accepted_once() {
    let bed = bed().await;
    let rec = bed.start(edit(every("10m"), None)).await;
    assert_eq!(rec.owner, Some(ME), "start claims the clock");
    let due = bed.due(1).await;
    assert_eq!((due.n, due.loop_id.as_str()), (1, rec.id.as_str()));
    assert!(due
        .prompt
        .starts_with("Loop tick 1 — \"Make every test pass\""));
    assert_eq!(
        rules::parse_tick_id(&due.message_id).unwrap().loop_id,
        rec.id
    );
    assert_eq!(
        bed.record().await.status,
        LoopStatus::Waiting,
        "an offer is not a status"
    );

    let forged = TickMeta { n: 2, ..meta(&due) };
    assert!(bed
        .runner
        .accept_offer(&bed.session, &forged)
        .await
        .is_none());
    let reserved = bed.runner.accept_offer(&bed.session, &meta(&due)).await;
    assert!(reserved.is_some());
    assert!(
        bed.runner
            .accept_offer(&bed.session, &meta(&due))
            .await
            .is_none(),
        "a reserved offer is not accepted twice"
    );
    drop(reserved);
    let (run, _) = bed.begin(&due).await;
    assert!(
        bed.runner
            .accept_offer(&bed.session, &meta(&due))
            .await
            .is_none(),
        "a started tick's offer is never accepted again"
    );
    let rec = bed.record().await;
    assert_eq!(rec.status, LoopStatus::Running);
    assert_eq!(rec.offer, None);
    assert_eq!(rec.ticks[0].origin, LoopTickOrigin::First);
    assert_eq!(rec.ticks[0].first_message_id, due.message_id);

    let started = parse_time(&rec.ticks[0].started_at).unwrap();
    bed.report(report(LoopVerdict::Progress, "add svc- accounts", None))
        .await;
    drop(run.ended(TickEnd::Completed));
    let rec = bed
        .until("tick 1 recorded", |r| r.ticks[0].outcome.is_some())
        .await;
    assert_eq!(rec.ticks[0].outcome, Some(LoopTickOutcome::Progress));
    assert_eq!(rec.status, LoopStatus::Waiting);
    let next = rec.next_tick.unwrap();
    assert_eq!(next.reason, LoopNextReason::Cadence);
    assert_eq!(
        parse_time(&next.at).unwrap(),
        started + Duration::minutes(10)
    );
    settle().await;
    assert_eq!(
        bed.door.dues().len(),
        1,
        "no second offer before the cadence"
    );
    bed.clock.advance(Duration::minutes(10));
    assert_eq!(bed.due(2).await.n, 2);
}

#[tokio::test]
async fn an_offer_re_sent_on_a_new_door_is_accepted_once() {
    let bed = bed().await;
    bed.start(edit(every("10m"), None)).await;
    let first = bed.due(1).await;
    let door2 = Arc::new(FakeDoor::default());
    let _guard2 = bed.runner.register_door(door2.clone());
    let again = due_on(&door2, 1).await;
    assert_eq!(
        again.message_id, first.message_id,
        "the same offer, re-sent"
    );
    let (_run, _) = bed.begin(&again).await;
    assert!(bed
        .runner
        .accept_offer(&bed.session, &meta(&first))
        .await
        .is_none());
    assert_eq!(bed.record().await.ticks.len(), 1);
}

#[tokio::test]
async fn a_refused_offer_waits_for_ready_and_never_for_a_timer() {
    let bed = bed().await;
    bed.start(edit(every("10m"), None)).await;
    let due = bed.due(1).await;
    bed.runner
        .tick_refused(LoopsTickRefusedRequest {
            session_id: bed.session.clone(),
            loop_id: due.loop_id.clone(),
            n: 1,
            reason: LoopRefuseReason::TurnRunning,
        })
        .await
        .unwrap();
    let rec = bed.record().await;
    assert_eq!(rec.status, LoopStatus::WaitingTurn);
    assert_eq!(
        rec.status_reason,
        Some(LoopStatusReason::Refused {
            refused: LoopRefuseReason::TurnRunning
        })
    );
    bed.clock.advance(Duration::hours(5));
    settle().await;
    assert_eq!(
        bed.door.dues().len(),
        1,
        "a refusal is never re-offered on a timer"
    );

    assert!(bed.runner.ready(&bed.session).await.unwrap());
    let again = bed.due(2).await;
    assert_eq!(again.message_id, due.message_id);
    assert_eq!(bed.record().await.status, LoopStatus::Waiting);

    // A refusal of an offer no longer open (the tick started, then failed before any reply) is
    // ignored.
    let (_run, _) = bed.begin(&again).await;
    bed.runner
        .tick_refused(LoopsTickRefusedRequest {
            session_id: bed.session.clone(),
            loop_id: due.loop_id.clone(),
            n: 1,
            reason: LoopRefuseReason::SubmitFailed {
                error: "session is busy in another run".into(),
            },
        })
        .await
        .unwrap();
    assert_eq!(bed.record().await.status, LoopStatus::Running);
}

#[tokio::test]
async fn a_submit_failed_refusal_is_re_offered_when_a_user_turn_ends() {
    let bed = bed().await;
    bed.start(edit(every("10m"), None)).await;
    let due = bed.due(1).await;
    let turn = bed.turns.user_turn_in(&bed.other);
    bed.runner
        .tick_refused(LoopsTickRefusedRequest {
            session_id: bed.session.clone(),
            loop_id: due.loop_id.clone(),
            n: 1,
            reason: LoopRefuseReason::SubmitFailed {
                error: "session is busy in another run".into(),
            },
        })
        .await
        .unwrap();
    settle().await;
    assert_eq!(bed.door.dues().len(), 1);
    drop(turn);
    assert_eq!(bed.due(2).await.message_id, due.message_id);
}

// ---------------------------------------------------------------------------------------------
// The user's turn wins
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn no_offer_while_a_user_turn_runs_and_the_tick_follows_that_turn() {
    let bed = bed().await;
    let turn = bed.turns.user_turn_in(&bed.other);
    bed.start(edit(every("10m"), None)).await;
    let rec = bed
        .until("held for the turn", |r| r.status == LoopStatus::WaitingTurn)
        .await;
    assert_eq!(
        rec.status_reason,
        Some(LoopStatusReason::UserTurn {
            session_id: bed.other.clone(),
            chat: "Kickoff notes".into()
        })
    );
    settle().await;
    assert!(
        bed.door.dues().is_empty(),
        "no offer while a user turn runs"
    );
    drop(turn);
    let due = bed.due(1).await;
    assert_eq!(
        bed.record().await.next_tick.unwrap().reason,
        LoopNextReason::AfterYourTurn
    );
    let (_run, _) = bed.begin(&due).await;
    assert_eq!(
        bed.record().await.ticks[0].origin,
        LoopTickOrigin::AfterYourTurn
    );
}

#[tokio::test]
async fn a_user_turn_mid_tick_yields_it_and_the_next_tick_follows_that_turn() {
    let bed = bed().await;
    bed.start(edit(every("10m"), None)).await;
    let due = bed.due(1).await;
    let (run, cancel) = bed.begin(&due).await;
    let turn = bed.turns.user_turn_in(&bed.other);
    for _ in 0..1000 {
        if cancel.is_cancelled() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(cancel.is_cancelled(), "the user's turn cancels the tick");
    // on_prompt reports the cancel; the runner reads the cause from its own cell.
    drop(run.ended(TickEnd::Cancelled { cause: None }));
    let rec = bed
        .until("tick 1 recorded", |r| r.ticks[0].outcome.is_some())
        .await;
    assert_eq!(
        rec.ticks[0].outcome,
        Some(LoopTickOutcome::Yielded {
            to_session: bed.other.clone(),
            to_chat: "Kickoff notes".into(),
            way: None
        }),
        "a yield is recorded yielded, never as the user's stop"
    );
    assert_eq!(rec.status, LoopStatus::WaitingTurn);
    settle().await;
    assert_eq!(bed.door.dues().len(), 1);
    drop(turn);
    let second = bed.due(2).await;
    assert!(
        second.prompt.contains("Tick 1 was stopped at")
            && second
                .prompt
                .contains("for the user's turn in \"Kickoff notes\""),
        "{}",
        second.prompt
    );
    let (_run, _) = bed.begin(&second).await;
    assert_eq!(
        bed.record().await.ticks[1].origin,
        LoopTickOrigin::AfterYourTurn
    );
}

// ---------------------------------------------------------------------------------------------
// A tick that asks, and the reviewers
// ---------------------------------------------------------------------------------------------

async fn a_tick_that_asks(bed: &Bed) -> String {
    bed.start(edit(every("10m"), None)).await;
    let due = bed.due(1).await;
    let (run, _) = bed.begin(&due).await;
    let item = needs_you::raise(
        &bed.sessions,
        &bed.session,
        NewQuestion {
            question: "Which CSV delimiter does the owner want?".into(),
            why: "The generator and the validator must agree.".into(),
            recommended_answer: "A comma".into(),
            options: vec![],
        },
    )
    .await
    .unwrap();
    drop(run.ended(TickEnd::Completed));
    let rec = bed
        .until("tick 1 recorded", |r| r.ticks[0].outcome.is_some())
        .await;
    assert_eq!(
        rec.ticks[0].outcome,
        Some(LoopTickOutcome::Asked {
            item_id: item.id.clone(),
            question: "Which CSV delimiter does the owner want?".into()
        })
    );
    assert_eq!(rec.status, LoopStatus::NeedsYou);
    bed.clock.advance(Duration::hours(1));
    settle().await;
    assert_eq!(
        bed.door.dues().len(),
        1,
        "no tick fires under an open question"
    );
    item.id
}

#[tokio::test]
async fn an_answered_question_holds_the_loop_until_the_answers_turn_ends() {
    let bed = bed().await;
    let item = a_tick_that_asks(&bed).await;
    needs_you::resolve(
        &bed.sessions,
        &bed.session,
        &item,
        Resolution::Answered("A semicolon".into()),
    )
    .await
    .unwrap();
    bed.runner.needs_you_resolved(&bed.session, &item);
    bed.until("the answer's turn runs", |r| {
        r.status_reason == Some(LoopStatusReason::AnswerRunning { n: 1 })
    })
    .await;
    settle().await;
    assert_eq!(bed.door.dues().len(), 1);
    bed.runner.user_turn_ended(&bed.session);
    let due = bed.due(2).await;
    assert!(
        due.prompt.contains("they answered: \"A semicolon\""),
        "{}",
        due.prompt
    );
    let (_run, _) = bed.begin(&due).await;
    assert_eq!(
        bed.record().await.ticks[1].origin,
        LoopTickOrigin::AfterYourAnswer
    );
}

#[tokio::test]
async fn a_dismissed_question_lets_the_next_tick_come_at_once() {
    let bed = bed().await;
    let item = a_tick_that_asks(&bed).await;
    needs_you::resolve(&bed.sessions, &bed.session, &item, Resolution::Dismissed)
        .await
        .unwrap();
    bed.runner.needs_you_resolved(&bed.session, &item);
    let due = bed.due(2).await;
    assert!(due.prompt.contains("they dismissed it"), "{}", due.prompt);
}

#[tokio::test]
async fn the_next_tick_waits_for_the_previous_ticks_reviewers() {
    let bed = bed().await;
    bed.start(edit(LoopCadence::BackToBack, None)).await;
    let due = bed.due(1).await;
    let (run, _) = bed.begin(&due).await;
    bed.report(report(LoopVerdict::Progress, "add svc- accounts", None))
        .await;
    let (done, reviewing) = tokio::sync::oneshot::channel::<()>();
    let reviewer = tokio::spawn(async move {
        let _ = reviewing.await;
    });
    run.ended(TickEnd::Completed).reviewers(vec![reviewer]);
    let rec = bed
        .until("held for the reviewers", |r| {
            r.status_reason == Some(LoopStatusReason::Reviewers { n: 1 })
        })
        .await;
    assert_eq!(rec.status, LoopStatus::WaitingTurn);
    settle().await;
    assert_eq!(bed.door.dues().len(), 1, "no tick beside the reviewers");
    done.send(()).unwrap();
    let second = bed.due(2).await;
    assert_eq!(second.n, 2);
    assert_eq!(
        bed.record().await.next_tick.unwrap().reason,
        LoopNextReason::BackToBack
    );
}

// ---------------------------------------------------------------------------------------------
// Stops, pauses and self-pacing
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn stop_loop_mid_tick_cancels_it_and_ends_the_loop() {
    let bed = bed().await;
    bed.start(edit(every("10m"), None)).await;
    let due = bed.due(1).await;
    let (run, cancel) = bed.begin(&due).await;
    let rec = bed
        .runner
        .control(&bed.session, LoopControlAction::Stop)
        .await
        .unwrap();
    assert_eq!(rec.status, LoopStatus::Ended);
    assert!(cancel.is_cancelled());
    drop(run.ended(TickEnd::Cancelled { cause: None }));
    let rec = bed
        .until("tick 1 recorded", |r| r.ticks[0].outcome.is_some())
        .await;
    assert_eq!(rec.ticks[0].outcome, Some(LoopTickOutcome::StoppedByYou));
    assert_eq!(rec.status, LoopStatus::Ended);
    assert_eq!(
        rec.status_reason,
        Some(LoopStatusReason::StoppedByYou { n: 1 })
    );
}

#[tokio::test]
async fn a_pause_during_a_tick_stands_when_the_tick_ends() {
    let bed = bed().await;
    bed.start(edit(every("10m"), None)).await;
    let due = bed.due(1).await;
    let (run, cancel) = bed.begin(&due).await;
    bed.runner
        .control(&bed.session, LoopControlAction::Pause)
        .await
        .unwrap();
    assert!(!cancel.is_cancelled(), "pause lets the running tick finish");
    bed.report(report(LoopVerdict::Progress, "next", None))
        .await;
    drop(run.ended(TickEnd::Completed));
    let rec = bed
        .until("tick 1 recorded", |r| r.ticks[0].outcome.is_some())
        .await;
    assert_eq!(rec.status, LoopStatus::Paused);
    assert_eq!(
        rec.status_reason,
        Some(LoopStatusReason::ByYou { after_tick: 1 })
    );
    bed.clock.advance(Duration::hours(1));
    settle().await;
    assert_eq!(bed.door.dues().len(), 1);
    bed.runner
        .control(&bed.session, LoopControlAction::Resume)
        .await
        .unwrap();
    let due = bed.due(2).await;
    let (_run, _) = bed.begin(&due).await;
    assert_eq!(bed.record().await.ticks[1].origin, LoopTickOrigin::Resume);
}

#[tokio::test]
async fn a_tick_dropped_without_an_end_is_recorded_failed_never_nothing() {
    let bed = bed().await;
    bed.start(edit(every("10m"), None)).await;
    let due = bed.due(1).await;
    let (run, _) = bed.begin(&due).await;
    drop(run);
    let rec = bed
        .until("tick 1 recorded", |r| r.ticks[0].outcome.is_some())
        .await;
    assert!(matches!(
        rec.ticks[0].outcome,
        Some(LoopTickOutcome::Failed { ref error_class, .. }) if error_class == "tick_lost"
    ));
}

#[tokio::test]
async fn a_self_paced_tick_that_names_no_delay_waits_for_you() {
    let bed = bed().await;
    bed.start(edit(LoopCadence::SelfPaced, None)).await;
    let due = bed.due(1).await;
    bed.tick(&due, Some(report(LoopVerdict::Progress, "next", None)))
        .await;
    let rec = bed
        .until("tick 1 recorded", |r| r.ticks[0].outcome.is_some())
        .await;
    assert_eq!(rec.status, LoopStatus::WaitingYou);
    assert_eq!(rec.status_reason, Some(LoopStatusReason::NoDelay { n: 1 }));
    bed.clock.advance(Duration::hours(3));
    settle().await;
    assert_eq!(bed.door.dues().len(), 1, "no default delay");
}

#[tokio::test]
async fn a_self_paced_tick_comes_back_when_it_said() {
    let bed = bed().await;
    bed.start(edit(LoopCadence::SelfPaced, None)).await;
    let due = bed.due(1).await;
    bed.tick(
        &due,
        Some(report(LoopVerdict::Progress, "next", Some("20m"))),
    )
    .await;
    let rec = bed
        .until("tick 1 recorded", |r| r.ticks[0].outcome.is_some())
        .await;
    let ended = parse_time(rec.ticks[0].ended_at.as_ref().unwrap()).unwrap();
    let next = rec.next_tick.unwrap();
    assert_eq!(parse_time(&next.at).unwrap(), ended + Duration::minutes(20));
    bed.clock.advance(Duration::minutes(19));
    settle().await;
    assert_eq!(bed.door.dues().len(), 1);
    bed.clock.advance(Duration::minutes(1));
    assert_eq!(bed.due(2).await.n, 2);
}

#[tokio::test]
async fn after_the_mac_slept_through_three_cadences_one_tick_runs_on_wake() {
    let bed = bed().await;
    bed.start(edit(every("10m"), None)).await;
    let due = bed.due(1).await;
    bed.tick(&due, Some(report(LoopVerdict::Progress, "next", None)))
        .await;
    bed.until("tick 1 recorded", |r| r.ticks[0].outcome.is_some())
        .await;
    bed.clock.sleep_through(Duration::minutes(35));
    settle().await;
    assert_eq!(
        bed.door.dues().len(),
        1,
        "tokio's timer did not run in the sleep"
    );
    assert_eq!(bed.runner.wake().await.unwrap(), 1);
    let due = bed.due(2).await;
    settle().await;
    assert_eq!(bed.door.dues().len(), 2, "one tick on wake, never a burst");
    let (_run, _) = bed.begin(&due).await;
    assert_eq!(bed.record().await.ticks[1].origin, LoopTickOrigin::OnWake);
}

// ---------------------------------------------------------------------------------------------
// Doors and owners
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_last_door_closing_pauses_every_owned_loop_and_a_reload_restores_it() {
    let mut bed = bed().await;
    bed.start(edit(every("10m"), None)).await;
    let due = bed.due(1).await;
    drop(bed.guard.take());
    let rec = bed
        .until("released", |r| r.status == LoopStatus::Paused)
        .await;
    assert_eq!(rec.owner, None);
    assert!(matches!(
        rec.status_reason,
        Some(LoopStatusReason::Closed { closed_at: Some(_) })
    ));
    assert_eq!(
        rules::effective_status(&rec, None).0,
        LoopStatus::Paused,
        "a read shows it closed"
    );

    let door2 = Arc::new(FakeDoor::default());
    let _guard2 = bed.runner.register_door(door2.clone());
    let rec = bed.until("restored", |r| r.owner == Some(ME)).await;
    assert_eq!(rec.status, LoopStatus::Waiting);
    assert_eq!(
        due_on(&door2, 1).await.message_id,
        due.message_id,
        "the open offer goes to the new door"
    );
}

#[tokio::test]
async fn a_read_of_a_loop_whose_owner_is_gone_derives_closed_and_writes_nothing() {
    let bed = bed().await;
    record::update(&bed.sessions, &bed.session, |_| {
        let mut rec = record::new_record(
            "lp_0a1b2c3d".into(),
            edit(every("10m"), None),
            bed.clock.now(),
        );
        rec.owner = Some(LoopOwner {
            goosed_pid: 999_999,
            goosed_started_at: 1,
            app_pid: 999_998,
        });
        Ok((rec, ()))
    })
    .await
    .unwrap();
    let before = bed.record().await;
    let proof = bed.runner.prove_owner(before.owner.as_ref().unwrap());
    assert!(matches!(proof, OwnerProof::Gone { .. }), "{proof:?}");
    let (status, reason) = rules::effective_status(&before, Some(&proof));
    assert_eq!(status, LoopStatus::Paused);
    assert_eq!(reason, Some(LoopStatusReason::Closed { closed_at: None }));
    assert_eq!(bed.record().await, before, "a read writes nothing");
}

#[tokio::test]
async fn start_refuses_a_loop_another_live_goose_runs() {
    let mut processes = FakeProcesses::default();
    processes.known.insert(777, (5, 776));
    let bed = bed_with(processes).await;
    record::update(&bed.sessions, &bed.session, |_| {
        let mut rec = record::new_record(
            "lp_0a1b2c3d".into(),
            edit(every("10m"), None),
            bed.clock.now(),
        );
        rec.owner = Some(LoopOwner {
            goosed_pid: 777,
            goosed_started_at: 5,
            app_pid: 776,
        });
        Ok((rec, ()))
    })
    .await
    .unwrap();
    let refusal = bed
        .runner
        .start(&bed.session, edit(every("5m"), None))
        .await
        .unwrap_err();
    assert!(
        refusal.reason.contains("another goose window"),
        "{refusal:?}"
    );
    let refusal = bed
        .runner
        .control(&bed.session, LoopControlAction::TickNow)
        .await
        .unwrap_err();
    assert!(
        refusal.reason.contains("another goose window"),
        "{refusal:?}"
    );
    assert_eq!(bed.record().await.id, "lp_0a1b2c3d");
}

#[cfg(unix)]
#[test]
fn owners_dead_reused_and_reparented_are_proven_gone() {
    use std::io::Read;
    let sys = SystemProcesses;
    let me = owner::this_process(&sys).unwrap();
    assert_eq!(me.goosed_pid, std::process::id());
    assert_eq!(owner::prove(&me, &me, &sys), OwnerProof::ThisProcess);

    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = child.id();
    let (started, _) = goose_sidecar::machine::process_start(pid).unwrap();
    let live = LoopOwner {
        goosed_pid: pid,
        goosed_started_at: started,
        app_pid: std::process::id(),
    };
    assert_eq!(sys.parent(pid), Some(std::process::id()), "the ppid read");
    assert_eq!(owner::prove(&live, &me, &sys), OwnerProof::Live);
    let reused = LoopOwner {
        goosed_started_at: started - 1,
        ..live
    };
    assert!(
        matches!(owner::prove(&reused, &me, &sys), OwnerProof::Gone { ref why } if why.contains("another process")),
        "a reused pid"
    );
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(matches!(
        owner::prove(&live, &me, &sys),
        OwnerProof::Gone { .. }
    ));

    // The Q-223 shape: the parent ("the app") exits and leaves its child running, reparented.
    let mut app = std::process::Command::new("sh")
        .args(["-c", "sleep 30 >/dev/null 2>&1 & echo $!"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let app_pid = app.id();
    let mut out = String::new();
    app.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    app.wait().unwrap();
    let orphan: u32 = out.trim().parse().unwrap();
    let (orphan_started, _) = goose_sidecar::machine::process_start(orphan).unwrap();
    let orphaned = LoopOwner {
        goosed_pid: orphan,
        goosed_started_at: orphan_started,
        app_pid,
    };
    let proof = owner::prove(&orphaned, &me, &sys);
    unsafe { libc::kill(orphan as libc::pid_t, libc::SIGKILL) };
    assert!(
        matches!(proof, OwnerProof::Gone { ref why } if why.contains("outlived its app")),
        "{proof:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Two processes racing the claim
// ---------------------------------------------------------------------------------------------

const CLAIM_CHILD: &str = "GOOSE_LOOP_CLAIM_CHILD";

async fn wait_for_file(path: &std::path::Path) {
    while !path.exists() {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

/// The child's half of the race (runs only when the parent test sets `GOOSE_LOOP_CLAIM_CHILD`).
#[tokio::test]
async fn claim_child() {
    let Ok(spec) = std::env::var(CLAIM_CHILD) else {
        return;
    };
    let parts: Vec<&str> = spec.split('|').collect();
    let (data, session, dir) = (parts[0], parts[1], std::path::Path::new(parts[2]));
    let processes: Arc<dyn ProcessTable> = Arc::new(SystemProcesses);
    let me = owner::this_process(processes.as_ref()).unwrap();
    let runner = Runner::new(RunnerDeps {
        sessions: Arc::new(SessionManager::new(PathBuf::from(data))),
        clock: Arc::new(SystemClock),
        processes,
        me,
        turns: Box::leak(Box::new(TurnPriority::new())),
        logs_dir: dir.join("logs"),
        check_path: CheckPath::Inherited,
    });
    wait_for_file(&dir.join("go")).await;
    let result = match runner.control(session, LoopControlAction::Resume).await {
        Ok(_) => format!("won {}", std::process::id()),
        Err(refusal) => format!("refused {}", refusal.reason),
    };
    let mine = dir.join(format!("result-{}", std::process::id()));
    std::fs::write(mine.with_extension("tmp"), result).unwrap();
    std::fs::rename(mine.with_extension("tmp"), mine).unwrap();
    wait_for_file(&dir.join("done")).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn two_processes_racing_the_claim_make_exactly_one_owner() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let sessions = SessionManager::new(data.clone());
    let session = sessions
        .create_session(
            dir.path().to_path_buf(),
            "raced".into(),
            SessionType::User,
            crate::config::GooseMode::default(),
        )
        .await
        .unwrap()
        .id;
    record::update(&sessions, &session, |_| {
        let mut rec =
            record::new_record("lp_0a1b2c3d".into(), edit(every("10m"), None), Utc::now());
        rec.status = LoopStatus::Paused;
        rec.status_reason = Some(LoopStatusReason::ByYou { after_tick: 0 });
        Ok((rec, ()))
    })
    .await
    .unwrap();
    let spec = format!("{}|{}|{}", data.display(), session, dir.path().display());
    let exe = std::env::current_exe().unwrap();
    let mut children: Vec<_> = (0..2)
        .map(|_| {
            std::process::Command::new(&exe)
                .args([
                    "--exact",
                    "session_loops::runner::tests::claim_child",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CLAIM_CHILD, &spec)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    std::fs::write(dir.path().join("go"), "").unwrap();
    let results = loop {
        let results: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("result-") && !n.ends_with(".tmp"))
            .map(|n| std::fs::read_to_string(dir.path().join(n)).unwrap())
            .collect();
        if results.len() == 2 {
            break results;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };
    std::fs::write(dir.path().join("done"), "").unwrap();
    for child in &mut children {
        assert!(child.wait().unwrap().success());
    }
    let won: Vec<&String> = results.iter().filter(|r| r.starts_with("won ")).collect();
    assert_eq!(won.len(), 1, "exactly one owner: {results:?}");
    assert!(
        results
            .iter()
            .any(|r| r.starts_with("refused") && r.contains("another goose window")),
        "{results:?}"
    );
    let winner: u32 = won[0].trim_start_matches("won ").parse().unwrap();
    let rec = record::read(&sessions, &session)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(rec.owner.unwrap().goosed_pid, winner);
}

// ---------------------------------------------------------------------------------------------
// The check
// ---------------------------------------------------------------------------------------------

async fn one_checked_tick(bed: &Bed, check: &str, verdict: LoopVerdict) -> LoopRecord {
    bed.start(edit(every("10m"), Some(check))).await;
    let due = bed.due(1).await;
    bed.tick(&due, Some(report(verdict, "next", None))).await;
    bed.until("tick 1 decided", |r| r.ticks[0].outcome.is_some())
        .await
}

#[tokio::test]
async fn a_check_that_passes_after_progress_ends_the_loop() {
    let bed = bed().await;
    let rec = one_checked_tick(&bed, "printf 'all green\\n'", LoopVerdict::Progress).await;
    assert_eq!(rec.status, LoopStatus::Ended);
    assert_eq!(
        rec.status_reason,
        Some(LoopStatusReason::GoalMet {
            n: 1,
            check: "printf 'all green\\n'".into()
        })
    );
    let run = rec.ticks[0].check.clone().unwrap();
    assert!(run.ran);
    assert_eq!(run.exit, Some(0));
    assert_eq!(run.output_tail, "all green\n");
    let log = std::fs::read_to_string(run.log_path.unwrap()).unwrap();
    assert_eq!(log, "all green\n");
}

#[tokio::test]
async fn a_failing_check_continues_and_the_next_prompt_quotes_it() {
    let bed = bed().await;
    let rec = one_checked_tick(
        &bed,
        "echo 'missing svc- accounts' >&2; exit 3",
        LoopVerdict::Done,
    )
    .await;
    assert_eq!(rec.status, LoopStatus::Waiting);
    assert_eq!(rec.ticks[0].check.as_ref().unwrap().exit, Some(3));
    bed.clock.advance(Duration::minutes(10));
    let due = bed.due(2).await;
    assert!(
        due.prompt
            .contains("You reported the goal done in tick 1, but")
            && due.prompt.contains("exited 3")
            && due.prompt.contains("missing svc- accounts"),
        "{}",
        due.prompt
    );
}

#[tokio::test]
async fn a_check_that_cannot_start_pauses_with_its_error() {
    let bed = bed().await;
    bed.start(edit(every("10m"), Some("true"))).await;
    let due = bed.due(1).await;
    std::fs::remove_dir_all(bed.dir.path().join("work")).unwrap();
    bed.tick(&due, Some(report(LoopVerdict::Progress, "next", None)))
        .await;
    let rec = bed
        .until("tick 1 decided", |r| r.ticks[0].outcome.is_some())
        .await;
    assert_eq!(rec.status, LoopStatus::Paused);
    assert!(
        matches!(rec.status_reason, Some(LoopStatusReason::CheckCouldNotRun { ref error, .. }) if error.contains("could not start")),
        "{:?}",
        rec.status_reason
    );
    assert!(!rec.ticks[0].check.as_ref().unwrap().ran);
}

#[tokio::test]
async fn a_stopped_check_pauses_the_loop() {
    let bed = bed().await;
    bed.start(edit(every("10m"), Some("sleep 30"))).await;
    let due = bed.due(1).await;
    bed.tick(&due, Some(report(LoopVerdict::Progress, "next", None)))
        .await;
    bed.until("checking", |r| r.status == LoopStatus::Checking)
        .await;
    bed.runner
        .control(&bed.session, LoopControlAction::StopCheck)
        .await
        .unwrap();
    let rec = bed
        .until("tick 1 decided", |r| r.ticks[0].outcome.is_some())
        .await;
    assert_eq!(rec.status, LoopStatus::Paused);
    assert_eq!(
        rec.status_reason,
        Some(LoopStatusReason::CheckCouldNotRun {
            n: 1,
            error: STOPPED_BY_YOU.into()
        })
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_check_ends_at_its_exit_even_when_a_grandchild_keeps_its_output_open() {
    let dir = tempfile::tempdir().unwrap();
    let spec = CheckSpec {
        command: "sleep 30 & echo $!".into(),
        working_dir: dir.path().to_path_buf(),
        log_path: dir.path().join("check-1.log"),
        path_env: None,
    };
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        check::run(&spec, CancellationToken::new()),
    )
    .await
    .expect("the check ended at its exit, not at its output's EOF");
    assert_eq!(outcome.end, CheckEnd::Exited { code: Some(0) });
    let grandchild: i32 = outcome.output.unwrap().trim().parse().unwrap();
    unsafe { libc::kill(grandchild, libc::SIGKILL) };
}

#[cfg(unix)]
#[tokio::test]
async fn stop_check_kills_the_checks_whole_group() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("check-1.log");
    let spec = CheckSpec {
        command: "sleep 30 & echo $!; wait".into(),
        working_dir: dir.path().to_path_buf(),
        log_path: log.clone(),
        path_env: None,
    };
    let stop = CancellationToken::new();
    let running = tokio::spawn({
        let stop = stop.clone();
        async move { check::run(&spec, stop).await }
    });
    let grandchild: u32 = loop {
        if let Ok(text) = std::fs::read_to_string(&log) {
            if let Ok(pid) = text.trim().parse() {
                break pid;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };
    let (started, _) = goose_sidecar::machine::process_start(grandchild).unwrap();
    stop.cancel();
    assert_eq!(running.await.unwrap().end, CheckEnd::Stopped);
    for _ in 0..400 {
        if goose_sidecar::machine::prove(grandchild, started)
            != goose_sidecar::machine::Liveness::Alive
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    unsafe { libc::kill(grandchild as libc::pid_t, libc::SIGKILL) };
    panic!("the check's grandchild outlived Stop check");
}

#[test]
fn a_prompts_loop_tick_meta_is_read_and_anything_else_is_not() {
    let meta: serde_json::Map<String, Value> = serde_json::from_value(serde_json::json!({
        "goose": {"loopTick": {"loopId": "lp_0a1b2c3d", "n": 3, "messageId": "looptick_lp_0a1b2c3d_3_ab"}}
    }))
    .unwrap();
    assert_eq!(
        tick_meta(&meta),
        Some(TickMeta {
            loop_id: "lp_0a1b2c3d".into(),
            n: 3,
            message_id: "looptick_lp_0a1b2c3d_3_ab".into()
        })
    );
    let partial: serde_json::Map<String, Value> =
        serde_json::from_value(serde_json::json!({"goose": {"loopTick": {"n": 3}}})).unwrap();
    assert_eq!(tick_meta(&partial), None);
}
