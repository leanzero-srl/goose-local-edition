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
                crate::context_mgmt::effective_context_limit(provider.as_ref(), &model_config).await
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
            let text = message_text(&hit.content_json);
            let terms: Vec<&transcript_index::Term> = query.terms().collect();
            let (who, snippet) = match scope {
                Scope::Tools => (
                    "tool",
                    transcript_index::snippet(&text.tool_io, terms.iter().copied(), SNIPPET_CHARS),
                ),
                _ => match transcript_index::snippet(
                    &text.said,
                    terms.iter().copied(),
                    SNIPPET_CHARS,
                ) {
                    Some(s) => (speaker(&hit.role), Some(s)),
                    None => (
                        "tool",
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
        let text = message_text(&message.content_json);
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
            let label = if message.role == "assistant" {
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
}
