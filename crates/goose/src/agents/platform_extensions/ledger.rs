//! The project ledger: a dated, append-only log of what happened in THIS project — findings,
//! decisions, what was tried and why it is not coming back, facts learned — kept in
//! `<working_dir>/.goose/ledger.md` and shown newest-first in every turn's context. It is the
//! chronological complement of project memory (facts, deduplicated) and of the scratchpad (this
//! session's state): after a compaction the model reads the ledger's tail and knows what the project
//! has already been through. Modelled on the operator's own agent ledgers (TICK-NOTES,
//! EXPERIMENTS-LEDGER): one line per entry, a date, a kind, the words.

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

    fn working_dir(&self) -> PathBuf {
        self.context
            .session
            .as_ref()
            .map(|s| s.working_dir.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    fn read_all(&self) -> std::io::Result<Vec<String>> {
        let path = ledger_path(&self.working_dir());
        if !path.exists() {
            return Ok(Vec::new());
        }
        Ok(parse_entries(&std::fs::read_to_string(path)?))
    }

    fn append(&self, kind: &str, text: &str) -> std::io::Result<usize> {
        use std::io::Write;
        let path = ledger_path(&self.working_dir());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let fresh = !path.exists();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)?;
        if fresh {
            writeln!(file, "# Project ledger — one dated line per finding, decision, attempt or fact; newest last\n")?;
        }
        let when = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        file.write_all(format_entry(&when, kind, text).as_bytes())?;
        Ok(self.read_all()?.len())
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
        _ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        _cancellation_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
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
                            match self.append(&kind, &params.text) {
                                Ok(total) => {
                                    tracing::info!(kind, total, "ledger appended");
                                    Ok(format!(
                                        "Appended [{kind}] to .goose/ledger.md ({total} entries)."
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
                match self.read_all() {
                    Ok(entries) if entries.is_empty() => {
                        Ok("The project ledger is empty (.goose/ledger.md).".to_string())
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

    async fn get_moim(&self, _session_id: &str) -> Option<String> {
        match self.read_all() {
            Ok(entries) if entries.is_empty() => Some(
                "<ledger>\nThe project ledger (.goose/ledger.md) is empty — ledger_append the first finding, decision or dead end you meet.\n</ledger>\n"
                    .to_string(),
            ),
            Ok(entries) => {
                let tail = select_entries(&entries, None, TAIL_ENTRIES);
                let mut out = format!(
                    "<ledger>\nProject ledger (.goose/ledger.md), {} entries, newest {} — what this project has already been through; ledger_read for more:\n",
                    entries.len(),
                    tail.len()
                );
                for entry in tail {
                    out.push_str("- ");
                    out.push_str(&entry);
                    out.push('\n');
                }
                out.push_str("</ledger>\n");
                Some(out)
            }
            Err(err) => {
                tracing::warn!(%err, "ledger unreadable");
                Some(format!(
                    "<ledger>\nThe project ledger could not be read ({err}).\n</ledger>\n"
                ))
            }
        }
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
}
