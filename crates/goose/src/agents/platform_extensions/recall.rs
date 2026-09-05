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
use anyhow::Result;
use async_trait::async_trait;
use goose_memory_store::{search_terms, MemoryStore, SearchHit};
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

/// Which hits are worth injecting: an entry that shares at least two terms with the request, or one
/// whose NAME (category, tags, headline) carries a request term. Anything matching one body word in
/// passing stays out.
pub fn select_hits(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    hits.into_iter()
        .filter(|hit| hit.matched_terms >= 2 || hit.name_terms >= 1)
        .take(RECALL_MAX_MEMORIES)
        .collect()
}

/// Skills whose name or description shares at least two terms with the request, or whose name shares
/// one, best first.
pub fn relevant_skills<'a>(skills: &'a [SourceEntry], terms: &[String]) -> Vec<&'a SourceEntry> {
    let mut scored: Vec<(usize, usize, &SourceEntry)> = skills
        .iter()
        .filter(|s| matches!(s.source_type, SourceType::Skill | SourceType::BuiltinSkill))
        .filter_map(|skill| {
            let name = skill.name.to_lowercase();
            let text = format!("{} {}", name, skill.description).to_lowercase();
            let name_terms = terms.iter().filter(|t| name.contains(t.as_str())).count();
            let matched = terms.iter().filter(|t| text.contains(t.as_str())).count();
            (matched >= 2 || name_terms >= 1).then_some((name_terms, matched, skill))
        })
        .collect();
    scored.sort_by(|a, b| {
        (b.0, b.1)
            .cmp(&(a.0, a.1))
            .then_with(|| a.2.name.cmp(&b.2.name))
    });
    scored
        .into_iter()
        .take(RECALL_MAX_SKILLS)
        .map(|(_, _, skill)| skill)
        .collect()
}

/// The turn-context part. None when there is nothing to say.
pub fn render(memories: &[SearchHit], skills: &[&SourceEntry]) -> Option<String> {
    if memories.is_empty() && skills.is_empty() {
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
                Ok(hits) => select_hits(hits),
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

        let part = render(&memories, &skills);
        tracing::info!(
            session_id,
            terms = terms.len(),
            memories = memories.len(),
            skills = skills.len(),
            "recall"
        );
        part
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_memory_store::MemoryEntry;

    fn hit(category: &str, matched: usize, name_terms: usize) -> SearchHit {
        SearchHit {
            matched_terms: matched,
            phrase: false,
            name_terms,
            occurrences: matched,
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
    fn select_hits_needs_two_shared_terms_or_a_name_match() {
        let hits = vec![
            hit("a-passing-mention", 1, 0),
            hit("postgres", 1, 1),
            hit("docker-compose", 2, 0),
            hit("three", 3, 1),
            hit("four", 2, 1),
            hit("five", 2, 0),
        ];
        let kept: Vec<String> = select_hits(hits)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(kept, vec!["postgres", "docker-compose", "three"]);
    }

    #[test]
    fn relevant_skills_match_on_name_or_two_description_terms() {
        let skills = vec![
            skill(
                "jira-api",
                "Jira Cloud REST API v3 integration from external apps",
            ),
            skill(
                "leanzero-newsroom",
                "Research and draft articles about local models",
            ),
            skill(
                "note-d90573",
                "Axpo Atlassian operations: Jira issue triage over the REST API",
            ),
            skill(
                "note-651f5d",
                "Siemens Atlassian operations on se-dps (Jira + JSM)",
            ),
        ];
        let terms = query_terms("create a jira issue through the rest api");
        let names: Vec<&str> = relevant_skills(&skills, &terms)
            .into_iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["jira-api", "note-d90573"],
            "name match first, then two description terms; one passing 'jira' is not enough"
        );
        assert!(relevant_skills(&skills, &query_terms("bake bread")).is_empty());
    }

    #[test]
    fn render_says_nothing_when_nothing_matched() {
        assert_eq!(render(&[], &[]), None);
        let hits = vec![hit("postgres", 2, 1)];
        let skills = vec![skill("jira-api", "Jira REST")];
        let refs: Vec<&SourceEntry> = skills.iter().collect();
        let out = render(&hits, &refs).unwrap();
        assert!(out.starts_with("<recalled-memories>"));
        assert!(out.contains("## postgres (global [project])\npostgres headline\nbody"));
        assert!(out.contains("<relevant-skills>\n"));
        assert!(out.contains("- jira-api: Jira REST"));
        assert!(out.ends_with("</relevant-skills>"));
    }
}
