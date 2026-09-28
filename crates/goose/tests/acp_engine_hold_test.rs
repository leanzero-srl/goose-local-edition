//! Q-397 (E2E #3r turn 18, 2026-09-28 14:18:25): the split's rank 0 was in its memory hold and
//! answered 503 "goose distributed engine is not admitting new requests"; goose's provider retry
//! spent its three tries in ~4 s of a hold that lasted minutes and the whole turn ended in an error.
//! Driven the way the desktop drives it — a window on a real websocket that hears goose's own
//! notifications — against a scripted endpoint that answers as the engine does.
//!
//! One runtime, one goose, one scripted model, one test at a time (the provider and its config are
//! the process's).

#[path = "acp_ws/mod.rs"]
mod acp_ws;

use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;

use acp_ws::{configure, eventually, serve, Answer, Model, Window};
use serde_json::Value;
use serial_test::serial;
use tokio::sync::OnceCell;

/// The watchdog's words on #3r (goose log 14:18:25), as the hold's 503 carried them.
const REASON: &str = "memory on Mihai Macbook is low (other apps and the system use 87.3 GiB; \
                      the engine holds 29.4 GiB); the engine is holding new requests until \
                      memory recovers — quitting other apps frees it";
const AFTER_THE_HOLD: &str = "The answer the engine wrote once memory recovered";
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

async fn goose(script: Vec<Answer>) -> (Model, std::net::SocketAddr) {
    let (model, addr) = GOOSE
        .get_or_init(|| async {
            let model = Model::start(Vec::new()).await;
            configure(&model, false);
            (model, serve().await)
        })
        .await
        .clone();
    model.script(script);
    (model, addr)
}

/// Every hold line goose put on this session's turn line, in order, among the frames the window
/// kept (a frame a test awaited by `Window::notification` is handed to it, not kept).
fn hold_lines(window: &Window, session_id: &str) -> Vec<String> {
    progress_lines(window, session_id)
        .into_iter()
        .filter(|line| {
            line.starts_with("Waiting: ") || line == goose_providers::engine_hold::ADMITTED_WORDS
        })
        .collect()
}

fn progress_lines(window: &Window, session_id: &str) -> Vec<String> {
    window
        .seen
        .iter()
        .filter(|frame| frame["method"] == SESSION_UPDATE)
        .map(|frame| &frame["params"])
        .filter(|params| {
            params["sessionId"] == session_id
                && params["update"]["sessionUpdate"] == "status_message"
                && params["update"]["status"]["type"] == "progress"
        })
        .filter_map(|params| params["update"]["status"]["message"].as_str())
        .map(str::to_string)
        .collect()
}

/// Every chat text chunk goose streamed on this session.
fn agent_text(window: &Window, session_id: &str) -> String {
    window
        .seen
        .iter()
        .filter(|frame| frame["method"] == "session/update")
        .map(|frame| &frame["params"])
        .filter(|params| {
            params["sessionId"] == session_id
                && params["update"]["sessionUpdate"] == "agent_message_chunk"
        })
        .filter_map(|params| params["update"]["content"]["text"].as_str())
        .collect()
}

async fn stop_reason(window: &mut Window, prompt: u64) -> Value {
    window.response(prompt).await.expect("the prompt answered")["stopReason"].clone()
}

/// The first "Waiting:" line, then the engine's `waits`-th admission wait. `waits` counts from the
/// start of the binary: the three tests share one scripted model, run in whatever order the serial
/// lock grants, and a stopped hold's wait stays counted (Q-420).
async fn until_waiting(
    window: &mut Window,
    model: &Model,
    session_id: &str,
    waits: usize,
) -> Value {
    let line = window
        .notification(SESSION_UPDATE, |p| {
            p["sessionId"] == session_id
                && p["update"]["status"]["type"] == "progress"
                && p["update"]["status"]["message"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("Waiting: "))
        })
        .await;
    eventually("goose waits on the engine's admission", || async {
        model.admission_waits() == waits
    })
    .await;
    line
}

/// The hold's 503 twice (the second after a lift that closed again at once), then the answer:
/// the turn completes with it, never an error, having sent the request exactly once per admission.
/// The turn line carries the engine's own words while goose waits, then the lift.
/// WITHOUT THE FIX: the three retries (1/3..3/3) exhaust on the 503s and the turn ends in
/// "Ran into this error"; no progress line is sent and nobody waits on the admission.
async fn a_hold_is_waited_out_on_the_engines_admission_and_the_turn_completes() {
    let (model, addr) = goose(vec![
        Answer::MemoryHold(REASON),
        Answer::MemoryHold(REASON),
        Answer::Finish(AFTER_THE_HOLD),
    ])
    .await;
    let (requests_before, waits_before) = (model.completion_requests(), model.admission_waits());
    let mut window = Window::open(addr, true).await;
    let work = tempfile::tempdir().unwrap();
    let session_id = window.new_chat(work.path()).await;

    let prompt = window
        .prompt(&session_id, "Plan the Jira migration", None)
        .await;
    let waiting = until_waiting(&mut window, &model, &session_id, waits_before + 1).await;
    assert_eq!(
        waiting["update"]["status"]["message"],
        format!("Waiting: {REASON}")
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        model.completion_requests() - requests_before,
        1,
        "nothing is resent while the engine holds"
    );

    model.admit().await;
    eventually("the second hold is waited on too", || async {
        model.admission_waits() == waits_before + 2
    })
    .await;
    assert_eq!(model.completion_requests() - requests_before, 2);
    model.admit().await;

    assert_eq!(stop_reason(&mut window, prompt).await, "end_turn");
    assert_eq!(model.completion_requests() - requests_before, 3);
    let text = agent_text(&window, &session_id);
    assert!(text.contains(AFTER_THE_HOLD), "{text}");
    assert!(!text.contains("Ran into this error"), "{text}");
    let admitted = goose_providers::engine_hold::ADMITTED_WORDS.to_string();
    assert_eq!(
        hold_lines(&window, &session_id),
        vec![admitted.clone(), format!("Waiting: {REASON}"), admitted],
        "after the first Waiting line (awaited above): the lift, the second hold, its lift"
    );
    window.close().await;
}

/// Stop while the engine holds: the prompt ends cancelled at once — the wait is the provider's
/// future, dropped with the stream — and no request is resent.
/// WITHOUT THE FIX: no wait exists; the turn ends by itself in the retries' error, and the
/// "Waiting:" line this test waits for is never sent.
async fn stop_during_a_hold_ends_the_turn_cleanly() {
    let (model, addr) = goose(vec![Answer::MemoryHold(REASON)]).await;
    let (requests_before, waits_before) = (model.completion_requests(), model.admission_waits());
    let mut window = Window::open(addr, true).await;
    let work = tempfile::tempdir().unwrap();
    let session_id = window.new_chat(work.path()).await;

    let prompt = window
        .prompt(&session_id, "Plan the Jira migration", None)
        .await;
    until_waiting(&mut window, &model, &session_id, waits_before + 1).await;
    window.cancel(&session_id).await;
    assert_eq!(stop_reason(&mut window, prompt).await, "cancelled");
    assert_eq!(model.completion_requests() - requests_before, 1);
    let text = agent_text(&window, &session_id);
    assert!(!text.contains("Ran into this error"), "{text}");
    window.close().await;
}

/// NEGATIVE CONTROL: a 503 with no hold code is any busy engine — today's three retries, then the
/// turn's error; goose never waits on an admission for it.
async fn any_other_503_keeps_the_three_retries() {
    let busy = "Server is busy (max concurrent requests reached)";
    let (model, addr) = goose(vec![
        Answer::Busy(busy),
        Answer::Busy(busy),
        Answer::Busy(busy),
        Answer::Busy(busy),
        Answer::Finish("never reached"),
    ])
    .await;
    let (requests_before, waits_before) = (model.completion_requests(), model.admission_waits());
    let mut window = Window::open(addr, true).await;
    let work = tempfile::tempdir().unwrap();
    let session_id = window.new_chat(work.path()).await;

    let prompt = window
        .prompt(&session_id, "Plan the Jira migration", None)
        .await;
    stop_reason(&mut window, prompt).await;
    assert_eq!(
        model.completion_requests() - requests_before,
        4,
        "the request and its three retries"
    );
    assert_eq!(model.admission_waits(), waits_before);
    let text = agent_text(&window, &session_id);
    assert!(text.contains("Ran into this error"), "{text}");
    assert!(text.contains(busy), "{text}");
    assert_eq!(hold_lines(&window, &session_id), Vec::<String>::new());
    model.script(Vec::new());
    window.close().await;
}

mod tests {
    use super::*;

    #[test]
    #[serial]
    fn a_hold_is_waited_out_on_the_engines_admission_and_the_turn_completes() {
        run(super::a_hold_is_waited_out_on_the_engines_admission_and_the_turn_completes());
    }

    #[test]
    #[serial]
    fn stop_during_a_hold_ends_the_turn_cleanly() {
        run(super::stop_during_a_hold_ends_the_turn_cleanly());
    }

    #[test]
    #[serial]
    fn any_other_503_keeps_the_three_retries() {
        run(super::any_other_503_keeps_the_three_retries());
    }
}
