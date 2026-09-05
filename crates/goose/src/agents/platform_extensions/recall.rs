//! Automatic recall: every user turn, the user's own words are searched against the memory store and
//! the skill catalogue, and what matches rides into the turn context — so a model gets the facts it
//! already saved and the skill that fits without first having to decide to call a tool. Measured need:
//! a local 27B rarely calls `search_memories` or `load_skill` on its own.
//!
//! Only the request turn triggers it (the last message is the user's text, not a tool response), only
//! when the memory / skills extensions are enabled in the session, and it names its own absence: no
//! match, no block.

use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::tool_execution::ToolCallContext;
use crate::config::paths::Paths;
use crate::conversation::effective_role;
use crate::conversation::message::{Message, MessageContent};
use crate::session::session_manager::SessionType;
use anyhow::Result;
use async_trait::async_trait;
use goose_memory_store::{
    headline, rarity_weight, search_terms, term_occurrences, tokenize, MemoryStore, SearchHit,
};
use goose_sdk_types::custom_requests::{SourceEntry, SourceType};
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, JsonObject, ListToolsResult,
    ServerCapabilities,
};
use tokio_util::sync::CancellationToken;

pub static EXTENSION_NAME: &str = "recall";

// ratio: at most a fifth of the index budget — three entries at the importer's 2,500-char cap against
// the 39k-char index measured on the first machine.
const RECALL_MAX_MEMORIES: usize = 3;
// ratio: a skill line is a name and a description; three of them is the same budget as one memory.
const RECALL_MAX_SKILLS: usize = 3;
// ratio: a candidate rides along only while it scores at least half of the best candidate — measured
// on the 171-entry store, the slots below that line were filled by entries sharing six common words.
const RECALL_MIN_SHARE_OF_TOP: f64 = 0.5;

/// Function words that match every memory and rank nothing.
const STOPWORDS: &[&str] = &[
    "a", "about", "after", "again", "all", "also", "an", "and", "any", "are", "as", "at", "be",
    "been", "before", "but", "by", "can", "could", "did", "do", "does", "doing", "done", "for",
    "from", "get", "give", "had", "has", "have", "he", "her", "here", "him", "his", "how", "i",
    "if", "in", "into", "is", "it", "its", "just", "let", "like", "make", "me", "more", "most",
    "my", "need", "no", "not", "now", "of", "on", "one", "only", "or", "other", "our", "out",
    "over", "please", "same", "she", "should", "so", "some", "than", "that", "the", "their",
    "them", "then", "there", "these", "they", "this", "those", "to", "too", "up", "us", "use",
    "very", "want", "was", "we", "were", "what", "when", "where", "which", "who", "why", "will",
    "with", "would", "you", "your",
];

pub struct RecallClient {
    info: InitializeResult,
    context: PlatformExtensionContext,
}

impl RecallClient {
    pub fn new(context: PlatformExtensionContext) -> Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(EXTENSION_NAME, "1.0.0").with_title("Recall"))
            .with_instructions(
                "When a request's words match saved memories or a skill's description, the turn context \
                 carries a <recalled-memories> block (those memories in full) and a <relevant-skills> block \
                 (skill names to load with load_skill). goose adds them; use them when they apply and ignore \
                 them when they do not."
                    .to_string(),
            );
        Ok(Self { info, context })
    }

    async fn extension_enabled(&self, name: &str) -> bool {
        match self
            .context
            .extension_manager
            .as_ref()
            .and_then(|weak| weak.upgrade())
        {
            Some(manager) => manager.is_extension_enabled(name).await,
            None => false,
        }
    }
}

/// The user's text when the last message is a fresh request; None during a tool loop, where the last
/// user-role message carries tool responses and the request has already been recalled.
pub fn last_user_text(messages: &[Message]) -> Option<String> {
    let last = messages.iter().rev().find(|m| m.is_agent_visible())?;
    if effective_role(last) != "user" {
        return None;
    }
    if last
        .content
        .iter()
        .any(|c| matches!(c, MessageContent::ToolResponse(_)))
    {
        return None;
    }
    let text: Vec<&str> = last
        .content
        .iter()
        .filter_map(|c| match c {
            MessageContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    let text = text.join("\n");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// The query terms a request contributes: its words minus function words and one-letter tokens.
pub fn query_terms(text: &str) -> Vec<String> {
    search_terms(text)
        .into_iter()
        .filter(|t| t.chars().count() > 1 && !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// Which hits are worth injecting: an entry that COVERS the request — it matches at least half of the
/// request's terms, at least one of them rare — and scores at least half of the best such hit. Read on
/// the 171-entry store: a name-term rule kept "list the files in this directory" → note-54c772
/// (a headline is a sentence, so nearly every entry has a request word in its name) and dropped the one
/// true hit for "write a blog post about local models" (note-5c9556, 3 of 5 terms, none in
/// the name); coverage keeps that one and drops the 1-of-5-term note-784cf8.
pub fn select_hits(hits: Vec<SearchHit>, term_count: usize) -> Vec<SearchHit> {
    let covers = |hit: &SearchHit| hit.rare_terms >= 1 && hit.matched_terms * 2 >= term_count;
    let top = hits
        .iter()
        .filter(|hit| covers(hit))
        .map(|hit| hit.score)
        .fold(0.0_f64, f64::max);
    hits.into_iter()
        .filter(|hit| covers(hit) && hit.score >= top * RECALL_MIN_SHARE_OF_TOP)
        .take(RECALL_MAX_MEMORIES)
        .collect()
}

/// Skills the request is about, by the same rule as memories: terms weighted by their rarity across
/// the catalogue (a name match counts twice), at least one rare term, at least half the best score.
pub fn relevant_skills<'a>(skills: &'a [SourceEntry], terms: &[String]) -> Vec<&'a SourceEntry> {
    let catalogue: Vec<(&SourceEntry, Vec<String>, Vec<String>)> = skills
        .iter()
        .filter(|s| matches!(s.source_type, SourceType::Skill | SourceType::BuiltinSkill))
        .map(|skill| {
            let name = tokenize(&skill.name);
            let text = tokenize(&format!("{} {}", skill.name, skill.description));
            (skill, name, text)
        })
        .collect();
    let n = catalogue.len();
    let document_frequency: Vec<usize> = terms
        .iter()
        .map(|term| {
            catalogue
                .iter()
                .filter(|(_, _, text)| term_occurrences(term, text) > 0)
                .count()
        })
        .collect();
    let mut scored: Vec<(f64, &SourceEntry)> = catalogue
        .iter()
        .filter_map(|(skill, name, text)| {
            let mut score = 0.0;
            let mut rare = 0;
            for (i, term) in terms.iter().enumerate() {
                if term_occurrences(term, text) == 0 {
                    continue;
                }
                let weight = rarity_weight(n, document_frequency[i]);
                score += weight;
                if document_frequency[i] * 2 <= n {
                    rare += 1;
                }
                if term_occurrences(term, name) > 0 {
                    score += weight;
                }
            }
            (rare >= 1).then_some((score, *skill))
        })
        .collect();
    let top = scored
        .iter()
        .map(|(score, _)| *score)
        .fold(0.0_f64, f64::max);
    scored.retain(|(score, _)| *score >= top * RECALL_MIN_SHARE_OF_TOP);
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.name.cmp(&b.1.name))
    });
    scored
        .into_iter()
        .take(RECALL_MAX_SKILLS)
        .map(|(_, skill)| skill)
        .collect()
}

/// The most recent earlier session whose words cover the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastSession {
    pub session_id: String,
    pub description: String,
    pub when: String,
    pub role: String,
    pub headline: String,
}

// ratio: the history search is an OR of LIKEs, so it returns every message sharing one word; twenty
// rows is enough to find one that covers the request when one exists, and cheap when none does.
const PAST_SESSION_ROWS: usize = 20;

/// From history-search rows (newest first), the first message in another session that covers the
/// request by the same rule memories use: at least half of the request's terms.
pub fn select_past_session(
    results: &[crate::session::chat_history_search::ChatRecallResult],
    terms: &[String],
) -> Option<PastSession> {
    let mut candidates: Vec<(chrono::DateTime<chrono::Utc>, PastSession)> = Vec::new();
    for result in results {
        for message in &result.messages {
            let tokens = tokenize(&message.content);
            let matched = terms
                .iter()
                .filter(|term| term_occurrences(term, &tokens) > 0)
                .count();
            if matched * 2 < terms.len() {
                continue;
            }
            candidates.push((
                message.timestamp,
                PastSession {
                    session_id: result.session_id.clone(),
                    description: result.session_description.clone(),
                    when: message.timestamp.format("%Y-%m-%d %H:%M").to_string(),
                    role: message.role.clone(),
                    headline: headline(&message.content),
                },
            ));
        }
    }
    candidates.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    candidates.into_iter().next().map(|(_, past)| past)
}

/// The turn-context part. None when there is nothing to say.
pub fn render(
    memories: &[SearchHit],
    skills: &[&SourceEntry],
    past: Option<&PastSession>,
) -> Option<String> {
    if memories.is_empty() && skills.is_empty() && past.is_none() {
        return None;
    }
    let mut out = String::new();
    if !memories.is_empty() {
        out.push_str("<recalled-memories>\n");
        out.push_str(
            "Saved memories whose words match this request — recalled by goose, use them if they apply:\n",
        );
        for hit in memories {
            out.push('\n');
            out.push_str(&hit.entry.render());
        }
        out.push_str("</recalled-memories>");
    }
    if !skills.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("<relevant-skills>\n");
        out.push_str("Skills whose description matches this request; load one with load_skill(name) before doing the work it covers:\n");
        for skill in skills {
            out.push_str(&format!("- {}: {}\n", skill.name, skill.description));
        }
        out.push_str("</relevant-skills>");
    }
    if let Some(past) = past {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!(
            "<past-session>\nThis was discussed before — session {} (\"{}\", {}), the {} said: \"{}\". \
             chatrecall(session_id) loads it if the history matters.\n</past-session>",
            past.session_id, past.description, past.when, past.role, past.headline
        ));
    }
    Some(out)
}

#[async_trait]
impl McpClientTrait for RecallClient {
    async fn list_tools(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        Ok(ListToolsResult {
            tools: Vec::new(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        _ctx: &ToolCallContext,
        name: &str,
        _arguments: Option<JsonObject>,
        _cancellation_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        Ok(CallToolResult::error(vec![Content::text(format!(
            "recall has no tools (asked for '{name}'); it works from the turn context"
        ))]))
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }

    async fn get_moim(&self, session_id: &str) -> Option<String> {
        let session = match self
            .context
            .session_manager
            .get_session(session_id, true)
            .await
        {
            Ok(session) => session,
            Err(err) => {
                tracing::warn!(session_id, %err, "recall: session unreadable, nothing recalled");
                return None;
            }
        };
        let text = last_user_text(session.conversation.as_ref()?.messages())?;
        let terms = query_terms(&text);
        if terms.is_empty() {
            return None;
        }
        let query = terms.join(" ");

        let memories = if self.extension_enabled("memory").await {
            let store = MemoryStore::new(Paths::config_dir().join("memory"), &session.working_dir);
            match store.search(&query, None) {
                Ok(hits) => select_hits(hits, terms.len()),
                Err(err) => {
                    tracing::warn!(%err, "recall: memory store unreadable, nothing recalled");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        let catalogue = if self.extension_enabled("skills").await {
            crate::skills::discover_skills(Some(&session.working_dir))
        } else {
            Vec::new()
        };
        let skills = relevant_skills(&catalogue, &terms);

        let session_types = match session.session_type {
            SessionType::Acp => vec![SessionType::Acp],
            _ => vec![SessionType::User, SessionType::Scheduled],
        };
        let history_started = std::time::Instant::now();
        let past = match self
            .context
            .session_manager
            .search_chat_history(
                &query,
                Some(PAST_SESSION_ROWS),
                None,
                None,
                Some(session_id.to_string()),
                session_types,
            )
            .await
        {
            Ok(results) => select_past_session(&results.results, &terms),
            Err(err) => {
                tracing::warn!(%err, "recall: chat history unreadable, no past session named");
                None
            }
        };

        let part = render(&memories, &skills, past.as_ref());
        let recalled: Vec<String> = memories
            .iter()
            .map(|hit| format!("{}({:.1})", hit.entry.category, hit.score))
            .collect();
        let suggested: Vec<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
        tracing::info!(
            session_id,
            terms = terms.len(),
            memories = memories.len(),
            recalled = ?recalled,
            skills = skills.len(),
            suggested = ?suggested,
            past_session = past.as_ref().map(|p| p.session_id.as_str()),
            history_ms = history_started.elapsed().as_millis(),
            "recall"
        );
        part
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_memory_store::MemoryEntry;

    fn hit(category: &str, score: f64, rare_terms: usize) -> SearchHit {
        named_hit(category, score, rare_terms, 1)
    }

    /// A hit matching `matched` terms, all of them rare.
    fn covering_hit(category: &str, score: f64, matched: usize) -> SearchHit {
        let mut hit = named_hit(category, score, matched, 0);
        hit.matched_terms = matched;
        hit
    }

    fn named_hit(category: &str, score: f64, rare_terms: usize, name_terms: usize) -> SearchHit {
        SearchHit {
            score,
            matched_terms: rare_terms.max(1),
            rare_terms,
            phrase: false,
            name_terms,
            occurrences: rare_terms.max(1),
            entry: MemoryEntry {
                is_global: true,
                category: category.to_string(),
                tags: vec!["project".to_string()],
                content: format!("{category} headline\nbody"),
            },
        }
    }

    fn skill(name: &str, description: &str) -> SourceEntry {
        SourceEntry {
            source_type: SourceType::Skill,
            name: name.to_string(),
            description: description.to_string(),
            content: String::new(),
            path: format!("/skills/{name}"),
            supporting_files: Vec::new(),
            global: true,
            writable: false,
            properties: Default::default(),
        }
    }

    #[test]
    fn last_user_text_is_the_fresh_request_only() {
        let request = vec![Message::user().with_text("fix the postgres docker port")];
        assert_eq!(
            last_user_text(&request).as_deref(),
            Some("fix the postgres docker port")
        );

        let mut tool_loop = request.clone();
        tool_loop.push(Message::assistant().with_text("looking"));
        tool_loop.push(Message::user().with_tool_response(
            "call-1",
            Ok(CallToolResult::success(vec![Content::text("ok")])),
        ));
        assert_eq!(last_user_text(&tool_loop), None);

        let answered = vec![
            Message::user().with_text("hello"),
            Message::assistant().with_text("hi"),
        ];
        assert_eq!(last_user_text(&answered), None);
    }

    #[test]
    fn query_terms_drop_function_words() {
        let terms = query_terms("Can you fix the Postgres port in docker, please?");
        assert_eq!(terms, vec!["docker", "fix", "port", "postgres"]);
        assert!(query_terms("do it").is_empty());
    }

    #[test]
    fn select_hits_needs_half_coverage_a_rare_term_and_half_the_top_score() {
        // a five-term request: coverage needs three matched terms
        let hits = vec![
            covering_hit("best", 4.0, 3),
            covering_hit("one-common-word", 5.0, 1),
            covering_hit("two-of-five", 4.5, 2),
            covering_hit("half", 2.0, 3),
            covering_hit("under-half", 1.9, 3),
            covering_hit("also-fine", 3.0, 4),
            covering_hit("fourth", 2.5, 3),
        ];
        let kept: Vec<String> = select_hits(hits, 5)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(kept, vec!["best", "half", "also-fine"]);
        assert!(select_hits(vec![named_hit("nothing-rare", 9.0, 0, 0)], 1).is_empty());
    }

    #[test]
    fn relevant_skills_need_a_rare_term_and_rank_name_matches_first() {
        let skills = vec![
            skill(
                "jira-api",
                "Jira Cloud REST API v3 integration from external apps",
            ),
            skill(
                "leanzero-newsroom",
                "Research and draft articles about local models and Atlassian tools",
            ),
            skill(
                "note-d90573",
                "Axpo Atlassian operations: Jira issue triage over the REST API",
            ),
            skill(
                "note-651f5d",
                "Siemens Atlassian operations on se-dps (Jira + JSM)",
            ),
            skill(
                "note-6799ed",
                "E.ON Atlassian operations (Jira Cloud and Data Center)",
            ),
        ];
        let terms = query_terms("create a jira issue through the rest api");
        let mut names: Vec<&str> = relevant_skills(&skills, &terms)
            .into_iter()
            .map(|s| s.name.as_str())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["note-d90573", "jira-api"],
            "'jira' is in four of five skills and weighs little; rest/api/issue decide"
        );
        assert!(relevant_skills(&skills, &query_terms("bake bread")).is_empty());
        assert!(
            relevant_skills(&skills, &query_terms("atlassian jira")).is_empty(),
            "words most skills share must not suggest any of them"
        );
    }

    #[test]
    fn past_session_is_the_newest_message_that_covers_the_request() {
        use crate::session::chat_history_search::{ChatRecallMessage, ChatRecallResult};
        let at = |h: u32| chrono::Utc::now() - chrono::Duration::hours(i64::from(h));
        let message = |role: &str, content: &str, h: u32| ChatRecallMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: at(h),
        };
        let results = vec![
            ChatRecallResult {
                session_id: "s-old".to_string(),
                session_description: "vendor port".to_string(),
                session_working_dir: "/tmp".to_string(),
                last_activity: at(50),
                total_messages_in_session: 2,
                messages: vec![message(
                    "user",
                    "which port does the bench vendor answer on?\nsecond line",
                    50,
                )],
            },
            ChatRecallResult {
                session_id: "s-new".to_string(),
                session_description: "unrelated".to_string(),
                session_working_dir: "/tmp".to_string(),
                last_activity: at(1),
                total_messages_in_session: 2,
                messages: vec![message("assistant", "the port is closed", 1)],
            },
        ];
        let terms = query_terms("Which port does the bench vendor answer on?");
        let past = select_past_session(&results, &terms).unwrap();
        assert_eq!(
            past.session_id, "s-old",
            "one shared word ('port') does not cover the request"
        );
        assert_eq!(past.role, "user");
        assert_eq!(past.headline, "which port does the bench vendor answer on?");
        let block = render(&[], &[], Some(&past)).unwrap();
        assert!(block.starts_with("<past-session>"));
        assert!(block.contains("session s-old (\"vendor port\","));
        assert!(select_past_session(&results, &query_terms("bake bread")).is_none());
    }

    #[test]
    fn render_says_nothing_when_nothing_matched() {
        assert_eq!(render(&[], &[], None), None);
        let hits = [hit("postgres", 2.0, 1)];
        let skills = [skill("jira-api", "Jira REST")];
        let refs: Vec<&SourceEntry> = skills.iter().collect();
        let out = render(&hits, &refs, None).unwrap();
        assert!(out.starts_with("<recalled-memories>"));
        assert!(out.contains("## postgres (global [project])\npostgres headline\nbody"));
        assert!(out.contains("<relevant-skills>\n"));
        assert!(out.contains("- jira-api: Jira REST"));
        assert!(out.ends_with("</relevant-skills>"));
    }
}
