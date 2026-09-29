//! Q-480: the person said "ask me every remaining question on it now, all at once, one separate
//! question for each" and goose asked ONE. Several questions are several `ask_user` calls in ONE
//! message: every call raises its own card, the turn ends once after the message, and the person
//! then answers the cards one by one, in any order — each answer reaches the model as its own
//! message and never supersedes the cards still open (the sibling rules of Q-340/341/344).
//!
//! Driven the way the desktop drives it: a window on a real websocket to the real ACP router, a
//! desktop chat (the only kind that has `ask_user`) and a scripted model. One goose and one model
//! per process, one test at a time.

#[path = "acp_ws/mod.rs"]
mod acp_ws;

use std::future::Future;
use std::sync::LazyLock;

use acp_ws::{configure, serve, Answer, Model, Window};
use goose::needs_you::{self, NeedsYouItem, NeedsYouState, NeedsYouStatus, Resolution};
use goose::session::extension_data::ExtensionState;
use goose::session::SessionManager;
use serde_json::{json, Value};
use serial_test::serial;
use tokio::sync::OnceCell;

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

const SUPERSEDED_NOTE: &str = "closed as superseded";

/// The three open questions of the café's notes, and the answer the person gives each.
const QUESTIONS: [(&str, &str); 3] = [
    (
        "Is the fruit scone one price in both shops?",
        "Three fifty in both shops",
    ),
    (
        "Does the flapjack use oats from a gluten-free mill?",
        "Yes, the Kilkenny mill",
    ),
    (
        "Should the menu list the soup of the day?",
        "No, it changes too often",
    ),
];

fn ask_user(question: &str) -> (&'static str, String) {
    (
        "ask_user",
        json!({
            "question": question,
            "why": "The menu page needs it.",
            "recommended_answer": "Leave it as it is",
        })
        .to_string(),
    )
}

async fn goose(script: Vec<Answer>) -> (Model, std::net::SocketAddr) {
    let (model, addr) = GOOSE
        .get_or_init(|| async {
            let model = Model::start(Vec::new()).await;
            model.keep_side_requests_off_the_script();
            configure(&model, false);
            (model, serve().await)
        })
        .await
        .clone();
    model.script(script);
    model.requests.lock().unwrap().clear();
    (model, addr)
}

async fn items(chat: &str) -> Vec<NeedsYouItem> {
    let session = SessionManager::instance()
        .get_session(chat, false)
        .await
        .unwrap();
    NeedsYouState::from_extension_data(&session.extension_data)
        .map(|state| state.items)
        .unwrap_or_default()
}

async fn item_for(chat: &str, question: &str) -> NeedsYouItem {
    items(chat)
        .await
        .into_iter()
        .find(|item| item.question == question)
        .unwrap_or_else(|| panic!("no card for {question:?}"))
}

async fn send(window: &mut Window, chat: &str, text: &str, meta: Option<Value>) {
    let prompt = window.prompt(chat, text, meta).await;
    let answered = window.response(prompt).await.expect("the prompt answered");
    assert_eq!(answered["stopReason"], "end_turn", "{answered}");
}

fn last_request(model: &Model) -> String {
    model
        .requests
        .lock()
        .unwrap()
        .last()
        .expect("the model was called")
        .clone()
}

async fn three_asks_in_one_message_then_three_answers_one_by_one() {
    let (model, addr) = goose(vec![
        Answer::ToolCalls(QUESTIONS.iter().map(|(q, _)| ask_user(q)).collect()),
        Answer::Finish("Noted the soup."),
        Answer::Finish("Noted the scone."),
        Answer::Finish("Noted the flapjack."),
    ])
    .await;
    let mut window = Window::open(addr, true).await;
    let work = tempfile::tempdir().unwrap();
    let chat = window
        .request(
            "session/new",
            json!({"cwd": work.path(), "mcpServers": [], "_meta": {"client": "goose-desktop"}}),
        )
        .await["sessionId"]
        .as_str()
        .unwrap()
        .to_string();

    send(
        &mut window,
        &chat,
        "Ask me every remaining question now, all at once, one separate question for each",
        None,
    )
    .await;

    assert_eq!(
        model.completion_requests(),
        1,
        "the turn ends once, after the message that asked all three"
    );
    for (question, _) in QUESTIONS {
        assert_eq!(
            item_for(&chat, question).await.status,
            NeedsYouStatus::Open,
            "every call raised its own card: {question}"
        );
    }
    assert_eq!(items(&chat).await.len(), QUESTIONS.len());

    // Answered out of order, as a person on a phone would.
    let mut answered: Vec<&str> = Vec::new();
    for index in [2, 0, 1] {
        let (question, answer) = QUESTIONS[index];
        let card = item_for(&chat, question).await;
        needs_you::resolve(
            &SessionManager::instance(),
            &chat,
            &card.id,
            Resolution::Answered(answer.into()),
        )
        .await
        .unwrap();
        send(
            &mut window,
            &chat,
            &format!("Answer to your question \"{question}\": {answer}"),
            Some(json!({"goose": {"needsYouAnswers": [card.id]}})),
        )
        .await;
        answered.push(question);

        let request = last_request(&model);
        assert!(request.contains(answer), "the model read {answer:?}");
        assert!(
            !request.contains(SUPERSEDED_NOTE),
            "a card's answer supersedes nothing"
        );
        if answered.len() == 1 {
            for id in ["call_1", "call_1_1", "call_1_2"] {
                assert!(
                    request.contains(id),
                    "the three asks of one message each carry their result: {id}"
                );
            }
        }
        let delivered = item_for(&chat, question).await;
        assert_eq!(delivered.status, NeedsYouStatus::Answered);
        assert!(delivered.answer_delivered_at.is_some());
        for (other, _) in QUESTIONS {
            let status = item_for(&chat, other).await.status;
            if answered.contains(&other) {
                assert_eq!(status, NeedsYouStatus::Answered, "{other}");
            } else {
                assert_eq!(
                    status,
                    NeedsYouStatus::Open,
                    "a card not yet answered stays open: {other}"
                );
            }
        }
    }
    assert_eq!(model.completion_requests(), 1 + QUESTIONS.len());
    window.close().await;
}

mod tests {
    use super::*;

    #[test]
    #[serial]
    fn three_asks_in_one_message_then_three_answers_one_by_one() {
        run(super::three_asks_in_one_message_then_three_answers_one_by_one());
    }
}
