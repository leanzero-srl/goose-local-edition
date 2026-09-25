//! The deterministic half of goose's reply review: what the model just wrote, held against what
//! this turn's tool calls actually returned. It runs after every model response of a chat turn, not
//! only on the final answer — round 4's two receipts were both mid-turn narration:
//!
//! - #2b turn 3 (sessions.db 764274), after two failed shell calls: "Here's the version I'm happy
//!   to hand over … 401 rows in the file I generated". `data/users.csv` was not on disk; the only
//!   call that named it had failed.
//! - #2b turn 2 (764200), after two searches that returned nothing: "the end of sale was October
//!   2029 and the final support ended May 2029" — a year no result of the turn contained.
//!
//! Each finding is a checked fact about the transcript and the disk, phrased as goose's own note;
//! none is a judgement of the model's intent. A contradiction the transcript cannot settle (the
//! reply disagreeing with itself) is not this module's to call.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::conversation::message::{
    Message, MessageContent, SystemNotificationContent, SystemNotificationType,
};
use rmcp::model::Role;

/// What the user sees under the reply, and what the session keeps.
pub const CHECK_PREFIX: &str = "goose check:";

struct ToolFact {
    arguments: String,
    output: String,
    succeeded: bool,
}

static PATH_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s`'(\[<])((?:~|\.{1,2})?/?(?:[\w.-]+/)*[\w-][\w.-]*\.[A-Za-z][A-Za-z0-9]{0,5})(?:$|[\s`'),.:;\]>])")
        .expect("static regex")
});

static YEAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:19|20)\d{2}\b").expect("static regex"));

fn is_human_message(message: &Message) -> bool {
    message.role == Role::User
        && message.is_user_visible()
        && message.is_agent_visible()
        && message
            .content
            .iter()
            .any(|c| matches!(c, MessageContent::Text(_)))
        && !message
            .content
            .iter()
            .any(|c| matches!(c, MessageContent::ToolResponse(_)))
}

/// Index of the message that opened this turn: the user's last own words.
fn turn_start(messages: &[Message]) -> usize {
    messages.iter().rposition(is_human_message).unwrap_or(0)
}

fn text_of(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|c| match c {
            MessageContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_facts(messages: &[Message]) -> Vec<ToolFact> {
    let mut facts = Vec::new();
    for message in messages {
        for content in &message.content {
            let MessageContent::ToolRequest(request) = content else {
                continue;
            };
            let Ok(call) = &request.tool_call else {
                continue;
            };
            let response = messages.iter().find_map(|m| {
                m.content.iter().find_map(|c| match c {
                    MessageContent::ToolResponse(r) if r.id == request.id => Some(r),
                    _ => None,
                })
            });
            let (output, succeeded) = match response.map(|r| &r.tool_result) {
                Some(Ok(result)) => {
                    let exit_code = result
                        .structured_content
                        .as_ref()
                        .and_then(|s| s.get("exit_code"))
                        .and_then(serde_json::Value::as_i64);
                    let text = result
                        .content
                        .iter()
                        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let succeeded =
                        result.is_error != Some(true) && exit_code.is_none_or(|code| code == 0);
                    (text, succeeded)
                }
                Some(Err(e)) => (e.message.to_string(), false),
                None => continue,
            };
            let mut arguments = String::new();
            if let Some(object) = &call.arguments {
                for value in object.values() {
                    string_leaves(value, &mut arguments);
                }
            }
            facts.push(ToolFact {
                arguments,
                output,
                succeeded,
            });
        }
    }
    facts
}

/// The call's argument text as the tool received it — string values unescaped, one per line — so a
/// path after a newline in a shell command reads as a path, not as `\n/Users/…`.
fn string_leaves(value: &serde_json::Value, into: &mut String) {
    match value {
        serde_json::Value::String(s) => {
            into.push_str(s);
            into.push('\n');
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| string_leaves(v, into)),
        serde_json::Value::Object(map) => map.values().for_each(|v| string_leaves(v, into)),
        _ => {}
    }
}

fn path_tokens(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = PATH_TOKEN
        .captures_iter(text)
        .filter_map(|c| {
            c.get(1)
                .map(|m| m.as_str().trim_end_matches('.').to_string())
        })
        .filter(|t| {
            let first = t.split('/').next().unwrap_or(t);
            // `support.atlassian.com/…` is a site, not a file in the work folder.
            !(t.contains('/') && first.contains('.') && first != "." && first != "..")
        })
        .collect();
    tokens.sort();
    tokens.dedup();
    tokens
}

/// Every absolute path in `arguments` that ends with `token` — where the call itself put the file.
fn absolute_spellings(arguments: &str, token: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for (at, _) in arguments.match_indices(token) {
        let before = arguments.get(..at).unwrap_or_default();
        let start = before
            .rfind(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '=' | '('))
            .map_or(0, |i| i + 1);
        let path = arguments.get(start..at + token.len()).unwrap_or_default();
        if path.starts_with('/') {
            found.push(PathBuf::from(path));
        }
    }
    found
}

/// Files the new text names that are not on disk, where every call of this turn that named the file
/// failed. A file no call named is left alone: the text may be announcing what comes next.
fn unwritten_files(
    texts: &str,
    tools: &[ToolFact],
    working_dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<String> {
    let mut findings = Vec::new();
    for token in path_tokens(texts) {
        let naming: Vec<&ToolFact> = tools
            .iter()
            .filter(|t| t.arguments.contains(&token))
            .collect();
        if naming.is_empty() || naming.iter().any(|t| t.succeeded) {
            continue;
        }
        let expanded = if let Some(rest) = token.strip_prefix("~/") {
            dirs::home_dir().map(|home| home.join(rest))
        } else {
            None
        };
        let mut candidates = vec![expanded.unwrap_or_else(|| working_dir.join(&token))];
        for tool in &naming {
            candidates.extend(absolute_spellings(&tool.arguments, &token));
        }
        if candidates.iter().any(|p| exists(p)) {
            continue;
        }
        let calls = match naming.len() {
            1 => "the 1 tool call this turn that named it failed".to_string(),
            n => format!("all {n} tool calls this turn that named it failed"),
        };
        findings.push(format!("`{token}` is not on disk; {calls}."));
    }
    findings
}

/// Years the new text states that nothing in the session supports: no tool output, no message from
/// the user, no earlier reply. Only asked of a turn that made tool calls — a turn with none answered
/// from the model's own knowledge on purpose.
fn unsourced_years(
    texts: &str,
    tools: &[ToolFact],
    earlier: &[Message],
    current_year: i32,
) -> Vec<String> {
    if tools.is_empty() {
        return Vec::new();
    }
    let mut support: String = earlier.iter().map(text_of).collect::<Vec<_>>().join("\n");
    for tool in tools {
        support.push('\n');
        support.push_str(&tool.output);
    }
    let supported: std::collections::HashSet<&str> =
        YEAR.find_iter(&support).map(|m| m.as_str()).collect();
    let mut years: Vec<&str> = YEAR
        .find_iter(texts)
        .map(|m| m.as_str())
        .filter(|y| !supported.contains(y) && y.parse::<i32>().ok() != Some(current_year))
        .collect();
    years.sort();
    years.dedup();
    if years.is_empty() {
        return Vec::new();
    }
    let listed = years.join(", ");
    let (noun, verb) = if years.len() == 1 {
        ("year", "comes")
    } else {
        ("years", "come")
    };
    vec![format!(
        "the {noun} {listed} {verb} from none of this turn's {} tool result(s), the user's messages, or earlier replies.",
        tools.len()
    )]
}

/// Check the assistant text in `messages[from..]` (this model response) against this turn's tool
/// calls and the disk. `current_year` is the session's clock (the turn context states it).
pub fn check_new_text(
    messages: &[Message],
    from: usize,
    working_dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
    current_year: i32,
) -> Vec<String> {
    let from = from.min(messages.len());
    let texts = messages[from..]
        .iter()
        .filter(|m| m.role == Role::Assistant && m.is_agent_visible())
        .map(text_of)
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if texts.is_empty() {
        return Vec::new();
    }
    let start = turn_start(messages).min(from);
    let tools = tool_facts(&messages[start..]);
    let earlier: Vec<Message> = messages[..from]
        .iter()
        .filter(|m| m.is_agent_visible())
        .cloned()
        .collect();
    let mut findings = unwritten_files(&texts, &tools, working_dir, exists);
    findings.extend(unsourced_years(&texts, &tools, &earlier, current_year));
    findings
}

/// The one line the user reads under the reply.
pub fn correction_line(findings: &[String]) -> Option<String> {
    (!findings.is_empty()).then(|| format!("{CHECK_PREFIX} {}", findings.join(" Also, ")))
}

const NOTICE_KIND: &str = "reply_check";

/// The notice as the session stores it: user-only, and marked so a reload shows it again where
/// every other stored notice stays silent.
pub fn correction_notice(line: String) -> Message {
    Message::assistant().with_system_notification_with_data(
        SystemNotificationType::InlineMessage,
        line,
        serde_json::json!({ "kind": NOTICE_KIND }),
    )
}

pub fn is_reply_check(notification: &SystemNotificationContent) -> bool {
    notification.notification_type == SystemNotificationType::InlineMessage
        && notification
            .data
            .as_ref()
            .and_then(|d| d.get("kind"))
            .and_then(serde_json::Value::as_str)
            == Some(NOTICE_KIND)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Content};

    fn call(id: &str, command: &str) -> Message {
        Message::assistant().with_tool_request(
            id,
            Ok(CallToolRequestParams::new("shell").with_arguments(
                serde_json::json!({ "command": command })
                    .as_object()
                    .unwrap()
                    .clone(),
            )),
        )
    }

    fn result(id: &str, text: &str, exit_code: i64) -> Message {
        let mut result = if exit_code == 0 {
            CallToolResult::success(vec![Content::text(text)])
        } else {
            CallToolResult::error(vec![Content::text(text)])
        };
        result.structured_content = Some(serde_json::json!({ "exit_code": exit_code }));
        Message::user().with_tool_response(id, Ok(result))
    }

    const WORK: &str = "/Users/me/goose-builds/quality/E2E-2b-remote-studio/work";

    /// #2b turn 3 as recorded: the request, 764270 (mkdir + node --version, exit 2), 764272 (a
    /// heredoc that writes the generator and its `data/users.csv`, exit 2), then 764274's text.
    fn turn_three(claim: &str) -> Vec<Message> {
        vec![
            Message::user().with_text("Write a small script that generates a realistic fake data/users.csv, about 400 rows"),
            call("c1", &format!("mkdir -p {WORK}/scripts && node --version\n</parameter>\n!")),
            result("c1", "v24.15.0\nbash: -c: line 1: syntax error near unexpected token `newline'", 2),
            call("c2", &format!("cd {WORK} && cat > scripts/gen_users_csv.js <<'GENEOF'\n// writes {WORK}/data/users.csv\nGENEOF\n</parameter>\n!")),
            result("c2", "bash: scripts/gen_users_csv.js: No such file or directory", 2),
            Message::assistant().with_text(claim),
        ]
    }

    #[test]
    fn a_file_every_naming_call_failed_to_write_is_corrected() {
        let messages = turn_three("My `data/users.csv` was never going to be right … Here's the version I'm happy to hand over … 401 rows in the file I generated.");
        let nothing_on_disk = |_: &Path| false;
        let findings = check_new_text(&messages, 5, Path::new("/Users/me"), &nothing_on_disk, 2026);
        assert_eq!(
            findings,
            vec![
                "`data/users.csv` is not on disk; the 1 tool call this turn that named it failed."
            ]
        );
        assert_eq!(
            correction_line(&findings).unwrap(),
            "goose check: `data/users.csv` is not on disk; the 1 tool call this turn that named it failed."
        );
    }

    #[test]
    fn a_file_that_exists_or_that_a_call_wrote_is_not_corrected() {
        let messages = turn_three("Wrote `data/users.csv`.");
        let at_the_calls_path = |p: &Path| p == Path::new(&format!("{WORK}/data/users.csv"));
        assert!(check_new_text(
            &messages,
            5,
            Path::new("/Users/me"),
            &at_the_calls_path,
            2026
        )
        .is_empty());

        let mut written = turn_three("Wrote `data/users.csv`.");
        written.insert(
            5,
            call("c3", &format!("node gen.js --out {WORK}/data/users.csv")),
        );
        written.insert(6, result("c3", "Wrote data/users.csv (400 rows)", 0));
        assert!(
            check_new_text(&written, 7, Path::new("/Users/me"), &|_: &Path| false, 2026).is_empty()
        );
    }

    #[test]
    fn a_file_no_call_named_is_left_alone_and_sites_are_not_files() {
        let messages = turn_three("Next I'll write `scripts/check_dupes.js`; the dates are on support.atlassian.com/migration/docs/x.html.");
        assert!(check_new_text(&messages, 5, Path::new("/w"), &|_: &Path| false, 2026).is_empty());
    }

    /// #2b turn 2 as recorded: two searches that found nothing (764197/764199), then 764200.
    #[test]
    fn a_year_nothing_in_the_session_supports_is_named() {
        let messages = vec![
            Message::user().with_text("First thing Aoife will ask: how long can they stay on Data Center? Look up what Atlassian has officially announced"),
            call("s1", "search Atlassian Data Center end of life"),
            result("s1", "Search completed with 0 results", 0),
            call("s2", "search JCMA supported versions"),
            result("s2", "Search completed with 0 results", 0),
            Message::assistant().with_text("I remember the shape of the announcement: the end of sale was October 2029 and the final support ended May 2029. It's 2026 now."),
        ];
        let findings = check_new_text(&messages, 5, Path::new("/w"), &|_: &Path| false, 2026);
        assert_eq!(
            findings,
            vec!["the year 2029 comes from none of this turn's 2 tool result(s), the user's messages, or earlier replies."]
        );
    }

    /// #1 turn 2: the same years, read off the fetched page, are supported.
    #[test]
    fn a_year_the_fetched_page_states_is_supported() {
        let messages = vec![
            Message::user().with_text("Look up what Atlassian has officially announced"),
            call(
                "f1",
                "curl https://www.atlassian.com/licensing/data-center-end-of-life",
            ),
            result("f1", "EOL: March 28, 2029 … end of sale March 30, 2026", 0),
            Message::assistant().with_text("End of sale 30 Mar 2026; end of life 28 Mar 2029."),
        ];
        assert!(check_new_text(&messages, 3, Path::new("/w"), &|_: &Path| false, 2026).is_empty());
    }

    #[test]
    fn the_stored_notice_is_user_only_and_marked_for_replay() {
        let notice = correction_notice("goose check: x".to_string());
        assert!(notice.is_user_visible() && !notice.is_agent_visible());
        let MessageContent::SystemNotification(n) = &notice.content[0] else {
            panic!("{notice:?}");
        };
        assert!(is_reply_check(n));
        let MessageContent::SystemNotification(other) = &Message::assistant()
            .with_system_notification(
                SystemNotificationType::InlineMessage,
                "Context near the cap",
            )
            .content[0]
        else {
            panic!();
        };
        assert!(!is_reply_check(other));
    }

    #[test]
    fn a_turn_without_tool_calls_is_not_asked_for_sources() {
        let messages = vec![
            Message::user().with_text("When did Python 3 come out?"),
            Message::assistant().with_text("Python 3.0 shipped in 2008."),
        ];
        assert!(check_new_text(&messages, 1, Path::new("/w"), &|_: &Path| false, 2026).is_empty());
    }

    /// Replays recorded sessions through the check as the agent loop runs it: once per model
    /// response, with that response's tool results in. The disk cannot be replayed, so nothing is
    /// on it — every flag is an upper bound, to be read against its transcript.
    /// `CLAIM_CHECK_REPLAY=<sqlite3 -json export of messages> cargo test -p goose --lib
    /// claim_check::tests::replay -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn replay() {
        let path = std::env::var("CLAIM_CHECK_REPLAY").expect("CLAIM_CHECK_REPLAY");
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut sessions: std::collections::BTreeMap<String, Vec<(i64, Message)>> =
            Default::default();
        for row in rows {
            let parse = |field: &str| {
                serde_json::from_str::<serde_json::Value>(row[field].as_str().unwrap_or("{}"))
                    .unwrap()
            };
            let message: Message = serde_json::from_value(serde_json::json!({
                "id": row["message_id"],
                "role": row["role"],
                "created": row["created_timestamp"],
                "content": parse("content_json"),
                "metadata": parse("metadata_json"),
            }))
            .unwrap();
            let summary = message.role == Role::User
                && !message.is_user_visible()
                && !message
                    .content
                    .iter()
                    .any(|c| matches!(c, MessageContent::ToolResponse(_)));
            if summary {
                continue;
            }
            sessions
                .entry(row["session_id"].as_str().unwrap().to_string())
                .or_default()
                .push((
                    row["id"].as_i64().unwrap(),
                    message.with_visibility(true, true),
                ));
        }
        let (mut responses, mut flagged) = (0, 0);
        for (session, rows) in &sessions {
            let messages: Vec<Message> = rows.iter().map(|(_, m)| m.clone()).collect();
            let mut i = 0;
            while i < messages.len() {
                if messages[i].role != Role::Assistant {
                    i += 1;
                    continue;
                }
                let mut j = i;
                while j < messages.len() && messages[j].role == Role::Assistant {
                    j += 1;
                }
                while j < messages.len()
                    && messages[j]
                        .content
                        .iter()
                        .any(|c| matches!(c, MessageContent::ToolResponse(_)))
                {
                    j += 1;
                }
                responses += 1;
                let nothing_on_disk = |_: &Path| false;
                let findings = check_new_text(
                    &messages[..j],
                    i,
                    Path::new("/Users/mihaiperdum"),
                    &nothing_on_disk,
                    2026,
                );
                if let Some(line) = correction_line(&findings) {
                    flagged += 1;
                    println!("{session} msg {}: {line}", rows[i].0);
                }
                i = j;
            }
        }
        println!("{flagged} of {responses} model responses flagged");
    }
}
