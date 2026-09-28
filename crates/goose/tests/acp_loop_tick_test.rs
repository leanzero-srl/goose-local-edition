//! Session loops L2b (DESIGN-SESSION-LOOPS §5.1–§5.3): the server's door, driven the way the desktop
//! drives it — a window on a real websocket that hears goose's own notifications, the runner's
//! `loops/tickDue` offer, and the tick submitted through `session/prompt` with
//! `_meta.goose.loopTick`.
//!
//! The runner is one per process and its driver task lives on the runtime that installed it, so
//! every test runs on ONE runtime against one goose and one scripted model, one test at a time
//! (they share the process's turn priority too).

#[path = "acp_ws/mod.rs"]
mod acp_ws;

use std::future::Future;
use std::sync::LazyLock;

use acp_ws::{configure, eventually, serve, user_turns_running, Answer, Model, Window};
use goose::conversation::message::MessageContent;
use goose::session::SessionManager;
use goose::session_loops::record;
use goose::turn_outcome::TurnOutcomeState;
use goose_sdk_types::custom_requests::{
    LoopCadence, LoopRecord, LoopStatus, LoopStatusReason, LoopTemplateId, LoopTickOutcome,
    LoopsStartRequest,
};
use serde_json::{json, Value};
use serial_test::serial;
use tokio::sync::OnceCell;

const LOOP_TICK: &str = "_goose/unstable/loops/tickDue";

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_stack_size(8 * 1024 * 1024)
        .enable_all()
        .build()
        .unwrap()
});

static GOOSE: OnceCell<(Model, std::net::SocketAddr)> = OnceCell::const_new();

fn run(test: impl Future<Output = ()>) {
    RUNTIME.block_on(test);
}

struct Bed {
    model: Model,
    addr: std::net::SocketAddr,
    work: tempfile::TempDir,
}

async fn bed(script: Vec<Answer>) -> Bed {
    let (model, addr) = GOOSE
        .get_or_init(|| async {
            let model = Model::start(Vec::new()).await;
            configure(&model, true);
            (model, serve().await)
        })
        .await
        .clone();
    model.script(script);
    Bed {
        model,
        addr,
        work: tempfile::tempdir().unwrap(),
    }
}

async fn stored(session_id: &str) -> LoopRecord {
    record::read(&SessionManager::instance(), session_id)
        .await
        .unwrap()
        .unwrap()
        .expect("the chat has a loop")
}

async fn start_loop(window: &mut Window, session_id: &str, cadence: LoopCadence) -> Value {
    let start = LoopsStartRequest {
        session_id: session_id.to_string(),
        goal: "Make every test pass".into(),
        template: LoopTemplateId::Blank,
        steps: String::new(),
        cadence,
        state_file: ".goose/loops/tests/NOW.md".into(),
        check: None,
        stop_after_ticks: None,
    };
    let response = window
        .request(
            "_goose/unstable/loops/start",
            serde_json::to_value(start).unwrap(),
        )
        .await;
    assert!(response.get("refusal").is_none(), "{response}");
    tick_due(window, session_id, 1).await
}

async fn tick_due(window: &mut Window, session_id: &str, n: u64) -> Value {
    window
        .notification(LOOP_TICK, |p| p["sessionId"] == session_id && p["n"] == n)
        .await
}

fn tick_meta(due: &Value) -> Value {
    json!({"goose": {"loopTick": {"loopId": due["loopId"], "n": due["n"], "messageId": due["messageId"]}}})
}

async fn submit(window: &mut Window, due: &Value) -> u64 {
    window
        .prompt(
            due["sessionId"].as_str().unwrap(),
            due["prompt"].as_str().unwrap(),
            Some(tick_meta(due)),
        )
        .await
}

async fn stop_reason(window: &mut Window, prompt: u64) -> String {
    window.response(prompt).await.expect("the prompt answered")["stopReason"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn messages(session_id: &str) -> Vec<goose::conversation::message::Message> {
    SessionManager::instance()
        .get_session(session_id, true)
        .await
        .unwrap()
        .conversation
        .map(|c| c.messages().clone())
        .unwrap_or_default()
}

async fn stopped_notice(session_id: &str) -> bool {
    messages(session_id).await.iter().any(|m| {
        m.content.iter().any(|c| match c {
            MessageContent::SystemNotification(n) => n.msg.starts_with("You stopped this answer"),
            _ => false,
        })
    })
}

async fn outcome_stopped(session_id: &str) -> Option<bool> {
    SessionManager::instance()
        .sessions_with_extension_state::<TurnOutcomeState>()
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.session_id == session_id)
        .map(|row| row.state.stopped.is_some())
}

/// An accepted tick holds no user turn (it would yield to itself), its stored message carries the
/// offer's id, and the record's tick is written before the reply runs. The person's Stop on it is
/// the person's: the notice is recorded, the tick is `stopped_by_you` and the loop pauses.
async fn an_accepted_tick_takes_no_user_turn_and_a_stop_on_it_is_the_persons() {
    let bed = bed(vec![Answer::Unfinished("Reading the failing tests first")]).await;
    let mut window = Window::open(bed.addr, true).await;
    let chat = window.new_chat(bed.work.path()).await;
    let due = start_loop(&mut window, &chat, LoopCadence::SelfPaced).await;
    let message_id = due["messageId"].as_str().unwrap().to_string();
    assert!(message_id.starts_with("looptick_"), "{message_id}");

    let prompt = submit(&mut window, &due).await;
    window.until_streaming(&chat).await;
    assert_eq!(user_turns_running(), 0, "a tick takes no user turn");
    let rec = stored(&chat).await;
    assert_eq!(rec.status, LoopStatus::Running);
    assert_eq!(rec.ticks.len(), 1);
    assert_eq!(rec.ticks[0].first_message_id, message_id);
    assert!(rec.offer.is_none(), "the offer is taken");
    assert!(
        messages(&chat)
            .await
            .iter()
            .any(|m| m.id.as_deref() == Some(message_id.as_str())),
        "the stored tick message carries the offer's id"
    );

    window.cancel(&chat).await;
    assert_eq!(stop_reason(&mut window, prompt).await, "cancelled");
    eventually("the stopped tick is recorded", || async {
        stored(&chat).await.ticks[0].outcome.is_some()
    })
    .await;
    let rec = stored(&chat).await;
    assert_eq!(rec.ticks[0].outcome, Some(LoopTickOutcome::StoppedByYou));
    assert_eq!(rec.status, LoopStatus::Paused);
    assert!(stopped_notice(&chat).await, "the person's stop is noticed");
    assert_eq!(outcome_stopped(&chat).await, Some(true));
    window.close().await;
}

/// A `loopTick` meta that is not this chat's open offer — a forged loop, a stale message id — is
/// an ordinary user prompt in every respect: a user turn, a normal id, no tick in the record.
async fn a_forged_or_stale_tick_meta_is_an_ordinary_user_prompt() {
    let bed = bed(vec![Answer::Unfinished("Answering the person")]).await;
    let mut window = Window::open(bed.addr, true).await;
    let chat = window.new_chat(bed.work.path()).await;
    let due = start_loop(&mut window, &chat, LoopCadence::SelfPaced).await;
    let mut forged = due.clone();
    forged["messageId"] = json!("looptick_lp_forged_1_0000");

    let prompt = submit(&mut window, &forged).await;
    window.until_streaming(&chat).await;
    assert_eq!(
        user_turns_running(),
        1,
        "the forged tick is the person's turn"
    );
    let rec = stored(&chat).await;
    assert!(
        rec.ticks.is_empty(),
        "no tick was recorded: {:?}",
        rec.ticks
    );
    assert_eq!(
        rec.offer.as_ref().map(|o| o.message_id.as_str()),
        due["messageId"].as_str(),
        "the real offer still stands"
    );
    assert!(
        messages(&chat)
            .await
            .iter()
            .all(|m| m.id.as_deref() != Some("looptick_lp_forged_1_0000")),
        "the prompt kept a normal id"
    );
    window.cancel(&chat).await;
    assert_eq!(stop_reason(&mut window, prompt).await, "cancelled");
    window.close().await;
}

/// v1a (§5.3): a person's reply that starts in this goose while a tick runs yields the tick. The
/// yield is nobody's stop: no "You stopped this answer", no Stopped outcome, the tick `yielded`
/// to that chat — and the next tick is offered once the person's turn ends.
async fn a_persons_reply_yields_a_running_tick_and_the_yield_is_nobodys_stop() {
    let bed = bed(vec![
        Answer::Unfinished("The tick reads the tests"),
        Answer::Unfinished("The person's answer"),
    ])
    .await;
    let mut window = Window::open(bed.addr, true).await;
    let loop_chat = window.new_chat(bed.work.path()).await;
    let other_chat = window.new_chat(bed.work.path()).await;
    let due = start_loop(&mut window, &loop_chat, LoopCadence::SelfPaced).await;
    let tick = submit(&mut window, &due).await;
    window.until_streaming(&loop_chat).await;

    let person = window
        .prompt(&other_chat, "What does the release checklist say?", None)
        .await;
    assert_eq!(stop_reason(&mut window, tick).await, "cancelled");
    eventually("the yield is recorded", || async {
        stored(&loop_chat).await.ticks[0].outcome.is_some()
    })
    .await;
    let rec = stored(&loop_chat).await;
    match &rec.ticks[0].outcome {
        Some(LoopTickOutcome::Yielded { to_session, .. }) => assert_eq!(to_session, &other_chat),
        other => panic!("the tick yielded, not {other:?}"),
    }
    assert!(!stopped_notice(&loop_chat).await, "a yield is not a stop");
    assert_eq!(outcome_stopped(&loop_chat).await, Some(false));

    window.until_streaming(&other_chat).await;
    window.cancel(&other_chat).await;
    assert_eq!(stop_reason(&mut window, person).await, "cancelled");
    let next = tick_due(&mut window, &loop_chat, 2).await;
    assert_ne!(next["messageId"], due["messageId"]);
    window.close().await;
}

/// L2a's edge, fixed in the order: a second prompt to the ticking chat is refused as busy BEFORE
/// it takes a user turn, so it never yields the tick it collided with.
async fn a_second_prompt_to_the_ticking_chat_is_refused_without_yielding_the_tick() {
    let bed = bed(vec![Answer::Unfinished("The tick reads the tests")]).await;
    let mut window = Window::open(bed.addr, true).await;
    let chat = window.new_chat(bed.work.path()).await;
    let due = start_loop(&mut window, &chat, LoopCadence::SelfPaced).await;
    let tick = submit(&mut window, &due).await;
    window.until_streaming(&chat).await;

    let second = window.prompt(&chat, "and also this", None).await;
    let refused = window
        .response(second)
        .await
        .expect_err("the busy chat refuses");
    assert!(
        refused.to_string().contains("active run"),
        "refused as busy: {refused}"
    );
    assert_eq!(user_turns_running(), 0, "the refused prompt took no turn");
    let rec = stored(&chat).await;
    assert_eq!(rec.status, LoopStatus::Running, "the tick still runs");
    assert!(rec.ticks[0].outcome.is_none());

    window.cancel(&chat).await;
    assert_eq!(stop_reason(&mut window, tick).await, "cancelled");
    window.close().await;
}

/// §5.2 step 7: the next tick waits for this tick's end-of-turn reviewers — here the memory
/// assessment, whose model call is held — and is offered the moment they end.
async fn the_next_tick_waits_for_the_ticks_reviewers() {
    let bed = bed(vec![Answer::Finish(
        "Tick one: the suite runs, 3 tests fail.",
    )])
    .await;
    let mut window = Window::open(bed.addr, true).await;
    let chat = window.new_chat(bed.work.path()).await;
    let due = start_loop(&mut window, &chat, LoopCadence::BackToBack).await;
    let held_before = bed.model.held();
    let tick = submit(&mut window, &due).await;
    assert_eq!(stop_reason(&mut window, tick).await, "end_turn");

    eventually("the assessment's call is held", || async {
        bed.model.held() == held_before + 1
    })
    .await;
    eventually("tick one is recorded", || async {
        stored(&chat).await.ticks[0].outcome.is_some()
    })
    .await;
    let rec = stored(&chat).await;
    assert_eq!(rec.ticks[0].outcome, Some(LoopTickOutcome::NoReport));
    assert_eq!(rec.status, LoopStatus::WaitingTurn);
    assert_eq!(
        rec.status_reason,
        Some(LoopStatusReason::Reviewers { n: 1 }),
        "back-to-back, yet held behind the reviewers"
    );
    assert!(
        rec.offer.is_none(),
        "no tick 2 while tick 1's reviewers run"
    );

    // The assessment's call ends: its retry, if the provider makes one, is answered.
    bed.model
        .script(vec![Answer::Finish("Nothing worth remembering."); 3]);
    bed.model.release_held();
    let next = tick_due(&mut window, &chat, 2).await;
    assert_eq!(next["n"], 2);
    window.close().await;
}

/// The window's door (§5.1): a closed connection releases the loops this goose owns
/// ("goose was closed"), and a window that connects again restores them and is offered the SAME
/// open tick, which it can run.
async fn a_closed_window_releases_its_loops_and_the_next_window_is_offered_the_same_tick() {
    let bed = bed(vec![Answer::Unfinished("Tick one on the new window")]).await;
    let mut window = Window::open(bed.addr, true).await;
    let chat = window.new_chat(bed.work.path()).await;
    let due = start_loop(&mut window, &chat, LoopCadence::SelfPaced).await;
    window.close().await;

    eventually("the loop is released", || async {
        let rec = stored(&chat).await;
        rec.owner.is_none() && rec.status == LoopStatus::Paused
    })
    .await;
    assert!(matches!(
        stored(&chat).await.status_reason,
        Some(LoopStatusReason::Closed { .. })
    ));

    // A window that does not hear goose's notifications is no door: nothing is restored for it.
    let deaf = Window::open(bed.addr, false).await;
    let rec = stored(&chat).await;
    assert!(rec.owner.is_none(), "a deaf window restores nothing");

    let mut reloaded = Window::open(bed.addr, true).await;
    let again = tick_due(&mut reloaded, &chat, 1).await;
    assert_eq!(again["messageId"], due["messageId"], "the same offer");
    let rec = stored(&chat).await;
    assert!(rec.owner.is_some());
    assert_eq!(rec.status, LoopStatus::Waiting);

    reloaded
        .request(
            "session/load",
            json!({"sessionId": chat, "cwd": bed.work.path(), "mcpServers": []}),
        )
        .await;
    let tick = submit(&mut reloaded, &again).await;
    reloaded.until_streaming(&chat).await;
    assert_eq!(stored(&chat).await.status, LoopStatus::Running);
    reloaded.cancel(&chat).await;
    assert_eq!(stop_reason(&mut reloaded, tick).await, "cancelled");
    reloaded.close().await;
    deaf.close().await;
}

/// Measured (Q9): a closed websocket drops the tick's prompt mid-await. The tick is still ended —
/// by name, `connection` — never left running, and the chat is not left busy.
async fn a_tick_dropped_with_its_connection_ends_by_name() {
    let bed = bed(vec![
        Answer::Unfinished("The tick reads the tests"),
        Answer::Unfinished("The reloaded window's answer"),
    ])
    .await;
    let mut window = Window::open(bed.addr, true).await;
    let chat = window.new_chat(bed.work.path()).await;
    let due = start_loop(&mut window, &chat, LoopCadence::SelfPaced).await;
    submit(&mut window, &due).await;
    window.until_streaming(&chat).await;
    window.close().await;

    eventually("the dropped tick is ended", || async {
        stored(&chat).await.ticks[0].outcome.is_some()
    })
    .await;
    match &stored(&chat).await.ticks[0].outcome {
        Some(LoopTickOutcome::Failed { error_class, error }) => {
            assert_eq!(error_class, "connection");
            assert!(error.contains("connection to goose ended"), "{error}");
        }
        other => panic!("ended by name, not {other:?}"),
    }

    let mut reloaded = Window::open(bed.addr, false).await;
    reloaded
        .request(
            "session/load",
            json!({"sessionId": chat, "cwd": bed.work.path(), "mcpServers": []}),
        )
        .await;
    let person = reloaded.prompt(&chat, "What happened?", None).await;
    reloaded.until_streaming(&chat).await;
    reloaded.cancel(&chat).await;
    assert_eq!(stop_reason(&mut reloaded, person).await, "cancelled");
    reloaded.close().await;
}

mod tests {
    use super::*;

    #[test]
    #[serial]
    fn an_accepted_tick_takes_no_user_turn_and_a_stop_on_it_is_the_persons() {
        run(super::an_accepted_tick_takes_no_user_turn_and_a_stop_on_it_is_the_persons());
    }

    #[test]
    #[serial]
    fn a_forged_or_stale_tick_meta_is_an_ordinary_user_prompt() {
        run(super::a_forged_or_stale_tick_meta_is_an_ordinary_user_prompt());
    }

    #[test]
    #[serial]
    fn a_persons_reply_yields_a_running_tick_and_the_yield_is_nobodys_stop() {
        run(super::a_persons_reply_yields_a_running_tick_and_the_yield_is_nobodys_stop());
    }

    #[test]
    #[serial]
    fn a_second_prompt_to_the_ticking_chat_is_refused_without_yielding_the_tick() {
        run(super::a_second_prompt_to_the_ticking_chat_is_refused_without_yielding_the_tick());
    }

    #[test]
    #[serial]
    fn the_next_tick_waits_for_the_ticks_reviewers() {
        run(super::the_next_tick_waits_for_the_ticks_reviewers());
    }

    #[test]
    #[serial]
    fn a_closed_window_releases_its_loops_and_the_next_window_is_offered_the_same_tick() {
        run(
            super::a_closed_window_releases_its_loops_and_the_next_window_is_offered_the_same_tick(
            ),
        );
    }

    #[test]
    #[serial]
    fn a_tick_dropped_with_its_connection_ends_by_name() {
        run(super::a_tick_dropped_with_its_connection_ends_by_name());
    }
}
