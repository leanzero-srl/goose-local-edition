//! Q-298: the person wrote a chat message while a question was open instead of answering it on the
//! card. The question stayed "Needs you" in five places for 70 minutes, because the card's answer
//! was the only path that closed it. The person's message now closes it as SUPERSEDED, recorded on
//! the question, and the model reads a note that says so and quotes the message.

#[allow(dead_code)]
#[path = "acp_common_tests/mod.rs"]
mod common_tests;

use common_tests::fixtures::server::AcpServerConnection;
use common_tests::fixtures::{
    run_test, Connection, OpenAiFixture, PermissionDecision, Session, SessionData,
    TestConnectionConfig,
};
use goose::conversation::message::Message;
use goose::needs_you::{self, NeedsYouState, NeedsYouStatus, NewQuestion};
use goose::session::extension_data::ExtensionState;
use goose::session::SessionManager;

const QUESTION: &str = "Which database should the service use?";
const WROTE: &str = "Use SQLite, and add a README while you are at it";

#[test]
fn a_message_sent_while_a_question_is_open_supersedes_it_and_the_model_is_told() {
    run_test(async {
        let expected_session_id = AcpServerConnection::expected_session_id();
        // The request only matches when the model's context carries the note, the question and
        // the person's message quoted (JSON-escaped in the request body).
        let note = format!(
            r#"Your question \"{QUESTION}\" was still open on the person's card when they sent the message below instead of answering there. It is now closed as superseded by that message: not answered from the card, and not dismissed. Their message: \"{WROTE}\""#
        );
        let openai = OpenAiFixture::new(
            vec![(note, include_str!("acp_test_data/openai_basic.txt"))],
            expected_session_id.clone(),
        )
        .await;
        let mut conn = AcpServerConnection::new(TestConnectionConfig::default(), openai).await;
        let SessionData { mut session, .. } = conn.new_session().await.unwrap();
        expected_session_id.set(&session.session_id().0);

        let store = SessionManager::new(conn.data_root());
        // The test client names no `client` in its meta, so its chat is an ACP session; the
        // desktop's is a User one. Superseding does not read the type.
        let chats = store.list_all_sessions().await.unwrap();
        assert_eq!(chats.len(), 1);
        let chat = chats[0].id.clone();
        let asked = needs_you::raise(
            &store,
            &chat,
            NewQuestion {
                question: QUESTION.into(),
                why: "The schema and the migration tool depend on it.".into(),
                recommended_answer: "PostgreSQL".into(),
                options: vec!["SQLite".into()],
            },
        )
        .await
        .unwrap();

        let output = session
            .prompt(WROTE, PermissionDecision::Cancel)
            .await
            .unwrap();
        assert_eq!(
            output.text, "2",
            "the model's request carried the superseded note"
        );

        assert!(
            needs_you::open_items(&store).await.unwrap().is_empty(),
            "no surface can list the question as needing the person any more"
        );
        let session_row = store.get_session(&chat, true).await.unwrap();
        let item = NeedsYouState::from_extension_data(&session_row.extension_data)
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.id == asked.id)
            .unwrap();
        assert_eq!(item.status, NeedsYouStatus::Superseded);
        assert_eq!(item.superseded_by.as_deref(), Some(WROTE));
        assert_eq!(item.answer, None);
        assert!(item.resolved_at.is_some());

        // The note is the model's alone and comes just before the person's message, which the
        // person sees exactly as they typed it.
        let messages: Vec<Message> = session_row.conversation.unwrap().messages().to_vec();
        let wrote_at = messages
            .iter()
            .position(|m| m.as_concat_text() == WROTE)
            .expect("the person's message is stored");
        let person = &messages[wrote_at];
        assert!(person.is_user_visible() && person.is_agent_visible());
        let note = &messages[wrote_at - 1];
        assert!(!note.is_user_visible() && note.is_agent_visible());
        assert!(note.as_concat_text().contains("closed as superseded"));
    });
}
