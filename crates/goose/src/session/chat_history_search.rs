//! The per-turn history search recall runs (Q-358: moved from a LIKE scan over
//! `json_each(content_json)` — 0.2–0.45 s a turn on the 2026-09-25 history — to the transcript
//! index). The ranking law is unchanged: the messages that carry the MOST of the request's terms
//! survive the limit first, then the newest; a term matches a whole word, or a longer word it is a
//! prefix of when the memory store's `term_occurrences` allows one, so "go" no longer matches
//! "goose".

use crate::conversation::message::MessageContent;
use crate::session::session_manager::SessionType;
use crate::session::transcript_index::{self, IndexCoverage, SearchFilter, Term};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{Pool, Sqlite};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct ChatRecallResult {
    pub session_id: String,
    pub session_description: String,
    pub session_working_dir: String,
    pub last_activity: DateTime<Utc>,
    pub total_messages_in_session: usize,
    pub messages: Vec<ChatRecallMessage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatRecallMessage {
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ChatRecallResults {
    pub results: Vec<ChatRecallResult>,
    pub total_matches: usize,
    /// How much of the history the index held when this search ran — a partial index is a
    /// partial answer, and the caller says so.
    pub index: IndexCoverage,
}

type MessageRow = (i64, String, String, String, String, String, DateTime<Utc>);

type SessionMessageGroup = (String, String, Vec<(String, String, DateTime<Utc>)>);

pub struct ChatHistorySearch<'a> {
    pool: &'a Pool<Sqlite>,
    query: &'a str,
    limit: usize,
    filter: SearchFilter,
}

/// The index term for one request word: a prefix exactly when the memory store's matcher would
/// let the word stand for a longer one — asked of `term_occurrences` itself, so the two never
/// disagree about the length at which a prefix starts.
fn recall_term(word: String) -> Term {
    let longer = format!("{word}x");
    let prefix = goose_memory_store::term_occurrences(&word, std::slice::from_ref(&longer)) > 0;
    Term {
        words: vec![word],
        prefix,
    }
}

impl<'a> ChatHistorySearch<'a> {
    pub fn new(
        pool: &'a Pool<Sqlite>,
        query: &'a str,
        limit: Option<usize>,
        after_date: Option<DateTime<Utc>>,
        before_date: Option<DateTime<Utc>>,
        exclude_session_id: Option<String>,
        session_types: Vec<SessionType>,
    ) -> Self {
        Self {
            pool,
            query,
            limit: limit.unwrap_or(10),
            filter: SearchFilter {
                session_types,
                exclude_session: exclude_session_id,
                after: after_date,
                before: before_date,
                ..SearchFilter::default()
            },
        }
    }

    pub async fn execute(self) -> Result<ChatRecallResults> {
        let index = transcript_index::coverage(self.pool).await?;
        let mut words = transcript_index::words(self.query);
        words.sort();
        words.dedup();
        if words.is_empty() {
            return Ok(ChatRecallResults {
                results: vec![],
                total_matches: 0,
                index,
            });
        }

        let mut coverage: HashMap<i64, (usize, DateTime<Utc>)> = HashMap::new();
        for word in words {
            let term = recall_term(word);
            for (id, at) in transcript_index::matching_ids(self.pool, &term, &self.filter).await? {
                coverage.entry(id).or_insert((0, at)).0 += 1;
            }
        }
        let mut ranked: Vec<(i64, usize, DateTime<Utc>)> = coverage
            .into_iter()
            .map(|(id, (count, at))| (id, count, at))
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(b.0.cmp(&a.0)));
        ranked.truncate(self.limit);

        let rows = self.fetch_rows(&ranked).await?;
        let session_messages = Self::process_rows(rows);
        let session_totals = self.get_session_totals(&session_messages).await?;
        Ok(Self::convert_to_results(
            session_messages,
            session_totals,
            index,
        ))
    }

    async fn fetch_rows(&self, ranked: &[(i64, usize, DateTime<Utc>)]) -> Result<Vec<MessageRow>> {
        let mut rows = Vec::with_capacity(ranked.len());
        for (id, _, _) in ranked {
            let row: MessageRow = sqlx::query_as(
                r#"
                SELECT m.id, s.id,
                       CASE WHEN s.name != '' THEN s.name ELSE s.description END,
                       s.working_dir, m.role, m.content_json, m.timestamp
                FROM messages m JOIN sessions s ON s.id = m.session_id
                WHERE m.id = ?
                "#,
            )
            .bind(id)
            .fetch_one(self.pool)
            .await?;
            rows.push(row);
        }
        Ok(rows)
    }

    fn process_rows(rows: Vec<MessageRow>) -> HashMap<String, SessionMessageGroup> {
        let mut session_messages: HashMap<String, SessionMessageGroup> = HashMap::new();

        for (
            id,
            session_id,
            session_description,
            session_working_dir,
            role,
            content_json,
            timestamp,
        ) in rows
        {
            let content_vec = match serde_json::from_str::<Vec<MessageContent>>(&content_json) {
                Ok(content_vec) => content_vec,
                Err(err) => {
                    tracing::warn!(
                        message_id = id,
                        %err,
                        "chat_history_message_unreadable: an indexed message does not parse as message content; recall skips it"
                    );
                    continue;
                }
            };
            let text_parts = Self::extract_text_content(content_vec);
            if text_parts.is_empty() {
                continue;
            }
            let entry = session_messages.entry(session_id).or_insert((
                session_description,
                session_working_dir,
                Vec::new(),
            ));
            entry.2.push((role, text_parts.join("\n"), timestamp));
        }

        session_messages
    }

    fn extract_text_content(content_vec: Vec<MessageContent>) -> Vec<String> {
        content_vec
            .into_iter()
            .filter_map(|content| match content {
                MessageContent::Text(ref tc) => Some(tc.text.clone()),
                MessageContent::ToolRequest(ref tr) => {
                    Some(format!("[Tool: {}]", tr.to_readable_string()))
                }
                MessageContent::ToolResponse(_) => Some("[Tool Response]".to_string()),
                MessageContent::Thinking(ref t) => Some(format!("[Thinking: {}]", t.thinking)),
                _ => None,
            })
            .collect()
    }

    async fn get_session_totals(
        &self,
        session_messages: &HashMap<String, SessionMessageGroup>,
    ) -> Result<HashMap<String, usize>> {
        let mut session_totals: HashMap<String, usize> = HashMap::new();
        for session_id in session_messages.keys() {
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE session_id = ?")
                    .bind(session_id)
                    .fetch_one(self.pool)
                    .await?;
            session_totals.insert(session_id.clone(), count as usize);
        }
        Ok(session_totals)
    }

    fn convert_to_results(
        session_messages: HashMap<String, SessionMessageGroup>,
        session_totals: HashMap<String, usize>,
        index: IndexCoverage,
    ) -> ChatRecallResults {
        let mut results: Vec<ChatRecallResult> = session_messages
            .into_iter()
            .filter_map(|(session_id, (description, working_dir, messages))| {
                let message_vec: Vec<ChatRecallMessage> = messages
                    .into_iter()
                    .map(|(role, content, timestamp)| ChatRecallMessage {
                        role,
                        content,
                        timestamp,
                    })
                    .collect();
                let last_activity = message_vec.iter().map(|m| m.timestamp).max()?;
                let total_messages_in_session = *session_totals.get(&session_id)?;
                Some(ChatRecallResult {
                    session_id,
                    session_description: description,
                    session_working_dir: working_dir,
                    last_activity,
                    total_messages_in_session,
                    messages: message_vec,
                })
            })
            .collect();

        results.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));

        let total_matches = results.iter().map(|r| r.messages.len()).sum();
        ChatRecallResults {
            results,
            total_matches,
            index,
        }
    }
}
