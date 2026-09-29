//! Q-504: a turn goose runs on its PROCESS-WIDE agents — one no window's connection sent (an
//! orchestrator subagent's `send_message`, goose-server's reply route) — reads Running in every
//! window, because every connection's `session_activity/get` reads the process-wide manager
//! (`acp/server/needs_you.rs` `busy_sessions`). Before the fix, `session/cancel` looked only in the
//! connection's own prompt runs (`acp/server.rs` `on_cancel`), so no window could stop it.
//!
//! The same rule the other way: a window's cancel never stops a turn ANOTHER window's connection
//! holds — a loop tick included, since a tick is submitted by the window whose door took it — Q-500
//! relays that Stop to the holding window.
//!
//! One runtime, one goose, one scripted model, one test at a time (as the loop tick tests).

#[path = "acp_ws/mod.rs"]
mod acp_ws;

use std::future::Future;
use std::sync::{Arc, LazyLock};

use acp_ws::{configure, eventually, serve, Answer, Model, Window};
use futures::StreamExt;
use goose::agents::{Agent, SessionConfig};
use goose::conversation::message::Message;
use goose::execution::manager::AgentManager;
use goose::session::SessionManager;
use goose_sdk_types::custom_requests::{LoopCadence, LoopTemplateId, LoopsStartRequest};
use serde_json::{json, Value};
use serial_test::serial;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

const SESSION_ACTIVITY: &str = "_goose/unstable/session_activity/get";
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
            configure(&model, false);
            (model, serve().await)
        })
        .await
        .clone();
    model.keep_side_requests_off_the_script();
    model.script(script);
    Bed {
        model,
        addr,
        work: tempfile::tempdir().unwrap(),
    }
}

/// A chat as the desktop opens one (`_meta.client`): a user session, the kind every window's
/// activity read lists.
async fn desktop_chat(window: &mut Window, work: &std::path::Path) -> String {
    window
        .request(
            "session/new",
            json!({"cwd": work, "mcpServers": [], "_meta": {"client": "goose-desktop"}}),
        )
        .await["sessionId"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn running(window: &mut Window) -> Vec<String> {
    window.request(SESSION_ACTIVITY, json!({})).await["running"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["sessionId"].as_str().unwrap().to_string())
        .collect()
}

/// The chat's agent on the process-wide manager, with the chat's model — what the orchestrator's
/// `send_message` does before its turn.
async fn app_wide_agent(manager: &AgentManager, session_id: &str) -> Arc<Agent> {
    let agent = manager
        .get_or_create_agent(session_id.to_string())
        .await
        .unwrap();
    if agent.provider().await.is_err() {
        let session = SessionManager::instance()
            .get_session(session_id, false)
            .await
            .unwrap();
        agent.restore_provider_from_session(&session).await.unwrap();
    }
    agent
}

/// A turn on the process-wide agents that no window sent, through the door the orchestrator's
/// `send_message` and goose-server's reply route use: the manager's cancel token, the reply under
/// it, the token released when the reply ends. Returns whether it ended cancelled.
fn app_wide_turn(
    manager: Arc<AgentManager>,
    agent: Arc<Agent>,
    session_id: String,
) -> (CancellationToken, tokio::task::JoinHandle<bool>) {
    let token = CancellationToken::new();
    let turn_token = token.clone();
    let turn = tokio::spawn(async move {
        manager
            .try_register_cancel_token(&session_id, turn_token.clone())
            .await
            .unwrap();
        let config = SessionConfig {
            id: session_id.clone(),
            schedule_id: None,
            max_turns: None,
            retry_config: None,
        };
        let mut stream = agent
            .reply(
                Message::user().with_text("Summarise the migration findings"),
                config,
                Some(turn_token.clone()),
            )
            .await
            .unwrap();
        loop {
            tokio::select! {
                _ = turn_token.cancelled() => break,
                event = stream.next() => if event.is_none() { break },
            }
        }
        drop(stream);
        manager.unregister_cancel_token(&session_id).await;
        turn_token.is_cancelled()
    });
    (token, turn)
}

/// Q-504: the chat reads Running in a window that never sent its turn, and that window's Stop
/// (`session/cancel` on its own connection) stops it; the chat is idle again everywhere.
async fn a_turn_no_window_sent_stops_from_a_windows_cancel() {
    let bed = bed(vec![Answer::Unfinished("Reading the Jira export")]).await;
    let mut window = Window::open(bed.addr, true).await;
    let chat = desktop_chat(&mut window, bed.work.path()).await;

    let manager = AgentManager::instance().await.unwrap();
    let asked = bed.model.completion_requests();
    let agent = app_wide_agent(&manager, &chat).await;
    let (token, turn) = app_wide_turn(manager.clone(), agent, chat.clone());
    eventually("the app-wide turn is busy", || async {
        manager.is_session_busy(&chat).await
    })
    .await;
    eventually("the app-wide turn reaches the model", || async {
        bed.model.completion_requests() > asked
    })
    .await;
    assert!(
        running(&mut window).await.contains(&chat),
        "every window reads the app-wide turn as Running"
    );

    window.cancel(&chat).await;
    eventually("the window's cancel reaches the app-wide turn", || async {
        token.is_cancelled()
    })
    .await;
    assert!(turn.await.unwrap(), "the turn ended because it was stopped");
    assert!(!manager.is_session_busy(&chat).await);
    assert!(
        !running(&mut window).await.contains(&chat),
        "the chat is idle again"
    );
    window.close().await;
}

/// The rule's other half, on a loop tick: the tick is window A's prompt (the tick door submits it
/// through A's connection), so window B's cancel leaves it running — Q-500 relays B's Stop to A —
/// and A's own cancel stops it.
async fn a_windows_cancel_leaves_a_tick_another_window_runs() {
    let bed = bed(vec![Answer::Unfinished("Running the failing tests")]).await;
    let mut a = Window::open(bed.addr, true).await;
    let mut b = Window::open(bed.addr, true).await;
    let chat = desktop_chat(&mut a, bed.work.path()).await;
    let start = LoopsStartRequest {
        session_id: chat.clone(),
        goal: "Make every test pass".into(),
        template: LoopTemplateId::Blank,
        steps: String::new(),
        cadence: LoopCadence::SelfPaced,
        state_file: ".goose/loops/tests/NOW.md".into(),
        check: None,
        stop_after_ticks: None,
    };
    let started = a
        .request(
            "_goose/unstable/loops/start",
            serde_json::to_value(start).unwrap(),
        )
        .await;
    assert!(started.get("refusal").is_none(), "{started}");
    let due: Value = a
        .notification(LOOP_TICK, |p| {
            p["sessionId"] == chat.as_str() && p["n"] == 1
        })
        .await;
    let tick = a
        .prompt(
            &chat,
            due["prompt"].as_str().unwrap(),
            Some(json!({"goose": {"loopTick": {
                "loopId": due["loopId"], "n": due["n"], "messageId": due["messageId"]
            }}})),
        )
        .await;
    a.until_streaming(&chat).await;

    b.cancel(&chat).await;
    // A notification has no answer: B's next read comes after goose took it, and the tick is
    // watched a while beyond that (a bound on the test, never on the product).
    assert!(
        !running(&mut b).await.contains(&chat),
        "B's own read does not list A's tick (Q-500 relays it)"
    );
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(
            running(&mut a).await.contains(&chat),
            "B's cancel left A's tick running"
        );
    }

    a.cancel(&chat).await;
    let answered = a.response(tick).await.expect("the tick answered");
    assert_eq!(answered["stopReason"], "cancelled");
    let mut idle = false;
    for _ in 0..500 {
        if !running(&mut a).await.contains(&chat) {
            idle = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(idle, "the chat is idle after A's own cancel");
    a.close().await;
    b.close().await;
}

mod tests {
    use super::*;

    #[test]
    #[serial]
    fn a_turn_no_window_sent_stops_from_a_windows_cancel() {
        run(super::a_turn_no_window_sent_stops_from_a_windows_cancel());
    }

    #[test]
    #[serial]
    fn a_windows_cancel_leaves_a_tick_another_window_runs() {
        run(super::a_windows_cancel_leaves_a_tick_another_window_runs());
    }
}
