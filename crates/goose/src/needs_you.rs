//! "Needs you": a question the model put to the person, pinned until it is answered or dismissed.
//!
//! An item lives in its session's `extension_data` (`needs_you.v0`), so it survives an app restart,
//! goes with the session when it is deleted, and every write is a per-key transaction that no other
//! extension's write can erase.
//!
//! THE TURN ENDS WHEN THE QUESTION IS ASKED. `ask_user` records the item and the agent loop stops;
//! the answer arrives later as the person's next message. Nothing waits in memory, so there is no
//! timeout to expire, no turn held open across a restart, and the answer is an ordinary user message
//! in the conversation — the one place goose already never loses.
//!
//! THE PERSON MAY WRITE INSTEAD OF ANSWERING (Q-298). Their next chat message then closes every
//! open question of that chat as SUPERSEDED — its own status, never folded into "answered" or
//! "dismissed" — with the message recorded on the question, and the model is told so in words that
//! quote it. Keeping the question open across the message was the bug: the model's next turn ran
//! on while five surfaces still said "Needs you" for as long as the chat stood. Reporting it as a
//! dismissal is the other known failure (anthropics/claude-code#88850: the model reads a refusal the
//! person never made). Superseded names what happened and decides nothing for them.
//!
//! THE CARD'S OWN ANSWER IS NOT A MESSAGE THAT SUPERSEDES (Q-344). An answer given on the card is
//! recorded first (Answered), then reaches the model as the next chat message — which, read as a
//! plain message, would close every OTHER question still open as superseded. So the client marks
//! that message (`_meta.goose.needsYouAnswers = [item ids]`) and the mark is honoured only when it
//! names questions of this chat that were answered on the card, whose answers this message carries
//! and that no earlier message delivered ([`take_card_answers`]). Anything else — a stale mark, a
//! forged one, a malformed one — is refused loudly and the message supersedes as a typed one does.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::session::extension_data::ExtensionState;
use crate::session::{SessionManager, SessionType};

/// A tool result carrying this meta key ends the turn once the tool batch it belongs to completes.
pub const END_TURN_META_KEY: &str = "goose_end_turn";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeedsYouStatus {
    Open,
    Answered,
    Dismissed,
    /// The person sent a chat message while the question was open instead of answering it.
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeedsYouItem {
    pub id: String,
    pub question: String,
    pub why: String,
    pub recommended_answer: String,
    #[serde(default)]
    pub options: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub status: NeedsYouStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// The text of the message that superseded the question, verbatim (empty when it carried only
    /// attachments). Set exactly when `status` is `Superseded`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
    /// When the chat message carrying the card's answer arrived (Q-344): a mark naming this item is
    /// honoured once, so a stale mark cannot shield a later typed message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_delivered_at: Option<DateTime<Utc>>,
}

/// Resolved items stay in the list: the record of what was asked and what the person said.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NeedsYouState {
    pub items: Vec<NeedsYouItem>,
}

impl ExtensionState for NeedsYouState {
    const EXTENSION_NAME: &'static str = "needs_you";
    const VERSION: &'static str = "v0";
}

#[derive(Debug, Clone, Default)]
pub struct NewQuestion {
    pub question: String,
    pub why: String,
    pub recommended_answer: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    Answered(String),
    Dismissed,
}

#[derive(Debug, Clone)]
pub struct OpenNeedsYou {
    pub session_id: String,
    pub session_name: String,
    pub working_dir: PathBuf,
    pub item: NeedsYouItem,
}

/// Only a session a PERSON answers may raise an item: a user session on a host that renders the
/// card (the desktop). Swarm workers (Hidden), subagents (SubAgent), scheduled and gateway runs, a
/// benchmark's `goose run` / `goose swarm run` children (CLI hosts) and a knowledge-blind benchmark
/// agent are refused at registration, so the tool never exists there to be called.
pub fn session_accepts_questions(
    human_host: bool,
    knowledge_blind: bool,
    session_type: Option<SessionType>,
) -> bool {
    human_host && !knowledge_blind && session_type == Some(SessionType::User)
}

fn validated(question: NewQuestion) -> Result<NewQuestion> {
    let text = |value: String, field: &str| -> Result<String> {
        let value = value.trim().to_string();
        if value.is_empty() {
            return Err(anyhow!("`{field}` must not be empty"));
        }
        Ok(value)
    };
    let mut options: Vec<String> = Vec::new();
    for option in question.options {
        let option = option.trim().to_string();
        if !option.is_empty() && !options.contains(&option) {
            options.push(option);
        }
    }
    Ok(NewQuestion {
        question: text(question.question, "question")?,
        why: text(question.why, "why")?,
        recommended_answer: text(question.recommended_answer, "recommended_answer")?,
        options,
    })
}

pub async fn raise(
    session_manager: &SessionManager,
    session_id: &str,
    question: NewQuestion,
) -> Result<NeedsYouItem> {
    let question = validated(question)?;
    let item = NeedsYouItem {
        id: format!("ny_{}", Uuid::new_v4().simple()),
        question: question.question,
        why: question.why,
        recommended_answer: question.recommended_answer,
        options: question.options,
        created_at: Utc::now(),
        status: NeedsYouStatus::Open,
        answer: None,
        superseded_by: None,
        resolved_at: None,
        answer_delivered_at: None,
    };
    session_manager
        .update_extension_state::<NeedsYouState, _>(session_id, |state| {
            let mut state = state.unwrap_or_default();
            state.items.push(item.clone());
            Ok((state, item))
        })
        .await
}

pub async fn resolve(
    session_manager: &SessionManager,
    session_id: &str,
    item_id: &str,
    resolution: Resolution,
) -> Result<NeedsYouItem> {
    session_manager
        .update_extension_state::<NeedsYouState, _>(session_id, |state| {
            let mut state = state.unwrap_or_default();
            let item = state
                .items
                .iter_mut()
                .find(|item| item.id == item_id)
                .ok_or_else(|| anyhow!("No needs-you item {item_id} in session {session_id}"))?;
            if item.status != NeedsYouStatus::Open {
                return Err(anyhow!("Needs-you item {item_id} is already resolved"));
            }
            match resolution {
                Resolution::Answered(answer) => {
                    let answer = answer.trim().to_string();
                    if answer.is_empty() {
                        return Err(anyhow!("An answer must not be empty"));
                    }
                    item.status = NeedsYouStatus::Answered;
                    item.answer = Some(answer);
                }
                Resolution::Dismissed => item.status = NeedsYouStatus::Dismissed,
            }
            item.resolved_at = Some(Utc::now());
            let resolved = item.clone();
            Ok((state, resolved))
        })
        .await
}

/// The person's chat message arrived while questions were open: every open item of the session is
/// closed as superseded by it, in one transaction, and returned oldest first (empty = none was
/// open). Called for the person's own plain message only — never a loop tick, a slash command, or
/// the card's answer (the card closes its item before it sends).
pub async fn supersede_open(
    session_manager: &SessionManager,
    session_id: &str,
    message_text: &str,
) -> Result<Vec<NeedsYouItem>> {
    session_manager
        .update_extension_state::<NeedsYouState, _>(session_id, |state| {
            let mut state = state.unwrap_or_default();
            let now = Utc::now();
            let mut superseded = Vec::new();
            for item in state
                .items
                .iter_mut()
                .filter(|item| item.status == NeedsYouStatus::Open)
            {
                item.status = NeedsYouStatus::Superseded;
                item.superseded_by = Some(message_text.to_string());
                item.resolved_at = Some(now);
                superseded.push(item.clone());
            }
            superseded.sort_by_key(|item| item.created_at);
            Ok((state, superseded))
        })
        .await
}

/// The prompt `_meta` key under `goose` that marks a chat message as the card's answer (Q-344).
pub const CARD_ANSWER_META_KEY: &str = "needsYouAnswers";

/// The item ids a prompt's `_meta.goose.needsYouAnswers` names: `None` when the prompt carries no
/// mark, `Some(Err)` when it carries one that is not a non-empty list of ids.
pub fn card_answer_ids(
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<Result<Vec<String>>> {
    let mark = meta?.get("goose")?.get(CARD_ANSWER_META_KEY)?;
    let ids = mark.as_array().and_then(|ids| {
        ids.iter()
            .map(|id| id.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
    });
    Some(match ids {
        Some(ids) if !ids.is_empty() => Ok(ids),
        _ => Err(anyhow!(
            "the needs-you answer mark is not a list of item ids: {mark}"
        )),
    })
}

/// Q-344: the message is the card's answer to `item_ids`. Honoured — `Ok`, the items stamped as
/// delivered in the same transaction — only when every id is a question of this chat answered on
/// the card, whose recorded answer this message carries, and whose answer no earlier message
/// delivered. Anything else is `Err` with the reason and nothing is written: the caller then
/// treats the message as typed, so a mark can never keep a question open that a typed message
/// would close unless the message really is that question's answer.
pub async fn take_card_answers(
    session_manager: &SessionManager,
    session_id: &str,
    item_ids: &[String],
    message_text: &str,
) -> Result<Vec<NeedsYouItem>> {
    session_manager
        .update_extension_state::<NeedsYouState, _>(session_id, |state| {
            let mut state = state.unwrap_or_default();
            let now = Utc::now();
            let mut taken = Vec::new();
            for item_id in item_ids {
                let item = state
                    .items
                    .iter_mut()
                    .find(|item| &item.id == item_id)
                    .ok_or_else(|| anyhow!("no needs-you item {item_id} in this chat"))?;
                if item.status != NeedsYouStatus::Answered {
                    return Err(anyhow!(
                        "needs-you item {item_id} was not answered on the card (it is {:?})",
                        item.status
                    ));
                }
                if let Some(at) = item.answer_delivered_at {
                    return Err(anyhow!(
                        "the answer to needs-you item {item_id} was already delivered at {at}"
                    ));
                }
                let carried = item
                    .answer
                    .as_deref()
                    .is_some_and(|answer| !answer.is_empty() && message_text.contains(answer));
                if !carried {
                    return Err(anyhow!(
                        "the message does not carry the recorded answer to needs-you item {item_id}"
                    ));
                }
                item.answer_delivered_at = Some(now);
                taken.push(item.clone());
            }
            Ok((state, taken))
        })
        .await
}

/// What the model reads just before the person's message when that message superseded its open
/// questions: which questions, that they were neither answered nor dismissed, and the message
/// quoted — so it decides from the person's words, not from a status it has to guess.
pub fn superseded_note(items: &[NeedsYouItem], message_text: &str) -> String {
    let questions = items
        .iter()
        .map(|item| format!("\"{}\"", item.question))
        .collect::<Vec<_>>()
        .join(" and ");
    let (subject, was, they_are) = match items.len() {
        1 => ("Your question", "was", "It is"),
        _ => ("Your questions", "were", "They are"),
    };
    let quoted = match message_text.trim() {
        "" => "Their message has no text, only attachments.".to_string(),
        text => format!("Their message: \"{text}\""),
    };
    format!(
        "{subject} {questions} {was} still open on the person's card when they sent the message \
         below instead of answering there. {they_are} now closed as superseded by that message: \
         not answered from the card, and not dismissed. {quoted}\n\
         Read their words: if they settle it, go on with that; if they do not and you still cannot \
         proceed without an answer, ask again with ask_user."
    )
}

/// Every open item in every session, oldest first.
pub async fn open_items(session_manager: &SessionManager) -> Result<Vec<OpenNeedsYou>> {
    let mut open: Vec<OpenNeedsYou> = session_manager
        .sessions_with_extension_state::<NeedsYouState>()
        .await?
        .into_iter()
        .flat_map(|row| {
            let session_id = row.session_id;
            let session_name = row.name;
            let working_dir = row.working_dir;
            row.state
                .items
                .into_iter()
                .filter(|item| item.status == NeedsYouStatus::Open)
                .map(move |item| OpenNeedsYou {
                    session_id: session_id.clone(),
                    session_name: session_name.clone(),
                    working_dir: working_dir.clone(),
                    item,
                })
        })
        .collect();
    open.sort_by_key(|entry| entry.item.created_at);
    Ok(open)
}
