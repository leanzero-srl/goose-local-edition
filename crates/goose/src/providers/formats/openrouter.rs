use crate::conversation::message::{Message, MessageContent, ProviderMetadata};
use goose_providers::base::DeclaredModelParameters;
use goose_providers::formats::openai;
use goose_providers::model::ModelConfig;
use goose_providers::thinking::ThinkingEffort;
use rmcp::model::Role;
use serde_json::{json, Value};

pub const REASONING_DETAILS_KEY: &str = "reasoning_details";

fn has_assistant_content(message: &Message) -> bool {
    message.content.iter().any(|c| match c {
        MessageContent::Text(t) => !t.text.is_empty(),
        MessageContent::Image(_) => true,
        MessageContent::ToolRequest(req) => req.tool_call.is_ok(),
        MessageContent::FrontendToolRequest(req) => req.tool_call.is_ok(),
        _ => false,
    })
}

pub fn extract_reasoning_details(response: &Value) -> Option<Vec<Value>> {
    response
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|m| m.get("message"))
        .and_then(|msg| msg.get("reasoning_details"))
        .and_then(|d| d.as_array())
        .cloned()
}

pub fn get_reasoning_details(metadata: &Option<ProviderMetadata>) -> Option<Vec<Value>> {
    metadata
        .as_ref()
        .and_then(|m| m.get(REASONING_DETAILS_KEY))
        .and_then(|v| v.as_array())
        .cloned()
}

pub fn response_to_message(response: &Value) -> anyhow::Result<Message> {
    let mut message = openai::response_to_message(response)?;

    if let Some(details) = extract_reasoning_details(response) {
        for content in &mut message.content {
            if let MessageContent::ToolRequest(req) = content {
                let mut meta = req.metadata.clone().unwrap_or_default();
                meta.insert(REASONING_DETAILS_KEY.to_string(), json!(details));
                req.metadata = Some(meta);
            }
        }
    }

    Ok(message)
}

pub fn add_reasoning_details_to_request(payload: &mut Value, messages: &[Message]) {
    let mut assistant_reasoning: Vec<Option<Vec<Value>>> = messages
        .iter()
        .filter(|m| m.is_agent_visible())
        .filter(|m| m.role == Role::Assistant)
        .filter(|m| has_assistant_content(m))
        .map(|message| {
            message.content.iter().find_map(|c| match c {
                MessageContent::ToolRequest(req) => get_reasoning_details(&req.metadata),
                _ => None,
            })
        })
        .collect();

    if let Some(payload_messages) = payload
        .as_object_mut()
        .and_then(|obj| obj.get_mut("messages"))
        .and_then(|m| m.as_array_mut())
    {
        let mut assistant_idx = 0;
        for payload_msg in payload_messages.iter_mut() {
            if payload_msg.get("role").and_then(|r| r.as_str()) == Some("assistant") {
                if assistant_idx < assistant_reasoning.len() {
                    if let Some(details) = assistant_reasoning
                        .get_mut(assistant_idx)
                        .and_then(|d| d.take())
                    {
                        if let Some(obj) = payload_msg.as_object_mut() {
                            obj.insert("reasoning_details".to_string(), json!(details));
                        }
                    }
                }
                assistant_idx += 1;
            }
        }
    }
}

fn reasoning_effort_for_openrouter(effort: ThinkingEffort) -> &'static str {
    match effort {
        ThinkingEffort::Off => "none",
        ThinkingEffort::Low => "low",
        ThinkingEffort::Medium => "medium",
        ThinkingEffort::High => "high",
        ThinkingEffort::Max => "xhigh",
    }
}

/// OpenRouter's `reasoning.effort` vocabulary, cheapest first.
const EFFORT_SCALE: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

fn effort_rank(level: &str) -> Option<usize> {
    EFFORT_SCALE.iter().position(|known| *known == level)
}

/// Fits the `reasoning.effort` a request carries to what OpenRouter's models listing declares
/// for the model, and returns `(sent, fitted)` when it changed. Two cases only:
///   - `none` on a model whose reasoning is MANDATORY: OpenRouter answers 400 "Reasoning is
///     mandatory for this endpoint and cannot be disabled" (MiniMax M3.1 Flash, 2026-10-20), so
///     every turn of an "Off" session failed. Off becomes the model's cheapest declared level, or
///     the model's own default when it declares none.
///   - goose's Max (sent as `xhigh`) on a model that declares a higher level: Max means the
///     model's own top level (`max` on DeepSeek/MiniMax), not a fixed name.
///
/// Any other level is left alone: OpenRouter maps an undeclared level itself (probe 2026-10-20:
/// `minimal` on MiniMax, which declares max..low, answered 200), and the benchmark pins
/// (`medium`) must reach every entrant unchanged. `goose_effort` is the effort goose itself
/// wrote (`None` when the request's `reasoning` came from the user's own parameters, which only
/// get the mandatory fix).
pub fn fit_reasoning_effort(
    payload: &mut Value,
    declared: &DeclaredModelParameters,
    goose_effort: Option<ThinkingEffort>,
) -> Option<(String, String)> {
    let reasoning = payload.get_mut("reasoning")?.as_object_mut()?;
    let sent = reasoning.get("effort")?.as_str()?.to_string();
    let known: Vec<&str> = declared
        .effort_levels
        .iter()
        .map(String::as_str)
        .filter(|level| effort_rank(level).is_some())
        .collect();
    let fitted = if sent == "none" && declared.reasoning_mandatory {
        known
            .iter()
            .copied()
            .filter(|level| *level != "none")
            .min_by_key(|level| effort_rank(level))
            .map(str::to_string)
    } else if goose_effort == Some(ThinkingEffort::Max) {
        match known.iter().copied().max_by_key(|level| effort_rank(level)) {
            Some(top) if effort_rank(top) > effort_rank(&sent) => Some(top.to_string()),
            _ => return None,
        }
    } else {
        return None;
    };
    match &fitted {
        Some(level) => {
            reasoning.insert("effort".to_string(), json!(level));
        }
        None => {
            reasoning.remove("effort");
            if reasoning.is_empty() {
                payload.as_object_mut()?.remove("reasoning");
            }
        }
    }
    Some((sent, fitted.unwrap_or_else(|| "model default".to_string())))
}

pub fn apply_reasoning_config(payload: &mut Value, model_config: &ModelConfig) {
    let Some(effort) = model_config.thinking_effort() else {
        return;
    };

    if let Some(obj) = payload.as_object_mut() {
        if obj.contains_key("reasoning") {
            obj.remove("reasoning_effort");
            return;
        }

        let clamped_effort = obj
            .remove("reasoning_effort")
            .and_then(|value| value.as_str().map(str::to_owned));
        // OpenRouter ignores `reasoning` on a model that cannot reason, so only a model declared
        // non-reasoning is skipped; a name heuristic missed qwen/glm/grok and left them on defaults.
        if clamped_effort.is_none() && model_config.reasoning == Some(false) {
            return;
        }

        obj.insert(
            "reasoning".to_string(),
            json!({ "effort": clamped_effort.as_deref().unwrap_or_else(|| reasoning_effort_for_openrouter(effort)) }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_extract_reasoning_details() {
        let response = json!({
            "choices": [{
                "message": {
                    "content": "Hello",
                    "reasoning_details": [
                        {"type": "text", "text": "Let me think..."},
                        {"type": "encrypted", "data": "abc123signature"}
                    ]
                }
            }]
        });

        let details = extract_reasoning_details(&response).unwrap();
        assert_eq!(details.len(), 2);
    }

    #[test]
    fn test_response_to_message_with_tool_calls() {
        let response = json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\": \"NYC\"}"
                        }
                    }],
                    "reasoning_details": [
                        {"type": "encrypted", "data": "sig456"}
                    ]
                }
            }]
        });

        let message = response_to_message(&response).unwrap();
        assert!(!message.content.is_empty());

        let tool_request = message
            .content
            .iter()
            .find_map(|c| {
                if let MessageContent::ToolRequest(req) = c {
                    Some(req)
                } else {
                    None
                }
            })
            .unwrap();

        assert!(tool_request.metadata.is_some());
        let details = get_reasoning_details(&tool_request.metadata).unwrap();
        assert_eq!(details.len(), 1);
    }

    #[test]
    fn test_apply_reasoning_config_uses_openrouter_reasoning_object() {
        let mut payload = json!({
            "model": "openai/gpt-5",
            "messages": [],
            "reasoning_effort": "high"
        });
        let mut model_config = ModelConfig::new("openai/gpt-5");
        let mut params = HashMap::new();
        params.insert("thinking_effort".to_string(), json!("max"));
        model_config.request_params = Some(params);

        apply_reasoning_config(&mut payload, &model_config);

        assert_eq!(payload["reasoning"], json!({ "effort": "high" }));
        assert!(payload.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_apply_reasoning_config_preserves_user_reasoning() {
        let mut payload = json!({
            "model": "openai/gpt-5",
            "messages": [],
            "reasoning": { "max_tokens": 2000 },
            "reasoning_effort": "high"
        });
        let mut model_config = ModelConfig::new("openai/gpt-5");
        let mut params = HashMap::new();
        params.insert("thinking_effort".to_string(), json!("high"));
        model_config.request_params = Some(params);

        apply_reasoning_config(&mut payload, &model_config);

        assert_eq!(payload["reasoning"], json!({ "max_tokens": 2000 }));
        assert!(payload.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_apply_reasoning_config_disables_reasoning_capable_model() {
        let mut payload = json!({
            "model": "google/gemini-2.5-flash",
            "messages": []
        });
        // Reasoning-capable model (per canonical) with thinking explicitly off, as a
        // fast-model config is built: OpenRouter must still emit the disable object.
        let mut model_config = ModelConfig::new("google/gemini-2.5-flash");
        model_config.reasoning = Some(true);
        let mut params = HashMap::new();
        params.insert("thinking_effort".to_string(), json!("off"));
        model_config.request_params = Some(params);

        apply_reasoning_config(&mut payload, &model_config);

        assert_eq!(payload["reasoning"], json!({ "effort": "none" }));
    }

    #[test]
    fn test_apply_reasoning_config_uses_reasoning_metadata() {
        let mut payload = json!({
            "model": "x-ai/grok-4",
            "messages": []
        });
        let mut model_config = ModelConfig::new("x-ai/grok-4");
        let mut params = HashMap::new();
        params.insert("thinking_effort".to_string(), json!("high"));
        model_config.request_params = Some(params);
        model_config.reasoning = Some(true);

        apply_reasoning_config(&mut payload, &model_config);

        assert_eq!(payload["reasoning"], json!({ "effort": "high" }));
    }

    #[test]
    fn test_apply_reasoning_config_uses_model_detection() {
        let mut payload = json!({
            "model": "anthropic/claude-sonnet-4",
            "messages": []
        });
        let mut model_config = ModelConfig::new("anthropic/claude-sonnet-4");
        let mut params = HashMap::new();
        params.insert("thinking_effort".to_string(), json!("high"));
        model_config.request_params = Some(params);

        apply_reasoning_config(&mut payload, &model_config);

        assert_eq!(payload["reasoning"], json!({ "effort": "high" }));
    }

    #[test]
    fn an_undeclared_model_still_gets_the_requested_effort() {
        let mut payload = json!({ "model": "qwen/qwen3.8-flash", "messages": [] });
        let model_config =
            ModelConfig::new("qwen/qwen3.8-flash").with_thinking_effort(ThinkingEffort::Medium);

        apply_reasoning_config(&mut payload, &model_config);

        assert_eq!(payload["reasoning"], json!({ "effort": "medium" }));
    }

    #[test]
    fn test_apply_reasoning_config_skips_non_reasoning_models() {
        let mut payload = json!({
            "model": "openai/gpt-4o",
            "messages": []
        });
        let mut model_config = ModelConfig::new("openai/gpt-4o");
        let mut params = HashMap::new();
        params.insert("thinking_effort".to_string(), json!("high"));
        model_config.request_params = Some(params);
        model_config.reasoning = Some(false);

        apply_reasoning_config(&mut payload, &model_config);

        assert!(payload.get("reasoning").is_none());
    }

    #[test]
    fn test_apply_reasoning_config_off_disables_reasoning() {
        let mut payload = json!({
            "model": "x-ai/grok-4",
            "messages": []
        });
        let mut model_config = ModelConfig::new("x-ai/grok-4");
        let mut params = HashMap::new();
        params.insert("thinking_effort".to_string(), json!("off"));
        model_config.request_params = Some(params);
        model_config.reasoning = Some(true);

        apply_reasoning_config(&mut payload, &model_config);

        assert_eq!(payload["reasoning"], json!({ "effort": "none" }));
    }

    fn declared(levels: &[&str], mandatory: bool) -> DeclaredModelParameters {
        DeclaredModelParameters {
            effort_levels: levels.iter().map(|l| l.to_string()).collect(),
            reasoning_mandatory: mandatory,
            ..Default::default()
        }
    }

    const MINIMAX: &[&str] = &["max", "xhigh", "high", "medium", "low"];

    #[test]
    fn off_on_a_mandatory_reasoning_model_sends_its_cheapest_level() {
        let mut payload = json!({ "reasoning": { "effort": "none" } });
        let fit = fit_reasoning_effort(
            &mut payload,
            &declared(MINIMAX, true),
            Some(ThinkingEffort::Off),
        );
        assert_eq!(payload["reasoning"], json!({ "effort": "low" }));
        assert_eq!(fit, Some(("none".to_string(), "low".to_string())));
    }

    #[test]
    fn off_on_a_mandatory_model_without_levels_falls_back_to_its_default() {
        let mut payload = json!({ "reasoning": { "effort": "none" } });
        let fit = fit_reasoning_effort(
            &mut payload,
            &declared(&[], true),
            Some(ThinkingEffort::Off),
        );
        assert!(payload.get("reasoning").is_none(), "{payload}");
        assert_eq!(fit.unwrap().1, "model default");
        // other reasoning keys the user set survive
        let mut payload = json!({ "reasoning": { "effort": "none", "exclude": true } });
        fit_reasoning_effort(&mut payload, &declared(&[], true), None);
        assert_eq!(payload["reasoning"], json!({ "exclude": true }));
    }

    #[test]
    fn off_on_a_model_that_can_switch_reasoning_off_stays_none() {
        let mut payload = json!({ "reasoning": { "effort": "none" } });
        let fit = fit_reasoning_effort(
            &mut payload,
            &declared(&["max", "high", "low"], false),
            Some(ThinkingEffort::Off),
        );
        assert_eq!(payload["reasoning"], json!({ "effort": "none" }));
        assert!(fit.is_none());
    }

    #[test]
    fn a_users_own_none_on_a_mandatory_model_is_fixed_too() {
        let mut payload = json!({ "reasoning": { "effort": "none" } });
        fit_reasoning_effort(&mut payload, &declared(MINIMAX, true), None);
        assert_eq!(payload["reasoning"], json!({ "effort": "low" }));
    }

    #[test]
    fn max_uses_the_models_own_top_level() {
        let mut payload = json!({ "reasoning": { "effort": "xhigh" } });
        fit_reasoning_effort(
            &mut payload,
            &declared(MINIMAX, true),
            Some(ThinkingEffort::Max),
        );
        assert_eq!(payload["reasoning"], json!({ "effort": "max" }));
        // a model whose top level is xhigh (or that declares nothing) keeps xhigh
        for levels in [&["xhigh", "high", "low"][..], &[][..]] {
            let mut payload = json!({ "reasoning": { "effort": "xhigh" } });
            assert!(fit_reasoning_effort(
                &mut payload,
                &declared(levels, false),
                Some(ThinkingEffort::Max)
            )
            .is_none());
            assert_eq!(payload["reasoning"], json!({ "effort": "xhigh" }));
        }
        // a user's own saved level is never raised by the global Max
        let mut payload = json!({ "reasoning": { "effort": "xhigh" } });
        assert!(fit_reasoning_effort(&mut payload, &declared(MINIMAX, true), None).is_none());
    }

    #[test]
    fn a_pinned_level_reaches_every_model_unchanged() {
        // the Forge benchmark pin is medium; DeepSeek declares max/high/low, OpenRouter maps it itself
        for (levels, mandatory) in [
            (&["max", "high", "low"][..], false),
            (MINIMAX, true),
            (&[][..], true),
        ] {
            let mut payload = json!({ "reasoning": { "effort": "medium" } });
            assert!(fit_reasoning_effort(
                &mut payload,
                &declared(levels, mandatory),
                Some(ThinkingEffort::Medium)
            )
            .is_none());
            assert_eq!(payload["reasoning"], json!({ "effort": "medium" }));
        }
    }

    #[test]
    fn a_request_without_an_effort_is_left_alone() {
        let mut payload = json!({ "reasoning": { "max_tokens": 2000 } });
        assert!(fit_reasoning_effort(
            &mut payload,
            &declared(MINIMAX, true),
            Some(ThinkingEffort::Off)
        )
        .is_none());
        let mut payload = json!({ "messages": [] });
        assert!(fit_reasoning_effort(
            &mut payload,
            &declared(MINIMAX, true),
            Some(ThinkingEffort::Off)
        )
        .is_none());
    }
}
