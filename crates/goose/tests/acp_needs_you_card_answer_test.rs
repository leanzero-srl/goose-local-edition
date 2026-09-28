//! Q-344: answering one of two open questions ON ITS CARD must not close the other. The card's
//! answer reaches the model as the next chat message, and Q-298 closes every question open when the
//! person's own message arrives — so the desktop marks the card's answer message with
//! `_meta.goose.needsYouAnswers = [item ids]` and goosed supersedes nothing for it. A typed message
//! still supersedes every open question, and a mark that is not the card's answer (a question not
//! answered on the card, a message that does not carry the answer, a mark already used) is read as
//! typed.
//!
//! Driven the way the desktop drives it: a window on a real websocket to the real ACP router and a
//! scripted model. One goose and one model per process, one test at a time.

#[path = "acp_ws/mod.rs"]
mod acp_ws;

use std::future::Future;
use std::sync::LazyLock;

use acp_ws::{configure, serve, Answer, Model, Window};
use goose::needs_you::{
    self, NeedsYouItem, NeedsYouState, NeedsYouStatus, NewQuestion, Resolution,
};
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

const DATABASE: &str = "Which database should the service use?";
const DELIMITER: &str = "Which CSV delimiter does the export use?";
const SUPERSEDED_NOTE: &str = "closed as superseded";

struct Bed {
    model: Model,
    window: Window,
    chat: String,
    _work: tempfile::TempDir,
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
    model.script(script);
    model.requests.lock().unwrap().clear();
    let mut window = Window::open(addr, true).await;
    let work = tempfile::tempdir().unwrap();
    let chat = window.new_chat(work.path()).await;
    Bed {
        model,
        window,
        chat,
        _work: work,
    }
}

async fn ask(chat: &str, question: &str) -> String {
    needs_you::raise(
        &SessionManager::instance(),
        chat,
        NewQuestion {
            question: question.into(),
            why: "The next step depends on it.".into(),
            recommended_answer: "The first option".into(),
            options: vec![],
        },
    )
    .await
    .unwrap()
    .id
}

/// What the card does first: the question is closed on the engine with the person's words.
async fn answer_on_card(chat: &str, item_id: &str, answer: &str) {
    needs_you::resolve(
        &SessionManager::instance(),
        chat,
        item_id,
        Resolution::Answered(answer.into()),
    )
    .await
    .unwrap();
}

/// The message the card sends next, in the words `answerMessage` gives it on the desktop.
fn answer_message(question: &str, answer: &str) -> String {
    format!("Answer to your question \"{question}\": {answer}")
}

fn card_mark(ids: &[&str]) -> Value {
    json!({"goose": {"needsYouAnswers": ids}})
}

async fn send(bed: &mut Bed, text: &str, meta: Option<Value>) {
    let prompt = bed.window.prompt(&bed.chat, text, meta).await;
    let answered = bed
        .window
        .response(prompt)
        .await
        .expect("the prompt answered");
    assert_eq!(answered["stopReason"], "end_turn", "{answered}");
}

async fn item(chat: &str, item_id: &str) -> NeedsYouItem {
    let session = SessionManager::instance()
        .get_session(chat, false)
        .await
        .unwrap();
    NeedsYouState::from_extension_data(&session.extension_data)
        .unwrap()
        .items
        .into_iter()
        .find(|item| item.id == item_id)
        .unwrap()
}

fn last_request_carries_the_superseded_note(model: &Model) -> bool {
    model
        .requests
        .lock()
        .unwrap()
        .last()
        .expect("the model was called")
        .contains(SUPERSEDED_NOTE)
}

/// Two questions open; the person answers the first on its card while the chat is idle. The
/// second stays open for them, and the model reads the answer with no superseded note.
async fn answering_one_card_leaves_the_other_open() {
    let mut bed = bed(vec![Answer::Finish("Noted: PostgreSQL.")]).await;
    let database = ask(&bed.chat, DATABASE).await;
    let delimiter = ask(&bed.chat, DELIMITER).await;

    answer_on_card(&bed.chat, &database, "PostgreSQL").await;
    send(
        &mut bed,
        &answer_message(DATABASE, "PostgreSQL"),
        Some(card_mark(&[&database])),
    )
    .await;

    let answered = item(&bed.chat, &database).await;
    assert_eq!(answered.status, NeedsYouStatus::Answered);
    assert!(answered.answer_delivered_at.is_some());
    let other = item(&bed.chat, &delimiter).await;
    assert_eq!(
        other.status,
        NeedsYouStatus::Open,
        "the question the person has not got to is still theirs to answer"
    );
    assert!(!last_request_carries_the_superseded_note(&bed.model));
    bed.window.close().await;
}

/// Answers queued while a turn ran go as ONE message naming every answered item (Q-341); a third
/// question still open stays open.
async fn a_batch_of_queued_answers_leaves_the_rest_open() {
    let mut bed = bed(vec![Answer::Finish("Noted both.")]).await;
    let database = ask(&bed.chat, DATABASE).await;
    let delimiter = ask(&bed.chat, DELIMITER).await;
    let third = ask(&bed.chat, "Should the export include archived rows?").await;

    answer_on_card(&bed.chat, &database, "PostgreSQL").await;
    answer_on_card(&bed.chat, &delimiter, "A tab").await;
    let batch = format!(
        "{}\n\n{}",
        answer_message(DATABASE, "PostgreSQL"),
        answer_message(DELIMITER, "A tab")
    );
    send(&mut bed, &batch, Some(card_mark(&[&database, &delimiter]))).await;

    assert_eq!(item(&bed.chat, &third).await.status, NeedsYouStatus::Open);
    assert!(!last_request_carries_the_superseded_note(&bed.model));
    bed.window.close().await;
}

/// Q-298 still holds: a message the person types closes every open question as superseded.
async fn a_typed_message_supersedes_every_open_question() {
    let mut bed = bed(vec![Answer::Finish("Understood.")]).await;
    let database = ask(&bed.chat, DATABASE).await;
    let delimiter = ask(&bed.chat, DELIMITER).await;

    send(&mut bed, "Use SQLite and semicolons", None).await;

    assert_eq!(
        item(&bed.chat, &database).await.status,
        NeedsYouStatus::Superseded
    );
    assert_eq!(
        item(&bed.chat, &delimiter).await.status,
        NeedsYouStatus::Superseded
    );
    assert!(last_request_carries_the_superseded_note(&bed.model));
    bed.window.close().await;
}

/// A mark that is not the card's answer is read as typed: a question still open named in it, a
/// typed message borrowing an answered question's id, and the same mark sent a second time.
async fn a_mark_that_is_not_the_cards_answer_supersedes_as_typed() {
    let mut bed = bed(vec![
        Answer::Finish("One."),
        Answer::Finish("Two."),
        Answer::Finish("Three."),
    ])
    .await;

    // Names a question that was never answered on the card.
    let database = ask(&bed.chat, DATABASE).await;
    send(&mut bed, "Use SQLite", Some(card_mark(&[&database]))).await;
    assert_eq!(
        item(&bed.chat, &database).await.status,
        NeedsYouStatus::Superseded
    );
    assert!(last_request_carries_the_superseded_note(&bed.model));

    // A typed message that borrows the id of a question answered on the card.
    let delimiter = ask(&bed.chat, DELIMITER).await;
    let archived = ask(&bed.chat, "Should the export include archived rows?").await;
    answer_on_card(&bed.chat, &delimiter, "A tab").await;
    send(
        &mut bed,
        "Also skip the header row",
        Some(card_mark(&[&delimiter])),
    )
    .await;
    assert_eq!(
        item(&bed.chat, &archived).await.status,
        NeedsYouStatus::Superseded
    );
    assert!(item(&bed.chat, &delimiter)
        .await
        .answer_delivered_at
        .is_none());

    // The genuine answer message, but its mark was already honoured once.
    let owner = ask(&bed.chat, "Who owns the export job?").await;
    let region = ask(&bed.chat, "Which region hosts it?").await;
    answer_on_card(&bed.chat, &owner, "The data team").await;
    let owner_answer = answer_message("Who owns the export job?", "The data team");
    send(&mut bed, &owner_answer, Some(card_mark(&[&owner]))).await;
    assert_eq!(item(&bed.chat, &region).await.status, NeedsYouStatus::Open);
    bed.model.script(vec![Answer::Finish("Four.")]);
    send(&mut bed, &owner_answer, Some(card_mark(&[&owner]))).await;
    assert_eq!(
        item(&bed.chat, &region).await.status,
        NeedsYouStatus::Superseded,
        "a mark is honoured once: its second use is read as typed"
    );
    bed.window.close().await;
}

mod tests {
    use super::*;

    #[test]
    #[serial]
    fn answering_one_card_leaves_the_other_open() {
        run(super::answering_one_card_leaves_the_other_open());
    }

    #[test]
    #[serial]
    fn a_batch_of_queued_answers_leaves_the_rest_open() {
        run(super::a_batch_of_queued_answers_leaves_the_rest_open());
    }

    #[test]
    #[serial]
    fn a_typed_message_supersedes_every_open_question() {
        run(super::a_typed_message_supersedes_every_open_question());
    }

    #[test]
    #[serial]
    fn a_mark_that_is_not_the_cards_answer_supersedes_as_typed() {
        run(super::a_mark_that_is_not_the_cards_answer_supersedes_as_typed());
    }
}
