//! The two tool-call labels the desktop shows: one per call, one per chain of calls. Both run on the
//! fast model — the chat's own model when no fast model is configured — with reasoning off
//! through `complete_helper`, like every other helper: a 3–8 word label gains nothing from
//! thinking first, and on a local engine each label's reasoning competes with the agent's turn
//! for the same decode.

use crate::conversation::message::{Message, MessageContent};
use crate::providers::base::Provider;
use goose_providers::model::ModelConfig;
use tracing::warn;

pub(super) const TOOL_CALL_LABEL_SYSTEM: &str =
    "Summarize this tool call in a short lowercase phrase (3-8 words). \
     No punctuation. No quotes. Examples: reading project configuration, \
     checking network connectivity, listing files in src directory";

pub(super) const TOOL_CHAIN_LABEL_SYSTEM: &str =
    "Summarize this sequence of tool calls in a short lowercase phrase \
     (3-8 words). No punctuation. No quotes. \
     Examples: applied dark mode polish, scanned for security issues, \
     refactored config loading";

/// One label, or `Ok(None)` when the model answered empty or failed twice (each attempt is
/// logged under `what`). The fast model occasionally returns an empty response under load (rate
/// limiting, transient network); one retry with a short backoff recovers the common cases without
/// paying for the regular model. `Err` = the fast model's config could not be resolved.
pub(super) async fn complete_tool_label(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    system: &str,
    message: &Message,
    what: &str,
) -> anyhow::Result<Option<String>> {
    let fast_model_config =
        crate::model_config::get_fast_model(provider.get_name(), model_config).await?;
    for attempt in 0..2 {
        match crate::model_config::complete_helper(
            provider,
            &fast_model_config,
            session_id,
            system,
            std::slice::from_ref(message),
            &[],
        )
        .await
        {
            Ok((response, _)) => {
                let label = response
                    .content
                    .iter()
                    .filter_map(|c: &MessageContent| c.as_text())
                    .collect::<String>()
                    .trim()
                    .to_string();
                if !label.is_empty() {
                    return Ok(Some(label));
                }
                if attempt == 0 {
                    warn!("{what}: fast_complete returned empty, retrying once");
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                }
            }
            Err(e) => {
                if attempt == 0 {
                    warn!("{what}: fast_complete errored: {e}, retrying once");
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                } else {
                    warn!("{what}: fast_complete errored after retry: {e}");
                }
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_config::mlx_endpoint::{thinking_off, MlxEndpoint, SERVED};

    /// calls.csv (E2E-1-split-tensor, 2026-09-25): both labels reached the split with no template
    /// switch and thought first — 125–152 of 130–158 output chunks for a tool label, 178 of 189 for
    /// a chain label. Each now carries `enable_thinking: false`, and each label still arrives.
    #[tokio::test]
    async fn both_tool_labels_reach_the_mlx_engine_with_thinking_off() {
        let session = ModelConfig::new(SERVED);
        for (system, user) in [
            (
                TOOL_CALL_LABEL_SYSTEM,
                "Tool: developer__shell\nArguments: {\"command\":\"ls\"}",
            ),
            (
                TOOL_CHAIN_LABEL_SYSTEM,
                "Tool call sequence:\nStep 1: developer__shell {}\nStep 2: developer__text_editor {}\n",
            ),
        ] {
            let engine = MlxEndpoint::start().await;
            let label = complete_tool_label(
                engine.provider.as_ref(),
                &session,
                "s",
                system,
                &Message::user().with_text(user),
                "test label",
            )
            .await
            .unwrap();
            assert_eq!(label.as_deref(), Some("reading project configuration"));
            let bodies = engine.bodies().await;
            assert_eq!(bodies.len(), 1, "one request, no retry");
            assert_eq!(bodies[0]["messages"][0]["content"], system);
            assert_eq!(bodies[0]["chat_template_kwargs"], thinking_off());
        }
    }
}
