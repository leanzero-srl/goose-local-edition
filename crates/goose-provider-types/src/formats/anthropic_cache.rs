//! Anthropic prompt-cache breakpoints for an OpenAI-format chat request whose model is Claude,
//! served through a router that passes `cache_control` through to Anthropic (OpenRouter, LiteLLM,
//! Databricks).

use crate::conversation::{is_turn_context_text, CURRENT_TIME_TAG, TURN_CONTEXT_TAG};
use serde_json::{json, Value};

const CACHE_CONTROL: &str = "cache_control";

/// Marks four breakpoints, Anthropic's maximum: the last tool definition, the system prompt, the
/// last STABLE history message, and the position the previous request's moving breakpoint held.
///
/// The per-turn `<turn-context>` block (current minute, context usage) rides the end of the
/// request and differs on every request, so a breakpoint on it writes a cache entry no later
/// request can read — every turn then pays the write premium on the whole prompt and reads
/// nothing. The moving breakpoint therefore sits on the last block BEFORE that block (usually the
/// last tool result), and the second one on the last user/tool message before the latest
/// assistant turn — where the previous request put its moving breakpoint — so this request reads
/// the history back from that entry however many blocks the latest turn added.
pub fn apply_anthropic_cache_breakpoints(payload: &mut Value) {
    if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
        if let Some(system) = messages.iter_mut().find(|m| m["role"] == json!("system")) {
            mark_last_text_part(system);
        }
        mark_history(messages);
    }

    if let Some(function) = payload
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .and_then(|tools| tools.last_mut())
        .and_then(|tool| tool.get_mut("function"))
        .and_then(Value::as_object_mut)
    {
        function.insert(CACHE_CONTROL.to_string(), json!({ "type": "ephemeral" }));
    }
}

fn mark_history(messages: &mut [Value]) {
    if let Some(tail) = messages.iter().rposition(is_user_or_tool) {
        isolate_turn_context(&mut messages[tail]);
    }

    let Some(moving) = mark_latest_before(messages, messages.len()) else {
        return;
    };
    if let Some(latest_answer) = messages[..moving]
        .iter()
        .rposition(|m| m["role"] == json!("assistant"))
    {
        mark_latest_before(messages, latest_answer);
    }
}

/// Marks the latest user/tool message before `end` that can carry a breakpoint; its index.
fn mark_latest_before(messages: &mut [Value], end: usize) -> Option<usize> {
    (0..end)
        .rev()
        .find(|&i| is_user_or_tool(&messages[i]) && mark_last_text_part(&mut messages[i]))
}

fn is_user_or_tool(message: &Value) -> bool {
    message["role"] == json!("user") || message["role"] == json!("tool")
}

/// Makes the turn-context block its own trailing text part, so the message's stable text ends on
/// a part boundary a breakpoint can sit on. The OpenAI formatter joins the block onto the last
/// user/tool message's string content with a newline; an array content gets it as a part, which is
/// moved last.
fn isolate_turn_context(message: &mut Value) {
    match message.get_mut("content") {
        Some(Value::String(text)) => {
            let Some((stable, block)) = split_turn_context_suffix(text) else {
                return;
            };
            let parts = json!([
                { "type": "text", "text": stable },
                { "type": "text", "text": block },
            ]);
            message["content"] = parts;
        }
        Some(Value::Array(parts)) => {
            let (blocks, mut rest): (Vec<Value>, Vec<Value>) =
                parts.drain(..).partition(is_turn_context_part);
            rest.extend(blocks);
            *parts = rest;
        }
        _ => {}
    }
}

fn split_turn_context_suffix(text: &str) -> Option<(String, String)> {
    let opening = format!("\n<{TURN_CONTEXT_TAG}>\n<{CURRENT_TIME_TAG}>");
    let at = text.rfind(&opening)?;
    let (stable, block) = (text.get(..at)?, text.get(at + 1..)?);
    (!stable.is_empty() && is_turn_context_text(block))
        .then(|| (stable.to_string(), block.to_string()))
}

fn is_turn_context_part(part: &Value) -> bool {
    part["type"] == json!("text") && part["text"].as_str().is_some_and(is_turn_context_text)
}

fn is_cacheable_text_part(part: &Value) -> bool {
    part["type"] == json!("text")
        && part["text"]
            .as_str()
            .is_some_and(|text| !text.is_empty() && !is_turn_context_text(text))
}

/// Puts `cache_control` on the message's last non-empty text part that is not the turn-context
/// block, converting string content to the one-part array form (the only form that carries
/// `cache_control`, for tool messages too). Anthropic refuses `cache_control` on an empty text
/// block, and OpenRouter documents it on text parts only, so a message with neither is skipped.
fn mark_last_text_part(message: &mut Value) -> bool {
    match message.get_mut("content") {
        Some(Value::String(text)) if !text.is_empty() && !is_turn_context_text(text) => {
            let part = json!([{
                "type": "text",
                "text": std::mem::take(text),
                CACHE_CONTROL: { "type": "ephemeral" },
            }]);
            message["content"] = part;
            true
        }
        Some(Value::Array(parts)) => {
            match parts.iter_mut().rev().find(|p| is_cacheable_text_part(p)) {
                Some(part) => {
                    part[CACHE_CONTROL] = json!({ "type": "ephemeral" });
                    true
                }
                None => false,
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn_context(minute: &str) -> String {
        format!(
            "<{TURN_CONTEXT_TAG}>\n<{CURRENT_TIME_TAG}>2026-10-01 20:{minute}</{CURRENT_TIME_TAG}>\n\
             <working-directory>/tmp</working-directory>\n</{TURN_CONTEXT_TAG}>"
        )
    }

    fn marked(message: &Value) -> Vec<String> {
        match &message["content"] {
            Value::Array(parts) => parts
                .iter()
                .filter(|p| p.get(CACHE_CONTROL).is_some())
                .map(|p| p["text"].as_str().unwrap_or_default().to_string())
                .collect(),
            _ => vec![],
        }
    }

    fn breakpoints(payload: &Value) -> Vec<(usize, String)> {
        payload["messages"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .flat_map(|(i, m)| marked(m).into_iter().map(move |t| (i, t)))
            .collect()
    }

    fn tool_call(id: &str) -> Value {
        json!({"role": "assistant", "content": null, "tool_calls": [
            {"id": id, "type": "function", "function": {"name": "shell", "arguments": "{}"}}
        ]})
    }

    fn tool_loop(minute: &str) -> Value {
        json!({
            "messages": [
                {"role": "system", "content": "system prompt"},
                {"role": "user", "content": "read the five files"},
                tool_call("c1"),
                {"role": "tool", "tool_call_id": "c1", "content": "output one"},
                tool_call("c2"),
                {"role": "tool", "tool_call_id": "c2", "content": "output two"},
                {"role": "user", "content": turn_context(minute)},
            ],
            "tools": [
                {"type": "function", "function": {"name": "a"}},
                {"type": "function", "function": {"name": "shell"}},
            ],
        })
    }

    #[test]
    fn a_tool_loop_breaks_on_the_last_tool_result_and_the_previous_one() {
        let mut payload = tool_loop("01");
        apply_anthropic_cache_breakpoints(&mut payload);

        assert_eq!(
            breakpoints(&payload),
            vec![
                (0, "system prompt".to_string()),
                (3, "output one".to_string()),
                (5, "output two".to_string()),
            ]
        );
        assert_eq!(payload["messages"][6]["content"], json!(turn_context("01")));
        assert!(payload["tools"][1]["function"].get(CACHE_CONTROL).is_some());
        assert!(payload["tools"][0]["function"].get(CACHE_CONTROL).is_none());
    }

    #[test]
    fn the_previous_requests_breakpoint_is_this_requests_read_point() {
        let mut previous = json!({"messages": [
            {"role": "system", "content": "system prompt"},
            {"role": "user", "content": "read the five files"},
            tool_call("c1"),
            {"role": "tool", "tool_call_id": "c1", "content": "output one"},
            {"role": "user", "content": turn_context("00")},
        ]});
        apply_anthropic_cache_breakpoints(&mut previous);
        let mut current = tool_loop("01");
        apply_anthropic_cache_breakpoints(&mut current);

        let previous_moving = breakpoints(&previous).last().unwrap().clone();
        assert_eq!(previous_moving, (3, "output one".to_string()));
        assert!(breakpoints(&current).contains(&previous_moving));
        assert_eq!(
            rendered(&previous)[..4],
            rendered(&current)[..4],
            "the prefix the previous request cached renders identically in this one"
        );
    }

    /// The messages as Anthropic renders them: a marker is not content, and a string is one text
    /// block.
    fn rendered(payload: &Value) -> Vec<Value> {
        let mut messages = payload["messages"].as_array().unwrap().clone();
        for message in &mut messages {
            if let Some(text) = message["content"].as_str() {
                message["content"] = json!([{"type": "text", "text": text}]);
            }
            if let Some(parts) = message["content"].as_array_mut() {
                for part in parts {
                    part.as_object_mut().unwrap().remove(CACHE_CONTROL);
                }
            }
        }
        messages
    }

    #[test]
    fn a_turn_context_joined_to_the_users_words_is_split_off_and_left_unmarked() {
        let mut payload = json!({"messages": [
            {"role": "system", "content": "system prompt"},
            {"role": "user", "content": "read the five files"},
            tool_call("c1"),
            {"role": "tool", "tool_call_id": "c1", "content": "output one"},
            {"role": "assistant", "content": "done"},
            {"role": "user", "content": format!("now the sixth\n{}", turn_context("05"))},
        ]});
        apply_anthropic_cache_breakpoints(&mut payload);

        assert_eq!(
            payload["messages"][5]["content"],
            json!([
                {"type": "text", "text": "now the sixth", CACHE_CONTROL: {"type": "ephemeral"}},
                {"type": "text", "text": turn_context("05")},
            ])
        );
        assert_eq!(
            breakpoints(&payload),
            vec![
                (0, "system prompt".to_string()),
                (3, "output one".to_string()),
                (5, "now the sixth".to_string()),
            ]
        );
    }

    #[test]
    fn a_turn_context_joined_to_tool_output_leaves_the_output_marked() {
        let mut payload = json!({"messages": [
            {"role": "user", "content": "go"},
            tool_call("c1"),
            {"role": "tool", "tool_call_id": "c1", "content": format!("output one\n{}", turn_context("07"))},
        ]});
        apply_anthropic_cache_breakpoints(&mut payload);

        assert_eq!(
            payload["messages"][2]["content"],
            json!([
                {"type": "text", "text": "output one", CACHE_CONTROL: {"type": "ephemeral"}},
                {"type": "text", "text": turn_context("07")},
            ])
        );
        assert_eq!(
            breakpoints(&payload),
            vec![(0, "go".to_string()), (2, "output one".to_string())]
        );
    }

    #[test]
    fn a_turn_context_part_moves_behind_the_parts_it_preceded() {
        let mut payload = json!({"messages": [
            {"role": "user", "content": [
                {"type": "text", "text": turn_context("09")},
                {"type": "text", "text": "and the tests"},
            ]},
        ]});
        apply_anthropic_cache_breakpoints(&mut payload);

        assert_eq!(
            payload["messages"][0]["content"],
            json!([
                {"type": "text", "text": "and the tests", CACHE_CONTROL: {"type": "ephemeral"}},
                {"type": "text", "text": turn_context("09")},
            ])
        );
    }

    #[test]
    fn empty_and_image_only_messages_never_carry_a_breakpoint() {
        let mut payload = json!({"messages": [
            {"role": "user", "content": "go"},
            tool_call("c1"),
            {"role": "tool", "tool_call_id": "c1", "content": "output one"},
            tool_call("c2"),
            {"role": "tool", "tool_call_id": "c2", "content": ""},
            {"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
                {"type": "text", "text": turn_context("11")},
            ]},
        ]});
        apply_anthropic_cache_breakpoints(&mut payload);

        assert_eq!(payload["messages"][4]["content"], json!(""));
        assert!(marked(&payload["messages"][5]).is_empty());
        assert_eq!(
            breakpoints(&payload),
            vec![(0, "go".to_string()), (2, "output one".to_string())]
        );
    }

    #[test]
    fn never_more_than_four_breakpoints() {
        let mut payload = tool_loop("13");
        apply_anthropic_cache_breakpoints(&mut payload);
        let tool_marks = payload["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["function"].get(CACHE_CONTROL).is_some())
            .count();
        assert_eq!(breakpoints(&payload).len() + tool_marks, 4);
    }
}
