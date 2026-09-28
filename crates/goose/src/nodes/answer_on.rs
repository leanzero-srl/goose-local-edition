//! "Answer on {next} for now" (Q-381, DESIGN-Q359-CHAT-NODES.md "Failures"): a chat on its own node
//! set whose lead can't run, with the failover switch off, ends its turn with a refusal that offers
//! to run THIS turn on the set's next node without changing the set. The offer's prompt carries
//! `_meta.goose.answerOn = {node}`; `on_prompt` holds the ask for exactly that prompt's reply (the
//! guard drops when the reply ends, however it ends), and the router reads it for that session's
//! Chat route. The next prompt carries no mark, so it goes back to the lead.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Result};

/// The prompt `_meta` key under `goose` that asks one turn to answer on a node of the chat's set.
pub const ANSWER_ON_META_KEY: &str = "answerOn";

/// The node a prompt's `_meta.goose.answerOn` names: `None` when the prompt carries no mark,
/// `Some(Err)` when it carries one that names no node.
pub fn answer_on_mark(
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<Result<String>> {
    let mark = meta?.get("goose")?.get(ANSWER_ON_META_KEY)?;
    Some(
        match mark
            .get("node")
            .and_then(|n| n.as_str())
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            Some(node) => Ok(node.to_string()),
            None => Err(anyhow!(
                "the answer-on mark names no node (expected {{\"node\": \"<id>\"}}): {mark}"
            )),
        },
    )
}

fn asks() -> &'static Mutex<HashMap<String, String>> {
    static ASKS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    ASKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The ask of one reply: `session` answers on `node` until this drops.
pub struct AnswerOnAsk(String);

impl Drop for AnswerOnAsk {
    fn drop(&mut self) {
        asks()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

/// `session`'s reply answers on `node` for as long as the returned guard lives.
pub fn ask(session: &str, node: String) -> AnswerOnAsk {
    asks()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(session.to_string(), node);
    AnswerOnAsk(session.to_string())
}

/// The node `session`'s running reply was asked to answer on, if it was.
pub fn asked(session: &str) -> Option<String> {
    asks()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn the_mark_names_its_node_or_is_refused_by_name() {
        assert!(answer_on_mark(None).is_none());
        assert!(answer_on_mark(Some(&meta(json!({"goose": {"loopTick": {}}})))).is_none());
        let node = answer_on_mark(Some(&meta(
            json!({"goose": {"answerOn": {"node": "flash"}}}),
        )));
        assert_eq!(node.unwrap().unwrap(), "flash");
        for bad in [
            json!({"node": ""}),
            json!({}),
            json!("flash"),
            json!({"node": 3}),
        ] {
            let said = answer_on_mark(Some(&meta(json!({"goose": {"answerOn": bad}}))))
                .unwrap()
                .unwrap_err()
                .to_string();
            assert!(said.contains("names no node"), "{said}");
        }
    }

    #[test]
    fn an_ask_lives_exactly_as_long_as_its_guard() {
        assert_eq!(asked("answer-on-test"), None);
        let guard = ask("answer-on-test", "flash".to_string());
        assert_eq!(asked("answer-on-test").as_deref(), Some("flash"));
        assert_eq!(asked("another-session"), None);
        drop(guard);
        assert_eq!(asked("answer-on-test"), None);
    }
}
