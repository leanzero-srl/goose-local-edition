//! Deferred tool schemas: the tools of extensions the user added from OUTSIDE goose (MCP servers over
//! stdio / HTTP / SSE, inline Python) stay callable, but their parameter schemas leave the request's
//! tool list. The system prompt names every one of them, and `extensionmanager__load_tools` returns
//! the schemas the model asks for as a tool RESULT — appended after the stable prefix, so the prompt
//! cache keeps the system prompt and tool list byte-identical from turn to turn.
//!
//! Measured need (VA-190, session 20260925_30, a 27B on a 128k window): 79 tools, 105,747 chars of
//! schemas on every call — leanzerodocuments 49,963, playwright 19,305, leanzerowebsearch 13,552 —
//! for "create a Python package". Off unless `GOOSE_TOOL_DEFERRAL` is true.

use crate::agents::extension_manager::get_tool_owner;
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
    let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
    for tool in deferred {
        let owner = get_tool_owner(tool).unwrap_or_default();
        match groups.iter_mut().find(|(name, _)| *name == owner) {
            Some((_, names)) => names.push(&tool.name),
            None => groups.push((owner, vec![&tool.name])),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = format!(
        "\n\n# Deferred tools\n\nThese tools are available, but their parameters are not in your tool list. \
         To use one, call {LOAD_TOOLS_TOOL_NAME_COMPLETE} with its name (or a query describing what you \
         need) to read its parameters, then call it by that exact name.\n"
    );
    for (owner, names) in groups {
        out.push_str(&format!("- {owner}: {}\n", names.join(", ")));
    }
    out
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
        assert!(section
            .contains("- playwright: playwright__browser_navigate, playwright__browser_click\n"));
        assert!(section.contains("- websearch: websearch__search\n"));
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
        let schema = render(&find(&deferred, &["websearch__search".to_string()], None));
        assert!(schema.starts_with(
            "## websearch__search\nSearch the web for a query\nParameters (JSON Schema): {"
        ));
    }
}
