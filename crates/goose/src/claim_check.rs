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
//! And of the reply that closes a turn (Q-109): census n1 said "All 20 steps succeeded." after 19
//! calls, none of them step 20.
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

pub(crate) struct ToolFact {
    name: String,
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
pub(crate) fn turn_start(messages: &[Message]) -> usize {
    messages.iter().rposition(is_human_message).unwrap_or(0)
}

pub(crate) fn text_of(message: &Message) -> String {
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

pub(crate) fn tool_facts(messages: &[Message]) -> Vec<ToolFact> {
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
                name: call.name.to_string(),
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
        let arguments: Vec<&str> = naming.iter().map(|t| t.arguments.as_str()).collect();
        if on_disk(&token, &arguments, working_dir, exists) {
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

// ---- The user's numbered steps against the calls that did them (Q-109) ------------------------
//
// Census n1 (2026-09-26, goose from main, lz.7 engine): the user numbered 20 steps, one call each;
// the turn made 19 calls — none ran step 20's `wc -l …` — and the closing reply said "All 20 steps
// succeeded." Nothing held that sentence against the calls. Steps and calls are paired by the
// distinctive words they share (a word's weight falls with the number of steps that use it), each
// step with the call that fits it best while every other step keeps its own; a step left without a
// call, whose own words — the ones no other step uses — appear in no call, was never run. Anything
// the calls cannot settle (a step with no words of its own, a call that did two steps at once, a
// file only that step names that is on disk anyway) is left alone. Replayed on the nine recorded
// censuses (q85, q108): n1's step 20 is the one step found never run among the seven that ran to
// the end; g2, which stopped at step 12 without claiming anything, shows three of its eight.

static NUMBERED_ITEM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(\d+)[.)]\s+(\S.*)$").expect("static regex"));

static STEP_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_*$%+~-][A-Za-z0-9_.*$%+~-]*").expect("static regex"));

/// The longest run of items numbered 1, 2, 3 … in `text`, each without its number and with the lines
/// that continue it (an indented line, or a line straight under the item). Fewer than two items is
/// not a list.
pub(crate) fn numbered_steps(text: &str) -> Vec<String> {
    let mut runs: Vec<Vec<String>> = Vec::new();
    let mut run: Vec<String> = Vec::new();
    let mut after_blank = false;
    for line in text.lines() {
        let item = NUMBERED_ITEM.captures(line).and_then(|c| {
            let number = c.get(1)?.as_str().parse::<usize>().ok()?;
            Some((number, c.get(2)?.as_str().trim().to_string()))
        });
        let number = item.as_ref().map(|(n, _)| *n);
        let body = item.map(|(_, body)| body).unwrap_or_default();
        match number {
            Some(n) if n == run.len() + 1 => run.push(body),
            Some(1) => runs.push(std::mem::replace(&mut run, vec![body])),
            Some(_) => runs.push(std::mem::take(&mut run)),
            None if run.is_empty() => {}
            None if line.trim().is_empty() => {
                after_blank = true;
                continue;
            }
            None if !after_blank || line.starts_with(char::is_whitespace) => {
                if let Some(last) = run.last_mut() {
                    last.push('\n');
                    last.push_str(line.trim());
                }
            }
            None => runs.push(std::mem::take(&mut run)),
        }
        after_blank = false;
    }
    runs.push(run);
    let longest = runs.into_iter().max_by_key(Vec::len).unwrap_or_default();
    if longest.len() < 2 {
        return Vec::new();
    }
    longest
}

fn step_words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for m in STEP_WORD.find_iter(text) {
        let word = m.as_str().trim_end_matches(['.', '-']).to_lowercase();
        if word.chars().count() >= 2 && !words.contains(&word) {
            words.push(word);
        }
    }
    words
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StepState {
    Done,
    /// No call matched it, and no call carries a word only this step uses.
    NeverRun {
        own_words: Vec<String>,
    },
    /// Every call matched to it failed, and no call that carries its own words succeeded.
    Failed {
        calls: usize,
    },
    Unclear,
}

fn on_disk(
    token: &str,
    arguments: &[&str],
    working_dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> bool {
    let expanded = token
        .strip_prefix("~/")
        .and_then(|rest| dirs::home_dir().map(|home| home.join(rest)));
    let mut candidates = vec![expanded.unwrap_or_else(|| working_dir.join(token))];
    for args in arguments {
        candidates.extend(absolute_spellings(args, token));
    }
    candidates.iter().any(|p| exists(p))
}

/// The maximum-weight matching of rows to columns (Kuhn–Munkres with potentials): for each row, the
/// column it is matched to, if the pair scores above zero.
fn best_assignment(scores: &[Vec<f64>]) -> Vec<Option<usize>> {
    let rows = scores.len();
    let cols = scores.first().map_or(0, Vec::len);
    let n = rows.max(cols);
    let score = |i: usize, j: usize| {
        scores
            .get(i)
            .and_then(|row| row.get(j))
            .copied()
            .unwrap_or(0.0)
    };
    // 1-based arrays, column 0 the virtual start: the textbook form of the algorithm.
    let (mut u, mut v) = (vec![0.0f64; n + 1], vec![0.0f64; n + 1]);
    let (mut owner, mut way) = (vec![0usize; n + 1], vec![0usize; n + 1]);
    for i in 1..=n {
        owner[0] = i;
        let mut j0 = 0;
        let mut min_v = vec![f64::INFINITY; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[j0] = true;
            let i0 = owner[j0];
            let (mut delta, mut j1) = (f64::INFINITY, 0);
            for j in 1..=n {
                if used[j] {
                    continue;
                }
                let reduced = -score(i0 - 1, j - 1) - u[i0] - v[j];
                if reduced < min_v[j] {
                    min_v[j] = reduced;
                    way[j] = j0;
                }
                if min_v[j] < delta {
                    delta = min_v[j];
                    j1 = j;
                }
            }
            for j in 0..=n {
                if used[j] {
                    u[owner[j]] += delta;
                    v[j] -= delta;
                } else {
                    min_v[j] -= delta;
                }
            }
            j0 = j1;
            if owner[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            owner[j0] = owner[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut assigned = vec![None; rows];
    for (j, &i) in owner.iter().enumerate().skip(1) {
        if i >= 1 && i <= rows && j <= cols && score(i - 1, j - 1) > 0.0 {
            assigned[i - 1] = Some(j - 1);
        }
    }
    assigned
}

/// Which of `steps` this turn's `tools` did.
pub(crate) fn audit_steps(
    steps: &[String],
    tools: &[ToolFact],
    working_dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<StepState> {
    let words: Vec<Vec<String>> = steps.iter().map(|s| step_words(s)).collect();
    let mut used_by: std::collections::HashMap<&str, usize> = Default::default();
    for step in &words {
        for word in step {
            *used_by.entry(word.as_str()).or_default() += 1;
        }
    }
    let weight = |word: &str| (steps.len() as f64 / used_by[word] as f64).ln();
    let calls: Vec<std::collections::HashSet<String>> = tools
        .iter()
        .map(|t| {
            step_words(&format!("{}\n{}", t.name, t.arguments))
                .into_iter()
                .collect()
        })
        .collect();

    let scores: Vec<Vec<f64>> = words
        .iter()
        .map(|step| {
            calls
                .iter()
                .map(|call| {
                    step.iter()
                        .filter(|w| call.contains(*w))
                        .map(|w| weight(w))
                        .sum()
                })
                .collect()
        })
        .collect();
    // Each step takes the call that fits it best while every other step does too — a README edit
    // whose `before` quotes step 4's command belongs to "add a line under Usage", because step 4
    // has its own call. A call left over (a retry) goes with the step it fits best.
    let assigned = best_assignment(&scores);
    let mut matched: Vec<Vec<usize>> = vec![Vec::new(); steps.len()];
    for (s, call) in assigned.iter().enumerate() {
        if let Some(c) = call {
            matched[s].push(*c);
        }
    }
    for c in (0..calls.len()).filter(|c| !assigned.contains(&Some(*c))) {
        let best = scores.iter().map(|row| row[c]).fold(0.0, f64::max);
        if best > 0.0 {
            if let Some(s) = scores.iter().position(|row| row[c] == best) {
                matched[s].push(c);
            }
        }
    }

    let paths: Vec<Vec<String>> = steps.iter().map(|s| path_tokens(s)).collect();
    let arguments: Vec<&str> = tools.iter().map(|t| t.arguments.as_str()).collect();
    (0..steps.len())
        .map(|s| {
            let own_words: Vec<String> = words[s]
                .iter()
                .filter(|w| used_by[w.as_str()] == 1)
                .cloned()
                .collect();
            let carrying: Vec<usize> = (0..tools.len())
                .filter(|&c| own_words.iter().any(|w| calls[c].contains(w)))
                .collect();
            if !matched[s].is_empty() {
                let all_failed = matched[s].iter().all(|&c| !tools[c].succeeded);
                return if all_failed && !carrying.iter().any(|&c| tools[c].succeeded) {
                    StepState::Failed {
                        calls: matched[s].len(),
                    }
                } else {
                    StepState::Done
                };
            }
            if own_words.is_empty() || !carrying.is_empty() {
                return StepState::Unclear;
            }
            let own_files_on_disk = paths[s]
                .iter()
                .filter(|p| !p.contains('*'))
                .filter(|p| paths.iter().filter(|other| other.contains(p)).count() == 1)
                .any(|p| on_disk(p, &arguments, working_dir, exists));
            if own_files_on_disk {
                return StepState::Unclear;
            }
            StepState::NeverRun { own_words }
        })
        .collect()
}

/// The sentence of `reply` that says all `n` steps are done — "All 20 steps succeeded.", "20/20",
/// "steps 1–20". Only the list's own count is read here; a vaguer claim is the reviewer's to read.
pub(crate) fn claim_of_all(reply: &str, n: usize) -> Option<String> {
    let claim = Regex::new(&format!(
        r"(?i)\b(?:all|every|each)(?:\W+\w+){{0,2}}?\W+{n}\b|\b{n}\s*(?:/|of|out of)\s*{n}\b|\b1\s*(?:-|–|—|to|through)\s*{n}\b"
    ))
    .expect("the pattern is built from a number");
    reply.lines().find_map(|line| {
        let found = claim.find(line)?;
        let start = line
            .get(..found.start())?
            .rfind(['.', '!', '?'])
            .map_or(0, |i| i + 1);
        let end = line
            .get(found.end()..)?
            .find(['.', '!', '?'])
            .map_or(line.len(), |i| found.end() + i + 1);
        Some(line.get(start..end)?.trim().to_string())
    })
}

fn or_list(words: &[String]) -> String {
    let shown: Vec<String> = words.iter().take(3).map(|w| format!("`{w}`")).collect();
    match shown.as_slice() {
        [one] => one.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
        [] => String::new(),
    }
}

/// What the user reads about one step the calls did not do.
pub(crate) fn step_clause(number: usize, state: &StepState) -> Option<String> {
    match state {
        StepState::NeverRun { own_words } => Some(format!(
            "step {number} was never run — no tool call this turn carries {}, words only that step uses",
            or_list(own_words)
        )),
        StepState::Failed { calls: 1 } => Some(format!("step {number}'s only tool call failed")),
        StepState::Failed { calls } => Some(format!(
            "all {calls} tool calls for step {number} failed"
        )),
        StepState::Done | StepState::Unclear => None,
    }
}

/// The steps of this turn's numbered list the calls did not do, as `(step number, clause)`; empty
/// when the turn's request numbers no list. `messages[..to]` ends in the turn.
pub(crate) fn unfinished_steps(
    messages: &[Message],
    to: usize,
    working_dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> (usize, Vec<(usize, String)>) {
    let to = to.min(messages.len());
    let start = turn_start(&messages[..to]);
    let Some(request) = messages.get(start).filter(|m| is_human_message(m)) else {
        return (0, Vec::new());
    };
    let steps = numbered_steps(&text_of(request));
    if steps.is_empty() {
        return (0, Vec::new());
    }
    let tools = tool_facts(&messages[start..to]);
    let unfinished = audit_steps(&steps, &tools, working_dir, exists)
        .iter()
        .enumerate()
        .filter_map(|(i, state)| step_clause(i + 1, state).map(|clause| (i + 1, clause)))
        .collect();
    (steps.len(), unfinished)
}

/// A closing reply that says all of the user's numbered steps are done, held against the steps the
/// calls did. Only the reply that ends the turn is read — a mid-turn line ("Step 7 — heredoc:") is
/// narration, not a claim.
fn unfinished_claim(
    messages: &[Message],
    from: usize,
    texts: &str,
    working_dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<String> {
    let closing = !messages[from..].iter().any(|m| {
        m.content
            .iter()
            .any(|c| matches!(c, MessageContent::ToolRequest(_)))
    });
    if !closing {
        return Vec::new();
    }
    let (count, unfinished) = unfinished_steps(messages, messages.len(), working_dir, exists);
    if unfinished.is_empty() {
        return Vec::new();
    }
    let Some(claim) = claim_of_all(texts, count) else {
        return Vec::new();
    };
    let clauses: Vec<String> = unfinished.into_iter().map(|(_, clause)| clause).collect();
    vec![format!(
        "the answer says “{claim}”, but {}.",
        clauses.join("; ")
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
    findings.extend(unfinished_claim(
        messages,
        from,
        &texts,
        working_dir,
        exists,
    ));
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

    /// A recorded census session (fixture.py over its sessions.db), the work dir spelled WORKDIR.
    fn census(fixture: &str) -> Vec<Message> {
        let entries: Vec<serde_json::Value> = serde_json::from_str(fixture).unwrap();
        let mut messages = Vec::new();
        for entry in entries {
            let text = entry["text"].as_str().unwrap();
            if entry["role"] == "user" {
                let mut message = Message::user();
                if !text.is_empty() {
                    message = message.with_text(text);
                }
                for r in entry["results"].as_array().unwrap() {
                    let content = vec![Content::text(r["text"].as_str().unwrap())];
                    let result = if r["error"].as_bool().unwrap() {
                        CallToolResult::error(content)
                    } else {
                        CallToolResult::success(content)
                    };
                    message = message.with_tool_response(r["id"].as_str().unwrap(), Ok(result));
                }
                messages.push(message);
            } else {
                let mut message = Message::assistant();
                if !text.is_empty() {
                    message = message.with_text(text);
                }
                for c in entry["calls"].as_array().unwrap() {
                    message = message.with_tool_request(
                        c["id"].as_str().unwrap(),
                        Ok(
                            CallToolRequestParams::new(c["name"].as_str().unwrap().to_string())
                                .with_arguments(c["arguments"].as_object().unwrap().clone()),
                        ),
                    );
                }
                messages.push(message);
            }
        }
        messages
    }

    const CENSUS_N1: &str = include_str!("claim_check_fixtures/census_n1.json");
    const CENSUS_N3: &str = include_str!("claim_check_fixtures/census_n3.json");

    #[test]
    fn the_census_request_is_read_as_its_twenty_steps() {
        let messages = census(CENSUS_N1);
        let steps = numbered_steps(&text_of(&messages[0]));
        assert_eq!(steps.len(), 20, "{steps:#?}");
        assert!(
            steps[0].starts_with("shell: mkdir -p WORKDIR/src"),
            "{}",
            steps[0]
        );
        assert!(
            steps[3].contains("for w in north south; do")
                && steps[3].ends_with("python3 src/inventory.py data/stock.csv"),
            "the indented lines belong to step 4: {}",
            steps[3]
        );
        assert!(
            !steps[19].contains("When all 20 steps are done"),
            "the closing instruction is not part of step 20"
        );
        assert!(numbered_steps("1. only one").is_empty());
        assert_eq!(
            numbered_steps("notes:\n1. a\n2. b\n\n1. x\n2. y\n3. z\ndone").len(),
            3
        );
    }

    /// Census n1: 19 calls, none ran step 20's `wc -l …`, and the reply says "All 20 steps
    /// succeeded." — the goose check says so under it.
    #[test]
    fn a_claim_that_all_steps_ran_when_one_never_did_is_corrected() {
        let messages = census(CENSUS_N1);
        let last = messages.len() - 1;
        assert_eq!(text_of(&messages[last]), "All 20 steps succeeded.");
        let findings = check_new_text(&messages, last, Path::new("/w"), &|_: &Path| true, 2026);
        assert_eq!(
            findings,
            vec!["the answer says “All 20 steps succeeded.”, but step 20 was never run — no tool call this turn carries `wc`, `-l` or `*.md`, words only that step uses."]
        );

        let (count, unfinished) =
            unfinished_steps(&messages, messages.len(), Path::new("/w"), &|_: &Path| true);
        assert_eq!(count, 20);
        assert_eq!(
            unfinished.len(),
            1,
            "every other step was done: {unfinished:?}"
        );
    }

    /// Census n3 did step 6 before steps 4 and 5, then every other step: all 20 ran, so "All 20 steps
    /// succeeded: 1–20." holds — out of order is not undone.
    #[test]
    fn steps_done_out_of_order_are_still_done() {
        let messages = census(CENSUS_N3);
        let last = messages.len() - 1;
        assert!(text_of(&messages[last]).starts_with("All 20 steps succeeded"));
        assert!(
            check_new_text(&messages, last, Path::new("/w"), &|_: &Path| false, 2026).is_empty()
        );
        let (_, unfinished) =
            unfinished_steps(&messages, messages.len(), Path::new("/w"), &|_: &Path| {
                false
            });
        assert!(unfinished.is_empty(), "{unfinished:?}");
    }

    #[test]
    fn a_step_whose_only_call_failed_did_not_succeed_and_mid_turn_narration_is_no_claim() {
        let messages = vec![
            Message::user().with_text("1. shell: mkdir -p out\n2. shell: cp notes.txt out/notes.txt\n3. shell: wc -l out/notes.txt\n\nThen tell me how it went."),
            call("a", "mkdir -p out"),
            result("a", "", 0),
            call("b", "cp notes.txt out/notes.txt"),
            result("b", "cp: notes.txt: No such file or directory", 1),
            Message::assistant().with_text("Step 3 next:").with_tool_request(
                "c",
                Ok(CallToolRequestParams::new("shell").with_arguments(
                    serde_json::json!({ "command": "wc -l out/notes.txt" }).as_object().unwrap().clone(),
                )),
            ),
            result("c", "wc: out/notes.txt: open: No such file or directory", 1),
            Message::assistant().with_text("Done — 3/3 steps completed."),
        ];
        assert!(
            check_new_text(&messages[..7], 5, Path::new("/w"), &|_: &Path| false, 2026).is_empty(),
            "a response that calls a tool is not the closing reply"
        );
        let findings = check_new_text(&messages, 7, Path::new("/w"), &|_: &Path| false, 2026);
        assert_eq!(
            findings,
            vec!["the answer says “Done — 3/3 steps completed.”, but step 2's only tool call failed; step 3's only tool call failed."]
        );

        let admits = {
            let mut m = messages.clone();
            m[7] = Message::assistant()
                .with_text("Steps 2 and 3 failed: the file to copy does not exist.");
            m
        };
        assert!(
            check_new_text(&admits, 7, Path::new("/w"), &|_: &Path| false, 2026).is_empty(),
            "a reply that does not claim the whole list is not corrected here"
        );
    }

    #[test]
    fn a_step_the_calls_cannot_settle_is_left_alone() {
        let steps = vec![
            "write notes/a.md with a title".to_string(),
            "write notes/b.md with a title".to_string(),
        ];
        let did_a = [ToolFact {
            name: "write".to_string(),
            arguments: "notes/a.md\n# A".to_string(),
            output: String::new(),
            succeeded: true,
        }];
        assert_eq!(
            audit_steps(&steps, &did_a, Path::new("/w"), &|_: &Path| false),
            vec![
                StepState::Done,
                StepState::NeverRun {
                    own_words: vec!["b.md".to_string()]
                }
            ]
        );
        assert_eq!(
            audit_steps(&steps, &did_a, Path::new("/w"), &|p: &Path| p
                == Path::new("/w/notes/b.md")),
            vec![StepState::Done, StepState::Unclear],
            "a file only step 2 names is on disk: something made it"
        );
        let both_at_once = [ToolFact {
            name: "shell".to_string(),
            arguments: "for f in a b; do echo '# t' > notes/$f.md; done; ls notes/a.md notes/b.md"
                .to_string(),
            output: String::new(),
            succeeded: true,
        }];
        assert!(
            !audit_steps(&steps, &both_at_once, Path::new("/w"), &|_: &Path| false)
                .iter()
                .any(|s| matches!(s, StepState::NeverRun { .. }))
        );
    }

    #[test]
    fn only_a_claim_naming_the_whole_list_is_read_here() {
        assert_eq!(
            claim_of_all("Ran them. All 20 steps succeeded. Bye", 20).as_deref(),
            Some("All 20 steps succeeded.")
        );
        assert_eq!(
            claim_of_all("All 20 steps succeeded: 1–20.", 20).as_deref(),
            Some("All 20 steps succeeded: 1–20.")
        );
        assert!(claim_of_all("20/20 done", 20).is_some());
        assert!(claim_of_all("I completed all of the 20 steps", 20).is_some());
        assert!(claim_of_all("All 19 steps succeeded.", 20).is_none());
        assert!(claim_of_all("Done.", 20).is_none());
        assert!(claim_of_all("Steps 1–19 succeeded; step 20 failed.", 20).is_none());
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
    /// on it — every flag is an upper bound, to be read against its transcript — unless
    /// `CLAIM_CHECK_REPLAY_DISK` is set, for sessions whose work folders are still as they ended.
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
        let real_disk = std::env::var("CLAIM_CHECK_REPLAY_DISK").is_ok();
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
                let findings = check_new_text(
                    &messages[..j],
                    i,
                    Path::new("/Users/mihaiperdum"),
                    &|p: &Path| real_disk && p.exists(),
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
