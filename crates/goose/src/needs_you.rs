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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
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
        resolved_at: None,
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
