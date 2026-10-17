//! Compaction stage 1: MASK. At the compaction point, before any summary is asked for, goose
//! drops what it can give back or the model no longer needs — old tool outputs (each replaced by a
//! stub of facts: the outcome and the output's first and last lines) and old reasoning — keeps
//! every message the model and the person wrote, and places the working set (`working_set`).
//! No model call. The prefix changes once, here; nothing changes per turn.
//!
//! Why (Forge 2.0 runs A and B on Gabee, Qwen3.8-27B; `local-edition/design/COMPACTION-V2.md`):
//! at each of run B's five compactions the context (~102-110k tokens) held 15-43k tokens of old
//! reasoning and 33-44k tokens of tool output, and the summary that replaced it all cost 214-1,280
//! s to write; afterwards the model re-read what it had lost (B C1: 37 calls and no write before
//! the next compaction; C2: 18 calls, 74.6 min to the first write). Per-turn condensation is not an
//! option on a caching engine: Q-294 measured it re-reading 100k+ tokens cold every turn.
//!
//! Stage 1 is taken only when it leaves at least as much room before the next compaction point as
//! the window had between that point and its end (`accepts`); otherwise the summary (stage 2) runs.

use rmcp::model::Role;

use super::working_set::{is_working_set_text, WORKING_SET_CLOSE, WORKING_SET_OPEN};
use super::{outcome_and_output, RECORD_EXCERPT_CHARS};
use crate::conversation::message::{
    model_visible_texts, Message, MessageContent, MessageMetadata, ToolResponse,
};

/// `GOOSE_COMPACTION_MASK_FIRST` (env > config > default true): an automatic compaction tries
/// stage 1 before asking for a summary.
pub const MASK_FIRST_KEY: &str = "GOOSE_COMPACTION_MASK_FIRST";

/// `GOOSE_COMPACTION_DROP_REASONING` (env > config > default FALSE): stage 1 also drops the old
/// turns' reasoning. Off by default because the reasoning is where the plan lives: in run B's
/// first compaction span (B_C1) the 89 assistant messages held 555 chars of text and 250,387 of
/// reasoning — the visible text was "The design is clear now. Time to implement." and the design
/// itself only in the reasoning. JetBrains' measured masking kept reasoning and actions whole.
/// When on, only the reasoning the provider actually rendered counts as freed (`mask_stage`).
pub const DROP_REASONING_KEY: &str = "GOOSE_COMPACTION_DROP_REASONING";

pub fn mask_first() -> bool {
    super::knob_bool(MASK_FIRST_KEY, true)
}

pub fn drop_reasoning() -> bool {
    super::knob_bool(DROP_REASONING_KEY, false)
}

/// Whether stage 1 is enough: the context it leaves, `after` tokens, is at most
/// `limit × (2 × threshold − 1)` — the room it leaves before the compaction point is at least the
/// room the window has past it. At the default 0.8 that is 60% of the window. A stage 1 that
/// gives back less would be followed by another one a few turns later, each a cold prefill.
pub fn accepts(after: usize, limit: usize, threshold: f64) -> bool {
    let ceiling = limit as f64 * (2.0 * threshold - 1.0);
    ceiling > 0.0 && (after as f64) <= ceiling
}

/// A tool output at most this long is kept whole: its stub would be about as long.
fn worth_masking(text_chars: usize) -> bool {
    text_chars > 2 * RECORD_EXCERPT_CHARS
}

/// What stage 1 would make of the agent-visible conversation.
#[derive(Debug, Clone)]
pub struct Masked {
    /// The agent's copy of each agent-visible message, in order, outputs and reasoning dropped
    /// where they were.
    pub messages: Vec<Message>,
    pub outputs_masked: usize,
    pub reasoning_dropped: usize,
    /// The output text and the reasoning that leave the agent's copy, and the text that replaces
    /// them, for the token estimate. Kept apart: an output is always rendered, reasoning only where
    /// the provider sends it.
    pub removed_outputs: String,
    pub removed_reasoning: String,
    pub added: String,
}

/// How every stub opens: a stub is never masked again.
const STUB_OPEN: &str = "[Output elided at compaction";

fn stub(result: &rmcp::model::CallToolResult, chars: usize) -> String {
    let (outcome, output) = outcome_and_output(result);
    format!(
        "{STUB_OPEN} ({chars} chars): the call {outcome}. {output} Files it printed are in \
         \"Current file contents after compaction\" when listed there.]"
    )
}

/// The agent's copy of `messages` (the agent-visible ones) with the outputs of tool calls older
/// than the newest `keep_recent_chars` of output replaced by stubs, earlier working-set blocks
/// replaced by a line saying they are superseded, and — with `drop_reasoning` — the reasoning of
/// every assistant message before the oldest output kept whole. The newest output is always kept.
pub fn mask(messages: &[Message], keep_recent_chars: usize, drop_reasoning: bool) -> Masked {
    let visible: Vec<&Message> = messages.iter().filter(|m| m.is_agent_visible()).collect();
    let output_chars = |m: &Message| -> usize {
        m.content
            .iter()
            .filter_map(|c| match c {
                MessageContent::ToolResponse(r) => r.tool_result.as_ref().ok(),
                _ => None,
            })
            .map(|r| {
                model_visible_texts(r)
                    .map(|t| t.chars().count())
                    .sum::<usize>()
            })
            .sum()
    };
    // The oldest message whose outputs stay whole: walk back from the newest output until the
    // budget is spent.
    let mut kept_from = visible.len();
    let mut kept = 0usize;
    for (i, message) in visible.iter().enumerate().rev() {
        let chars = output_chars(message);
        if chars == 0 {
            continue;
        }
        if kept > 0 && kept + chars > keep_recent_chars {
            break;
        }
        kept += chars;
        kept_from = i;
    }
    // The turn that asked for the oldest output kept whole keeps its reasoning too.
    if let Some(ids) = visible.get(kept_from).map(|m| response_ids(m)) {
        if let Some(asked) = visible[..kept_from].iter().rposition(|m| {
            m.content.iter().any(
                |c| matches!(c, MessageContent::ToolRequest(req) if ids.contains(&req.id.as_str())),
            )
        }) {
            kept_from = asked;
        }
    }
    // The newest assistant turn always keeps its reasoning, outputs or not: Anthropic requires the
    // final tool-loop turn's thinking, and it is the model's current step.
    if let Some(last) = visible.iter().rposition(|m| m.role == Role::Assistant) {
        kept_from = kept_from.min(last);
    }

    let mut out = Masked {
        messages: Vec::with_capacity(visible.len()),
        outputs_masked: 0,
        reasoning_dropped: 0,
        removed_outputs: String::new(),
        removed_reasoning: String::new(),
        added: String::new(),
    };
    for (i, message) in visible.iter().enumerate() {
        let mut copy = (*message).clone();
        let old = i < kept_from;
        copy.content = copy
            .content
            .into_iter()
            .filter_map(|content| match content {
                MessageContent::Thinking(t) if old && drop_reasoning => {
                    out.reasoning_dropped += 1;
                    out.removed_reasoning.push_str(&t.thinking);
                    None
                }
                MessageContent::RedactedThinking(_) if old && drop_reasoning => {
                    out.reasoning_dropped += 1;
                    None
                }
                MessageContent::ToolResponse(response) if old => Some(
                    MessageContent::ToolResponse(mask_response(response, &mut out)),
                ),
                MessageContent::Text(text)
                    if message.role == Role::User
                        && is_working_set_text(&text.text)
                        && !text.text.contains(SUPERSEDED) =>
                {
                    out.removed_outputs.push_str(&text.text);
                    let line = format!("{WORKING_SET_OPEN}{SUPERSEDED}{WORKING_SET_CLOSE}");
                    out.added.push_str(&line);
                    Some(MessageContent::text(line))
                }
                other => Some(other),
            })
            .collect();
        if copy.content.is_empty() {
            continue;
        }
        out.messages
            .push(copy.with_metadata(MessageMetadata::agent_only()));
    }
    out
}

fn response_ids(message: &Message) -> Vec<&str> {
    message
        .content
        .iter()
        .filter_map(|c| match c {
            MessageContent::ToolResponse(r) => Some(r.id.as_str()),
            _ => None,
        })
        .collect()
}

const SUPERSEDED: &str =
    "An earlier copy of the files' contents; superseded by the newest one in this conversation.";

fn mask_response(response: ToolResponse, out: &mut Masked) -> ToolResponse {
    let Ok(result) = &response.tool_result else {
        return response;
    };
    let text: String = model_visible_texts(result).collect::<Vec<_>>().join("\n");
    let chars = text.chars().count();
    if !worth_masking(chars) || text.starts_with(STUB_OPEN) {
        return response;
    }
    let line = stub(result, chars);
    out.outputs_masked += 1;
    out.removed_outputs.push_str(&text);
    out.added.push_str(&line);
    let mut masked = result.clone();
    masked.content = vec![rmcp::model::Content::text(line)];
    // The file diff and the structured output stay on the person's copy only: the kept block
    // and the working set read them from there, once.
    masked.structured_content = None;
    masked.meta = None;
    ToolResponse {
        id: response.id,
        tool_result: Ok(masked),
        metadata: response.metadata,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Content};

    fn pair(id: &str, command: &str, output: &str, thinking: Option<&str>) -> [Message; 2] {
        let mut params = CallToolRequestParams::new("shell");
        params.arguments = serde_json::json!({ "command": command })
            .as_object()
            .cloned();
        let mut request = Message::assistant();
        if let Some(thinking) = thinking {
            request = request.with_thinking(thinking, "sig");
        }
        [
            request.with_tool_request(id, Ok(params)),
            Message::user()
                .with_tool_response(id, Ok(CallToolResult::success(vec![Content::text(output)]))),
        ]
    }

    fn outputs(masked: &Masked) -> Vec<String> {
        masked
            .messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|c| match c {
                MessageContent::ToolResponse(r) => r.tool_result.as_ref().ok(),
                _ => None,
            })
            .map(|r| model_visible_texts(r).collect::<Vec<_>>().join(""))
            .collect()
    }

    /// Old outputs become stubs of facts, the newest within the budget stay whole, small ones
    /// stay whole, old reasoning goes and the newest turn's stays; every message is still there,
    /// agent-only, in order.
    #[test]
    fn old_outputs_and_reasoning_go_the_newest_stay_whole() {
        let big = |c: char| format!("first line\n{}\nlast line", c.to_string().repeat(2_000));
        let mut messages = vec![Message::user().with_text("ship v2")];
        messages.extend(pair("a", "cat src/a.js", &big('a'), Some("plan A")));
        messages.extend(pair("b", "echo ok", "ok", Some("plan B")));
        messages.extend(pair("c", "cat src/c.js", &big('c'), Some("plan C")));
        messages.extend(pair("d", "npm test", &big('d'), Some("plan D")));
        messages.push(Message::assistant().with_text("tests pass; next sync.js"));

        let masked = mask(&messages, 3_000, true);
        assert_eq!(masked.messages.len(), messages.len());
        assert!(masked
            .messages
            .iter()
            .all(|m| m.is_agent_visible() && !m.is_user_visible()));
        let outputs = outputs(&masked);
        assert!(outputs[0].starts_with("[Output elided at compaction (2021 chars): the call succeeded. Output, 3 lines; first: \"first line\"; last: \"last line\""), "{}", outputs[0]);
        assert_eq!(outputs[1], "ok", "a short output is its own stub");
        assert!(outputs[2].starts_with("[Output elided"), "{}", outputs[2]);
        assert_eq!(outputs[3], big('d'), "the newest output stays whole");
        assert_eq!(masked.outputs_masked, 2);
        let thinking: Vec<&str> = masked
            .messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|c| match c {
                MessageContent::Thinking(t) => Some(t.thinking.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(thinking, ["plan D"], "only the kept turn's reasoning stays");
        assert_eq!(masked.reasoning_dropped, 3);
        assert!(masked.removed_reasoning.contains("plan A"));
        assert!(masked.removed_outputs.contains(&"a".repeat(2_000)));
        assert!(
            !outputs[0].contains("run the call again"),
            "facts, no instruction"
        );

        let kept_reasoning = mask(&messages, 3_000, false);
        assert_eq!(kept_reasoning.reasoning_dropped, 0);
    }

    /// A budget wide enough for every output masks nothing; the newest output stays whole even
    /// when it alone is over the budget.
    #[test]
    fn the_budget_decides_which_outputs_stay() {
        let mut messages = vec![Message::user().with_text("go")];
        messages.extend(pair("a", "cat a", &"a".repeat(1_000), None));
        messages.extend(pair("b", "cat b", &"b".repeat(5_000), None));
        assert_eq!(mask(&messages, 10_000, true).outputs_masked, 0);
        let tight = mask(&messages, 10, true);
        assert_eq!(tight.outputs_masked, 1);
        assert_eq!(outputs(&tight)[1], "b".repeat(5_000));
    }

    /// An earlier working set is replaced by one line saying so (the new one follows); hidden
    /// messages stay out.
    #[test]
    fn an_earlier_working_set_is_said_superseded() {
        let block = format!(
            "{WORKING_SET_OPEN}\nCurrent file contents …\n===== /a.js\nx\n{WORKING_SET_CLOSE}"
        );
        let messages = vec![
            Message::user()
                .with_text("hidden")
                .with_visibility(true, false),
            Message::user().with_text(&block).agent_only(),
        ];
        let masked = mask(&messages, 1_000, true);
        assert_eq!(masked.messages.len(), 1);
        let text = masked.messages[0].as_concat_text();
        assert!(
            is_working_set_text(&text) && text.contains(SUPERSEDED),
            "{text}"
        );
        assert_eq!(
            mask(&masked.messages, 1_000, true).added,
            "",
            "superseded once"
        );
    }

    /// A stub from an earlier stage 1 is longer than the "worth masking" floor; it is never masked
    /// again (a stub of a stub would lose the original's facts).
    #[test]
    fn a_stub_is_never_masked_again() {
        let mut messages = vec![Message::user().with_text("go")];
        let output = format!(
            "{}\n{}\n{}",
            "f".repeat(150),
            "a".repeat(2_000),
            "l".repeat(150)
        );
        messages.extend(pair("a", "cat a", &output, None));
        messages.extend(pair("b", "cat b", &"b".repeat(5_000), None));
        let once = mask(&messages, 10, false);
        assert_eq!(once.outputs_masked, 1);
        let stub = outputs(&once)[0].clone();
        assert!(stub.chars().count() > 2 * RECORD_EXCERPT_CHARS, "{stub}");
        let twice = mask(&once.messages, 10, false);
        assert_eq!(twice.outputs_masked, 0);
        assert_eq!(outputs(&twice)[0], stub);
    }

    /// With no tool output at all, the newest assistant turn still keeps its reasoning.
    #[test]
    fn the_newest_turn_keeps_its_reasoning_when_no_output_has_text() {
        let messages = vec![
            Message::user().with_text("plan it"),
            Message::assistant()
                .with_thinking("old plan", "sig")
                .with_text("first"),
            Message::user().with_text("again"),
            Message::assistant()
                .with_thinking("current plan", "sig")
                .with_text("second"),
        ];
        let masked = mask(&messages, 1_000, true);
        assert_eq!(masked.removed_reasoning, "old plan");
        assert!(serde_json::to_string(&masked.messages[3].content)
            .unwrap()
            .contains("current plan"));
    }

    #[test]
    fn stage_one_is_enough_only_when_it_leaves_the_windows_own_headroom() {
        assert!(accepts(95_299, 158_833, 0.8));
        assert!(!accepts(95_300, 158_833, 0.8));
        assert!(
            !accepts(1, 158_833, 0.5),
            "no room at a threshold of one half"
        );
    }
}

/// The offline replay of a run's compaction point through stage 1 (COMPACTION-V2.md's test plan):
/// `GOOSE_MASK_TRACE=<messages.json>` (the agent-visible messages at that point, `[{role, created,
/// content, metadata}]`), `GOOSE_MASK_TRACE_DIR=<the app folder>`, `GOOSE_MASK_TRACE_USED=<the
/// measured prompt tokens>`, `GOOSE_MASK_TRACE_WINDOW=<tokens>`; prints what stage 1 frees, what
/// the working set adds, the estimate and the decision at threshold 0.8.
#[cfg(test)]
mod trace {
    use super::*;
    use crate::context_mgmt::working_set;

    #[tokio::test]
    #[ignore = "reads a local export of a run's session"]
    async fn trace_a_runs_compaction_point() {
        let var = |k: &str| std::env::var(k).unwrap();
        let messages: Vec<Message> =
            serde_json::from_str(&std::fs::read_to_string(var("GOOSE_MASK_TRACE")).unwrap())
                .unwrap();
        let dir = std::path::PathBuf::from(var("GOOSE_MASK_TRACE_DIR"));
        let used: usize = var("GOOSE_MASK_TRACE_USED").parse().unwrap();
        let window: usize = var("GOOSE_MASK_TRACE_WINDOW").parse().unwrap();
        let counter = crate::token_counter::create_token_counter().await.unwrap();
        let keep = crate::context_mgmt::kept_budget_chars(Some(window)).unwrap();
        let masked = mask(&messages, keep, true);
        let files = working_set::touched_files(&messages, Some(&dir));
        let budget = working_set::working_set_budget_chars(
            Some(window),
            working_set::DEFAULT_WORKING_SET_SHARE,
        )
        .unwrap();
        let block = working_set::working_set_block(&files, budget, Some(&dir)).unwrap_or_default();
        let outputs = counter.count_tokens(&masked.removed_outputs);
        let reasoning = counter.count_tokens(&masked.removed_reasoning);
        let stubs = counter.count_tokens(&masked.added);
        let set = counter.count_tokens(&block);
        let kept_reasoning = (used + stubs + set).saturating_sub(outputs);
        let dropped_too = kept_reasoning.saturating_sub(reasoning);
        let verdict = |after: usize| {
            if accepts(after, window, 0.8) {
                "TAKEN"
            } else {
                "not enough -> summary"
            }
        };
        println!(
            "window {window} used {used}: {} outputs masked ({outputs} tok), stubs {stubs}, working \
             set {set} -> {kept_reasoning} ({:.0}%) {} [default, reasoning kept]; dropping {} \
             reasoning items ({reasoning} tok, an upper bound on what was rendered) -> \
             {dropped_too} {}",
            masked.outputs_masked,
            kept_reasoning as f64 * 100.0 / window as f64,
            verdict(kept_reasoning),
            masked.reasoning_dropped,
            verdict(dropped_too),
        );
    }
}
