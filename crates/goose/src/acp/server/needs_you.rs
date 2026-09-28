//! Session activity over ACP: which sessions have a turn in flight and which are waiting on the
//! person (`ask_user` items), and closing an item with the person's answer or a dismissal — or,
//! when the person writes a chat message instead, as superseded by it (Q-298) — unless the message
//! is the card's own answer, marked as such (Q-344).

use std::collections::HashMap;

use goose_sdk_types::custom_requests::{
    BackgroundSessionDto, FailedSessionDto, NeedsYouAction, NeedsYouItemDto,
    NeedsYouStatus as NeedsYouStatusDto, ResolveNeedsYouRequest, ResolveNeedsYouResponse,
    RunningSessionDto, SessionActivityRequest, SessionActivityResponse, StoppedSessionDto,
};
use tracing::warn;

use super::{GooseAcpAgent, ResultExt};
use crate::conversation::message::Message;
use crate::execution::manager::AgentManager;
use crate::needs_you::{self, NeedsYouItem, NeedsYouStatus, Resolution};
use crate::session::{SessionManager, SessionType};

/// Close the item, then tell the loop runner (session loops §4.6, the asked rule): a question a
/// tick left open holds that chat's loop, and its resolution is what lets the next tick be offered
/// — after the answer's turn ends, or at once when dismissed. The runner is told only AFTER the
/// resolution is written, so reading `needs_you.v0` it finds the item closed and how; a refused
/// resolve (unknown item, already closed, empty answer) changed nothing and tells it nothing.
async fn resolve_then_tell_the_loop(
    session_manager: &SessionManager,
    session_id: &str,
    item_id: &str,
    resolution: Resolution,
    loop_resolved: impl FnOnce(&str, &str),
) -> anyhow::Result<NeedsYouItem> {
    let item = needs_you::resolve(session_manager, session_id, item_id, resolution).await?;
    loop_resolved(session_id, item_id);
    Ok(item)
}

/// Q-298: the person's own chat message arrived while this chat had open questions. They close as
/// superseded by it, the model's note is stored BEFORE the message (agent-only: the person sees
/// their words unchanged, the model reads the note and their message as one turn, since
/// consecutive user messages merge), and only then is the loop told — the same order
/// `resolve_then_tell_the_loop` keeps. Returns the superseded items (empty = none was open).
async fn supersede_then_tell_the_loop(
    session_manager: &SessionManager,
    session_id: &str,
    message_text: &str,
    loop_resolved: impl Fn(&str, &str),
) -> anyhow::Result<Vec<NeedsYouItem>> {
    let superseded = needs_you::supersede_open(session_manager, session_id, message_text).await?;
    if superseded.is_empty() {
        return Ok(superseded);
    }
    let note = Message::user()
        .with_text(needs_you::superseded_note(&superseded, message_text))
        .with_visibility(false, true);
    session_manager.add_message(session_id, &note).await?;
    for item in &superseded {
        loop_resolved(session_id, &item.id);
    }
    Ok(superseded)
}

fn dto(
    session_id: &str,
    session_name: &str,
    working_dir: &std::path::Path,
    item: NeedsYouItem,
) -> NeedsYouItemDto {
    NeedsYouItemDto {
        id: item.id,
        session_id: session_id.to_string(),
        session_name: session_name.to_string(),
        working_dir: working_dir.to_string_lossy().to_string(),
        question: item.question,
        why: item.why,
        recommended_answer: item.recommended_answer,
        options: item.options,
        created_at: item.created_at.to_rfc3339(),
        status: match item.status {
            NeedsYouStatus::Open => NeedsYouStatusDto::Open,
            NeedsYouStatus::Answered => NeedsYouStatusDto::Answered,
            NeedsYouStatus::Dismissed => NeedsYouStatusDto::Dismissed,
            NeedsYouStatus::Superseded => NeedsYouStatusDto::Superseded,
        },
        answer: item.answer,
        superseded_by: item.superseded_by,
    }
}

impl GooseAcpAgent {
    /// The busy set of this process: the ACP server's own manager and, when goose-server built
    /// one, the process singleton (the same union LeanZero Link reads). Earliest start wins.
    async fn busy_sessions(&self) -> HashMap<String, chrono::DateTime<chrono::Utc>> {
        let mut busy: HashMap<String, chrono::DateTime<chrono::Utc>> = HashMap::new();
        let mut managers = vec![self.agent_manager.clone()];
        if let Some(shared) = AgentManager::instance_if_built() {
            if !std::sync::Arc::ptr_eq(&shared, &self.agent_manager) {
                managers.push(shared);
            }
        }
        for manager in managers {
            for (session_id, since) in manager.busy_sessions().await {
                busy.entry(session_id)
                    .and_modify(|earliest| *earliest = (*earliest).min(since))
                    .or_insert(since);
            }
        }
        busy
    }

    /// goose's in-flight calls for user and scheduled sessions (Q-185), one row per call, oldest
    /// first — the fact check after a reply keeps its session visibly busy.
    async fn background_sessions(&self) -> Vec<BackgroundSessionDto> {
        let mut background = Vec::new();
        for call in crate::background_work::snapshot() {
            match self
                .session_manager
                .get_session(&call.session_id, false)
                .await
            {
                Ok(session)
                    if matches!(
                        session.session_type,
                        SessionType::User | SessionType::Scheduled
                    ) =>
                {
                    background.push(BackgroundSessionDto {
                        session_id: call.session_id,
                        session_name: session.name,
                        working_dir: session.working_dir.to_string_lossy().to_string(),
                        kind: call.kind,
                        started_at: call.started_at.to_rfc3339(),
                    });
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(session_id = %call.session_id, %error, "a session goose works for is unreadable in the store");
                }
            }
        }
        background
    }

    pub(super) async fn on_session_activity(
        &self,
        _req: SessionActivityRequest,
    ) -> Result<SessionActivityResponse, agent_client_protocol::Error> {
        let mut running = Vec::new();
        for (session_id, since) in self.busy_sessions().await {
            match self.session_manager.get_session(&session_id, false).await {
                Ok(session)
                    if matches!(
                        session.session_type,
                        SessionType::User | SessionType::Scheduled
                    ) =>
                {
                    running.push(RunningSessionDto {
                        session_id,
                        session_name: session.name,
                        working_dir: session.working_dir.to_string_lossy().to_string(),
                        started_at: since.to_rfc3339(),
                    });
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(%session_id, %error, "a busy session is unreadable in the store");
                }
            }
        }
        running.sort_by(|a, b| a.started_at.cmp(&b.started_at));

        let needs_you = needs_you::open_items(&self.session_manager)
            .await
            .internal_err_ctx("Failed to list needs-you items")?
            .into_iter()
            .map(|open| {
                dto(
                    &open.session_id,
                    &open.session_name,
                    &open.working_dir,
                    open.item,
                )
            })
            .collect();

        let failed = crate::turn_outcome::failed_sessions(&self.session_manager)
            .await
            .internal_err_ctx("Failed to list failed sessions")?
            .into_iter()
            .map(|failed| FailedSessionDto {
                session_id: failed.session_id,
                session_name: failed.session_name,
                working_dir: failed.working_dir.to_string_lossy().to_string(),
                failed_at: failed.at.to_rfc3339(),
                reason: failed.reason,
            })
            .collect();
        let stopped = crate::turn_outcome::stopped_sessions(&self.session_manager)
            .await
            .internal_err_ctx("Failed to list stopped sessions")?
            .into_iter()
            .map(|stopped| StoppedSessionDto {
                session_id: stopped.session_id,
                session_name: stopped.session_name,
                working_dir: stopped.working_dir.to_string_lossy().to_string(),
                stopped_at: stopped.at.to_rfc3339(),
                elapsed_ms: stopped.stopped.elapsed_ms,
                output_tokens: stopped.stopped.output_tokens,
            })
            .collect();
        let background = self.background_sessions().await;
        let looping = crate::session_loops::acp::looping(&self.session_manager).await?;
        let notes_waiting = self.notes_waiting().await?;
        Ok(SessionActivityResponse {
            running,
            needs_you,
            failed,
            stopped,
            background,
            looping,
            notes_waiting,
        })
    }

    /// Called by `on_prompt` for the person's own plain message (not a loop tick, not a slash
    /// command) just before the reply stores it. A failed write is logged as an error and the
    /// person's message still goes to the model: when the needs-you store could not be written the
    /// questions stay open and the card stays up where the person sees it; when only the note
    /// could not be stored, the questions are closed and the model reads the message without it.
    pub(super) async fn supersede_open_questions(&self, session_id: &str, message_text: &str) {
        if let Err(error) = supersede_then_tell_the_loop(
            &self.session_manager,
            session_id,
            message_text,
            crate::session_loops::seam::needs_you_resolved,
        )
        .await
        {
            tracing::error!(
                session_id,
                %error,
                "the person's message could not close this chat's open questions as superseded"
            );
        }
    }

    /// Q-344: whether this prompt is the card's own answer, per its `_meta.goose.needsYouAnswers`
    /// mark — then it supersedes nothing: the questions it answers are closed already and the
    /// chat's other open questions stay open for the person. A mark that is not honoured (stale,
    /// forged, malformed, or the store unreadable) is logged with its reason and the message is
    /// read as typed, so it supersedes as Q-298 says.
    pub(super) async fn is_card_answer(
        &self,
        session_id: &str,
        meta: Option<&serde_json::Map<String, serde_json::Value>>,
        message_text: &str,
    ) -> bool {
        let Some(ids) = needs_you::card_answer_ids(meta) else {
            return false;
        };
        let taken = match ids {
            Ok(ids) => {
                needs_you::take_card_answers(&self.session_manager, session_id, &ids, message_text)
                    .await
            }
            Err(error) => Err(error),
        };
        match taken {
            Ok(_) => true,
            Err(error) => {
                warn!(
                    session_id,
                    %error,
                    "the message's needs-you answer mark was not honoured; it supersedes the chat's open questions as a typed message does"
                );
                false
            }
        }
    }

    pub(super) async fn on_resolve_needs_you(
        &self,
        req: ResolveNeedsYouRequest,
    ) -> Result<ResolveNeedsYouResponse, agent_client_protocol::Error> {
        let resolution = match req.action {
            NeedsYouAction::Answer => Resolution::Answered(req.answer.ok_or_else(|| {
                agent_client_protocol::Error::invalid_params().data("An answer needs `answer`")
            })?),
            NeedsYouAction::Dismiss => Resolution::Dismissed,
        };
        let session = self
            .session_manager
            .get_session(&req.session_id, false)
            .await
            .invalid_params_err_ctx("Unknown session")?;
        let item = resolve_then_tell_the_loop(
            &self.session_manager,
            &req.session_id,
            &req.item_id,
            resolution,
            crate::session_loops::seam::needs_you_resolved,
        )
        .await
        .invalid_params_err()?;
        Ok(ResolveNeedsYouResponse {
            item: dto(&session.id, &session.name, &session.working_dir, item),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::needs_you::{NeedsYouState, NewQuestion};
    use crate::session::extension_data::ExtensionState;

    async fn a_chat_with_a_question() -> (tempfile::TempDir, SessionManager, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let manager = SessionManager::new(dir.path().to_path_buf());
        let session = manager
            .create_session(
                dir.path().to_path_buf(),
                "loop chat".into(),
                SessionType::User,
                crate::config::GooseMode::default(),
            )
            .await
            .unwrap();
        let item = needs_you::raise(
            &manager,
            &session.id,
            NewQuestion {
                question: "Which CSV delimiter does the owner want?".into(),
                why: "The generator and the validator must agree on it.".into(),
                recommended_answer: "A comma".into(),
                options: vec![],
            },
        )
        .await
        .unwrap();
        (dir, manager, session.id, item.id)
    }

    /// What the loop runner sees when it is told: the ids, and the item's status in the store at
    /// that moment (the runner reads `needs_you.v0` to tell an answer from a dismissal).
    fn told(
        manager: &SessionManager,
        seen: &Mutex<Vec<(String, String, NeedsYouStatus)>>,
        session_id: &str,
        item_id: &str,
    ) {
        let session = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(manager.get_session(session_id, false))
        })
        .unwrap();
        let status = NeedsYouState::from_extension_data(&session.extension_data)
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.id == item_id)
            .unwrap()
            .status;
        seen.lock()
            .unwrap()
            .push((session_id.to_string(), item_id.to_string(), status));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn needs_you_answered_tells_the_loop_once_after_the_answer_is_written() {
        let (_dir, manager, session_id, item_id) = a_chat_with_a_question().await;
        let seen = Mutex::new(Vec::new());
        let item = resolve_then_tell_the_loop(
            &manager,
            &session_id,
            &item_id,
            Resolution::Answered("A semicolon".into()),
            |s, i| told(&manager, &seen, s, i),
        )
        .await
        .unwrap();
        assert_eq!(item.status, NeedsYouStatus::Answered);
        assert_eq!(
            seen.into_inner().unwrap(),
            vec![(session_id, item_id, NeedsYouStatus::Answered)]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn needs_you_dismissed_tells_the_loop_once_after_the_dismissal_is_written() {
        let (_dir, manager, session_id, item_id) = a_chat_with_a_question().await;
        let seen = Mutex::new(Vec::new());
        resolve_then_tell_the_loop(
            &manager,
            &session_id,
            &item_id,
            Resolution::Dismissed,
            |s, i| told(&manager, &seen, s, i),
        )
        .await
        .unwrap();
        assert_eq!(
            seen.into_inner().unwrap(),
            vec![(session_id, item_id, NeedsYouStatus::Dismissed)]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn needs_you_a_message_supersedes_the_open_question_then_tells_the_loop() {
        let (_dir, manager, session_id, item_id) = a_chat_with_a_question().await;
        let seen = Mutex::new(Vec::new());
        let wrote = "Semicolons — and stop after the next file";
        let superseded = supersede_then_tell_the_loop(&manager, &session_id, wrote, |s, i| {
            told(&manager, &seen, s, i)
        })
        .await
        .unwrap();
        assert_eq!(superseded.len(), 1);
        assert_eq!(superseded[0].status, NeedsYouStatus::Superseded);
        assert_eq!(superseded[0].superseded_by.as_deref(), Some(wrote));
        assert_eq!(
            seen.into_inner().unwrap(),
            vec![(session_id.clone(), item_id, NeedsYouStatus::Superseded)]
        );
        assert!(needs_you::open_items(&manager).await.unwrap().is_empty());

        let messages = manager
            .get_session(&session_id, true)
            .await
            .unwrap()
            .conversation
            .unwrap()
            .messages()
            .to_vec();
        let note = messages.last().unwrap();
        assert!(!note.is_user_visible() && note.is_agent_visible());
        assert_eq!(
            note.as_concat_text(),
            "Your question \"Which CSV delimiter does the owner want?\" was still open on the \
             person's card when they sent the message below instead of answering there. It is \
             now closed as superseded by that message: not answered from the card, and not \
             dismissed. Their message: \"Semicolons — and stop after the next file\"\n\
             Read their words: if they settle it, go on with that; if they do not and you still \
             cannot proceed without an answer, ask again with ask_user."
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn needs_you_a_message_with_nothing_open_changes_nothing() {
        let (_dir, manager, session_id, item_id) = a_chat_with_a_question().await;
        // The card closes its own item before it sends the answer as a message.
        needs_you::resolve(
            &manager,
            &session_id,
            &item_id,
            Resolution::Answered("A comma".into()),
        )
        .await
        .unwrap();
        let seen = Mutex::new(Vec::new());
        let superseded = supersede_then_tell_the_loop(
            &manager,
            &session_id,
            "Answer to your question \"Which CSV delimiter does the owner want?\": A comma",
            |s, i| told(&manager, &seen, s, i),
        )
        .await
        .unwrap();
        assert!(superseded.is_empty());
        assert!(seen.into_inner().unwrap().is_empty());
        let session = manager.get_session(&session_id, true).await.unwrap();
        assert!(session.conversation.unwrap().messages().is_empty());
        let item = NeedsYouState::from_extension_data(&session.extension_data)
            .unwrap()
            .items
            .remove(0);
        assert_eq!(item.status, NeedsYouStatus::Answered);
        assert_eq!(item.superseded_by, None);
    }

    #[test]
    fn needs_you_the_superseded_note_names_every_question_and_an_empty_message() {
        let item = |question: &str| NeedsYouItem {
            id: "ny_1".into(),
            question: question.into(),
            why: "w".into(),
            recommended_answer: "r".into(),
            options: vec![],
            created_at: chrono::Utc::now(),
            status: NeedsYouStatus::Superseded,
            answer: None,
            superseded_by: Some(String::new()),
            resolved_at: None,
            answer_delivered_at: None,
        };
        let note = needs_you::superseded_note(&[item("Comma?"), item("Which folder?")], "  ");
        assert!(
            note.starts_with(
                "Your questions \"Comma?\" and \"Which folder?\" were still open on the person's \
                 card"
            ),
            "{note}"
        );
        assert!(note.contains("They are now closed as superseded"), "{note}");
        assert!(
            note.contains("Their message has no text, only attachments."),
            "{note}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn needs_you_a_refused_resolve_tells_the_loop_nothing() {
        let (_dir, manager, session_id, item_id) = a_chat_with_a_question().await;
        let seen = Mutex::new(Vec::new());
        let refusals = [
            (item_id.as_str(), Resolution::Answered("   ".into())),
            ("ny_not_there", Resolution::Dismissed),
        ];
        for (id, resolution) in refusals {
            assert!(
                resolve_then_tell_the_loop(&manager, &session_id, id, resolution, |s, i| {
                    told(&manager, &seen, s, i)
                })
                .await
                .is_err()
            );
        }
        resolve_then_tell_the_loop(
            &manager,
            &session_id,
            &item_id,
            Resolution::Dismissed,
            |s, i| told(&manager, &seen, s, i),
        )
        .await
        .unwrap();
        // Closed once: a second answer is refused, so the loop hears of the item exactly once.
        assert!(resolve_then_tell_the_loop(
            &manager,
            &session_id,
            &item_id,
            Resolution::Answered("A comma".into()),
            |s, i| told(&manager, &seen, s, i),
        )
        .await
        .is_err());
        assert_eq!(
            seen.into_inner().unwrap(),
            vec![(session_id, item_id, NeedsYouStatus::Dismissed)]
        );
    }

    fn marked(ids: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        serde_json::json!({"goose": {"needsYouAnswers": ids}})
            .as_object()
            .unwrap()
            .clone()
    }

    #[test]
    fn needs_you_the_card_answer_mark_is_read_only_as_a_list_of_ids() {
        assert!(needs_you::card_answer_ids(None).is_none());
        let unmarked = serde_json::json!({"goose": {"loopTick": {}}});
        assert!(needs_you::card_answer_ids(unmarked.as_object()).is_none());
        assert_eq!(
            needs_you::card_answer_ids(Some(&marked(serde_json::json!(["ny_a", "ny_b"]))))
                .unwrap()
                .unwrap(),
            vec!["ny_a".to_string(), "ny_b".to_string()]
        );
        for malformed in [
            serde_json::json!([]),
            serde_json::json!("ny_a"),
            serde_json::json!(["ny_a", 7]),
        ] {
            assert!(
                needs_you::card_answer_ids(Some(&marked(malformed.clone())))
                    .unwrap()
                    .is_err(),
                "{malformed}"
            );
        }
    }

    /// Q-344: the card's answer message is honoured once, only for a question answered on the card
    /// whose words it carries; every refusal writes nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn needs_you_a_card_answer_mark_is_honoured_once_and_only_for_its_own_answer() {
        let (_dir, manager, session_id, item_id) = a_chat_with_a_question().await;
        let ids = vec![item_id.clone()];
        let message = "Answer to your question \"Which CSV delimiter does the owner want?\": A tab";

        let open = needs_you::take_card_answers(&manager, &session_id, &ids, message).await;
        assert!(
            open.unwrap_err()
                .to_string()
                .contains("not answered on the card"),
            "a mark on a question still open does not shield the message"
        );

        needs_you::resolve(
            &manager,
            &session_id,
            &item_id,
            Resolution::Answered("A tab".into()),
        )
        .await
        .unwrap();
        let typed = needs_you::take_card_answers(&manager, &session_id, &ids, "Use pipes").await;
        assert!(
            typed.unwrap_err().to_string().contains("does not carry"),
            "a typed message with a borrowed mark is not the answer"
        );
        let unknown =
            needs_you::take_card_answers(&manager, &session_id, &["ny_other".into()], message)
                .await;
        assert!(unknown
            .unwrap_err()
            .to_string()
            .contains("no needs-you item"));

        let taken = needs_you::take_card_answers(&manager, &session_id, &ids, message)
            .await
            .unwrap();
        assert_eq!(taken.len(), 1);
        assert!(taken[0].answer_delivered_at.is_some());

        let stale = needs_you::take_card_answers(&manager, &session_id, &ids, message).await;
        assert!(
            stale.unwrap_err().to_string().contains("already delivered"),
            "a mark is honoured once"
        );
    }
}
