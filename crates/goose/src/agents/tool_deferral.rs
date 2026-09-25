//! Deferred tool schemas: the tools of extensions the user added from OUTSIDE goose (MCP servers over
//! stdio / HTTP / SSE, inline Python) stay callable, but their parameter schemas leave the request's
//! tool list. The system prompt names every one of them; `extensionmanager__load_tools` returns the
//! schemas the model asks for, and from then on those tools are declared AFTER the core tools — the
//! system prompt and the core tool list stay byte-identical, a load only extends the list at its end.
//!
//! Measured need (VA-190, session 20260925_30, a 27B on a 128k window): 79 tools, 105,747 chars of
//! schemas on every call — leanzerodocuments 49,963, playwright 19,305, leanzerowebsearch 13,552 —
//! for "create a Python package". Off unless `GOOSE_TOOL_DEFERRAL` is true.

use crate::agents::extension_manager::get_tool_owner;
use crate::conversation::message::{Message, MessageContent};
use goose_memory_store::{term_occurrences, tokenize};
use rmcp::model::Tool;
use std::collections::HashSet;

pub const LOAD_TOOLS_TOOL_NAME: &str = "load_tools";
pub const LOAD_TOOLS_TOOL_NAME_COMPLETE: &str = "extensionmanager__load_tools";

pub fn enabled() -> bool {
    crate::config::Config::global()
        .get_param::<bool>("GOOSE_TOOL_DEFERRAL")
        .unwrap_or(false)
}

/// The tools a request carries and the tools it defers: a tool is deferred when its owner is one of
/// the `deferrable` extensions. Order is kept, so a sorted list stays sorted.
pub fn split(tools: &[Tool], deferrable: &HashSet<String>) -> (Vec<Tool>, Vec<Tool>) {
    tools
        .iter()
        .cloned()
        .partition(|tool| !get_tool_owner(tool).is_some_and(|owner| deferrable.contains(&owner)))
}

/// The system-prompt section naming every deferred tool, grouped by extension. Empty when nothing
/// is deferred, so a request without outside extensions is byte-identical to one without deferral.
pub fn catalogue(deferred: &[Tool]) -> String {
    if deferred.is_empty() {
        return String::new();
    }
    let mut lines: Vec<(String, String)> = deferred
        .iter()
        .map(|tool| {
            let owner = get_tool_owner(tool).unwrap_or_default();
            (
                owner,
                format!(
                    "- {} — {}\n",
                    tool.name,
                    first_sentence(tool.description.as_deref().unwrap_or_default())
                ),
            )
        })
        .collect();
    lines.sort();
    let mut out = format!(
        "\n\n# Deferred tools\n\nThese tools are available, but their parameters are not in your tool list. \
         To use one, call {LOAD_TOOLS_TOOL_NAME_COMPLETE} with its name (or a query describing what you \
         need) to read its parameters, then call it by that exact name. Prefer one of them over a shell \
         workaround when it does the job.\n"
    );
    for (_, line) in lines {
        out.push_str(&line);
    }
    out
}

/// A tool description's first sentence — what the tool is for, without its parameter notes.
/// Measured (VA-190, qwen/qwen3.8-27b, names only in the list): "Read the sitemap of
/// https://www.rust-lang.org" went to curl in the shell and never loaded
/// `leanzerowebsearch__get-website-sitemap`; "Create a Word document report.docx" loaded the docx
/// SKILL and wrote the file with python-docx instead of `leanzerodocuments__create-doc`.
fn first_sentence(description: &str) -> &str {
    let line = description.trim().lines().next().unwrap_or_default();
    line.split_inclusive(". ")
        .next()
        .map_or(line, |sentence| sentence.trim_end())
}

/// The deferred tools a `load_tools` call asks for: every tool named (full name, or the name after
/// the extension prefix), and for a query, the tools whose name and description carry the most of
/// its words (every tool tied at that count).
pub fn find<'a>(deferred: &'a [Tool], names: &[String], query: Option<&str>) -> Vec<&'a Tool> {
    let mut found: Vec<&Tool> = deferred
        .iter()
        .filter(|tool| {
            names.iter().any(|name| {
                let name = name.trim();
                *tool.name == *name
                    || tool
                        .name
                        .split_once("__")
                        .is_some_and(|(_, bare)| bare == name)
            })
        })
        .collect();
    if let Some(query) = query {
        let terms: Vec<String> = tokenize(query)
            .into_iter()
            .filter(|t| !goose_memory_store::STOPWORDS.contains(&t.as_str()))
            .collect();
        let matched = |tool: &Tool| {
            let text = tokenize(&format!(
                "{} {}",
                tool.name.replace("__", " ").replace('_', " "),
                tool.description.as_deref().unwrap_or_default()
            ));
            terms
                .iter()
                .filter(|term| term_occurrences(term, &text) > 0)
                .count()
        };
        let best = deferred.iter().map(matched).max().unwrap_or(0);
        if best > 0 {
            for tool in deferred.iter().filter(|tool| matched(tool) == best) {
                if !found.iter().any(|f| f.name == tool.name) {
                    found.push(tool);
                }
            }
        }
    }
    found
}

/// The deferred tools this session has LOADED, in the order it first asked for them: every
/// `load_tools` request in the conversation, resolved again by `find`. They join the request's tool
/// list after the core tools, so a load extends the prefix at its end instead of rewriting it.
/// Measured (VA-190, qwen/qwen3.8-27b over OpenRouter, 10 tasks): with the schema only in the tool
/// RESULT, the model called an undeclared tool on 2 of 5 outside-tool tasks — on "create an Excel
/// file" it called load_tools(create-excel) ten times and then wrote the file with openpyxl.
pub fn loaded(deferred: &[Tool], messages: &[Message]) -> Vec<Tool> {
    let mut out: Vec<Tool> = Vec::new();
    for call in messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolRequest(req) => req.tool_call.as_ref().ok(),
            _ => None,
        })
        .filter(|call| call.name == LOAD_TOOLS_TOOL_NAME_COMPLETE)
    {
        let arguments = call.arguments.clone().unwrap_or_default();
        let names: Vec<String> = arguments
            .get("names")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let query = arguments.get("query").and_then(|v| v.as_str());
        for tool in find(deferred, &names, query) {
            if !out.iter().any(|t| t.name == tool.name) {
                out.push(tool.clone());
            }
        }
    }
    out
}

/// The schemas as the model reads them: name, description, parameters.
pub fn render(tools: &[&Tool]) -> String {
    tools
        .iter()
        .map(|tool| {
            format!(
                "## {}\n{}\nParameters (JSON Schema): {}",
                tool.name,
                tool.description.as_deref().unwrap_or_default(),
                serde_json::Value::Object(tool.input_schema.as_ref().clone())
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn tool(owner: &str, name: &str, description: &str) -> Tool {
        let mut meta = serde_json::Map::new();
        meta.insert("goose_extension".to_string(), owner.into());
        let mut tool = Tool::new(
            format!("{owner}__{name}"),
            description.to_string(),
            Arc::new(
                serde_json::json!({"type": "object", "properties": {"url": {"type": "string"}}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        );
        tool.meta = Some(rmcp::model::Meta(meta));
        tool
    }

    #[test]
    fn outside_tools_are_deferred_and_named_in_the_catalogue() {
        let tools = vec![
            tool("developer", "shell", "Run a shell command"),
            tool("playwright", "browser_navigate", "Navigate to a URL"),
            tool(
                "playwright",
                "browser_click",
                "Click an element on the page",
            ),
            tool("websearch", "search", "Search the web for a query"),
        ];
        let deferrable: HashSet<String> = ["playwright", "websearch"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (sent, deferred) = split(&tools, &deferrable);
        assert_eq!(sent.len(), 1);
        assert_eq!(deferred.len(), 3);
        let section = catalogue(&deferred);
        assert!(section.contains(
            "- playwright__browser_click — Click an element on the page\n- playwright__browser_navigate — Navigate to a URL\n"
        ));
        assert!(section.contains("- websearch__search — Search the web for a query\n"));
        assert_eq!(
            first_sentence("Read a sitemap. Filter it by keywords.\nMore."),
            "Read a sitemap."
        );
        assert_eq!(catalogue(&[]), "", "nothing deferred, nothing said");
        assert_eq!(catalogue(&deferred), section, "the prefix is stable");

        let names = |found: Vec<&Tool>| -> Vec<String> {
            found.iter().map(|t| t.name.to_string()).collect()
        };
        assert_eq!(
            names(find(&deferred, &["browser_click".to_string()], None)),
            vec!["playwright__browser_click"]
        );
        assert_eq!(
            names(find(&deferred, &[], Some("open a url in the browser"))),
            vec!["playwright__browser_navigate"]
        );
        assert!(find(&deferred, &[], Some("bake bread")).is_empty());
        let mut args = serde_json::Map::new();
        args.insert("names".to_string(), serde_json::json!(["browser_click"]));
        let mut query = serde_json::Map::new();
        query.insert("query".to_string(), serde_json::json!("search the web"));
        let messages = vec![
            Message::assistant().with_tool_request(
                "1",
                Ok(
                    rmcp::model::CallToolRequestParams::new(LOAD_TOOLS_TOOL_NAME_COMPLETE)
                        .with_arguments(args.clone()),
                ),
            ),
            Message::assistant().with_tool_request(
                "2",
                Ok(
                    rmcp::model::CallToolRequestParams::new(LOAD_TOOLS_TOOL_NAME_COMPLETE)
                        .with_arguments(query),
                ),
            ),
            Message::assistant().with_tool_request(
                "3",
                Ok(
                    rmcp::model::CallToolRequestParams::new(LOAD_TOOLS_TOOL_NAME_COMPLETE)
                        .with_arguments(args),
                ),
            ),
        ];
        assert_eq!(
            loaded(&deferred, &messages)
                .iter()
                .map(|t| t.name.to_string())
                .collect::<Vec<_>>(),
            vec!["playwright__browser_click", "websearch__search"],
            "load order, each once"
        );
        let schema = render(&find(&deferred, &["websearch__search".to_string()], None));
        assert!(schema.starts_with(
            "## websearch__search\nSearch the web for a query\nParameters (JSON Schema): {"
        ));
    }
}
