//! The project ledger: a dated, append-only log of what happened in THIS project — findings,
//! decisions, what was tried and why it is not coming back, facts learned — kept in
//! `<working_dir>/.goose/ledger.md` and shown newest-first in every turn's context. It is the
//! chronological complement of project memory (facts, deduplicated) and of the scratchpad (this
//! session's state): after a compaction the model reads the ledger's tail and knows what the project
//! has already been through. Modelled on the operator's own agent ledgers (TICK-NOTES,
//! EXPERIMENTS-LEDGER): one line per entry, a date, a kind, the words. The ledger follows the chat's
//! CURRENT folder; a chat whose folder is the home folder has no project and keeps its own
//! ledger (`LedgerFile::for_chat`).

use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::tool_execution::ToolCallContext;
use anyhow::Result;
use async_trait::async_trait;
use goose_memory_store::{search_terms, term_occurrences, tokenize};
use indoc::indoc;
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, JsonObject, ListToolsResult,
    ServerCapabilities, Tool, ToolAnnotations,
};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

pub static EXTENSION_NAME: &str = "ledger";
pub const LEDGER_FILE: &str = "ledger.md";

// ratio: the newest five entries are one screen of context per turn; the whole ledger is a tool call
// away.
const TAIL_ENTRIES: usize = 5;
// ratio: a read returns at most twenty entries — four tails — so one call cannot flood the window.
const READ_MAX_ENTRIES: usize = 20;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct LedgerAppendParams {
    /// finding | decision | tried | fact
    kind: String,
    /// One entry: what happened, the number or file that proves it, and why it matters
    text: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct LedgerReadParams {
    /// Words to look for; omit for the newest entries
    #[serde(default)]
    query: Option<String>,
    /// Maximum entries (default 20)
    #[serde(default)]
    limit: Option<usize>,
}

pub struct LedgerClient {
    info: InitializeResult,
    context: PlatformExtensionContext,
}

pub fn ledger_path(working_dir: &Path) -> PathBuf {
    working_dir.join(".goose").join(LEDGER_FILE)
}

/// Where one chat's ledger lives and how the tools name it to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerFile {
    pub path: PathBuf,
    /// The chat runs in the home folder: the ledger is the chat's own, not a project's.
    pub chat_only: bool,
}

impl LedgerFile {
    /// The ledger of a chat in `working_dir`. A project folder keeps `.goose/ledger.md`, shared by
    /// every chat in it. The HOME folder is no project — it is where every new chat starts — so a
    /// chat there keeps its own ledger under `~/.goose/ledgers/<session>.md`.
    ///
    /// Why (Q-89, E2E #2b, 2026-09-25): two chats about two clients both ran in the home folder and
    /// shared `~/.goose/ledger.md`; #2b's turn context carried #1's "[decision] Harbourline…".
    pub fn for_chat(working_dir: &Path, session_id: &str, home: Option<&Path>) -> Self {
        if home.is_some_and(|home| home == working_dir) {
            let key: String = session_id
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            Self {
                path: working_dir
                    .join(".goose")
                    .join("ledgers")
                    .join(format!("{key}.md")),
                chat_only: true,
            }
        } else {
            Self {
                path: ledger_path(working_dir),
                chat_only: false,
            }
        }
    }

    fn label(&self) -> String {
        if self.chat_only {
            format!("this chat's ledger, {}", self.path.display())
        } else {
            ".goose/ledger.md".to_string()
        }
    }

    pub fn read_all(&self) -> std::io::Result<Vec<String>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        Ok(parse_entries(&std::fs::read_to_string(&self.path)?))
    }

    fn append(&self, kind: &str, text: &str) -> std::io::Result<usize> {
        use std::io::Write;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let fresh = !self.path.exists();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;
        if fresh {
            if self.chat_only {
                writeln!(file, "# Chat ledger — one dated line per finding, decision, attempt or fact; newest last\n")?;
            } else {
                writeln!(file, "# Project ledger — one dated line per finding, decision, attempt or fact; newest last\n")?;
            }
        }
        let when = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        file.write_all(format_entry(&when, kind, text).as_bytes())?;
        Ok(self.read_all()?.len())
    }

    /// The `<ledger>` part of a turn's context.
    pub fn moim(&self) -> String {
        let where_ = if self.chat_only {
            "This chat's folder is the home folder, which is no project, so this chat keeps its own \
             ledger; other chats never see it. When the user names a folder to work in, offer to make \
             it this chat's folder (the folder chip under the message box) — the ledger and the \
             project memories then follow the chat there.\n"
        } else {
            ""
        };
        match self.read_all() {
            Ok(entries) if entries.is_empty() => {
                if self.chat_only {
                    format!(
                        "<ledger>\n{where_}This chat's ledger ({}) is empty — ledger_append the first finding, decision or dead end you meet.\n</ledger>\n",
                        self.path.display()
                    )
                } else {
                    "<ledger>\nThe project ledger (.goose/ledger.md) is empty — ledger_append the first finding, decision or dead end you meet.\n</ledger>\n"
                        .to_string()
                }
            }
            Ok(entries) => {
                let tail = select_entries(&entries, None, TAIL_ENTRIES);
                let mut out = if self.chat_only {
                    format!(
                        "<ledger>\n{where_}Chat ledger ({}), {} entries, newest {} — what this chat has already been through; ledger_read for more:\n",
                        self.path.display(),
                        entries.len(),
                        tail.len()
                    )
                } else {
                    format!(
                        "<ledger>\nProject ledger (.goose/ledger.md), {} entries, newest {} — what this project has already been through; ledger_read for more:\n",
                        entries.len(),
                        tail.len()
                    )
                };
                for entry in tail {
                    out.push_str("- ");
                    out.push_str(&entry);
                    out.push('\n');
                }
                out.push_str("</ledger>\n");
                out
            }
            Err(err) => {
                tracing::warn!(%err, path = %self.path.display(), "ledger unreadable");
                format!(
                    "<ledger>\nThe {} could not be read ({err}).\n</ledger>\n",
                    if self.chat_only {
                        "chat ledger"
                    } else {
                        "project ledger"
                    }
                )
            }
        }
    }
}

/// One entry per line: `- <YYYY-MM-DD HH:MM> [<kind>] <text>`; blank lines and headings are skipped.
pub fn parse_entries(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(str::to_string)
        .collect()
}

pub fn format_entry(when: &str, kind: &str, text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("- {when} [{kind}] {text}\n")
}

/// Newest first, at most `limit`; with a query, only entries sharing at least half its terms.
pub fn select_entries(entries: &[String], query: Option<&str>, limit: usize) -> Vec<String> {
    let terms = query.map(search_terms).unwrap_or_default();
    entries
        .iter()
        .rev()
        .filter(|entry| {
            if terms.is_empty() {
                return true;
            }
            let tokens = tokenize(entry);
            let matched = terms
                .iter()
                .filter(|term| term_occurrences(term, &tokens) > 0)
                .count();
            matched * 2 >= terms.len()
        })
        .take(limit.max(1))
        .cloned()
        .collect()
}

impl LedgerClient {
    pub fn new(context: PlatformExtensionContext) -> Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(EXTENSION_NAME.to_string(), "1.0.0".to_string())
                    .with_title("Ledger"),
            )
            .with_instructions(
                indoc! {r#"
                The project ledger (.goose/ledger.md) is this project's dated log — findings, decisions,
                what was tried and why it is not coming back, facts learned. Its newest entries are in
                every turn's context and it survives compaction and sessions. Append to it with
                ledger_append the moment you learn something a future session would otherwise rediscover:
                a measurement, a dead end, a decision with its reason. Read further back with
                ledger_read. Facts that should be recalled by topic belong in memory; the ledger is
                what happened, in order.
            "#}
                .to_string(),
            );
        Ok(Self { info, context })
    }

    /// The chat's CURRENT folder: the session is read at every call, because the user can move a
    /// chat to another folder mid-session (the folder chip → `update_working_dir`), and a folder
    /// captured when the extension was built kept writing to the old one.
    async fn ledger_file(&self, session_id: &str) -> LedgerFile {
        let working_dir = match self
            .context
            .session_manager
            .get_session(session_id, false)
            .await
        {
            Ok(session) => session.working_dir,
            Err(err) => {
                tracing::warn!(session_id, %err, "ledger: session unreadable, using the folder the extension started in");
                self.context
                    .session
                    .as_ref()
                    .map(|s| s.working_dir.clone())
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            }
        };
        LedgerFile::for_chat(&working_dir, session_id, dirs::home_dir().as_deref())
    }

    fn get_tools() -> Vec<Tool> {
        let append_schema = serde_json::to_value(schema_for!(LedgerAppendParams))
            .expect("LedgerAppendParams schema");
        let read_schema =
            serde_json::to_value(schema_for!(LedgerReadParams)).expect("LedgerReadParams schema");
        vec![
            Tool::new(
                "ledger_append".to_string(),
                "Append one dated entry to the project ledger (.goose/ledger.md): a finding, a decision \
                 with its reason, something tried that is not coming back, or a fact learned. Do it the \
                 moment you learn it — a future session, or you after a compaction, reads this first."
                    .to_string(),
                append_schema.as_object().unwrap().clone(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Ledger append".to_string()),
                Some(false),
                Some(false),
                Some(false),
                Some(false),
            )),
            Tool::new(
                "ledger_read".to_string(),
                "Read the project ledger, newest first: the latest entries, or the entries whose words \
                 match a query."
                    .to_string(),
                read_schema.as_object().unwrap().clone(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Ledger read".to_string()),
                Some(true),
                Some(false),
                Some(false),
                Some(false),
            )),
        ]
    }
}

#[async_trait]
impl McpClientTrait for LedgerClient {
    async fn list_tools(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        Ok(ListToolsResult {
            tools: Self::get_tools(),
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
        let ledger = match &ctx.working_dir {
            Some(working_dir) => {
                LedgerFile::for_chat(working_dir, &ctx.session_id, dirs::home_dir().as_deref())
            }
            None => self.ledger_file(&ctx.session_id).await,
        };
        let result: std::result::Result<String, String> = match name {
            "ledger_append" => {
                let parsed: std::result::Result<LedgerAppendParams, String> = arguments
                    .ok_or_else(|| "Missing arguments".to_string())
                    .and_then(|a| {
                        serde_json::from_value(serde_json::Value::Object(a))
                            .map_err(|e| e.to_string())
                    });
                match parsed {
                    Err(e) => Err(e),
                    Ok(params) => {
                        let kind = params.kind.trim().to_lowercase();
                        if !matches!(kind.as_str(), "finding" | "decision" | "tried" | "fact") {
                            Err(format!(
                                "kind must be finding, decision, tried or fact (got '{}')",
                                params.kind
                            ))
                        } else if params.text.trim().is_empty() {
                            Err("text must not be empty".to_string())
                        } else {
                            match ledger.append(&kind, &params.text) {
                                Ok(total) => {
                                    tracing::info!(kind, total, "ledger appended");
                                    Ok(format!(
                                        "Appended [{kind}] to {} ({total} entries).",
                                        ledger.label()
                                    ))
                                }
                                Err(e) => Err(format!("ledger write failed: {e}")),
                            }
                        }
                    }
                }
            }
            "ledger_read" => {
                let parsed: std::result::Result<LedgerReadParams, String> = match arguments {
                    Some(a) => serde_json::from_value(serde_json::Value::Object(a))
                        .map_err(|e| e.to_string()),
                    None => Ok(LedgerReadParams {
                        query: None,
                        limit: None,
                    }),
                };
                let params = match parsed {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(CallToolResult::error(vec![Content::text(format!(
                            "Error: {e}"
                        ))]))
                    }
                };
                match ledger.read_all() {
                    Ok(entries) if entries.is_empty() => {
                        if ledger.chat_only {
                            Ok(format!("{} is empty.", ledger.label()))
                        } else {
                            Ok("The project ledger is empty (.goose/ledger.md).".to_string())
                        }
                    }
                    Ok(entries) => {
                        let limit = params
                            .limit
                            .unwrap_or(READ_MAX_ENTRIES)
                            .min(READ_MAX_ENTRIES);
                        let picked = select_entries(&entries, params.query.as_deref(), limit);
                        tracing::info!(query = ?params.query, returned = picked.len(), total = entries.len(), "ledger read");
                        if picked.is_empty() {
                            Ok(format!(
                                "No ledger entry matches \"{}\" ({} entries in total).",
                                params.query.unwrap_or_default(),
                                entries.len()
                            ))
                        } else {
                            let mut out = format!(
                                "{} of {} ledger entries, newest first:\n",
                                picked.len(),
                                entries.len()
                            );
                            for entry in picked {
                                out.push_str("- ");
                                out.push_str(&entry);
                                out.push('\n');
                            }
                            Ok(out)
                        }
                    }
                    Err(e) => Err(format!("ledger read failed: {e}")),
                }
            }
            _ => Err(format!("Unknown tool: {name}")),
        };
        Ok(match result {
            Ok(text) => CallToolResult::success(vec![Content::text(text)]),
            Err(error) => CallToolResult::error(vec![Content::text(format!("Error: {error}"))]),
        })
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }

    async fn get_moim(&self, session_id: &str) -> Option<String> {
        Some(self.ledger_file(session_id).await.moim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_round_trip_and_headings_are_skipped() {
        let mut content = String::from("# Project ledger\n\n");
        content.push_str(&format_entry(
            "2026-09-06 10:00",
            "finding",
            "the port is\n8850",
        ));
        content.push_str(&format_entry(
            "2026-09-06 10:05",
            "tried",
            "killpg — took the engine",
        ));
        let entries = parse_entries(&content);
        assert_eq!(
            entries,
            vec![
                "2026-09-06 10:00 [finding] the port is 8850",
                "2026-09-06 10:05 [tried] killpg — took the engine"
            ]
        );
    }

    #[test]
    fn select_is_newest_first_and_query_needs_half_the_terms() {
        let entries: Vec<String> = (1..=7)
            .map(|i| format!("2026-09-0{i} 09:00 [fact] entry {i} about the vendor port"))
            .collect();
        let tail = select_entries(&entries, None, 3);
        assert_eq!(tail.len(), 3);
        assert!(tail[0].starts_with("2026-09-07"));
        let hits = select_entries(&entries, Some("vendor port 8850"), 10);
        assert_eq!(hits.len(), 7, "two of three terms cover it");
        assert!(select_entries(&entries, Some("kubernetes ingress"), 10).is_empty());
    }

    /// Q-89: E2E #1 and #2b, two chats about two clients, both ran in the home folder; #2b's turn
    /// context carried #1's "[decision] Harbourline…" from the one shared `~/.goose/ledger.md`.
    #[test]
    fn chats_in_the_home_folder_keep_their_own_ledgers_and_a_project_shares_one() {
        let home = tempfile::tempdir().unwrap();
        let first = LedgerFile::for_chat(home.path(), "20260925_33", Some(home.path()));
        let second = LedgerFile::for_chat(home.path(), "20260925_39", Some(home.path()));
        assert!(first.chat_only && second.chat_only);
        assert_ne!(first.path, second.path);
        assert_eq!(
            first.path,
            home.path().join(".goose/ledgers/20260925_33.md"),
            "never the shared {}",
            ledger_path(home.path()).display()
        );
        first
            .append("decision", "Harbourline: 24-month inactivity cutoff")
            .unwrap();
        assert!(second.read_all().unwrap().is_empty());
        assert!(!second.moim().contains("Harbourline"));
        assert!(first.moim().contains("Harbourline"));
        assert!(
            second
                .moim()
                .contains("offer to make it this chat's folder"),
            "{}",
            second.moim()
        );

        let project = home.path().join("work");
        let a = LedgerFile::for_chat(&project, "20260925_33", Some(home.path()));
        let b = LedgerFile::for_chat(&project, "20260925_39", Some(home.path()));
        assert_eq!(a, b);
        assert_eq!(a.path, ledger_path(&project));
        assert!(!a.chat_only);
        assert_eq!(
            a.moim(),
            "<ledger>\nThe project ledger (.goose/ledger.md) is empty — ledger_append the first finding, decision or dead end you meet.\n</ledger>\n",
            "a project's turn context is unchanged"
        );
        assert_eq!(
            LedgerFile::for_chat(home.path(), "a/../b", Some(home.path())).path,
            home.path().join(".goose/ledgers/a____b.md")
        );
    }
}
