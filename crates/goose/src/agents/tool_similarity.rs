//! Which offered tools a call to a name that does not exist most likely meant, so the error answers
//! "did you mean …" instead of handing a small model an unordered list of every tool.
//!
//! Measured need (Q-367, E2E #3r on 3.0.70, session 20260928_21 turn 2): the 27B called `websearch`
//! eight times and `fetch` four times across five answers; each error was "Tool 'websearch' not
//! found. Available tools: [80 names]" in extension order, `leanzerowebsearch__full-web-search` the
//! 51st name. It curled guessed URLs (404s) for seven minutes and only then called the real tool,
//! which answered at once. E2E #3p (3.0.69, session 20260928_19) called `web-search` — a skill's
//! name — three times the same way.
//!
//! Two measures of closeness, each in [0, 1], summed:
//! - NAME: the Sørensen–Dice coefficient of the letter trigrams of the called name and the tool's
//!   name (both without their extension prefix and separators), so `websearch`, `web-search` and
//!   `search_web` all sit near `full-web-search`.
//! - PURPOSE: the share of the call's words — its name's words and its argument names — found in the
//!   words of the tool's name, the first sentence of its description (the text a deferred tool's
//!   skeleton shows) and its parameter names, each word weighted by its rarity across the offered
//!   tools (`rarity_weight`, the memory search's weight), so `fetch`, carried by 2 of #3r's 81
//!   tools, decides more than `url`, carried by 18. A run-together name (`websearch`) is read as
//!   the tool words it is made of (`web`, `search`) when they cover it letter for letter.
//!
//! A tool is suggested when half of the call's purpose or half of its name matches it (the
//! `load_tools` query floor, Q-96) and it scores at least half of the best tool. Nothing is mapped by
//! hand: a name no tool's words or letters resemble gets no suggestion, and the error says so.

use crate::agents::tool_deferral::first_sentence;
use goose_memory_store::{rarity_weight, stem, STOPWORDS};
use rmcp::model::{JsonObject, Tool};
use std::collections::HashSet;

// An algorithm constant (gate 10, class c — a shingle width): the name measure compares letter
// trigrams, and a run-together name is split only into tool words at least one shingle long.
const SHINGLE: usize = 3;

/// The words of an identifier or a sentence, lower-cased: split at every non-alphanumeric and at a
/// lower-to-upper step (`maxContentLength` → max, content, length); one-letter words and function
/// words out.
fn words(text: &str) -> Vec<String> {
    let mut spaced = String::with_capacity(text.len());
    let mut previous: Option<char> = None;
    for c in text.chars() {
        if c.is_uppercase() && previous.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit()) {
            spaced.push(' ');
        }
        spaced.push(c);
        previous = Some(c);
    }
    spaced
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() > 1 && !STOPWORDS.contains(w))
        .map(String::from)
        .collect()
}

fn unprefixed(name: &str) -> &str {
    name.split_once("__").map_or(name, |(_, bare)| bare)
}

fn letters(name: &str) -> String {
    unprefixed(name)
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn trigrams(text: &str) -> HashSet<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.windows(SHINGLE).map(|w| w.iter().collect()).collect()
}

fn dice(a: &str, b: &str) -> f64 {
    let (a, b) = (trigrams(a), trigrams(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    2.0 * a.intersection(&b).count() as f64 / (a.len() + b.len()) as f64
}

/// The unstemmed words a tool is described by: its name, its summary sentence, its parameter names.
fn tool_words(tool: &Tool) -> Vec<String> {
    let mut out = words(unprefixed(&tool.name));
    out.extend(words(first_sentence(
        tool.description.as_deref().unwrap_or_default(),
    )));
    if let Some(serde_json::Value::Object(properties)) = tool.input_schema.get("properties") {
        for name in properties.keys() {
            out.extend(words(name));
        }
    }
    out
}

/// A called word as the tool words it is made of, when they cover every letter of it — `websearch`
/// is `web` + `search`; a word they do not cover whole stays itself.
fn split_run_together(word: &str, vocabulary: &HashSet<String>) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut covered = vec![false; chars.len()];
    let mut parts = Vec::new();
    for part in vocabulary {
        let part_chars: Vec<char> = part.chars().collect();
        if part_chars.len() < SHINGLE || part_chars.len() >= chars.len() {
            continue;
        }
        let mut found = false;
        for start in 0..=chars.len() - part_chars.len() {
            if chars[start..start + part_chars.len()] == part_chars[..] {
                covered[start..start + part_chars.len()].fill(true);
                found = true;
            }
        }
        if found {
            parts.push(part.clone());
        }
    }
    if !parts.is_empty() && covered.iter().all(|c| *c) {
        parts.sort();
        parts
    } else {
        vec![word.to_string()]
    }
}

/// How close one offered tool is to a call that named no tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Closeness {
    pub name: f64,
    pub purpose: f64,
}

impl Closeness {
    pub fn score(&self) -> f64 {
        self.name + self.purpose
    }
}

/// Every offered tool with its closeness to `called` (with `argument_names`, the keys the call
/// carried), in the order given.
pub fn closeness<'a>(
    tools: &'a [Tool],
    called: &str,
    argument_names: &[&str],
) -> Vec<(&'a Tool, Closeness)> {
    let described: Vec<Vec<String>> = tools.iter().map(tool_words).collect();
    let vocabulary: HashSet<String> = described.iter().flatten().cloned().collect();
    let tool_terms: Vec<HashSet<String>> = described
        .iter()
        .map(|ws| ws.iter().map(|w| stem(w)).collect())
        .collect();

    let mut call_terms: HashSet<String> = words(unprefixed(called))
        .iter()
        .flat_map(|w| split_run_together(w, &vocabulary))
        .map(|w| stem(&w))
        .collect();
    call_terms.extend(
        argument_names
            .iter()
            .flat_map(|name| words(name))
            .map(|w| stem(&w)),
    );

    let weights: Vec<(String, f64)> = call_terms
        .into_iter()
        .map(|term| {
            let df = tool_terms.iter().filter(|t| t.contains(&term)).count();
            let weight = rarity_weight(tools.len(), df);
            (term, weight)
        })
        .collect();
    let total: f64 = weights.iter().map(|(_, w)| w).sum();
    let called_letters = letters(called);

    tools
        .iter()
        .zip(&tool_terms)
        .map(|(tool, terms)| {
            let purpose = if total > 0.0 {
                weights
                    .iter()
                    .filter(|(term, _)| terms.contains(term))
                    .map(|(_, w)| w)
                    .sum::<f64>()
                    / total
            } else {
                0.0
            };
            let name = dice(&called_letters, &letters(&tool.name));
            (tool, Closeness { name, purpose })
        })
        .collect()
}

/// The tools a call to the unknown name `called` most likely meant, closest first.
pub fn closest_tools<'a>(
    tools: &'a [Tool],
    called: &str,
    argument_names: &[&str],
) -> Vec<&'a Tool> {
    let scored = closeness(tools, called, argument_names);
    let best = scored
        .iter()
        .map(|(_, c)| c.score())
        .fold(0.0_f64, f64::max);
    let mut close: Vec<(&Tool, Closeness)> = scored
        .into_iter()
        .filter(|(_, c)| (c.purpose * 2.0 >= 1.0 || c.name * 2.0 >= 1.0) && c.score() * 2.0 >= best)
        .collect();
    close.sort_by(|a, b| b.1.score().total_cmp(&a.1.score()));
    close.into_iter().map(|(tool, _)| tool).collect()
}

/// The lines naming `suggested`, each with the summary sentence the model can choose by.
pub fn describe(suggested: &[&Tool]) -> String {
    suggested
        .iter()
        .map(|tool| {
            let summary = first_sentence(tool.description.as_deref().unwrap_or_default());
            if summary.is_empty() {
                format!("- {}", tool.name)
            } else {
                format!("- {}: {}", tool.name, summary)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The error for a call to a tool that does not exist: the closest tools by name and purpose first,
/// then every offered tool, closest first.
pub fn not_found_message(tools: &[Tool], called: &str, arguments: Option<&JsonObject>) -> String {
    let argument_names: Vec<&str> = arguments
        .map(|args| args.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let suggested = closest_tools(tools, called, &argument_names);
    let mut ranked = closeness(tools, called, &argument_names);
    ranked.sort_by(|a, b| b.1.score().total_cmp(&a.1.score()));
    let all = ranked
        .iter()
        .map(|(tool, _)| tool.name.as_ref())
        .collect::<Vec<_>>()
        .join(", ");
    let head = if suggested.is_empty() {
        format!(
            "Tool '{called}' not found, and no available tool's name or summary is close to it."
        )
    } else {
        format!(
            "Tool '{called}' not found. The closest tools by name and purpose:\n{}\nCall one of them \
             by that exact name.",
            describe(&suggested)
        )
    };
    format!(
        "{head}\nAll {} available tools, closest first: [{all}]",
        tools.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn tool(name: &str, description: &str, properties: &[&str]) -> Tool {
        let props: serde_json::Map<String, serde_json::Value> = properties
            .iter()
            .map(|p| (p.to_string(), serde_json::json!({"type": "string"})))
            .collect();
        let schema = serde_json::json!({"type": "object", "properties": props});
        Tool::new(
            name.to_string(),
            description.to_string(),
            Arc::new(schema.as_object().unwrap().clone()),
        )
    }

    /// The tools E2E #3r's turn-2 requests declared (llm_request.9.jsonl, 2026-09-28 10:37 UTC):
    /// every web-search tool and the tools whose names or summaries share its words, as the
    /// deferred skeletons showed them (first sentence, parameter names).
    fn offered() -> Vec<Tool> {
        vec![
            tool("load", "Load knowledge into your current context or discover available sources.", &["source", "cancel", "peek"]),
            tool("delegate", "Delegate a task to a subagent that runs independently with its own context.", &["instructions", "source"]),
            tool("load_skill", "Load a skill's full content into your context so you can follow its instructions.", &["name", "args"]),
            tool("memory__search_memories", "Search your long-term memory by keywords: the entries ABOUT them come IN FULL, best match first (category, tag", &["query", "is_global", "limit"]),
            tool("memory__propose_knowledge", "File a KNOWLEDGE PIECE you learned by RESEARCHING — a fact you looked up (web search, library docs, a document, a file you read) that would be worth having next time.", &["category", "data", "sources", "tags", "is_global"]),
            tool("extensionmanager__search_available_extensions", "Searches for additional extensions available to help complete tasks.", &[]),
            tool("extensionmanager__load_tools", "Read the full description and parameter notes of the tools your instructions name under", &["names", "query"]),
            tool("write", "Create a new file or overwrite an existing file.", &["path", "content"]),
            tool("edit", "Edit a file by finding and replacing text.", &["path", "before", "after"]),
            tool("shell", "Execute a shell command in the current dir.", &["command", "timeout_secs"]),
            tool("tree", "List a directory tree with line counts.", &["path", "depth"]),
            tool("read_image", "Read an image from a local file path or http(s) URL and return it as image content for the model to inspect.", &["source", "crop"]),
            tool("playwright__browser_navigate", "Navigate to a URL", &["url"]),
            tool("playwright__browser_tabs", "List, create, close, or select a browser tab.", &["action", "index", "url"]),
            tool("playwright__browser_find", "Search the accessibility snapshot of the current page for text or a regular expression.", &["text", "regex"]),
            tool("leanzerowebsearch__full-web-search", "Search the web AND fetch full page content from the top results — the DEFAULT for general research when you will actually read the results.", &["query", "limit", "includeContent", "maxContentLength"]),
            tool("leanzerowebsearch__get-web-search-summaries", "Search the web and return ONLY the result snippets/titles/URLs — no page content is fetched.", &["query", "limit"]),
            tool("leanzerowebsearch__get-single-web-page-content", "Extract and return the full content from a single web page URL.", &["url", "maxContentLength"]),
            tool("leanzerowebsearch__research_and_save_to_markdown", "Research web pages and save their content, research digest (entities, claims, terms), and source information i", &["url", "maxContentLength", "maxFiles", "filenamePrefix", "template"]),
            tool("leanzerowebsearch__get-website-sitemap", "Read a website's sitemap.xml and (optionally) filter it by keywords.", &["url", "keywords", "offset", "limit"]),
            tool("leanzerowebsearch__progressive-web-search", "Web search that AUTO-EXPANDS the query: it tries the exact query first, then progressively widens with synonyms, related terms, and alternative phrasi", &["query", "maxDepth", "limit"]),
            tool("leanzerowebsearch__get-pdf-content", "Extract and return text content from a PDF document.", &["url", "maxContentLength"]),
            tool("leanzerodocuments__fact-check", "Fact-check a document (or explicit claims) against the LIVE WEB.", &["claims", "filePath", "content", "webSearchBearer"]),
            tool("leanzerodocuments__read-doc", "Read and analyze PDF, DOCX, Excel, or PowerPoint (.pptx) files.", &["filePath", "url", "mode"]),
            tool("ledger__ledger_read", "Read the project ledger, newest first: the latest entries, or the entries whose words match a query.", &["query", "limit"]),
        ]
    }

    fn names(tools: &[&Tool]) -> Vec<String> {
        tools.iter().map(|t| t.name.to_string()).collect()
    }

    #[test]
    fn websearch_as_3r_called_it_leads_with_the_web_search_tools() {
        let tools = offered();
        let got = names(&closest_tools(&tools, "websearch", &["queries"]));
        assert_eq!(
            got.first().map(String::as_str),
            Some("leanzerowebsearch__full-web-search"),
            "{got:?}"
        );
        let web_searches = got.iter().take_while(|n| n.contains("web-search")).count();
        assert_eq!(web_searches, 3, "the three web-search tools lead: {got:?}");
    }

    #[test]
    fn fetch_as_3r_called_it_leads_with_the_tools_that_say_they_fetch() {
        let tools = offered();
        let got = names(&closest_tools(&tools, "fetch", &["url"]));
        assert!(
            got.iter()
                .any(|n| n == "leanzerowebsearch__full-web-search"),
            "{got:?}"
        );
        assert!(
            got.iter().all(|n| n.starts_with("leanzerowebsearch__")),
            "{got:?}"
        );
    }

    #[test]
    fn a_skill_name_called_as_a_tool_as_3p_did_meets_the_web_search_tools() {
        let tools = offered();
        let got = names(&closest_tools(&tools, "web-search", &["query", "depth"]));
        assert!(!got.is_empty());
        assert!(got.iter().all(|n| n.contains("web-search")), "{got:?}");
    }

    #[test]
    fn a_name_nothing_resembles_gets_no_suggestion() {
        let tools = offered();
        assert!(closest_tools(&tools, "xyzzy", &[]).is_empty());
        assert!(closest_tools(&tools, "grep", &["pattern", "path"]).is_empty());
    }

    #[test]
    fn the_bare_name_of_a_real_tool_ranks_that_tool_first() {
        let tools = offered();
        let got = names(&closest_tools(
            &tools,
            "get-single-web-page-content",
            &["url"],
        ));
        assert_eq!(
            got.first().map(String::as_str),
            Some("leanzerowebsearch__get-single-web-page-content")
        );
    }

    #[test]
    fn a_run_together_word_splits_only_when_tool_words_cover_it() {
        let vocabulary: HashSet<String> = ["web", "search", "page"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            split_run_together("websearch", &vocabulary),
            vec!["search".to_string(), "web".to_string()]
        );
        assert_eq!(
            split_run_together("xyzsearch", &vocabulary),
            vec!["xyzsearch".to_string()]
        );
    }

    #[test]
    fn the_message_leads_with_the_closest_and_keeps_every_tool() {
        let tools = offered();
        let mut args = JsonObject::new();
        args.insert("queries".into(), serde_json::json!("[\"x\"]"));
        let msg = not_found_message(&tools, "websearch", Some(&args));
        let lead = msg.find("leanzerowebsearch__full-web-search").unwrap();
        let list = msg.find("available tools, closest first").unwrap();
        assert!(lead < list, "{msg}");
        assert!(msg.starts_with("Tool 'websearch' not found."), "{msg}");
        assert!(
            msg.contains("Search the web AND fetch full page content"),
            "{msg}"
        );
        for tool in &tools {
            assert!(
                msg[list..].contains(tool.name.as_ref()),
                "{} missing",
                tool.name
            );
        }
    }

    #[test]
    fn camel_case_and_separators_split_into_words() {
        assert_eq!(words("maxContentLength"), vec!["max", "content", "length"]);
        assert_eq!(
            words("get-web-search_summaries"),
            vec!["web", "search", "summaries"]
        );
    }
}
