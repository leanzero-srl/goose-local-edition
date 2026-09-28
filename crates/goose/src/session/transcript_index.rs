//! The transcript index (Q-358 S1): one row of a contentless FTS5 table per message of every chat a
//! person can read back — never a hidden swarm worker's — so goose searches other chats in a
//! millisecond instead of the 0.2–0.45 s LIKE scan over `json_each(content_json)` that ran every
//! turn before.
//!
//! Two columns: `said` (the message's text parts — what the person and goose said) and `tool_io`
//! (each tool call's name and arguments, each tool result's text and error, and the text of a
//! message goose keeps for itself). SQL triggers on `messages` keep it current for every write path
//! (insert, delete, update of `content_json` or `metadata_json`) and
//! on `sessions` for a session that becomes or stops being hidden. Rows older than the migration
//! are indexed by `backfill`, newest first, in batches OUTSIDE the startup migration transaction;
//! `messages_fts_state.backfill_below` is its watermark, so a process that dies mid-way resumes
//! where the last committed batch left it.
//!
//! Contentless (`content=''`) keeps the index at ~40 MB for the 24,661 readable messages of the
//! 2026-09-28 history (a full-content table measured 156 MB), so snippets are cut in Rust from the
//! message's own `content_json` (`message_text`), with the same rule the SQL extraction applies.

use crate::session::extension_data::ExtensionState;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Pool, Sqlite, Transaction};

pub const FTS_TABLE: &str = "messages_fts";
const STATE_TABLE: &str = "messages_fts_state";

/// Every session that is not a hidden swarm worker. Written as two ranges, not `!= 'hidden'`, so
/// SQLite reads it from `idx_sessions_type` instead of scanning the sessions table — measured on
/// the 2026-09-28 history (73,267 sessions, 72,658 hidden): the count of readable messages took
/// 27 ms with `!=` and 4 ms with the ranges.
const READABLE_SESSIONS: &str =
    "SELECT id FROM sessions WHERE session_type < 'hidden' OR session_type > 'hidden'";

// measured: on a copy of the 2026-09-28 history (24,661 readable rows, debug build) 50 batches of 500
// took 5.5 s — median 60 ms, slowest 0.3 s (a batch of long tool outputs) — so a chat's own message
// write waits at most that long behind one batch, well inside the pool's 30 s busy timeout.
pub const BACKFILL_BATCH_ROWS: i64 = 500;

// ratio: inside one message the words it SAID count four times its tool traffic in bm25. A weight
// alone cannot keep chat text above tool output — measured 2026-09-28 on the real history, the
// ranking of "split mesh", "killpg" and "tenant" with tool output included was the same at weights
// 1, 2, 4, 8, 10, 20 and 50, and a short `git branch` listing repeating both words outranked a long
// message saying them once at 4:1 — so `ranked` orders every message whose SAID text matches ahead
// of every tool-only match first, and the weight ranks within that.
pub const SAID_WEIGHT: f64 = 4.0;
// ratio: the unit the said weight is measured against.
pub const TOOL_IO_WEIGHT: f64 = 1.0;

/// A message goose keeps for itself and never shows in the chat (`userVisible: false`): a
/// compaction summary, or "goose's record of an earlier tool call, condensed to save context" —
/// 1,547 of the 24,661 readable messages on 2026-09-28. Its text is not what anyone SAID, so it is
/// indexed as goose's own traffic, with the tool calls; searched as said, the condensed records
/// came back labelled as the person's words.
fn agent_only_sql(meta: &str) -> String {
    format!("(json_valid({meta}) AND json_extract({meta}, '$.userVisible') = 0)")
}

/// The text a message SAID: its text parts joined by newlines, unless goose kept it for itself.
/// Mirrors `message_text`'s `said`.
fn said_sql(content: &str, meta: &str) -> String {
    format!(
        "(CASE WHEN {} THEN NULL ELSE \
          (SELECT group_concat(json_extract(p.value, '$.text'), char(10)) FROM json_each({content}) p \
            WHERE json_extract(p.value, '$.type') = 'text') END)",
        agent_only_sql(meta)
    )
}

/// The tool traffic of a message: the text goose kept for itself, each call's name and arguments,
/// each result's text parts and error. Mirrors `message_text`'s `tool_io`.
fn tool_io_sql(content: &str, meta: &str) -> String {
    let agent_only = agent_only_sql(meta);
    format!(
        "(SELECT group_concat(t, char(10)) FROM ( \
           SELECT json_extract(p.value, '$.text') AS t \
             FROM json_each({content}) p WHERE json_extract(p.value, '$.type') = 'text' AND {agent_only} \
           UNION ALL \
           SELECT json_extract(p.value, '$.toolCall.value.name') || ' ' || \
                  COALESCE(json_extract(p.value, '$.toolCall.value.arguments'), '') AS t \
             FROM json_each({content}) p WHERE json_extract(p.value, '$.type') = 'toolRequest' \
           UNION ALL \
           SELECT json_extract(r.value, '$.text') \
             FROM json_each({content}) p, json_each(p.value, '$.toolResult.value.content') r \
            WHERE json_extract(p.value, '$.type') = 'toolResponse' \
              AND json_extract(r.value, '$.type') = 'text' \
           UNION ALL \
           SELECT json_extract(p.value, '$.toolResult.error') \
             FROM json_each({content}) p WHERE json_extract(p.value, '$.type') = 'toolResponse'))"
    )
}

/// A message row belongs in the index when its session is not a hidden swarm worker and its JSON
/// parses (a malformed row is never indexed — `json_each` would fail the chat's own write — and
/// the footer's count shows it missing).
fn indexable_sql(session_id: &str, content: &str) -> String {
    format!(
        "(SELECT session_type FROM sessions WHERE id = {session_id}) IS NOT 'hidden' \
         AND json_valid({content})"
    )
}

/// Create the index, its watermark and the triggers. Run inside migration v15 and inside a fresh
/// schema; both are the startup transaction, so this does no per-row work: the watermark is set to
/// one past the newest message, and `backfill` indexes everything below it later.
pub(crate) async fn create_objects(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    let said_new = said_sql("NEW.content_json", "NEW.metadata_json");
    let tool_new = tool_io_sql("NEW.content_json", "NEW.metadata_json");
    let new_ok = indexable_sql("NEW.session_id", "NEW.content_json");
    let said_m = said_sql("m.content_json", "m.metadata_json");
    let tool_m = tool_io_sql("m.content_json", "m.metadata_json");
    let statements = [
        format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS {FTS_TABLE} USING fts5(said, tool_io, content='', contentless_delete=1)"
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {STATE_TABLE} (id INTEGER PRIMARY KEY CHECK (id = 1), backfill_below INTEGER NOT NULL)"
        ),
        format!(
            "INSERT OR IGNORE INTO {STATE_TABLE} (id, backfill_below) SELECT 1, COALESCE(MAX(id) + 1, 0) FROM messages"
        ),
        // A rowid is deleted before it is inserted everywhere: a plain INSERT over an existing
        // rowid of a contentless-delete table leaves the old row's words matching (measured with
        // SQLite 3.53 on 2026-09-28), and REPLACE on `messages` fires no delete trigger.
        format!(
            "CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages WHEN {new_ok} BEGIN \
               DELETE FROM {FTS_TABLE} WHERE rowid = NEW.id; \
               INSERT INTO {FTS_TABLE} (rowid, said, tool_io) VALUES (NEW.id, {said_new}, {tool_new}); \
             END"
        ),
        format!(
            "CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN \
               DELETE FROM {FTS_TABLE} WHERE rowid = OLD.id; \
             END"
        ),
        format!(
            "CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE OF content_json, metadata_json ON messages BEGIN \
               DELETE FROM {FTS_TABLE} WHERE rowid = OLD.id; \
               INSERT INTO {FTS_TABLE} (rowid, said, tool_io) SELECT NEW.id, {said_new}, {tool_new} WHERE {new_ok}; \
             END"
        ),
        format!(
            "CREATE TRIGGER IF NOT EXISTS sessions_fts_hidden AFTER UPDATE OF session_type ON sessions \
             WHEN NEW.session_type = 'hidden' AND OLD.session_type IS NOT 'hidden' BEGIN \
               DELETE FROM {FTS_TABLE} WHERE rowid IN (SELECT id FROM messages WHERE session_id = NEW.id); \
             END"
        ),
        format!(
            "CREATE TRIGGER IF NOT EXISTS sessions_fts_unhidden AFTER UPDATE OF session_type ON sessions \
             WHEN OLD.session_type = 'hidden' AND NEW.session_type IS NOT 'hidden' BEGIN \
               INSERT OR REPLACE INTO {FTS_TABLE} (rowid, said, tool_io) \
                 SELECT m.id, {said_m}, {tool_m} FROM messages m \
                  WHERE m.session_id = NEW.id AND json_valid(m.content_json); \
             END"
        ),
    ];
    for statement in statements {
        sqlx::query(&statement).execute(&mut **tx).await?;
    }
    Ok(())
}

/// How much of the readable history the index holds, both sides MEASURED: `indexed` counts the
/// index's own rows, `indexable` the messages of every session that is not hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct IndexCoverage {
    pub indexed: u64,
    pub indexable: u64,
    /// The backfill has older rows still to index (its watermark is above zero).
    pub backfilling: bool,
}

impl IndexCoverage {
    pub fn missing(&self) -> u64 {
        self.indexable.saturating_sub(self.indexed)
    }

    /// The footer every search result ends with. A partial index says how much is missing and why,
    /// so no answer reads as complete when it is not (gate 1: a loud absence, never a quiet one).
    pub fn footer(&self) -> String {
        let head = format!(
            "Indexed {} of {} messages.",
            group_digits(self.indexed),
            group_digits(self.indexable)
        );
        let missing = self.missing();
        if missing == 0 {
            head
        } else if self.backfilling {
            format!(
                "{head} {} older messages are not indexed yet — the index is still being built, so \
                 these results can miss them; searching again later covers more.",
                group_digits(missing)
            )
        } else {
            format!(
                "{head} {} messages could not be indexed (their stored content does not parse), so \
                 these results cannot include them.",
                group_digits(missing)
            )
        }
    }
}

pub fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub(crate) async fn coverage(pool: &Pool<Sqlite>) -> Result<IndexCoverage> {
    let indexed: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {FTS_TABLE}_docsize"))
        .fetch_one(pool)
        .await?;
    let indexable: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions s JOIN messages m ON m.session_id = s.id \
         WHERE s.session_type < 'hidden' OR s.session_type > 'hidden'",
    )
    .fetch_one(pool)
    .await?;
    let below = backfill_below(pool).await?;
    Ok(IndexCoverage {
        indexed: indexed as u64,
        indexable: indexable as u64,
        backfilling: below > 0,
    })
}

pub(crate) async fn backfill_below(pool: &Pool<Sqlite>) -> Result<i64> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT backfill_below FROM {STATE_TABLE} WHERE id = 1"
    ))
    .fetch_one(pool)
    .await?)
}

/// One committed backfill step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillStep {
    /// Rows `[from, below)` of the readable history were indexed; the watermark is now `from`.
    Indexed { rows: u64, from: i64 },
    /// Nothing is left below the watermark; it is now zero.
    Done,
}

/// Index the newest `batch` readable rows below the watermark and lower it, in ONE `BEGIN
/// IMMEDIATE` transaction — the watermark is read inside it, so two processes backfilling the same
/// file take turns instead of indexing a batch twice.
pub(crate) async fn backfill_step(pool: &Pool<Sqlite>, batch: i64) -> Result<BackfillStep> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let below: i64 = sqlx::query_scalar(&format!(
        "SELECT backfill_below FROM {STATE_TABLE} WHERE id = 1"
    ))
    .fetch_one(&mut *tx)
    .await?;
    if below <= 0 {
        tx.commit().await?;
        return Ok(BackfillStep::Done);
    }
    let from: Option<i64> = sqlx::query_scalar(&format!(
        "SELECT MIN(id) FROM (SELECT id FROM messages WHERE id < ? AND session_id IN \
         ({READABLE_SESSIONS}) ORDER BY id DESC LIMIT ?)"
    ))
    .bind(below)
    .bind(batch)
    .fetch_one(&mut *tx)
    .await?;
    let step = match from {
        Some(from) => {
            let said = said_sql("m.content_json", "m.metadata_json");
            let tool = tool_io_sql("m.content_json", "m.metadata_json");
            let rows = sqlx::query(&format!(
                "INSERT OR REPLACE INTO {FTS_TABLE} (rowid, said, tool_io) \
                 SELECT m.id, {said}, {tool} FROM messages m \
                  WHERE m.id >= ? AND m.id < ? AND json_valid(m.content_json) \
                    AND m.session_id IN ({READABLE_SESSIONS})"
            ))
            .bind(from)
            .bind(below)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            sqlx::query(&format!(
                "UPDATE {STATE_TABLE} SET backfill_below = ? WHERE id = 1"
            ))
            .bind(from)
            .execute(&mut *tx)
            .await?;
            BackfillStep::Indexed { rows, from }
        }
        None => {
            sqlx::query(&format!(
                "UPDATE {STATE_TABLE} SET backfill_below = 0 WHERE id = 1"
            ))
            .execute(&mut *tx)
            .await?;
            BackfillStep::Done
        }
    };
    tx.commit().await?;
    Ok(step)
}

/// Run `backfill_step` until the watermark reaches zero. A failed step is logged loudly and ends
/// this run; the watermark keeps the last committed batch, the next start resumes from it, and
/// every search footer meanwhile says how much is missing.
pub(crate) async fn backfill(pool: Pool<Sqlite>) {
    let started = std::time::Instant::now();
    let mut indexed = 0u64;
    let mut batches = 0u64;
    let mut slowest_batch = std::time::Duration::ZERO;
    loop {
        let step_started = std::time::Instant::now();
        match backfill_step(&pool, BACKFILL_BATCH_ROWS).await {
            Ok(BackfillStep::Indexed { rows, .. }) => {
                indexed += rows;
                batches += 1;
                slowest_batch = slowest_batch.max(step_started.elapsed());
                tokio::task::yield_now().await;
            }
            Ok(BackfillStep::Done) => {
                tracing::info!(
                    indexed,
                    batches,
                    slowest_batch_ms = slowest_batch.as_millis(),
                    elapsed_ms = started.elapsed().as_millis(),
                    "transcript_index_backfilled"
                );
                return;
            }
            Err(err) => {
                tracing::error!(
                    %err,
                    indexed,
                    "transcript_index_backfill_failed: older messages stay out of chat search until the next start resumes the backfill"
                );
                return;
            }
        }
    }
}

/// The per-chat opt-out, "Keep this chat out of search" (`chat_search.v0` in `extension_data`).
/// Honoured when searching, reading and listing — the chat stays in the index, so turning the flag
/// off brings it straight back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSearchState {
    #[serde(default)]
    pub keep_out: bool,
}

impl ExtensionState for ChatSearchState {
    const EXTENSION_NAME: &'static str = "chat_search";
    const VERSION: &'static str = "v0";
}

/// SQL true for a session that is NOT kept out of search (`s` is the sessions alias).
pub(crate) const SEARCHABLE_SESSION_SQL: &str =
    "COALESCE(json_extract(s.extension_data, '$.\"chat_search.v0\".keep_out'), 0) = 0";

/// A message's two indexed texts, extracted in Rust with the rule the SQL triggers apply — the
/// snippets are cut from these because a contentless index stores no text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageText {
    pub said: String,
    pub tool_io: String,
}

/// Whether a stored message is one goose keeps for itself (`userVisible: false` in its metadata).
pub fn agent_only(metadata_json: Option<&str>) -> bool {
    metadata_json
        .and_then(|meta| serde_json::from_str::<Value>(meta).ok())
        .and_then(|meta| meta.get("userVisible").and_then(Value::as_bool))
        == Some(false)
}

pub fn message_text(content_json: &str, metadata_json: Option<&str>) -> MessageText {
    let parts: Vec<Value> = match serde_json::from_str(content_json) {
        Ok(Value::Array(parts)) => parts,
        _ => return MessageText::default(),
    };
    let agent_only = agent_only(metadata_json);
    let str_at = |v: &Value, path: &[&str]| -> Option<String> {
        let mut cur = v;
        for key in path {
            cur = cur.get(*key)?;
        }
        match cur {
            Value::String(s) => Some(s.clone()),
            Value::Null => None,
            other => Some(other.to_string()),
        }
    };
    let mut said = Vec::new();
    let mut requests = Vec::new();
    let mut results = Vec::new();
    let mut errors = Vec::new();
    for part in &parts {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => said.extend(str_at(part, &["text"])),
            Some("toolRequest") => {
                if let Some(name) = str_at(part, &["toolCall", "value", "name"]) {
                    let args =
                        str_at(part, &["toolCall", "value", "arguments"]).unwrap_or_default();
                    requests.push(format!("{name} {args}"));
                }
            }
            Some("toolResponse") => {
                if let Some(Value::Array(items)) = part
                    .get("toolResult")
                    .and_then(|r| r.get("value"))
                    .and_then(|v| v.get("content"))
                {
                    for item in items {
                        if item.get("type").and_then(Value::as_str) == Some("text") {
                            results.extend(str_at(item, &["text"]));
                        }
                    }
                }
                errors.extend(str_at(part, &["toolResult", "error"]));
            }
            _ => {}
        }
    }
    let (said, mut tool_io) = if agent_only {
        (Vec::new(), said)
    } else {
        (said, Vec::new())
    };
    tool_io.extend(requests);
    tool_io.extend(results);
    tool_io.extend(errors);
    MessageText {
        said: said.join("\n"),
        tool_io: tool_io.join("\n"),
    }
}

/// One term of a search: the words of a bare word or a "quoted phrase" as the index tokenizes them,
/// optionally a prefix (`word*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub words: Vec<String>,
    pub prefix: bool,
}

impl Term {
    fn fts(&self) -> String {
        let star = if self.prefix { " *" } else { "" };
        format!("\"{}\"{star}", self.words.join(" "))
    }

    /// Whether a lower-cased token is this term's first word (or starts with it, for a prefix).
    fn opens(&self, token: &str) -> bool {
        match self.words.first() {
            Some(first) if self.prefix && self.words.len() == 1 => token.starts_with(first),
            Some(first) => token == first,
            None => false,
        }
    }
}

/// The index's word split: lower-cased runs of letters and digits — what FTS5's `unicode61`
/// tokenizer keeps (`_`, `.`, `/` and `-` separate words, so "tenant_id" is "tenant id").
pub fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// A parsed search: words, "phrases" and `prefix*` terms joined by AND unless the person writes OR
/// between two of them, and `-excluded` terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    /// Each positive term and whether an explicit OR joins it to the previous one.
    pub positive: Vec<(Term, bool)>,
    pub excluded: Vec<Term>,
}

impl SearchQuery {
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut positive: Vec<(Term, bool)> = Vec::new();
        let mut excluded = Vec::new();
        let mut or_next = false;
        let mut rest = input.trim();
        while !rest.is_empty() {
            let negate = rest.starts_with('-');
            if negate {
                rest = &rest[1..];
            }
            let (raw, quoted, after) = if let Some(stripped) = rest.strip_prefix('"') {
                match stripped.find('"') {
                    Some(end) => (&stripped[..end], true, &stripped[end + 1..]),
                    None => (stripped, true, ""),
                }
            } else {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                (&rest[..end], false, &rest[end..])
            };
            let prefix_after_quote = quoted && after.starts_with('*');
            let after = if prefix_after_quote {
                &after[1..]
            } else {
                after
            };
            rest = after.trim_start();
            if !quoted && !negate && raw == "OR" {
                or_next = !positive.is_empty();
                continue;
            }
            let prefix = prefix_after_quote || (!quoted && raw.ends_with('*'));
            let words = words(raw);
            if words.is_empty() {
                continue;
            }
            let term = Term { words, prefix };
            if negate {
                excluded.push(term);
            } else {
                positive.push((term, or_next));
                or_next = false;
            }
        }
        if positive.is_empty() {
            return Err(if excluded.is_empty() {
                "the query has no words to search for".to_string()
            } else {
                "the query only excludes words; add at least one word to search for".to_string()
            });
        }
        Ok(Self { positive, excluded })
    }

    /// Two or more terms and no OR written: the query `any` can broaden to.
    pub fn broadenable(&self) -> bool {
        self.positive.len() > 1 && self.positive.iter().all(|(_, or)| !or)
    }

    /// The FTS5 expression; `any` joins every positive term with OR (the broadened search).
    pub fn fts(&self, any: bool) -> String {
        let mut expr = String::new();
        for (i, (term, or)) in self.positive.iter().enumerate() {
            if i > 0 {
                expr.push_str(if any || *or { " OR " } else { " AND " });
            }
            expr.push_str(&term.fts());
        }
        if self.excluded.is_empty() {
            format!("({expr})")
        } else {
            let excluded: Vec<String> = self.excluded.iter().map(Term::fts).collect();
            format!("({expr}) NOT ({})", excluded.join(" OR "))
        }
    }

    pub fn terms(&self) -> impl Iterator<Item = &Term> {
        self.positive.iter().map(|(term, _)| term)
    }
}

/// Which texts of a message a search reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// What the person and goose said (the default).
    Said,
    /// Said and tool traffic (`include_tool_output`).
    SaidAndTools,
    /// Tool traffic only (`role: tool`).
    Tools,
}

impl Scope {
    fn columns(self) -> &'static str {
        match self {
            Scope::Said => "{said}",
            Scope::SaidAndTools => "{said tool_io}",
            Scope::Tools => "{tool_io}",
        }
    }
}

/// Everything a transcript search filters on, besides the words.
#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    pub session_types: Vec<crate::session::SessionType>,
    pub exclude_session: Option<String>,
    /// One chat: its id, or words of its title.
    pub chat: Option<String>,
    /// A working folder; its subfolders count.
    pub folder: Option<String>,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    /// `user` or `assistant`: only that side's messages.
    pub role: Option<String>,
    /// A tool name the message's tool traffic carries.
    pub tool: Option<String>,
    /// A file path said or used.
    pub file: Option<String>,
}

enum Bind {
    Text(String),
    Time(DateTime<Utc>),
}

/// The chats kept out of search, read once per search: a `json_extract` of `extension_data` per
/// MATCHING row cost 5 of 8 ms on a 1,602-hit term (2026-09-28 history); the LIKE finds the few
/// sessions carrying the key at all before any JSON is parsed.
pub(crate) async fn kept_out_sessions(pool: &Pool<Sqlite>) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT s.id FROM sessions s WHERE s.id IN ({READABLE_SESSIONS}) \
           AND s.extension_data LIKE '%chat\\_search.v0%' ESCAPE '\\' \
           AND NOT ({SEARCHABLE_SESSION_SQL})"
    ))
    .fetch_all(pool)
    .await?)
}

impl SearchFilter {
    /// The MATCH expression and the WHERE clause after it, with their binds in order.
    fn clause(&self, query: &str, scope: Scope, kept_out: &[String]) -> (String, Vec<Bind>) {
        let mut expr = format!("{} : {query}", scope.columns());
        if let Some(tool) = &self.tool {
            let tool = Term {
                words: words(tool),
                prefix: false,
            };
            if !tool.words.is_empty() {
                expr = format!("({expr}) AND ({{tool_io}} : {})", tool.fts());
            }
        }
        if let Some(file) = &self.file {
            let file = Term {
                words: words(file),
                prefix: false,
            };
            if !file.words.is_empty() {
                expr = format!("({expr}) AND ({{said tool_io}} : {})", file.fts());
            }
        }
        let mut sql = format!("{FTS_TABLE} MATCH ?");
        let mut binds = vec![Bind::Text(expr)];
        if !kept_out.is_empty() {
            let marks = vec!["?"; kept_out.len()].join(", ");
            sql.push_str(&format!(" AND m.session_id NOT IN ({marks})"));
            binds.extend(kept_out.iter().cloned().map(Bind::Text));
        }
        if !self.session_types.is_empty() {
            let marks = vec!["?"; self.session_types.len()].join(", ");
            sql.push_str(&format!(" AND s.session_type IN ({marks})"));
            binds.extend(self.session_types.iter().map(|t| Bind::Text(t.to_string())));
        }
        if let Some(id) = &self.exclude_session {
            sql.push_str(" AND m.session_id != ?");
            binds.push(Bind::Text(id.clone()));
        }
        if let Some(chat) = &self.chat {
            sql.push_str(" AND (s.id = ? OR s.name LIKE ?)");
            binds.push(Bind::Text(chat.clone()));
            binds.push(Bind::Text(format!("%{chat}%")));
        }
        if let Some(folder) = &self.folder {
            let folder = folder.trim_end_matches('/').to_string();
            sql.push_str(" AND (s.working_dir = ? OR s.working_dir LIKE ?)");
            binds.push(Bind::Text(folder.clone()));
            binds.push(Bind::Text(format!("{folder}/%")));
        }
        // The column holds "YYYY-MM-DD HH:MM:SS" and a bound DateTime encodes as RFC 3339; compared as
        // text a same-day later message passed a `before` bound (VA-189), so both go through datetime().
        if let Some(after) = self.after {
            sql.push_str(" AND datetime(m.timestamp) >= datetime(?)");
            binds.push(Bind::Time(after));
        }
        if let Some(before) = self.before {
            sql.push_str(" AND datetime(m.timestamp) <= datetime(?)");
            binds.push(Bind::Time(before));
        }
        if let Some(role) = &self.role {
            sql.push_str(" AND m.role = ?");
            binds.push(Bind::Text(role.clone()));
        }
        (sql, binds)
    }
}

const FROM_INDEX: &str =
    "FROM messages_fts JOIN messages m ON m.id = messages_fts.rowid JOIN sessions s ON s.id = m.session_id";

fn bind_all<'q, O>(
    mut q: sqlx::query::QueryAs<'q, Sqlite, O, sqlx::sqlite::SqliteArguments<'q>>,
    binds: Vec<Bind>,
) -> sqlx::query::QueryAs<'q, Sqlite, O, sqlx::sqlite::SqliteArguments<'q>> {
    for bind in binds {
        q = match bind {
            Bind::Text(text) => q.bind(text),
            Bind::Time(time) => q.bind(time),
        };
    }
    q
}

/// One matching message, as the ranked search returns it.
#[derive(Debug, Clone)]
pub struct Hit {
    pub message_id: i64,
    pub session_id: String,
    pub session_name: String,
    pub working_dir: String,
    pub role: String,
    pub timestamp: DateTime<Utc>,
    pub content_json: String,
    pub metadata_json: Option<String>,
    pub rank: f64,
}

type HitRow = (
    String,
    String,
    String,
    String,
    DateTime<Utc>,
    String,
    Option<String>,
);

/// The best `limit` hits — every message whose said text matches before any tool-only match, then
/// by bm25 with said weighted above tool traffic — and how many hits and chats match in all. The
/// ranking reads ids only; content is fetched for the `limit` winners alone (reading it for every
/// match took 176 ms on a 3,723-hit query of the 2026-09-28 history).
pub(crate) async fn ranked(
    pool: &Pool<Sqlite>,
    query: &str,
    scope: Scope,
    filter: &SearchFilter,
    limit: i64,
) -> Result<(Vec<Hit>, u64, u64)> {
    let kept_out = kept_out_sessions(pool).await?;
    let (where_sql, binds) = filter.clause(query, scope, &kept_out);
    let sql = format!(
        "SELECT m.id, bm25({FTS_TABLE}, {SAID_WEIGHT:?}, {TOOL_IO_WEIGHT:?}) AS rank \
         {FROM_INDEX} WHERE {where_sql} \
         ORDER BY (m.id IN (SELECT rowid FROM {FTS_TABLE} WHERE {FTS_TABLE} MATCH ?)) DESC, rank \
         LIMIT ?"
    );
    let winners: Vec<(i64, f64)> = bind_all(sqlx::query_as(&sql), binds)
        .bind(format!("{{said}} : {query}"))
        .bind(limit)
        .fetch_all(pool)
        .await?;
    let (where_sql, binds) = filter.clause(query, scope, &kept_out);
    let (hits, chats): (i64, i64) = bind_all(
        sqlx::query_as(&format!(
            "SELECT COUNT(*), COUNT(DISTINCT m.session_id) {FROM_INDEX} WHERE {where_sql}"
        )),
        binds,
    )
    .fetch_one(pool)
    .await?;
    let mut out = Vec::with_capacity(winners.len());
    for (message_id, rank) in winners {
        let (session_id, session_name, working_dir, role, timestamp, content_json, metadata_json): HitRow =
            sqlx::query_as(
                "SELECT m.session_id, s.name, s.working_dir, m.role, m.timestamp, m.content_json, \
                        m.metadata_json \
                 FROM messages m JOIN sessions s ON s.id = m.session_id WHERE m.id = ?",
            )
            .bind(message_id)
            .fetch_one(pool)
            .await?;
        out.push(Hit {
            message_id,
            session_id,
            session_name,
            working_dir,
            role,
            timestamp,
            content_json,
            metadata_json,
            rank,
        });
    }
    Ok((out, hits as u64, chats as u64))
}

/// Every message matching one term in `said` — the id sets recall's coverage ranking counts across
/// terms. Ids only: reading each row's timestamp, stored after its content, doubled a 1,602-hit
/// term's time (3 → 15 ms on the 2026-09-28 history).
pub(crate) async fn matching_ids(
    pool: &Pool<Sqlite>,
    term: &Term,
    filter: &SearchFilter,
    kept_out: &[String],
) -> Result<Vec<i64>> {
    let (where_sql, binds) = filter.clause(&term.fts(), Scope::Said, kept_out);
    Ok(bind_all(
        sqlx::query_as::<_, (i64,)>(&format!("SELECT m.id {FROM_INDEX} WHERE {where_sql}")),
        binds,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(id,)| id)
    .collect())
}

/// One stored message of a chat, as `read_chat` shows it.
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub message_id: i64,
    pub role: String,
    pub timestamp: DateTime<Utc>,
    pub content_json: String,
    pub metadata_json: Option<String>,
}

/// A chat as the chat tools name it.
#[derive(Debug, Clone)]
pub struct ChatRow {
    pub id: String,
    pub name: String,
    pub working_dir: String,
    pub session_type: String,
    pub updated_at: DateTime<Utc>,
    pub extension_data: String,
    pub message_count: i64,
}

type ChatRowTuple = (String, String, String, String, DateTime<Utc>, String, i64);

fn chat_row(row: ChatRowTuple) -> ChatRow {
    let (id, name, working_dir, session_type, updated_at, extension_data, message_count) = row;
    ChatRow {
        id,
        name,
        working_dir,
        session_type,
        updated_at,
        extension_data,
        message_count,
    }
}

impl ChatRow {
    pub fn kept_out(&self) -> bool {
        serde_json::from_str::<crate::session::ExtensionData>(&self.extension_data)
            .ok()
            .and_then(|data| ChatSearchState::from_extension_data(&data))
            .is_some_and(|state| state.keep_out)
    }
}

const CHAT_COLUMNS: &str = "s.id, s.name, s.working_dir, s.session_type, s.updated_at, \
     COALESCE(s.extension_data, '{}'), (SELECT COUNT(*) FROM messages WHERE session_id = s.id)";

fn type_marks(session_types: &[crate::session::SessionType]) -> String {
    vec!["?"; session_types.len()].join(", ")
}

/// The chat a `chat` argument names — its id, else the most recently active chat whose title
/// contains the words — among the given types. A kept-out chat is returned so the caller can say
/// so; it is never opened.
pub(crate) async fn find_chat(
    pool: &Pool<Sqlite>,
    chat: &str,
    session_types: &[crate::session::SessionType],
) -> Result<Option<ChatRow>> {
    let sql = format!(
        "SELECT {CHAT_COLUMNS} FROM sessions s WHERE (s.id = ? OR s.name LIKE ?) \
           AND s.session_type IN ({}) \
         ORDER BY (s.id = ?) DESC, s.updated_at DESC LIMIT 1",
        type_marks(session_types)
    );
    let mut q = sqlx::query_as::<_, ChatRowTuple>(&sql)
        .bind(chat.to_string())
        .bind(format!("%{chat}%"));
    for t in session_types {
        q = q.bind(t.to_string());
    }
    Ok(q.bind(chat.to_string())
        .fetch_optional(pool)
        .await?
        .map(chat_row))
}

/// The chats with messages, most recently active first, and how many exist in all and how many of
/// those are kept out of search (counted, never listed).
pub(crate) async fn list_chats(
    pool: &Pool<Sqlite>,
    session_types: &[crate::session::SessionType],
    folder: Option<&str>,
    limit: i64,
) -> Result<(Vec<ChatRow>, u64, u64)> {
    let mut where_sql = format!(
        "s.session_type IN ({}) AND EXISTS (SELECT 1 FROM messages WHERE session_id = s.id)",
        type_marks(session_types)
    );
    if folder.is_some() {
        where_sql.push_str(" AND (s.working_dir = ? OR s.working_dir LIKE ?)");
    }
    let mut binds: Vec<String> = session_types.iter().map(|t| t.to_string()).collect();
    if let Some(folder) = folder {
        let folder = folder.trim_end_matches('/');
        binds.push(folder.to_string());
        binds.push(format!("{folder}/%"));
    }
    let sql = format!(
        "SELECT {CHAT_COLUMNS} FROM sessions s WHERE {where_sql} AND {SEARCHABLE_SESSION_SQL} \
         ORDER BY s.updated_at DESC LIMIT ?"
    );
    let mut q = sqlx::query_as::<_, ChatRowTuple>(&sql);
    for b in &binds {
        q = q.bind(b.clone());
    }
    let rows = q.bind(limit).fetch_all(pool).await?;
    let count_sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(NOT ({SEARCHABLE_SESSION_SQL})), 0) FROM sessions s WHERE {where_sql}"
    );
    let mut c = sqlx::query_as::<_, (i64, i64)>(&count_sql);
    for b in &binds {
        c = c.bind(b.clone());
    }
    let (all, kept_out) = c.fetch_one(pool).await?;
    Ok((
        rows.into_iter().map(chat_row).collect(),
        (all - kept_out) as u64,
        kept_out as u64,
    ))
}

/// Up to `span` messages of a chat before message `around` and `span` from it on, oldest first;
/// without `around`, the chat's newest `span` messages.
pub(crate) async fn window(
    pool: &Pool<Sqlite>,
    session_id: &str,
    around: Option<i64>,
    span: i64,
) -> Result<Vec<StoredMessage>> {
    type Row = (i64, String, DateTime<Utc>, String, Option<String>);
    let rows: Vec<Row> = match around {
        Some(around) => {
            let mut earlier: Vec<Row> = sqlx::query_as(
                "SELECT id, role, timestamp, content_json, metadata_json FROM messages \
                 WHERE session_id = ? AND id < ? ORDER BY id DESC LIMIT ?",
            )
            .bind(session_id)
            .bind(around)
            .bind(span)
            .fetch_all(pool)
            .await?;
            earlier.reverse();
            let later: Vec<Row> = sqlx::query_as(
                "SELECT id, role, timestamp, content_json, metadata_json FROM messages \
                 WHERE session_id = ? AND id >= ? ORDER BY id ASC LIMIT ?",
            )
            .bind(session_id)
            .bind(around)
            .bind(span + 1)
            .fetch_all(pool)
            .await?;
            earlier.into_iter().chain(later).collect()
        }
        None => {
            let mut newest: Vec<Row> = sqlx::query_as(
                "SELECT id, role, timestamp, content_json, metadata_json FROM messages \
                 WHERE session_id = ? ORDER BY id DESC LIMIT ?",
            )
            .bind(session_id)
            .bind(span)
            .fetch_all(pool)
            .await?;
            newest.reverse();
            newest
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(message_id, role, timestamp, content_json, metadata_json)| StoredMessage {
                message_id,
                role,
                timestamp,
                content_json,
                metadata_json,
            },
        )
        .collect())
}

/// A snippet of `text` around the first word a term opens, `width` characters wide, the matched
/// word marked «like this». None when no term opens a word of the text.
pub fn snippet<'a>(
    text: &str,
    terms: impl IntoIterator<Item = &'a Term> + Clone,
    width: usize,
) -> Option<String> {
    let lower = text.to_lowercase();
    if lower.len() != text.len() {
        return snippet_by_chars(text, terms, width);
    }
    let mut start = None;
    let mut word_start = None;
    for (i, c) in lower
        .char_indices()
        .chain(std::iter::once((lower.len(), ' ')))
    {
        match (c.is_alphanumeric(), word_start) {
            (true, None) => word_start = Some(i),
            (false, Some(ws)) => {
                let token = &lower[ws..i];
                if terms.clone().into_iter().any(|t| t.opens(token)) {
                    start = Some((ws, i));
                    break;
                }
                word_start = None;
            }
            _ => {}
        }
    }
    let (ws, we) = start?;
    Some(cut(text, ws, we, width))
}

fn snippet_by_chars<'a>(
    text: &str,
    terms: impl IntoIterator<Item = &'a Term> + Clone,
    width: usize,
) -> Option<String> {
    let mut word_start = None;
    let bounds: Vec<(usize, char)> = text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
        .collect();
    for (i, c) in bounds {
        match (c.is_alphanumeric(), word_start) {
            (true, None) => word_start = Some(i),
            (false, Some(ws)) => {
                let token = text[ws..i].to_lowercase();
                if terms.clone().into_iter().any(|t| t.opens(&token)) {
                    return Some(cut(text, ws, i, width));
                }
                word_start = None;
            }
            _ => {}
        }
    }
    None
}

/// `width` characters of `text` around the byte range `[ws, we)`, on one line, ellipses where cut.
fn cut(text: &str, ws: usize, we: usize, width: usize) -> String {
    let before: Vec<char> = text[..ws].chars().collect();
    let word: String = text[ws..we].chars().collect();
    let after: Vec<char> = text[we..].chars().collect();
    let room = width.saturating_sub(word.chars().count());
    let lead = (room / 3).min(before.len());
    let trail = (room - lead).min(after.len());
    let lead = (room - trail).min(before.len());
    let head: String = before[before.len() - lead..].iter().collect();
    let tail: String = after[..trail].iter().collect();
    let mut out = String::new();
    if lead < before.len() {
        out.push('…');
    }
    out.push_str(&head);
    out.push('«');
    out.push_str(&word);
    out.push('»');
    out.push_str(&tail);
    if trail < after.len() {
        out.push('…');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_reads_words_phrases_prefixes_or_and_exclusions() {
        let q =
            SearchQuery::parse(r#"split "mesh node" pref* OR other -git -"branch list""#).unwrap();
        assert_eq!(
            q.fts(false),
            r#"("split" AND "mesh node" AND "pref" * OR "other") NOT ("git" OR "branch list")"#
        );
        assert!(!q.broadenable());
        assert_eq!(
            SearchQuery::parse("tenant_id").unwrap().fts(false),
            r#"("tenant id")"#
        );
        let two = SearchQuery::parse("split mesh").unwrap();
        assert!(two.broadenable());
        assert_eq!(two.fts(true), r#"("split" OR "mesh")"#);
        assert!(SearchQuery::parse("-git").is_err());
        assert!(SearchQuery::parse("  ").is_err());
    }

    #[test]
    fn the_rust_extraction_reads_what_the_sql_extraction_indexes() {
        let content = r#"[{"type":"text","text":"about the split mesh"},
            {"type":"toolRequest","id":"a","toolCall":{"status":"success","value":{"name":"shell","arguments":{"command":"git branch"}}}},
            {"type":"toolResponse","id":"a","toolResult":{"status":"success","value":{"content":[{"type":"text","text":"split-mesh-branch"}]}}},
            {"type":"toolResponse","id":"b","toolResult":{"status":"error","error":"Tool 'read' not found"}},
            {"type":"thinking","thinking":"hidden thought"}]"#;
        let text = message_text(content, None);
        assert_eq!(text.said, "about the split mesh");
        assert_eq!(
            text.tool_io,
            "shell {\"command\":\"git branch\"}\nsplit-mesh-branch\nTool 'read' not found"
        );
    }

    #[test]
    fn a_snippet_marks_the_first_matching_word_and_stays_within_its_width() {
        let terms = [Term {
            words: vec!["mesh".to_string()],
            prefix: false,
        }];
        let text = "We talked for a long while about many things and then about the split mesh plan, \
                    which runs on two Macs over Thunderbolt and needs a lot of care to set up well.";
        let s = snippet(text, &terms, 60).unwrap();
        assert!(s.contains("«mesh»"), "{s}");
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
        assert!(s.chars().count() <= 60 + 4, "{s}");
        assert_eq!(snippet("nothing here", &terms, 60), None);
        let prefix = [Term {
            words: vec!["thunder".to_string()],
            prefix: true,
        }];
        assert!(snippet(text, &prefix, 40)
            .unwrap()
            .contains("«Thunderbolt»"));
    }

    #[test]
    fn the_footer_names_what_is_missing_and_why() {
        let full = IndexCoverage {
            indexed: 24_598,
            indexable: 24_598,
            backfilling: false,
        };
        assert_eq!(full.footer(), "Indexed 24,598 of 24,598 messages.");
        let partial = IndexCoverage {
            indexed: 1_000,
            indexable: 24_598,
            backfilling: true,
        };
        assert!(partial
            .footer()
            .contains("23,598 older messages are not indexed yet"));
        let broken = IndexCoverage {
            indexed: 24_597,
            indexable: 24_598,
            backfilling: false,
        };
        assert!(broken.footer().contains("1 messages could not be indexed"));
    }

    use crate::config::GooseMode;
    use crate::conversation::message::Message;
    use crate::conversation::Conversation;
    use crate::session::{SessionManager, SessionType};
    use std::path::PathBuf;
    use tempfile::TempDir;

    async fn store() -> (TempDir, SessionManager) {
        let dir = TempDir::new().unwrap();
        let sm = SessionManager::new(dir.path().to_path_buf());
        (dir, sm)
    }

    async fn chat(sm: &SessionManager, name: &str, session_type: SessionType) -> String {
        sm.create_session(
            PathBuf::from("/tmp/transcripts"),
            name.to_string(),
            session_type,
            GooseMode::default(),
        )
        .await
        .unwrap()
        .id
    }

    async fn matching(sm: &SessionManager, expr: &str) -> Vec<i64> {
        let pool = sm.storage().pool().await.unwrap();
        sqlx::query_scalar(&format!(
            "SELECT rowid FROM {FTS_TABLE} WHERE {FTS_TABLE} MATCH ? ORDER BY rowid"
        ))
        .bind(expr)
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn ids_of(sm: &SessionManager, session_id: &str) -> Vec<i64> {
        let pool = sm.storage().pool().await.unwrap();
        sqlx::query_scalar("SELECT id FROM messages WHERE session_id = ? ORDER BY id")
            .bind(session_id)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    async fn indexed(sm: &SessionManager) -> u64 {
        coverage(sm.storage().pool().await.unwrap())
            .await
            .unwrap()
            .indexed
    }

    fn tool_call(id: &str, name: &str) -> Message {
        Message::assistant().with_tool_request(
            id.to_string(),
            Ok(rmcp::model::CallToolRequestParams::new(name.to_string())),
        )
    }

    /// Every way a message row changes reaches the index: insert, an update of `content_json`
    /// (raw, and through `update_tool_request_meta`, the one production UPDATE), a whole-chat
    /// replace, a truncation and a delete.
    #[tokio::test]
    async fn every_write_path_keeps_the_index_current() {
        let (_dir, sm) = store().await;
        let id = chat(&sm, "Billing", SessionType::User).await;

        sm.add_message(
            &id,
            &Message::user().with_text("the new column is tenant_id"),
        )
        .await
        .unwrap();
        let first = ids_of(&sm, &id).await;
        assert_eq!(matching(&sm, "said : tenant").await, first, "insert");

        let pool = sm.storage().pool().await.unwrap();
        sqlx::query("UPDATE messages SET content_json = ? WHERE id = ?")
            .bind(r#"[{"type":"text","text":"the billing ledger moved"}]"#)
            .bind(first[0])
            .execute(pool)
            .await
            .unwrap();
        assert!(matching(&sm, "tenant").await.is_empty(), "old words gone");
        assert_eq!(matching(&sm, "ledger").await, first, "new words indexed");

        sm.add_message(&id, &tool_call("call-1", "shell").with_id("msg-tool"))
            .await
            .unwrap();
        let tool_row = *ids_of(&sm, &id).await.last().unwrap();
        sm.update_tool_request_meta(
            &id,
            "msg-tool",
            "call-1",
            serde_json::json!({"title": "list files"}),
        )
        .await
        .unwrap();
        assert_eq!(
            matching(&sm, "tool_io : shell").await,
            vec![tool_row],
            "a tool-meta update re-indexes the row once"
        );
        assert_eq!(indexed(&sm).await, 2);

        sm.replace_conversation(
            &id,
            &Conversation::new_unvalidated(vec![
                Message::user().with_text("compacted summary of mesh work"),
                Message::assistant().with_text("the split mesh runs on two Macs"),
            ]),
        )
        .await
        .unwrap();
        assert!(
            matching(&sm, "ledger").await.is_empty(),
            "replaced rows gone"
        );
        assert_eq!(matching(&sm, "mesh").await, ids_of(&sm, &id).await);
        assert_eq!(indexed(&sm).await, 2);

        sm.truncate_conversation(&id, 0).await.unwrap();
        assert!(
            matching(&sm, "mesh").await.is_empty(),
            "truncated rows gone"
        );

        sm.add_message(&id, &Message::user().with_text("one more about mesh"))
            .await
            .unwrap();
        assert_eq!(indexed(&sm).await, 1);
        sm.delete_session(&id).await.unwrap();
        assert_eq!(
            indexed(&sm).await,
            0,
            "a deleted chat leaves nothing behind"
        );
    }

    /// What goose keeps for itself (a compaction summary, a condensed tool record) is its own
    /// traffic, not what anyone said — and a later metadata update moves the row between columns.
    #[tokio::test]
    async fn goose_s_own_notes_are_indexed_as_its_traffic_not_as_said() {
        let (_dir, sm) = store().await;
        let id = chat(&sm, "Compacted", SessionType::User).await;
        sm.add_message(
            &id,
            &Message::user()
                .with_text("[goose's record of an earlier tool call] shell ls split-tensor")
                .agent_only()
                .with_id("note-1"),
        )
        .await
        .unwrap();
        let row = ids_of(&sm, &id).await;
        assert!(matching(&sm, "said : tensor").await.is_empty());
        assert_eq!(matching(&sm, "tool_io : tensor").await, row);

        sm.update_message_metadata(&id, "note-1", |meta| meta.with_user_visible())
            .await
            .unwrap();
        assert_eq!(
            matching(&sm, "said : tensor").await,
            row,
            "a metadata update re-indexes"
        );
    }

    /// A hidden swarm worker's messages never enter the index — at insert, and when a session
    /// becomes hidden later; one that stops being hidden is indexed whole.
    #[tokio::test]
    async fn a_hidden_session_is_never_indexed() {
        let (_dir, sm) = store().await;
        let worker = chat(&sm, "swarm-task", SessionType::Hidden).await;
        sm.add_message(
            &worker,
            &Message::user().with_text("split mesh worker brief"),
        )
        .await
        .unwrap();
        assert!(matching(&sm, "mesh").await.is_empty());
        let pool = sm.storage().pool().await.unwrap();
        let cov = coverage(pool).await.unwrap();
        assert_eq!((cov.indexed, cov.indexable), (0, 0));

        let person = chat(&sm, "Explore split mesh", SessionType::User).await;
        sm.add_message(&person, &Message::user().with_text("split mesh plan"))
            .await
            .unwrap();
        assert_eq!(matching(&sm, "mesh").await, ids_of(&sm, &person).await);

        sqlx::query("UPDATE sessions SET session_type = 'hidden' WHERE id = ?")
            .bind(&person)
            .execute(pool)
            .await
            .unwrap();
        assert!(
            matching(&sm, "mesh").await.is_empty(),
            "hidden later: removed"
        );

        sqlx::query("UPDATE sessions SET session_type = 'user' WHERE id = ?")
            .bind(&worker)
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(matching(&sm, "mesh").await, ids_of(&sm, &worker).await);
    }

    /// Turn a store back into one that predates the index: no index rows, the watermark one past
    /// the newest message — what migration v15 leaves on an existing history.
    async fn unindex(sm: &SessionManager) {
        let pool = sm.storage().pool().await.unwrap();
        sqlx::query(&format!(
            "INSERT INTO {FTS_TABLE} ({FTS_TABLE}) VALUES ('delete-all')"
        ))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "UPDATE {STATE_TABLE} SET backfill_below = (SELECT MAX(id) + 1 FROM messages)"
        ))
        .execute(pool)
        .await
        .unwrap();
    }

    /// The backfill indexes the newest rows first, a batch per transaction, and a later run picks
    /// up exactly at the watermark the last committed batch left — hidden rows are stepped over.
    #[tokio::test]
    async fn the_backfill_resumes_from_its_watermark() {
        let (_dir, sm) = store().await;
        let person = chat(&sm, "Person", SessionType::User).await;
        let worker = chat(&sm, "Worker", SessionType::Hidden).await;
        for i in 0..5 {
            sm.add_message(
                &person,
                &Message::user().with_text(format!("note {i} mesh")),
            )
            .await
            .unwrap();
            sm.add_message(
                &worker,
                &Message::user().with_text(format!("worker {i} mesh")),
            )
            .await
            .unwrap();
        }
        let rows = ids_of(&sm, &person).await;
        unindex(&sm).await;
        let pool = sm.storage().pool().await.unwrap();
        let before = coverage(pool).await.unwrap();
        assert_eq!((before.indexed, before.indexable), (0, 5));
        assert!(before.backfilling);

        let step = backfill_step(pool, 2).await.unwrap();
        assert_eq!(
            step,
            BackfillStep::Indexed {
                rows: 2,
                from: rows[3]
            }
        );
        assert_eq!(
            matching(&sm, "mesh").await,
            rows[3..].to_vec(),
            "newest first"
        );
        assert_eq!(backfill_below(pool).await.unwrap(), rows[3]);

        // A second process (or this one after a restart) reads the watermark and continues below it.
        let reopened = SessionManager::new(_dir.path().to_path_buf());
        let pool2 = reopened.storage().pool().await.unwrap();
        let step = backfill_step(pool2, 2).await.unwrap();
        assert_eq!(
            step,
            BackfillStep::Indexed {
                rows: 2,
                from: rows[1]
            }
        );
        assert_eq!(matching(&sm, "mesh").await, rows[1..].to_vec());

        while backfill_step(pool, 2).await.unwrap() != BackfillStep::Done {}
        let after = coverage(pool).await.unwrap();
        assert_eq!(
            (after.indexed, after.indexable, after.backfilling),
            (5, 5, false)
        );
        assert_eq!(matching(&sm, "mesh").await, rows);
        assert_eq!(backfill_step(pool, 2).await.unwrap(), BackfillStep::Done);
    }

    /// A store written before v15 is migrated at startup without indexing anything inside the
    /// migration, and the startup's background backfill then indexes the history.
    #[tokio::test]
    async fn a_v14_store_is_indexed_by_the_startup_backfill() {
        let dir = TempDir::new().unwrap();
        {
            let sm = SessionManager::new(dir.path().to_path_buf());
            let id = chat(&sm, "Old", SessionType::User).await;
            for i in 0..3 {
                sm.add_message(&id, &Message::user().with_text(format!("old mesh {i}")))
                    .await
                    .unwrap();
            }
            let pool = sm.storage().pool().await.unwrap();
            for statement in [
                "DROP TRIGGER messages_fts_insert",
                "DROP TRIGGER messages_fts_delete",
                "DROP TRIGGER messages_fts_update",
                "DROP TRIGGER sessions_fts_hidden",
                "DROP TRIGGER sessions_fts_unhidden",
                "DROP TABLE messages_fts",
                "DROP TABLE messages_fts_state",
                "UPDATE schema_version SET version = 14 WHERE version = 15",
            ] {
                sqlx::query(statement).execute(pool).await.unwrap();
            }
            pool.close().await;
        }
        let sm = SessionManager::new(dir.path().to_path_buf());
        let pool = sm.storage().pool().await.unwrap();
        let mut cov = coverage(pool).await.unwrap();
        for _ in 0..200 {
            if cov.missing() == 0 && !cov.backfilling {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            cov = coverage(pool).await.unwrap();
        }
        assert_eq!((cov.indexed, cov.indexable, cov.backfilling), (3, 3, false));
        assert_eq!(matching(&sm, "mesh").await.len(), 3);
    }

    /// Recall's per-turn search reads the index: the message carrying every term wins the limit,
    /// a kept-out chat and a hidden worker are never returned, and the results carry the coverage.
    #[tokio::test]
    async fn recall_reads_the_index_and_honours_the_opt_out() {
        let (_dir, sm) = store().await;
        let fact = chat(&sm, "Release notes", SessionType::User).await;
        sm.add_message(
            &fact,
            &Message::user().with_text("release notes are published with just"),
        )
        .await
        .unwrap();
        let private = chat(&sm, "Private", SessionType::User).await;
        sm.add_message(
            &private,
            &Message::user().with_text("release notes published private copy"),
        )
        .await
        .unwrap();
        sm.set_extension_state(&private, &ChatSearchState { keep_out: true })
            .await
            .unwrap();
        let worker = chat(&sm, "w", SessionType::Hidden).await;
        sm.add_message(
            &worker,
            &Message::user().with_text("release notes published by a worker"),
        )
        .await
        .unwrap();

        let results = sm
            .search_chat_history(
                "release notes published",
                Some(5),
                None,
                None,
                None,
                vec![SessionType::User, SessionType::Hidden],
            )
            .await
            .unwrap();
        let ids: Vec<&str> = results
            .results
            .iter()
            .map(|r| r.session_id.as_str())
            .collect();
        assert_eq!(ids, vec![fact.as_str()]);
        assert_eq!(results.index.missing(), 0);
    }
}
