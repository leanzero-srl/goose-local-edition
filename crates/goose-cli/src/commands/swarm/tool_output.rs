//! A tool call's output as the swarm reads it: the run panel's "it printed:", the judge's evidence
//! and the repeat hash all take this one text (Q-211).

use super::clip_tail;

/// The text a tool call produced (its output), tail-capped for the run panel — this is the real "what
/// happened": pytest results, a traceback, a printed value. Empty for image/resource-only results.
/// The judge and the repeat hash read it too, so it is the MODEL-visible text (Q-211): an MCP item
/// marked for the user alone never enters; goose's own tools carry no audience, so it is unchanged.
pub(super) fn tool_result_text<E>(result: &Result<rmcp::model::CallToolResult, E>) -> String {
    let Ok(r) = result else {
        return String::new();
    };
    let joined = goose::conversation::message::model_visible_texts(r)
        .collect::<Vec<_>>()
        .join("\n");
    // Generous cap: the desktop run panel shows this as the tool's real output (pytest results, a
    // traceback, a printed value). Enough to read what happened, tail-kept so the informative end (the
    // pass/fail line) survives even for a long log.
    clip_tail(&joined, 4000)
}

#[cfg(test)]
mod tests {
    use super::super::repeat_call_hash;
    use super::*;

    /// Q-211: the swarm judge and its repeat hash read a call's MODEL-visible text. goose's own tools
    /// and the benchmark carry no audience, so their text — and so every repeat key — is
    /// byte-identical to the pre-Q-211 join; an MCP item marked for the user alone never enters.
    #[test]
    fn tool_result_text_is_the_model_visible_text() {
        use rmcp::model::{CallToolResult, Content, RawContent, Role};
        let pre_q211 = |r: &CallToolResult| {
            let joined = r
                .content
                .iter()
                .filter_map(|c| match &c.raw {
                    RawContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            clip_tail(&joined, 4000)
        };
        let long = "y".repeat(5000);
        for r in [
            CallToolResult::success(vec![Content::text("3 passed in 0.12s")]),
            CallToolResult::success(vec![
                Content::text("a"),
                Content::image("aGk=", "image/png"),
                Content::text("b"),
            ]),
            CallToolResult::error(vec![Content::text(long.as_str())]),
            CallToolResult::success(vec![]),
        ] {
            let ok: Result<CallToolResult, ()> = Ok(r.clone());
            assert_eq!(tool_result_text(&ok), pre_q211(&r));
            assert_eq!(
                repeat_call_hash("shell", "pytest", true, &tool_result_text(&ok)),
                repeat_call_hash("shell", "pytest", true, &pre_q211(&r))
            );
        }

        let with_user_only: Result<CallToolResult, ()> = Ok(CallToolResult::success(vec![
            Content::text("3 passed"),
            Content::text("SECRET for the person").with_audience(vec![Role::User]),
        ]));
        assert_eq!(tool_result_text(&with_user_only), "3 passed");
    }
}
