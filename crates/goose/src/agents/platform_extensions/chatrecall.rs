//! Chat search (Q-358 S2): three tools over the transcript index — `search_chats` finds messages
//! in the person's other chats, `read_chat` reads a stretch of one chat around a hit, `list_chats`
//! names the chats there are. The extension id stays `chatrecall` so configs that name it keep it.
//!
//! Every answer fits a budget that is a share of the model's context window and ends with the
//! index's footer, which says how much of the history was searchable when it ran.

use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::tool_execution::ToolCallContext;
use crate::needs_you::{NeedsYouState, NeedsYouStatus};
use crate::session::extension_data::ExtensionState;
use crate::session::session_manager::SessionType;
use crate::session::transcript_index::{
    self, group_digits, message_text, ChatRow, IndexCoverage, Scope, SearchFilter, SearchQuery,
    StoredMessage,
};
use crate::session::ExtensionData;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use indoc::indoc;
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, JsonObject, ListToolsResult,
    ServerCapabilities, Tool, ToolAnnotations,
};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use tokio_util::sync::CancellationToken;

pub static EXTENSION_NAME: &str = "chatrecall";
pub static SEARCH_TOOL: &str = "search_chats";
/// The tool that opens a chat — what recall's `<past-session>` block names.
pub static READ_TOOL: &str = "read_chat";
pub static LIST_TOOL: &str = "list_chats";

// ratio: a tool answer takes at most a thirty-second of the context window — the share recall's
// auto-loaded skill may take — so a 262k window reads ~32k chars of hits and a 32k one ~4k.
const OUTPUT_WINDOW_SHARE: f64 = 1.0 / 32.0;
// ratio: one hit line carries two lines of an 80-column terminal of the message around the match.
const SNIPPET_CHARS: usize = 160;
// ratio: read_chat shows six messages each side of the one asked for — a screen of conversation.
const READ_SPAN: i64 = 6;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct SearchChatsParams {
    /// Words to find. All words must appear unless you write OR between two of them; "an exact
    /// phrase" in double quotes; prefix* for any word starting so; -word to exclude.
    query: String,
    /// Only this chat: its id, or words of its title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chat: Option<String>,
    /// Only chats working in this folder (its subfolders count).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    folder: Option<String>,
    /// Only messages on or after this date (YYYY-MM-DD or RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after: Option<String>,
    /// Only messages on or before this date (YYYY-MM-DD or RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<String>,
    /// Only what the person said ("person"), what goose said ("goose"), or tool calls and output
    /// ("tool").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    /// Only messages that called this tool (e.g. "shell").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    /// Only messages that name this file path, said or used by a tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    /// Also search tool calls and tool output (command lines, file contents, listings). Off by
    /// default: output is noisy and rarely what the person meant.
    #[serde(default)]
    include_tool_output: bool,
    /// Also search chats goose ran as subagents.
    #[serde(default)]
    include_subagents: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct ReadChatParams {
    /// The chat: its id (from search_chats or list_chats), or words of its title.
    chat: String,
    /// A message id from search_chats (like "m1182"): read the messages around it. Without it,
    /// the chat's newest messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    around: Option<String>,
    /// How many messages to show on each side (default 6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    span: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct ListChatsParams {
    /// Only chats working in this folder (its subfolders count).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    folder: Option<String>,
    /// Also list chats goose ran as subagents.
    #[serde(default)]
    include_subagents: bool,
}

/// How much text one answer may carry, and why that much.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputBudget {
    pub chars: usize,
    /// Set when the window is not the measured one — stated in the answer, never silent.
    pub note: Option<String>,
}

impl OutputBudget {
    pub fn for_window(context_tokens: usize, measured: bool) -> Self {
        let chars =
            (context_tokens as f64 * super::recall::CHARS_PER_TOKEN * OUTPUT_WINDOW_SHARE) as usize;
        let note = (!measured).then(|| {
            format!(
                "The model's context window is not known here, so this answer is sized for goose's \
                 default {}-token window.",
                group_digits(context_tokens as u64)
            )
        });
        Self { chars, note }
    }
}

pub struct ChatRecallClient {
    info: InitializeResult,
    context: PlatformExtensionContext,
}

fn parse_date(value: &str, end_of_day: bool) -> Result<DateTime<Utc>, String> {
    if let Ok(at) = DateTime::parse_from_rfc3339(value) {
        return Ok(at.with_timezone(&Utc));
    }
    let day = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("\"{value}\" is not a date; write YYYY-MM-DD or an RFC 3339 time"))?;
    let time = if end_of_day {
        NaiveTime::from_hms_opt(23, 59, 59)
    } else {
        NaiveTime::from_hms_opt(0, 0, 0)
    }
    .ok_or("unreachable time of day")?;
    Local
        .from_local_datetime(&day.and_time(time))
        .earliest()
        .map(|at| at.with_timezone(&Utc))
        .ok_or_else(|| format!("\"{value}\" does not exist in this time zone"))
}

fn when(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).format("%b %d %H:%M").to_string()
}

fn speaker(role: &str) -> &'static str {
    match role {
        "user" => "you",
        "assistant" => "goose",
        _ => "tool",
    }
}

fn home_folder(dir: &str) -> String {
    match dirs::home_dir().and_then(|home| home.to_str().map(String::from)) {
        Some(home) if dir == home => "~".to_string(),
        Some(home) => match dir.strip_prefix(&format!("{home}/")) {
            Some(rest) => format!("~/{rest}"),
            None => dir.to_string(),
        },
        None => dir.to_string(),
    }
}

fn expand_home(dir: &str) -> String {
    match (dir.strip_prefix('~'), dirs::home_dir()) {
        (Some(rest), Some(home)) => format!("{}{rest}", home.display()),
        _ => dir.to_string(),
    }
}

fn title(name: &str) -> &str {
    if name.trim().is_empty() {
        "Untitled chat"
    } else {
        name
    }
}

/// `text` cut to `max` characters on one line, saying how much was left out.
fn clip(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let total = flat.chars().count();
    if total <= max {
        return flat;
    }
    let kept: String = flat.chars().take(max).collect();
    format!(
        "{kept}… [{} more chars]",
        group_digits((total - max) as u64)
    )
}

/// Append `line` while the answer stays within `budget`; false once it would not.
fn push_within(out: &mut String, line: &str, budget: usize) -> bool {
    if out.chars().count() + line.chars().count() + 1 > budget {
        return false;
    }
    out.push_str(line);
    out.push('\n');
    true
}

impl ChatRecallClient {
    pub fn new(context: PlatformExtensionContext) -> Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(EXTENSION_NAME.to_string(), "2.0.0".to_string())
                    .with_title("Chat Search"),
            )
            .with_instructions(
                indoc! {r#"
                Chat Search

                The person's other chats with goose are searchable. Use it when they refer to
                something discussed before ("what did we decide about…", "the column we added in the
                billing chat"), or when an earlier chat plainly holds what this one needs.

                - search_chats: words, "phrases", prefix*, -excluded; one line per hit with its
                  message id (m1182) and chat id. Tool output is left out unless you ask for it.
                - read_chat: the messages around a hit (chat + around=m1182), or a chat's newest.
                - list_chats: the chats there are, newest first.
            "#}
                .to_string(),
            );

        Ok(Self { info, context })
    }

    fn manager(
        &self,
    ) -> Option<std::sync::Arc<crate::agents::extension_manager::ExtensionManager>> {
        self.context
            .extension_manager
            .as_ref()
            .and_then(|weak| weak.upgrade())
    }

    /// The benchmark invariant (frame 1.14 §2.6), recall's predicate: a knowledge-blind agent is
    /// never given the person's other chats — its score must not depend on this machine's history.
    fn open(&self) -> bool {
        let blind = self.manager().is_some_and(|m| m.knowledge_blind());
        super::recall::knowledge_source_open(blind, true)
    }

    fn session_types(&self, include_subagents: bool) -> Vec<SessionType> {
        match self.context.session.as_ref().map(|s| s.session_type) {
            Some(SessionType::Acp) => vec![SessionType::Acp],
            _ if include_subagents => vec![
                SessionType::User,
                SessionType::Scheduled,
                SessionType::SubAgent,
            ],
            _ => vec![SessionType::User, SessionType::Scheduled],
        }
    }

    async fn pool(&self) -> Result<&Pool<Sqlite>, String> {
        self.context
            .session_manager
            .storage()
            .pool()
            .await
            .map_err(|e| format!("the chat store could not be opened: {e}"))
    }

    async fn budget(&self, session_id: &str) -> OutputBudget {
        let model_config = match self.context.model_config_for_session(session_id).await {
            Ok(model_config) => model_config,
            Err(reason) => {
                tracing::warn!(%reason, "chat_search_window_unknown: no model config; answer sized for the default window");
                return OutputBudget::for_window(
                    goose_providers::model::DEFAULT_CONTEXT_LIMIT,
                    false,
                );
            }
        };
        let provider = match self.manager() {
            Some(manager) => manager.get_provider().lock().await.clone(),
            None => None,
        };
        let measured = match provider {
            Some(provider) => {
                crate::context_mgmt::effective_context_limit(
                    provider.as_ref(),
                    &model_config,
                    Some(session_id),
                )
                .await
            }
            None => None,
        };
        match measured {
            Some(tokens) => OutputBudget::for_window(tokens, true),
            None => OutputBudget::for_window(model_config.context_limit(), false),
        }
    }

    async fn search_chats(
        &self,
        session_id: &str,
        args: SearchChatsParams,
    ) -> Result<String, String> {
        let query = SearchQuery::parse(&args.query)?;
        let role = match args.role.as_deref().map(str::to_lowercase).as_deref() {
            None | Some("") => None,
            Some("person") | Some("you") | Some("user") => Some("user"),
            Some("goose") | Some("assistant") => Some("assistant"),
            Some("tool") => Some("tool"),
            Some(other) => {
                return Err(format!(
                    "role \"{other}\" is not one of person, goose or tool"
                ))
            }
        };
        let scope = match (role, args.include_tool_output) {
            (Some("tool"), _) => Scope::Tools,
            (_, true) => Scope::SaidAndTools,
            _ => Scope::Said,
        };
        let names_this_chat = args.chat.as_deref() == Some(session_id);
        let filter = SearchFilter {
            session_types: self.session_types(args.include_subagents),
            exclude_session: (!names_this_chat).then(|| session_id.to_string()),
            chat: args.chat.clone(),
            folder: args.folder.as_deref().map(expand_home),
            after: args
                .after
                .as_deref()
                .map(|d| parse_date(d, false))
                .transpose()?,
            before: args
                .before
                .as_deref()
                .map(|d| parse_date(d, true))
                .transpose()?,
            role: role.filter(|r| *r != "tool").map(String::from),
            tool: args.tool.clone(),
            file: args.file.clone(),
        };
        let budget = self.budget(session_id).await;
        let pool = self.pool().await?;
        let fetch = (budget.chars / SNIPPET_CHARS).max(1) as i64;
        let run = |any: bool| {
            let expr = query.fts(any);
            let filter = &filter;
            async move {
                transcript_index::ranked(pool, &expr, scope, filter, fetch)
                    .await
                    .map_err(|e| format!("chat search failed: {e}"))
            }
        };
        let (mut hits, mut total, mut chats) = run(false).await?;
        let broadened = total == 0 && query.broadenable();
        if broadened {
            (hits, total, chats) = run(true).await?;
        }
        let index = transcript_index::coverage(pool)
            .await
            .map_err(|e| format!("the index could not be measured: {e}"))?;
        Ok(render_search(
            &args.query,
            &query,
            scope,
            &hits,
            total,
            chats,
            broadened,
            &budget,
            &index,
        ))
    }

    async fn read_chat(&self, session_id: &str, args: ReadChatParams) -> Result<String, String> {
        let pool = self.pool().await?;
        let types = self.session_types(true);
        let chat = transcript_index::find_chat(pool, &args.chat, &types)
            .await
            .map_err(|e| format!("the chat store could not be read: {e}"))?
            .ok_or_else(|| format!("no chat is named or titled \"{}\"", args.chat))?;
        if chat.kept_out() && chat.id != session_id {
            return Err(format!(
                "\"{}\" is kept out of search by the person; it cannot be read from another chat",
                title(&chat.name)
            ));
        }
        let around = match args.around.as_deref() {
            None => None,
            Some(raw) => Some(
                raw.trim()
                    .trim_start_matches(['m', 'M'])
                    .parse::<i64>()
                    .map_err(|_| format!("\"{raw}\" is not a message id like m1182"))?,
            ),
        };
        let span = args.span.map(i64::from).unwrap_or(READ_SPAN).max(1);
        let messages = transcript_index::window(pool, &chat.id, around, span)
            .await
            .map_err(|e| format!("the chat could not be read: {e}"))?;
        let budget = self.budget(session_id).await;
        Ok(render_read(&chat, around, &messages, &budget))
    }

    async fn list_chats(&self, session_id: &str, args: ListChatsParams) -> Result<String, String> {
        let pool = self.pool().await?;
        let budget = self.budget(session_id).await;
        let types = self.session_types(args.include_subagents);
        let folder = args.folder.as_deref().map(expand_home);
        let fetch = (budget.chars / SNIPPET_CHARS).max(1) as i64;
        let (rows, total, kept_out) =
            transcript_index::list_chats(pool, &types, folder.as_deref(), fetch)
                .await
                .map_err(|e| format!("the chat store could not be read: {e}"))?;
        Ok(render_list(session_id, &rows, total, kept_out, &budget))
    }

    fn get_tools() -> Vec<Tool> {
        fn schema<T: JsonSchema>() -> rmcp::model::JsonObject {
            serde_json::to_value(schema_for!(T))
                .expect("a derived schema serializes")
                .as_object()
                .expect("a derived schema is an object")
                .clone()
        }
        let read_only = |title: &str| {
            ToolAnnotations::from_raw(
                Some(title.to_string()),
                Some(true),
                Some(false),
                Some(true),
                Some(false),
            )
        };
        vec![
            Tool::new(
                SEARCH_TOOL.to_string(),
                indoc! {r#"
                    Search the person's other chats with goose. Returns one line per matching message —
                    its id (m1182), when, who said it, and the words around the match — grouped by chat,
                    best matches first. All words must match unless you write OR; "phrases", prefix*,
                    -excluded. Narrow with chat, folder, after/before, role, tool or file. Tool output
                    is searched only with include_tool_output.
                "#}
                .to_string(),
                schema::<SearchChatsParams>(),
            )
            .annotate(read_only("Search chats")),
            Tool::new(
                READ_TOOL.to_string(),
                indoc! {r#"
                    Read part of a chat: the messages around a message id from search_chats
                    (around="m1182"), or the chat's newest messages. Long messages are cut to fit.
                "#}
                .to_string(),
                schema::<ReadChatParams>(),
            )
            .annotate(read_only("Read a chat")),
            Tool::new(
                LIST_TOOL.to_string(),
                indoc! {r#"
                    List the person's chats with goose, most recently active first: title, folder, when
                    it was last active, how many messages, and whether goose has a question waiting
                    there for the person.
                "#}
                .to_string(),
                schema::<ListChatsParams>(),
            )
            .annotate(read_only("List chats")),
        ]
    }
}

#[allow(clippy::too_many_arguments)]
fn render_search(
    raw: &str,
    query: &SearchQuery,
    scope: Scope,
    hits: &[transcript_index::Hit],
    total: u64,
    chats: u64,
    broadened: bool,
    budget: &OutputBudget,
    index: &IndexCoverage,
) -> String {
    let searched = match scope {
        Scope::Said => {
            "what you and goose said (tool output not searched; include_tool_output adds it)"
        }
        Scope::SaidAndTools => "what you and goose said and the tool calls and output",
        Scope::Tools => "tool calls and output only",
    };
    let mut out = String::new();
    if total == 0 {
        out.push_str(&format!(
            "No message in your other chats matches \"{raw}\". Searched {searched}.\n"
        ));
    } else {
        let noun = |n: u64, one: &str, many: &str| {
            format!("{} {}", group_digits(n), if n == 1 { one } else { many })
        };
        out.push_str(&format!(
            "{} in {} for \"{raw}\". Searched {searched}.\n",
            noun(total, "hit", "hits"),
            noun(chats, "chat", "chats"),
        ));
        if broadened {
            out.push_str(
                "No message has every word, so these are the messages with any of them, best first.\n",
            );
        }
    }
    let footer = index.footer();
    let reserve = footer.chars().count()
        + budget.note.as_ref().map_or(0, |n| n.chars().count() + 1)
        + SNIPPET_CHARS;
    let room = budget.chars.saturating_sub(reserve);
    let mut groups: Vec<(&str, Vec<&transcript_index::Hit>)> = Vec::new();
    for hit in hits {
        match groups.iter_mut().find(|(id, _)| *id == hit.session_id) {
            Some((_, list)) => list.push(hit),
            None => groups.push((&hit.session_id, vec![hit])),
        }
    }
    let mut shown = 0u64;
    'groups: for (_, list) in &groups {
        let first = list[0];
        let header = format!(
            "\n\"{}\" · {} · chat {}",
            title(&first.session_name),
            home_folder(&first.working_dir),
            first.session_id
        );
        if !push_within(&mut out, &header, room) {
            break;
        }
        for hit in list {
            let text = message_text(&hit.content_json, hit.metadata_json.as_deref());
            let kept_for_itself = transcript_index::agent_only(hit.metadata_json.as_deref());
            let terms: Vec<&transcript_index::Term> = query.terms().collect();
            let tool_label = if kept_for_itself {
                "goose's own note, not shown in the chat"
            } else {
                "tool"
            };
            let (who, snippet) = match scope {
                Scope::Tools => (
                    tool_label,
                    transcript_index::snippet(&text.tool_io, terms.iter().copied(), SNIPPET_CHARS),
                ),
                _ => match transcript_index::snippet(
                    &text.said,
                    terms.iter().copied(),
                    SNIPPET_CHARS,
                ) {
                    Some(s) => (speaker(&hit.role), Some(s)),
                    None => (
                        tool_label,
                        transcript_index::snippet(
                            &text.tool_io,
                            terms.iter().copied(),
                            SNIPPET_CHARS,
                        ),
                    ),
                },
            };
            let snippet = snippet.unwrap_or_else(|| {
                let source = if text.said.is_empty() {
                    &text.tool_io
                } else {
                    &text.said
                };
                clip(source, SNIPPET_CHARS)
            });
            let line = format!(
                "  m{} · {} · {who}: {snippet}",
                hit.message_id,
                when(hit.timestamp)
            );
            if !push_within(&mut out, &line, room) {
                break 'groups;
            }
            shown += 1;
        }
    }
    if shown < total {
        out.push_str(&format!(
            "\n{} more {} not shown — this answer is held to a share of the context window; narrow \
             with chat, folder, after/before or a \"phrase\", or read_chat around a hit.\n",
            group_digits(total - shown),
            if total - shown == 1 { "hit" } else { "hits" }
        ));
    }
    out.push('\n');
    if let Some(note) = &budget.note {
        out.push_str(note);
        out.push('\n');
    }
    out.push_str(&footer);
    out
}

fn render_read(
    chat: &ChatRow,
    around: Option<i64>,
    messages: &[StoredMessage],
    budget: &OutputBudget,
) -> String {
    let mut out = format!(
        "\"{}\" · {} · chat {} · {} messages\n",
        title(&chat.name),
        home_folder(&chat.working_dir),
        chat.id,
        group_digits(chat.message_count as u64)
    );
    if let Some(around) = around {
        if !messages.iter().any(|m| m.message_id == around) {
            out.push_str(&format!(
                "Message m{around} is not in this chat; these are the messages nearest to it.\n"
            ));
        }
    }
    if messages.is_empty() {
        out.push_str("This chat has no messages.\n");
    }
    let reserve = out.chars().count() + budget.note.as_ref().map_or(0, |n| n.chars().count());
    let share = budget.chars.saturating_sub(reserve) / messages.len().max(1);
    for message in messages {
        let text = message_text(&message.content_json, message.metadata_json.as_deref());
        let mark = if Some(message.message_id) == around {
            "»"
        } else {
            " "
        };
        let mut body = Vec::new();
        if !text.said.is_empty() {
            body.push(format!("{}: {}", speaker(&message.role), text.said));
        }
        if !text.tool_io.is_empty() {
            let label = if transcript_index::agent_only(message.metadata_json.as_deref()) {
                "goose's own note, not shown in the chat"
            } else if message.role == "assistant" {
                "tool call"
            } else {
                "tool output"
            };
            body.push(format!("[{label}] {}", text.tool_io));
        }
        if body.is_empty() {
            body.push(format!("{}: (no text)", speaker(&message.role)));
        }
        let head = format!(
            "{mark}m{} · {} · ",
            message.message_id,
            when(message.timestamp)
        );
        let room = share.saturating_sub(head.chars().count());
        out.push_str(&head);
        out.push_str(&clip(&body.join(" "), room));
        out.push('\n');
    }
    if let Some(note) = &budget.note {
        out.push_str(note);
        out.push('\n');
    }
    out
}

fn render_list(
    session_id: &str,
    rows: &[ChatRow],
    total: u64,
    kept_out: u64,
    budget: &OutputBudget,
) -> String {
    let mut out = format!(
        "{} chats, most recently active first.\n",
        group_digits(total)
    );
    let mut shown = 0u64;
    for row in rows {
        let waiting = serde_json::from_str::<ExtensionData>(&row.extension_data)
            .ok()
            .and_then(|data| NeedsYouState::from_extension_data(&data))
            .map_or(0, |state| {
                state
                    .items
                    .iter()
                    .filter(|item| item.status == NeedsYouStatus::Open)
                    .count()
            });
        let mut line = format!(
            "\"{}\" · {} · chat {} · last active {} · {} messages",
            title(&row.name),
            home_folder(&row.working_dir),
            row.id,
            when(row.updated_at),
            group_digits(row.message_count as u64)
        );
        if waiting > 0 {
            line.push_str(&format!(
                " · goose asked the person {waiting} {} there",
                if waiting == 1 {
                    "question"
                } else {
                    "questions"
                }
            ));
        }
        if row.id == session_id {
            line.push_str(" · this chat");
        }
        if row.session_type == SessionType::SubAgent.to_string() {
            line.push_str(" · subagent");
        }
        if !push_within(&mut out, &line, budget.chars) {
            break;
        }
        shown += 1;
    }
    if shown < total {
        out.push_str(&format!(
            "{} older chats not shown — this answer is held to a share of the context window; \
             narrow with folder.\n",
            group_digits(total - shown)
        ));
    }
    if kept_out > 0 {
        out.push_str(&format!(
            "{} {} kept out of search by the person and not listed.\n",
            group_digits(kept_out),
            if kept_out == 1 {
                "chat is"
            } else {
                "chats are"
            }
        ));
    }
    out.push_str("Whether a chat is running a turn right now is not visible to this tool.\n");
    if let Some(note) = &budget.note {
        out.push_str(note);
        out.push('\n');
    }
    out
}

fn parse<T: for<'de> Deserialize<'de>>(arguments: Option<JsonObject>) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::Object(arguments.unwrap_or_default()))
        .map_err(|e| format!("invalid arguments: {e}"))
}

#[async_trait]
impl McpClientTrait for ChatRecallClient {
    async fn list_tools(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        Ok(ListToolsResult {
            tools: if self.open() {
                Self::get_tools()
            } else {
                Vec::new()
            },
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        _cancellation_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        let session_id = &ctx.session_id;
        let content = if !self.open() {
            Err("chat search is closed to a knowledge-blind (benchmark) agent".to_string())
        } else {
            match name {
                n if n == SEARCH_TOOL => match parse(arguments) {
                    Ok(args) => self.search_chats(session_id, args).await,
                    Err(e) => Err(e),
                },
                n if n == READ_TOOL => match parse(arguments) {
                    Ok(args) => self.read_chat(session_id, args).await,
                    Err(e) => Err(e),
                },
                n if n == LIST_TOOL => match parse(arguments) {
                    Ok(args) => self.list_chats(session_id, args).await,
                    Err(e) => Err(e),
                },
                _ => Err(format!("Unknown tool: {name}")),
            }
        };

        match content {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Error: {error}"
            ))])),
        }
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_is_a_share_of_the_window_and_says_when_the_window_is_a_default() {
        let measured = OutputBudget::for_window(262_144, true);
        assert_eq!(measured.chars, 32_768);
        assert_eq!(measured.note, None);
        let small = OutputBudget::for_window(32_768, true);
        assert_eq!(small.chars, 4_096);
        let default = OutputBudget::for_window(128_000, false);
        assert!(default
            .note
            .as_deref()
            .is_some_and(|n| n.contains("128,000-token window")));
    }

    #[test]
    fn a_date_is_a_day_or_a_time_and_anything_else_is_refused() {
        assert!(parse_date("2026-09-27", false).is_ok());
        assert!(parse_date("2026-09-27T14:02:00Z", true).is_ok());
        let err = parse_date("last tuesday", false).unwrap_err();
        assert!(err.contains("YYYY-MM-DD"), "{err}");
        assert!(parse_date("2026-09-27", false).unwrap() < parse_date("2026-09-27", true).unwrap());
    }

    use crate::config::GooseMode;
    use crate::conversation::message::Message;
    use crate::session::transcript_index::ChatSearchState;
    use crate::session::SessionManager;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct Fixture {
        _dir: TempDir,
        sm: Arc<SessionManager>,
        here: String,
        client: ChatRecallClient,
    }

    async fn chat(sm: &SessionManager, name: &str, t: SessionType, texts: &[Message]) -> String {
        let id = sm
            .create_session(
                PathBuf::from("/tmp/chats"),
                name.to_string(),
                t,
                GooseMode::default(),
            )
            .await
            .unwrap()
            .id;
        for message in texts {
            sm.add_message(&id, message).await.unwrap();
        }
        id
    }

    async fn fixture() -> Fixture {
        let dir = TempDir::new().unwrap();
        let sm = Arc::new(SessionManager::new(dir.path().to_path_buf()));
        let here = chat(&sm, "This chat", SessionType::User, &[]).await;
        let session = sm.get_session(&here, false).await.unwrap();
        let client = ChatRecallClient::new(PlatformExtensionContext {
            extension_manager: None,
            session_manager: sm.clone(),
            session: Some(Arc::new(session)),
            use_login_shell_path: false,
            working_dir: None,
        })
        .unwrap();
        Fixture {
            _dir: dir,
            sm,
            here,
            client,
        }
    }

    async fn call(f: &Fixture, tool: &str, args: serde_json::Value) -> (bool, String) {
        let result = f
            .client
            .call_tool(
                &ToolCallContext::new(f.here.clone(), None, None),
                tool,
                args.as_object().cloned(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap().text.clone();
        (result.is_error.unwrap_or(false), text)
    }

    fn shell_output(id: &str, output: &str) -> Message {
        Message::user().with_tool_response(
            id.to_string(),
            Ok(rmcp::model::CallToolResult::success(vec![
                rmcp::model::Content::text(output.to_string()),
            ])),
        )
    }

    /// The owner's case (DESIGN-Q358): "split mesh" must lead with chat text, not `git branch`
    /// listings. Default search never reads tool output; with it, said still outranks it 4:1.
    #[tokio::test]
    async fn split_mesh_finds_what_was_said_before_git_branch_listings() {
        let f = fixture().await;
        let listings: Vec<Message> = (0..3)
            .map(|i| {
                shell_output(
                    &format!("c{i}"),
                    "  main\n* split-mesh\n  split-mesh-relay\n  split-mesh-v2\n  mesh-split-probe",
                )
            })
            .collect();
        chat(&f.sm, "Git work", SessionType::User, &listings).await;
        let talk = chat(
            &f.sm,
            "Explore split mesh",
            SessionType::User,
            &[Message::user().with_text(
                "For the split mesh, rank 0 holds the first half of the layers and the relay \
                 forwards activations over Thunderbolt; we still need to decide how the two Macs \
                 agree on the model before loading.",
            )],
        )
        .await;

        let (_, said) = call(&f, SEARCH_TOOL, serde_json::json!({"query": "split mesh"})).await;
        assert!(said.starts_with("1 hit in 1 chat"), "{said}");
        assert!(said.contains(&format!("chat {talk}")), "{said}");
        assert!(!said.contains("split-mesh-relay"), "{said}");

        let (_, all) = call(
            &f,
            SEARCH_TOOL,
            serde_json::json!({"query": "split mesh", "include_tool_output": true}),
        )
        .await;
        assert!(all.starts_with("4 hits in 2 chats"), "{all}");
        let first_hit = all
            .lines()
            .find(|l| l.trim_start().starts_with('m'))
            .unwrap();
        assert!(first_hit.contains("you: "), "said leads: {all}");
    }

    /// Subagent chats only when asked; a kept-out chat never — in search, read and list; the
    /// current chat is not searched unless named.
    #[tokio::test]
    async fn the_scope_rules_hold_for_every_tool() {
        let f = fixture().await;
        let text = |t: &str| vec![Message::user().with_text(t)];
        chat(
            &f.sm,
            "Sub",
            SessionType::SubAgent,
            &text("tenant_id in a subagent"),
        )
        .await;
        let kept = chat(
            &f.sm,
            "Private",
            SessionType::User,
            &text("tenant_id private"),
        )
        .await;
        f.sm.set_extension_state(&kept, &ChatSearchState { keep_out: true })
            .await
            .unwrap();
        chat(
            &f.sm,
            "Worker",
            SessionType::Hidden,
            &text("tenant_id worker"),
        )
        .await;
        f.sm.add_message(&f.here, &Message::user().with_text("tenant_id here"))
            .await
            .unwrap();

        let (_, none) = call(&f, SEARCH_TOOL, serde_json::json!({"query": "tenant_id"})).await;
        assert!(none.starts_with("No message"), "{none}");
        let (_, sub) = call(
            &f,
            SEARCH_TOOL,
            serde_json::json!({"query": "tenant_id", "include_subagents": true}),
        )
        .await;
        assert!(sub.starts_with("1 hit in 1 chat"), "{sub}");
        assert!(sub.contains("\"Sub\""), "{sub}");
        let (_, here) = call(
            &f,
            SEARCH_TOOL,
            serde_json::json!({"query": "tenant_id", "chat": f.here}),
        )
        .await;
        assert!(here.starts_with("1 hit in 1 chat"), "{here}");

        let (err, read) = call(&f, READ_TOOL, serde_json::json!({"chat": kept})).await;
        assert!(err && read.contains("kept out of search"), "{read}");
        let (err, worker) = call(&f, READ_TOOL, serde_json::json!({"chat": "Worker"})).await;
        assert!(err && worker.contains("no chat is named"), "{worker}");

        let (_, list) = call(&f, LIST_TOOL, serde_json::json!({})).await;
        assert!(!list.contains("Private"), "{list}");
        assert!(list.contains("1 chat is kept out of search"), "{list}");
        assert!(
            !list.contains("Worker") && !list.contains("\"Sub\""),
            "{list}"
        );
        assert!(list.contains("· this chat"), "{list}");
    }

    /// A read around a hit shows the messages on both sides, the hit marked.
    #[tokio::test]
    async fn read_chat_opens_the_stretch_around_a_hit() {
        let f = fixture().await;
        let texts: Vec<Message> = (0..20)
            .map(|i| Message::user().with_text(format!("step {i} of the migration")))
            .collect();
        let id = chat(&f.sm, "Migrate billing", SessionType::User, &texts).await;
        let pool = f.sm.storage().pool().await.unwrap();
        let ids: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM messages WHERE session_id = ? ORDER BY id")
                .bind(&id)
                .fetch_all(pool)
                .await
                .unwrap();
        let (_, read) = call(
            &f,
            READ_TOOL,
            serde_json::json!({"chat": "billing", "around": format!("m{}", ids[10]), "span": 2}),
        )
        .await;
        let shown: Vec<&str> = read.lines().filter(|l| l.contains(" · you: ")).collect();
        assert_eq!(shown.len(), 5, "{read}");
        assert!(read.contains(&format!("»m{} ", ids[10])), "{read}");
        assert!(
            read.contains("step 8 ") && read.contains("step 12 "),
            "{read}"
        );
        assert!(
            !read.contains("step 7 ") && !read.contains("step 13 "),
            "{read}"
        );
    }

    /// Output never exceeds the share of the window, and says how many hits it left out.
    #[tokio::test]
    async fn a_search_stays_within_its_budget_and_counts_what_it_left_out() {
        let f = fixture().await;
        let texts: Vec<Message> = (0..300)
            .map(|i| {
                Message::user().with_text(format!(
                    "ledger entry {i}: {} and the ledger closes here",
                    "the quarterly numbers were reconciled against the bank export ".repeat(3)
                ))
            })
            .collect();
        chat(&f.sm, "Ledger", SessionType::User, &texts).await;
        let (_, out) = call(&f, SEARCH_TOOL, serde_json::json!({"query": "ledger"})).await;
        let budget = OutputBudget::for_window(goose_providers::model::DEFAULT_CONTEXT_LIMIT, false);
        assert!(
            out.starts_with("300 hits in 1 chat"),
            "{}",
            out.chars().take(200).collect::<String>()
        );
        assert!(
            out.chars().count() <= budget.chars,
            "{} chars against a budget of {}",
            out.chars().count(),
            budget.chars
        );
        assert!(out.contains("more hits not shown"), "{out}");
        assert!(out.contains("sized for goose's default"), "{out}");
    }

    /// While the backfill has not reached older messages the answer says so — never a quiet
    /// partial result.
    #[tokio::test]
    async fn a_partial_index_is_named_in_the_footer() {
        let f = fixture().await;
        let texts: Vec<Message> = (0..4)
            .map(|i| Message::user().with_text(format!("old tenant_id note {i}")))
            .collect();
        chat(&f.sm, "Old", SessionType::User, &texts).await;
        let pool = f.sm.storage().pool().await.unwrap();
        sqlx::query("INSERT INTO messages_fts (messages_fts) VALUES ('delete-all')")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE messages_fts_state SET backfill_below = (SELECT MAX(id) + 1 FROM messages)",
        )
        .execute(pool)
        .await
        .unwrap();
        let (_, partial) = call(&f, SEARCH_TOOL, serde_json::json!({"query": "tenant_id"})).await;
        assert!(
            partial.contains("Indexed 0 of 4 messages. 4 older messages are not indexed yet"),
            "{partial}"
        );
        while transcript_index::backfill_step(pool, 2).await.unwrap()
            != transcript_index::BackfillStep::Done
        {}
        let (_, full) = call(&f, SEARCH_TOOL, serde_json::json!({"query": "tenant_id"})).await;
        assert!(full.starts_with("4 hits in 1 chat"), "{full}");
        assert!(full.ends_with("Indexed 4 of 4 messages."), "{full}");
    }

    /// The Q-358 measurement, run by hand on a COPY of a real history — never the live file:
    /// `Q358_DATA_DIR=<dir holding sessions/sessions.db> cargo test -p goose --lib
    /// measure_on_a_copy_of_the_history -- --ignored --nocapture`. It migrates the copy, times the
    /// startup backfill, and times five queries through recall's per-turn search and search_chats.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn measure_on_a_copy_of_the_history() {
        let Some(dir) = std::env::var_os("Q358_DATA_DIR") else {
            panic!("set Q358_DATA_DIR to a directory holding a COPY at sessions/sessions.db");
        };
        let db = PathBuf::from(&dir).join("sessions").join("sessions.db");
        {
            // The index objects and the backfill, timed batch by batch on a bare pool, so no
            // startup task races the clock; the SessionManager below then finds them in place.
            let raw = sqlx::sqlite::SqlitePoolOptions::new()
                .connect_with(
                    sqlx::sqlite::SqliteConnectOptions::new()
                        .filename(&db)
                        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
                )
                .await
                .unwrap();
            let started = std::time::Instant::now();
            let mut tx = raw.begin_with("BEGIN IMMEDIATE").await.unwrap();
            transcript_index::create_objects(&mut tx).await.unwrap();
            tx.commit().await.unwrap();
            println!("index objects created: {:?}", started.elapsed());
            let mut times = Vec::new();
            let mut rows = 0;
            loop {
                let t = std::time::Instant::now();
                match transcript_index::backfill_step(&raw, transcript_index::BACKFILL_BATCH_ROWS)
                    .await
                    .unwrap()
                {
                    transcript_index::BackfillStep::Indexed { rows: n, .. } => {
                        rows += n;
                        times.push(t.elapsed());
                    }
                    transcript_index::BackfillStep::Done => break,
                }
            }
            times.sort();
            let total: std::time::Duration = times.iter().sum();
            println!(
                "backfill: {rows} rows in {} batches, {total:?} in batches; median batch {:?}, slowest {:?}",
                times.len(),
                times[times.len() / 2],
                times[times.len() - 1]
            );
            raw.close().await;
        }
        let started = std::time::Instant::now();
        let sm = Arc::new(SessionManager::new(PathBuf::from(dir)));
        let pool = sm.storage().pool().await.unwrap();
        println!("migration v15 + startup: {:?}", started.elapsed());
        let cov = transcript_index::coverage(pool).await.unwrap();
        println!("coverage: {cov:?}");
        assert!(!cov.backfilling);

        let here = chat(&sm, "measure", SessionType::User, &[]).await;
        let session = sm.get_session(&here, false).await.unwrap();
        let f = Fixture {
            _dir: TempDir::new().unwrap(),
            sm: sm.clone(),
            here,
            client: ChatRecallClient::new(PlatformExtensionContext {
                extension_manager: None,
                session_manager: sm.clone(),
                session: Some(Arc::new(session)),
                use_login_shell_path: false,
                working_dir: None,
            })
            .unwrap(),
        };
        for query in [
            "split mesh",
            "killpg",
            "tenant",
            "compaction pillars",
            "notarized release build",
        ] {
            let t = std::time::Instant::now();
            let recall = sm
                .search_chat_history(
                    query,
                    Some(super::super::recall::PAST_SESSION_ROWS),
                    None,
                    None,
                    None,
                    vec![SessionType::User, SessionType::Scheduled],
                )
                .await
                .unwrap();
            let recall_time = t.elapsed();
            // The LIKE scan recall ran before Q-358 (chat_history_search.rs at 2b17dd44d), same
            // build, same file, for the comparison.
            let keywords: Vec<String> = query
                .split_whitespace()
                .map(|w| format!("%{}%", w.to_lowercase()))
                .collect();
            let any =
                vec!["LOWER(json_extract(value, '$.text')) LIKE ?"; keywords.len()].join(" OR ");
            let score = vec!["(m.content_json LIKE ?)"; keywords.len()].join(" + ");
            let old_sql = format!(
                "SELECT s.id, m.content_json FROM messages m INNER JOIN sessions s ON m.session_id = s.id \
                 WHERE EXISTS (SELECT 1 FROM json_each(m.content_json) WHERE json_extract(value, '$.type') = 'text' AND ({any})) \
                 AND s.session_type IN ('user', 'scheduled') ORDER BY ({score}) DESC, m.timestamp DESC LIMIT 20"
            );
            let t = std::time::Instant::now();
            let mut old = sqlx::query_as::<_, (String, String)>(&old_sql);
            for k in keywords.iter().chain(keywords.iter()) {
                old = old.bind(k.clone());
            }
            let old_rows = old.fetch_all(pool).await.unwrap().len();
            println!("old LIKE scan: {:?} ({old_rows} rows)", t.elapsed());
            let t = std::time::Instant::now();
            let (_, out) = call(&f, SEARCH_TOOL, serde_json::json!({"query": query})).await;
            let search_time = t.elapsed();
            let t = std::time::Instant::now();
            let (_, with_tools) = call(
                &f,
                SEARCH_TOOL,
                serde_json::json!({"query": query, "include_tool_output": true}),
            )
            .await;
            let tools_time = t.elapsed();
            println!(
                "\n=== {query}: recall {recall_time:?} ({} rows), search_chats {search_time:?}, with tool output {tools_time:?}",
                recall.total_matches
            );
            println!("{}", out.chars().take(1400).collect::<String>());
            println!("--- with tool output ---");
            println!("{}", with_tools.chars().take(900).collect::<String>());
        }
    }
}
