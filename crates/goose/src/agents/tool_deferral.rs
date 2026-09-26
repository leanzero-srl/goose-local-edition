//! Deferred tool schemas: the tools of extensions the user added from OUTSIDE goose (MCP servers over
//! stdio / HTTP / SSE, inline Python) are declared from the first call as SKELETONS — the first
//! sentence of the description and the parameter schema without its documentation keywords
//! (`description`, `title`, `examples`). `extensionmanager__load_tools` returns the full description
//! and parameter notes into the conversation. The declared list is the same on every call of a
//! session, so a load leaves the rendered prefix — tools, system prompt, earlier messages —
//! byte-identical, and only the new messages are prefilled.
//!
//! Measured need (VA-190, session 20260925_30, a 27B on a 128k window): 79 tools, 105,747 chars of
//! schemas on every call — leanzerodocuments 49,963, playwright 19,305, leanzerowebsearch 13,552 —
//! for "create a Python package". Off unless `GOOSE_TOOL_DEFERRAL` is true.
//!
//! Why skeletons and not a list that grows on load (Q-107, E2E #3c on the tensor split, 3.0.51): the
//! Qwen3.8 templates (27B and Flash) render the tool list at the TOP of the system turn, before the
//! system prompt, so a load that appended two web-search tools rewrote the prompt from inside the
//! tool block — the engine read 34,335 then 53,870 prompt tokens cold where the call before read
//! 30,869 of 33,312 from cache. Rendered offline through the 27B's own template on the real
//! requests, the grown list kept 6,187 tokens (0.186) as an identical prefix; the skeleton list
//! keeps everything up to the new messages (0.996, 694 tokens to prefill). Declaring full schemas
//! only in the conversation is not an option: Rapid-MLX's qwen3_coder_xml parser drops a call to a
//! name the request did not declare, and VA-190 measured the model missing undeclared tools on 2 of
//! 5 tasks. A skeleton keeps the parameter names, types, enums and `required`, so the engines'
//! parsers type the arguments and Rapid-MLX's tool grammar still constrains them.

use crate::agents::extension_manager::get_tool_owner;
use crate::agents::platform_extensions::recall::query_terms;
use goose_memory_store::{term_occurrences, tokenize};
use rmcp::model::{JsonObject, Tool};
use serde_json::Value;
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

/// What a request declares and the system prompt it carries: every tool, a deferred one as its
/// skeleton, in the order given, and the prompt with the "Deferred tools" section. Neither reads the
/// conversation, so a `load_tools` call changes nothing before the new messages.
pub fn disclose(
    tools: &[Tool],
    system_prompt: &str,
    deferrable: &HashSet<String>,
) -> (Vec<Tool>, String) {
    let is_deferred =
        |tool: &Tool| get_tool_owner(tool).is_some_and(|owner| deferrable.contains(&owner));
    let declared = tools
        .iter()
        .map(|tool| {
            if is_deferred(tool) {
                skeleton(tool)
            } else {
                tool.clone()
            }
        })
        .collect();
    let deferred: Vec<Tool> = tools.iter().filter(|t| is_deferred(t)).cloned().collect();
    (declared, format!("{system_prompt}{}", catalogue(&deferred)))
}

/// A deferred tool as the model first sees it: the first sentence of its description and its
/// parameters without their notes, which `load_tools` returns. Measured on E2E #3c's three outside
/// servers (53 tools): 81,992 chars of full schemas, 33,185 as skeletons (0.405).
fn skeleton(tool: &Tool) -> Tool {
    let mut out = tool.clone();
    out.description = tool
        .description
        .as_deref()
        .map(|description| first_sentence(description).to_string().into());
    out.input_schema = std::sync::Arc::new(bare_schema(&tool.input_schema));
    out
}

const DOCUMENTATION_KEYWORDS: [&str; 3] = ["description", "title", "examples"];

/// A JSON Schema without its documentation keywords. Only schema POSITIONS are walked, so a
/// parameter literally named `description` (a key of `properties`) stays.
fn bare_schema(schema: &JsonObject) -> JsonObject {
    schema
        .iter()
        .filter(|(key, _)| !DOCUMENTATION_KEYWORDS.contains(&key.as_str()))
        .map(|(key, value)| {
            let value = match (key.as_str(), value) {
                (
                    "properties" | "patternProperties" | "$defs" | "definitions"
                    | "dependentSchemas",
                    Value::Object(named),
                ) => Value::Object(
                    named
                        .iter()
                        .map(|(name, sub)| (name.clone(), bare_value(sub)))
                        .collect(),
                ),
                (
                    "items"
                    | "additionalProperties"
                    | "not"
                    | "if"
                    | "then"
                    | "else"
                    | "contains"
                    | "propertyNames"
                    | "unevaluatedProperties"
                    | "unevaluatedItems"
                    | "additionalItems"
                    | "anyOf"
                    | "oneOf"
                    | "allOf"
                    | "prefixItems",
                    sub,
                ) => bare_value(sub),
                _ => value.clone(),
            };
            (key.clone(), value)
        })
        .collect()
}

fn bare_value(value: &Value) -> Value {
    match value {
        Value::Object(schema) => Value::Object(bare_schema(schema)),
        Value::Array(schemas) => Value::Array(schemas.iter().map(bare_value).collect()),
        other => other.clone(),
    }
}

/// The system-prompt section saying which declared tools are skeletons and how to read the rest.
/// Empty when nothing is deferred, so a request without outside extensions is byte-identical to one
/// without deferral.
fn catalogue(deferred: &[Tool]) -> String {
    let mut extensions: Vec<String> = deferred.iter().filter_map(get_tool_owner).collect();
    extensions.sort();
    extensions.dedup();
    if extensions.is_empty() {
        return String::new();
    }
    format!(
        "\n\n# Deferred tools\n\nThe tools of {} are in your tool list with a one-sentence summary \
         and bare parameters. Before the first call to one of them, call \
         {LOAD_TOOLS_TOOL_NAME_COMPLETE} with its name (or a query describing what you need) to read \
         its full description and parameter notes, then call it by that exact name. Prefer one of \
         them over a shell workaround when it does the job.\n",
        extensions.join(", ")
    )
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
/// the extension prefix), and for a query, the tools that carry at least HALF of its words in their
/// name and first sentence — the text a skeleton shows the model — ranked with
/// a word in the NAME counting twice (the memory search's rule), every tool tied at the top score.
/// The query is read as the recall reads a request (`query_terms`: function words, one-letter
/// tokens, URLs and path directories out).
///
/// Why (Q-96, E2E #2b turn 2): `load_tools("Atlassian support lifecycle data center end of sale end
/// of support")` — the research SUBJECT, not a capability — returned `create-doc`: its 4,474-char
/// description carries "Supported markdown", "tabular/numeric data", alignment "center" and "end
/// users" in its parameter notes, four of seven words, more than any tool that does a lookup. The
/// model read a Word-document schema, said "Wrong tool family" and lost 2 min 49 s. Counted over
/// the name and first sentence with the half floor, that query matches no tool (one word, "data",
/// in `browser_drop`), and the load says what a query is for instead of handing over a schema;
/// its next query, "fetch a web page and return its content", now loads
/// `get-single-web-page-content` rather than the two search tools.
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
        let terms = query_terms(query);
        let score = |tool: &Tool| {
            let name = tokenize(&tool.name.replace("__", " ").replace('_', " "));
            let summary = tokenize(first_sentence(
                tool.description.as_deref().unwrap_or_default(),
            ));
            let mut matched = 0;
            let mut score = 0;
            for term in &terms {
                let in_name = term_occurrences(term, &name) > 0;
                if in_name || term_occurrences(term, &summary) > 0 {
                    matched += 1;
                    score += if in_name { 2 } else { 1 };
                }
            }
            if matched > 0 && matched * 2 >= terms.len() {
                score
            } else {
                0
            }
        };
        let best = deferred.iter().map(score).max().unwrap_or(0);
        if best > 0 {
            for tool in deferred.iter().filter(|tool| score(tool) == best) {
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
    use crate::conversation::message::Message;
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

    /// Q-96: the real catalogue of E2E #2b's outside servers (leanzerodocuments, leanzerowebsearch,
    /// playwright; names and full descriptions as the servers list them on 2026-09-25) and the two
    /// queries of turn 2, then the capability queries the catalogue has to keep answering.
    #[test]
    fn load_tools_ranks_by_what_a_tool_does_on_the_real_catalogue() {
        let catalogue: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/tool-deferral/catalogue-2026-09-25.json"
        ))
        .unwrap();
        let deferred: Vec<Tool> = catalogue
            .iter()
            .map(|entry| {
                let full = entry["name"].as_str().unwrap();
                let (owner, name) = full.split_once("__").unwrap();
                tool(owner, name, entry["description"].as_str().unwrap())
            })
            .collect();
        let found = |query: &str| -> Vec<String> {
            find(&deferred, &[], Some(query))
                .iter()
                .map(|t| t.name.to_string())
                .collect()
        };

        assert_eq!(
            found("Atlassian support lifecycle data center end of sale end of support"),
            Vec::<String>::new(),
            "a research subject names no tool; it was create-doc, create-pdf and create-pptx on \
             'support', 'data', 'center' and 'end' in their parameter notes"
        );
        assert_eq!(
            found("fetch a web page and return its content"),
            vec!["leanzerowebsearch__get-single-web-page-content"]
        );
        assert_eq!(
            found("Read the sitemap of https://www.rust-lang.org"),
            vec!["leanzerowebsearch__get-website-sitemap"]
        );
        assert_eq!(
            found("Create a Word document report.docx"),
            vec!["leanzerodocuments__create-doc"]
        );
        assert_eq!(
            found("create an Excel file"),
            vec!["leanzerodocuments__create-excel"]
        );
        assert_eq!(
            found("search the web"),
            vec![
                "leanzerowebsearch__full-web-search",
                "leanzerowebsearch__get-web-search-summaries",
                "leanzerowebsearch__progressive-web-search"
            ]
        );
        assert_eq!(
            found("open a url in the browser"),
            vec!["playwright__browser_navigate"]
        );
        assert_eq!(
            found("take a screenshot of the page"),
            vec!["playwright__browser_take_screenshot"]
        );
        assert_eq!(
            found("fact check claims in a document"),
            vec!["leanzerodocuments__fact-check"]
        );
    }

    /// The outside servers of E2E #3c as their `tools/list` answered on 2026-09-26: 53 tools with
    /// their full schemas, plus goose's own shell.
    fn real_tools() -> (Vec<Tool>, HashSet<String>) {
        let servers: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            include_str!("../../tests/fixtures/tool-deferral/outside-tools-2026-09-26.json"),
        )
        .unwrap();
        let mut tools = vec![tool("developer", "shell", "Run a shell command")];
        for (owner, listed) in &servers {
            for entry in listed.as_array().unwrap() {
                let mut t = tool(
                    owner,
                    entry["name"].as_str().unwrap(),
                    entry["description"].as_str().unwrap_or_default(),
                );
                t.input_schema = Arc::new(entry["inputSchema"].as_object().unwrap().clone());
                tools.push(t);
            }
        }
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        (tools, servers.keys().cloned().collect())
    }

    /// The order the Qwen3.8 templates (27B and Flash) render a request in: the tool list inside the
    /// system turn, then the system prompt, then the messages — the payload an OpenAI-compatible
    /// local engine receives, laid out as the template lays it out.
    fn render_like_the_template(tools: &[Tool], system: &str, messages: &[Message]) -> String {
        let mut out = String::from("<|im_start|>system\n# Tools\n\n<tools>");
        for declared in goose_providers::formats::openai::format_tools(tools).unwrap() {
            out.push('\n');
            out.push_str(&declared.to_string());
        }
        out.push_str("\n</tools>\n\n");
        out.push_str(system);
        out.push_str("<|im_end|>\n");
        for message in goose_providers::formats::openai::format_messages(
            messages,
            &goose_providers::images::ImageFormat::OpenAi,
        ) {
            out.push_str(&message.to_string());
            out.push('\n');
        }
        out
    }

    fn common_prefix(a: &str, b: &str) -> usize {
        a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
    }

    /// Q-107: a mid-session `load_tools` leaves the rendered prefix byte-identical. The negative
    /// control is the list that grew on load (what shipped in 1f4f129d6): the same renderer shows
    /// its prefix breaking inside the tool block, as E2E #3c's engine read 34,335 tokens cold.
    #[test]
    fn a_tool_load_leaves_the_rendered_prefix_byte_identical() {
        let (tools, deferrable) = real_tools();
        let system = "You are goose.";
        let ask = Message::user().with_text("Look up the Data Center end of support dates.");
        let mut arguments = serde_json::Map::new();
        arguments.insert("query".to_string(), serde_json::json!("search the web"));
        let (_, deferred) = split(&tools, &deferrable);
        let found = find(&deferred, &[], Some("search the web"));
        assert_eq!(found.len(), 3, "the three web-search tools");
        let before = vec![ask.clone()];
        let after = vec![
            ask,
            Message::assistant().with_tool_request(
                "load-1",
                Ok(
                    rmcp::model::CallToolRequestParams::new(LOAD_TOOLS_TOOL_NAME_COMPLETE)
                        .with_arguments(arguments),
                ),
            ),
            Message::user().with_tool_response(
                "load-1",
                Ok(rmcp::model::CallToolResult::success(vec![
                    rmcp::model::Content::text(render(&found)),
                ])),
            ),
        ];

        let (declared_before, prompt_before) = disclose(&tools, system, &deferrable);
        let (declared_after, prompt_after) = disclose(&tools, system, &deferrable);
        let rendered_before = render_like_the_template(&declared_before, &prompt_before, &before);
        let rendered_after = render_like_the_template(&declared_after, &prompt_after, &after);
        assert!(
            rendered_after.starts_with(&rendered_before),
            "the prefix broke at byte {} of {}",
            common_prefix(&rendered_before, &rendered_after),
            rendered_before.len()
        );
        assert_eq!(declared_before.len(), tools.len(), "every tool is declared");

        let mut plain_turn = after.clone();
        plain_turn.push(Message::assistant().with_text("Searching now."));
        plain_turn.push(Message::user().with_text("Also check JCMA support for 9.12."));
        let (declared_later, prompt_later) = disclose(&tools, system, &deferrable);
        let rendered_later = render_like_the_template(&declared_later, &prompt_later, &plain_turn);
        assert!(
            rendered_later.starts_with(&rendered_after),
            "a plain turn after the load extends the prompt too"
        );

        let (core, _) = split(&tools, &deferrable);
        let mut grown = core.clone();
        grown.extend(found.iter().map(|t| (*t).clone()));
        let shipped_before = render_like_the_template(&core, system, &before);
        let shipped_after = render_like_the_template(&grown, system, &after);
        let kept = common_prefix(&shipped_before, &shipped_after);
        assert!(
            kept <= shipped_before.find("</tools>").unwrap(),
            "the negative control: a list that grows on load breaks the prefix inside the tool block"
        );
    }

    #[test]
    fn a_skeleton_keeps_what_a_call_needs_and_load_tools_returns_the_rest() {
        let (tools, deferrable) = real_tools();
        let (declared, prompt) = disclose(&tools, "", &deferrable);
        let by_name = |list: &[Tool], name: &str| -> Tool {
            list.iter().find(|t| t.name == name).unwrap().clone()
        };

        let shell = by_name(&declared, "developer__shell");
        assert_eq!(
            shell,
            by_name(&tools, "developer__shell"),
            "core tools untouched"
        );

        let full = by_name(&tools, "leanzerodocuments__create-doc");
        let bare = by_name(&declared, "leanzerodocuments__create-doc");
        assert_eq!(
            bare.description.as_deref(),
            Some("Create a styled, EDITABLE Word DOCX.")
        );
        assert_eq!(
            bare.input_schema.get("required"),
            full.input_schema.get("required")
        );
        let names = |t: &Tool| -> Vec<String> {
            t.input_schema["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect()
        };
        assert_eq!(names(&bare), names(&full), "every parameter stays declared");
        assert_eq!(
            bare.input_schema["properties"]["description"],
            serde_json::json!({"type": "string"}),
            "a parameter NAMED description is a parameter, not a note"
        );
        assert_eq!(
            bare.input_schema["properties"]["stylePreset"]["enum"],
            full.input_schema["properties"]["stylePreset"]["enum"]
        );
        assert!(
            !serde_json::to_string(&bare.input_schema["properties"]["paragraphs"])
                .unwrap()
                .contains("\"description\"")
        );

        let size = |list: &[Tool]| -> usize {
            goose_providers::formats::openai::format_tools(list)
                .unwrap()
                .iter()
                .map(|t| t.to_string().len())
                .sum()
        };
        let (_, deferred) = split(&tools, &deferrable);
        let skeletons: Vec<Tool> = declared
            .iter()
            .filter(|t| deferred.iter().any(|d| d.name == t.name))
            .cloned()
            .collect();
        assert!(
            size(&skeletons) * 2 < size(&deferred),
            "skeletons {} chars against {} full",
            size(&skeletons),
            size(&deferred)
        );

        let loaded = render(&[&full]);
        assert!(loaded.contains(full.description.as_deref().unwrap()));
        assert!(
            loaded.contains("\"description\":"),
            "the notes come back on load"
        );

        assert!(prompt.contains(
            "The tools of leanzerodocuments, leanzerowebsearch, playwright are in your tool list"
        ));
        assert_eq!(
            disclose(&[tool("developer", "shell", "Run")], "p", &deferrable).1,
            "p",
            "nothing deferred, nothing said"
        );
        assert_eq!(
            first_sentence("Read a sitemap. Filter it by keywords.\nMore."),
            "Read a sitemap."
        );
        assert!(find(&deferred, &[], Some("bake bread")).is_empty());
        assert_eq!(
            find(&deferred, &["browser_click".to_string()], None)
                .iter()
                .map(|t| t.name.to_string())
                .collect::<Vec<_>>(),
            vec!["playwright__browser_click"]
        );
    }
}
