//! Session loops L2c: the two ways a tick can end before its reply runs, driven the way the desktop
//! drives it (a window on a real websocket, `loops/tickDue`, the tick submitted with
//! `_meta.goose.loopTick`). The window pre-appends the tick's marker before it submits, so each
//! way must end with what the window needs to settle that marker: the tick's message stored under
//! the offer's id (a reload shows the same marker), and the person's Stop noticed where the answer
//! would have been — or, for a yield, no notice and the record saying whom it yielded to. The same
//! early exit for a PERSON's message: stored and noticed the same way (a slash command is not).
//!
//! Its own binary on purpose: the Stop cases hold a write lock on this process's sessions store
//! while the turn starts, and a loop left over from another test (its window's close releasing it
//! later) would write under that lock. One runtime, one goose, one test at a time, as in
//! `acp_loop_tick_test.rs`.

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
    LoopCadence, LoopEdit, LoopRecord, LoopStatus, LoopStatusReason, LoopTemplateId,
    LoopTickOutcome, LoopsStartRequest,
};
use serde_json::{json, Value};
use serial_test::serial;
use tokio::sync::OnceCell;

const LOOP_TICK: &str = "_goose/unstable/loops/tickDue";
const LOOP_CHANGED: &str = "_goose/unstable/loops/changed";
const SESSION_UPDATE: &str = "_goose/unstable/session/update";

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

async fn start_loop(window: &mut Window, session_id: &str) -> Value {
    let start = LoopsStartRequest {
        session_id: session_id.to_string(),
        goal: "Make every test pass".into(),
        template: LoopTemplateId::Blank,
        steps: String::new(),
        cadence: LoopCadence::SelfPaced,
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
    window
        .notification(LOOP_TICK, |p| p["sessionId"] == session_id && p["n"] == 1)
        .await
}

async fn submit(window: &mut Window, due: &Value) -> u64 {
    let meta = json!({"goose": {"loopTick": {"loopId": due["loopId"], "n": due["n"], "messageId": due["messageId"]}}});
    window
        .prompt(
            due["sessionId"].as_str().unwrap(),
            due["prompt"].as_str().unwrap(),
            Some(meta),
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

async fn stored_marker(session_id: &str, message_id: &str) -> bool {
    messages(session_id)
        .await
        .iter()
        .any(|m| m.id.as_deref() == Some(message_id))
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

/// Whether goose holds a run for the chat: the steer door's own check, asked for a run id no run
/// has, answers "no active run" or names the run it found — and steers nothing either way.
async fn holds_a_run(window: &mut Window, session_id: &str) -> bool {
    let probe = window
        .send(
            "_goose/unstable/session/steer",
            json!({"sessionId": session_id, "prompt": [{"type": "text", "text": "probe"}], "expectedRunId": "probe"}),
        )
        .await;
    let error = window
        .response(probe)
        .await
        .expect_err("a probe never steers")
        .to_string();
    assert!(
        error.contains("no active run") || error.contains("but found"),
        "{error}"
    );
    error.contains("but found")
}

/// Hold the next write to this process's sessions store: `BEGIN IMMEDIATE` on a connection of its
/// own. Reads go on (the store is WAL); a write waits on it until the returned lock rolls back.
async fn hold_the_store() -> sqlx::pool::PoolConnection<sqlx::Sqlite> {
    let store = goose::config::paths::Paths::data_dir()
        .join("sessions")
        .join("sessions.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&store))
        .await
        .unwrap();
    let mut lock = pool.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .unwrap();
    lock
}

/// Wait until goose has taken the chat's run for `prompt` — asked through the steer door, each
/// probe an answered request. Taking the run waits on no write to the store (the runner decides
/// on reads and writes only a change, Q-286), so nothing held here stands in its way; the prompt
/// answering first means it ended without a run, and its answer is the failure.
async fn until_goose_holds_a_run(window: &mut Window, chat: &str, prompt: u64) {
    while !holds_a_run(window, chat).await {
        if let Some(answer) = window
            .seen
            .iter()
            .find(|f| f["id"] == prompt && f.get("method").is_none())
        {
            panic!("the prompt ended before goose took its run: {answer}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

/// Close the window and wait until its door's close has released the loop, so nothing of this
/// test writes to the store under the next one.
async fn close_and_release(window: Window, session_id: &str) {
    window.close().await;
    eventually("the loop is released", || async {
        stored(session_id).await.owner.is_none()
    })
    .await;
}

/// A person's turn began between the offer and the tick's start: the tick yields at its start —
/// before it reaches the model — and is settled as a yield during the reply is. Its marker is
/// stored under the offer's id, no one "stopped" it, and the window hears the record naming the
/// chat it yielded to.
async fn a_tick_that_yields_at_its_start_is_stored_and_settled_as_a_yield() {
    let bed = bed(vec![Answer::Unfinished("The person's answer")]).await;
    let mut window = Window::open(bed.addr, true).await;
    let loop_chat = window.new_chat(bed.work.path()).await;
    let other_chat = window.new_chat(bed.work.path()).await;
    let due = start_loop(&mut window, &loop_chat).await;
    let message_id = due["messageId"].as_str().unwrap().to_string();

    let person = window
        .prompt(&other_chat, "What does the release checklist say?", None)
        .await;
    window.until_streaming(&other_chat).await;
    assert_eq!(user_turns_running(), 1);
    let calls = bed.model.requests.lock().unwrap().len();

    let tick = submit(&mut window, &due).await;
    assert_eq!(stop_reason(&mut window, tick).await, "cancelled");
    let changed = window
        .notification(LOOP_CHANGED, |p| {
            p["sessionId"] == loop_chat.as_str()
                && p["loop"]["ticks"][0]["outcome"]["kind"] == "yielded"
        })
        .await;
    assert_eq!(
        changed["loop"]["ticks"][0]["outcome"]["toSession"],
        other_chat.as_str(),
        "{changed}"
    );
    match &stored(&loop_chat).await.ticks[0].outcome {
        Some(LoopTickOutcome::Yielded { to_session, .. }) => assert_eq!(to_session, &other_chat),
        other => panic!("the tick yielded, not {other:?}"),
    }
    assert!(
        stored_marker(&loop_chat, &message_id).await,
        "the window's marker is a stored message: a reload shows it"
    );
    assert!(!stopped_notice(&loop_chat).await, "a yield is not a stop");
    assert_eq!(outcome_stopped(&loop_chat).await, Some(false));
    assert_eq!(
        bed.model.requests.lock().unwrap().len(),
        calls,
        "the yielded tick never reached the model"
    );

    window.cancel(&other_chat).await;
    assert_eq!(stop_reason(&mut window, person).await, "cancelled");
    window
        .notification(LOOP_TICK, |p| {
            p["sessionId"] == loop_chat.as_str() && p["n"] == 2
        })
        .await;
    close_and_release(window, &loop_chat).await;
}

/// The person's Stop lands while the tick is starting — after goose took the run, before the
/// reply. Held there by a write lock on the store (the tick's record write waits on it), the Stop
/// is sent, and the lock let go: the tick is the person's stop, and it is settled as a Stop during
/// the reply is — the marker stored, the notice stored AND sent to the window, the loop paused.
async fn a_persons_stop_while_the_tick_starts_is_noticed_and_its_marker_stored() {
    let bed = bed(Vec::new()).await;
    let mut window = Window::open(bed.addr, true).await;
    let chat = window.new_chat(bed.work.path()).await;
    let due = start_loop(&mut window, &chat).await;
    let message_id = due["messageId"].as_str().unwrap().to_string();

    let mut lock = hold_the_store().await;
    let tick = submit(&mut window, &due).await;
    until_goose_holds_a_run(&mut window, &chat, tick).await;
    assert!(
        stored(&chat).await.ticks.is_empty(),
        "the tick's record write is still held"
    );
    window.cancel(&chat).await;
    // The cancel is handled inline, in order: a later request's answer means it was.
    assert!(holds_a_run(&mut window, &chat).await);
    sqlx::query("ROLLBACK").execute(&mut *lock).await.unwrap();
    drop(lock);

    assert_eq!(stop_reason(&mut window, tick).await, "cancelled");
    let notice = window
        .notification(SESSION_UPDATE, |p| {
            p["sessionId"] == chat.as_str() && p.to_string().contains("You stopped this answer")
        })
        .await;
    assert!(
        notice
            .to_string()
            .contains("before the model wrote anything"),
        "{notice}"
    );
    eventually("the stopped tick is recorded", || async {
        stored(&chat).await.ticks[0].outcome.is_some()
    })
    .await;
    let rec = stored(&chat).await;
    assert_eq!(rec.ticks[0].outcome, Some(LoopTickOutcome::StoppedByYou));
    assert_eq!(rec.status, LoopStatus::Paused);
    assert!(
        stored_marker(&chat, &message_id).await,
        "the window's marker is a stored message: a reload shows it"
    );
    assert!(
        stopped_notice(&chat).await,
        "the notice is stored in the chat"
    );
    assert_eq!(outcome_stopped(&chat).await, Some(true));
    close_and_release(window, &chat).await;
}

/// A person's prompt held while it starts: the chat carries a loop record the window's agent has
/// not synced (paused, owned by no goose — the runner leaves it alone), so the prompt's sync of
/// the loop tool writes to the store before the reply, and waits there on a held store. The
/// person's Stop is sent while it waits; then the store is let go. Returns the prompt's id.
async fn a_persons_prompt_stopped_while_it_starts(
    window: &mut Window,
    chat: &str,
    text: &str,
) -> u64 {
    let edit = LoopEdit {
        goal: "Make every test pass".into(),
        template: LoopTemplateId::Blank,
        steps: String::new(),
        cadence: LoopCadence::SelfPaced,
        state_file: ".goose/loops/tests/NOW.md".into(),
        check: None,
        stop_after_ticks: None,
    };
    record::update(&SessionManager::instance(), chat, |_| {
        let mut rec = record::new_record(record::new_loop_id(), edit, chrono::Utc::now());
        rec.status = LoopStatus::Paused;
        rec.status_reason = Some(LoopStatusReason::ByYou { after_tick: 0 });
        Ok((rec, ()))
    })
    .await
    .unwrap();

    let mut lock = hold_the_store().await;
    let prompt = window.prompt(chat, text, None).await;
    until_goose_holds_a_run(window, chat, prompt).await;
    window.cancel(chat).await;
    // The cancel is handled inline, in order: a later request's answer means it was.
    assert!(holds_a_run(window, chat).await);
    sqlx::query("ROLLBACK").execute(&mut *lock).await.unwrap();
    drop(lock);
    prompt
}

async fn stored_text(session_id: &str, text: &str) -> bool {
    messages(session_id)
        .await
        .iter()
        .any(|m| m.as_concat_text() == text)
}

/// The gap L2c part 1 left in the same early exit: a PERSON's plain message stopped while it
/// starts — after goose took the run, before the reply — is settled as a Stop during the reply
/// is: the message the window already shows is stored (a reload keeps it), and the notice is
/// stored and sent. A slash command stopped there is not stored — what `agent.reply` stores for
/// one depends on running it — and so leaves no notice either.
async fn a_persons_message_stopped_while_it_starts_is_stored_and_noticed() {
    let bed = bed(Vec::new()).await;
    let mut window = Window::open(bed.addr, true).await;
    let chat = window.new_chat(bed.work.path()).await;
    let text = "What does the release checklist say?";

    let prompt = a_persons_prompt_stopped_while_it_starts(&mut window, &chat, text).await;
    assert_eq!(stop_reason(&mut window, prompt).await, "cancelled");
    let notice = window
        .notification(SESSION_UPDATE, |p| {
            p["sessionId"] == chat.as_str() && p.to_string().contains("You stopped this answer")
        })
        .await;
    assert!(
        notice
            .to_string()
            .contains("before the model wrote anything"),
        "{notice}"
    );
    assert!(
        stored_text(&chat, text).await,
        "the person's message is stored: a reload shows it"
    );
    assert!(
        stopped_notice(&chat).await,
        "the notice is stored in the chat"
    );
    assert_eq!(outcome_stopped(&chat).await, Some(true));

    let command_chat = window.new_chat(bed.work.path()).await;
    let prompt =
        a_persons_prompt_stopped_while_it_starts(&mut window, &command_chat, "/compact").await;
    assert_eq!(stop_reason(&mut window, prompt).await, "cancelled");
    assert!(
        !stored_text(&command_chat, "/compact").await,
        "a command stopped before it ran is not stored"
    );
    assert!(!stopped_notice(&command_chat).await);
    window.close().await;
}

mod tests {
    use super::*;

    #[test]
    #[serial]
    fn a_tick_that_yields_at_its_start_is_stored_and_settled_as_a_yield() {
        run(super::a_tick_that_yields_at_its_start_is_stored_and_settled_as_a_yield());
    }

    #[test]
    #[serial]
    fn a_persons_stop_while_the_tick_starts_is_noticed_and_its_marker_stored() {
        run(super::a_persons_stop_while_the_tick_starts_is_noticed_and_its_marker_stored());
    }

    #[test]
    #[serial]
    fn a_persons_message_stopped_while_it_starts_is_stored_and_noticed() {
        run(super::a_persons_message_stopped_while_it_starts_is_stored_and_noticed());
    }
}
