use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose::agents::{Agent, AgentConfig, AgentEvent, GoosePlatform, SessionConfig};
use goose::config::{ExtensionConfig, GooseMode, PermissionManager};
use goose::conversation::message::{Message, MessageContent};
use goose::conversation::Conversation;
use goose::providers::base::{
    stream_from_single_message, MessageStream, Provider, ProviderDef, ProviderMetadata,
};
use goose::session::session_manager::SessionType;
use goose::session::{Session, SessionManager};
use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use rmcp::model::{CallToolRequestParams, RawContent, Tool};
use rmcp::object;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

struct MockCompactionProvider {
    /// Tracks whether compaction has occurred (for context limit recovery case)
    has_compacted: Arc<AtomicBool>,
}

impl MockCompactionProvider {
    fn new() -> Self {
        Self {
            has_compacted: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Calculate input tokens based on system prompt and messages
    /// Simulates realistic token counts for different scenarios
    fn calculate_input_tokens(&self, system_prompt: &str, messages: &[Message]) -> i32 {
        // Check if this is a compaction call
        let is_compaction_call = messages.len() == 1
            && messages[0].content.iter().any(|c| {
                if let MessageContent::Text(text) = c {
                    text.text.to_lowercase().contains("summarize")
                } else {
                    false
                }
            });

        if is_compaction_call {
            // For compaction: system prompt length is a good proxy for conversation size
            // Base: 6000 (system) + conversation content embedded in prompt
            6000 + (system_prompt.len() as i32 / 4).max(400)
        } else {
            // Regular call: system prompt + messages
            let system_tokens = if system_prompt.is_empty() { 0 } else { 6000 };

            let message_tokens: i32 = messages
                .iter()
                .map(|msg| {
                    let mut tokens = 100;
                    for content in &msg.content {
                        if let MessageContent::Text(text) = content {
                            if text.text.contains("long_tool_call") {
                                tokens += 15000;
                            }
                        }
                    }
                    tokens
                })
                .sum();

            system_tokens + message_tokens
        }
    }

    /// Calculate output tokens based on response type
    fn calculate_output_tokens(&self, is_compaction: bool, messages: &[Message]) -> i32 {
        if is_compaction {
            // Compaction produces a compact summary
            200
        } else {
            // Regular responses vary by content
            let has_hello = messages.iter().any(|msg| {
                msg.content.iter().any(|c| {
                    if let MessageContent::Text(text) = c {
                        text.text.to_lowercase().contains("hello")
                    } else {
                        false
                    }
                })
            });

            if has_hello {
                50 // Simple greeting response
            } else {
                100 // Default response
            }
        }
    }
}

#[async_trait]
impl Provider for MockCompactionProvider {
    async fn stream(
        &self,
        _model_config: &ModelConfig,
        system_prompt: &str,
        messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        // Check if this is a compaction call (message contains "summarize")
        let is_compaction = messages.iter().any(|msg| {
            msg.content.iter().any(|content| {
                if let MessageContent::Text(text) = content {
                    text.text.to_lowercase().contains("summarize")
                } else {
                    false
                }
            })
        });

        // Calculate realistic token counts based on actual content
        let input_tokens = self.calculate_input_tokens(system_prompt, messages);
        let output_tokens = self.calculate_output_tokens(is_compaction, messages);

        // Simulate context limit: if input > 20k tokens and we haven't compacted yet, fail
        const CONTEXT_LIMIT: i32 = 20000;
        if !is_compaction
            && input_tokens > CONTEXT_LIMIT
            && !self.has_compacted.load(Ordering::SeqCst)
        {
            return Err(ProviderError::ContextLengthExceeded(format!(
                "Context limit exceeded: {} > {}",
                input_tokens, CONTEXT_LIMIT
            )));
        }

        // If this is a compaction call, mark that we've compacted
        if is_compaction {
            self.has_compacted.store(true, Ordering::SeqCst);
        }

        // Generate response
        let message = if is_compaction {
            Message::assistant().with_text("<mock summary of conversation>")
        } else {
            let response_text = if messages.iter().any(|msg| {
                msg.content.iter().any(|c| {
                    if let MessageContent::Text(text) = c {
                        text.text.to_lowercase().contains("hello")
                    } else {
                        false
                    }
                })
            }) {
                "Hi there! How can I help you?"
            } else {
                "This is a mock response."
            };
            Message::assistant().with_text(response_text)
        };

        let usage = ProviderUsage::new(
            "mock-model".to_string(),
            Usage::new(
                Some(input_tokens),
                Some(output_tokens),
                Some(input_tokens + output_tokens),
            ),
        );

        Ok(stream_from_single_message(message, usage))
    }

    fn get_name(&self) -> &str {
        "mock-compaction"
    }
}

impl goose::providers::base::ProviderDescriptor for MockCompactionProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            name: "mock".to_string(),
            display_name: "Mock Compaction Provider".to_string(),
            description: "Mock provider for compaction testing".to_string(),
            default_model: "mock-model".to_string(),
            known_models: vec![],
            model_doc_link: "".to_string(),
            config_keys: vec![],
            setup_steps: vec![],
            model_selection_hint: None,
            fast_model: None,
        }
    }
}

impl ProviderDef for MockCompactionProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<goose::config::ExtensionConfig>,
        _tls_config: Option<goose::providers::api_client::TlsConfig>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<Self>> {
        Box::pin(async { Ok(Self::new()) })
    }
}

/// Helper: Set up a test session with initial messages and token counts
async fn setup_test_session(
    agent: &Agent,
    temp_dir: &TempDir,
    session_name: &str,
    messages: Vec<Message>,
) -> Result<Session> {
    let session = agent
        .config
        .session_manager
        .create_session(
            temp_dir.path().to_path_buf(),
            session_name.to_string(),
            SessionType::Hidden,
            GooseMode::default(),
        )
        .await?;

    let conversation = Conversation::new_unvalidated(messages);
    agent
        .config
        .session_manager
        .replace_conversation(&session.id, &conversation)
        .await?;

    // Set initial token counts
    agent
        .config
        .session_manager
        .update(&session.id)
        .usage(Usage::new(Some(600), Some(400), Some(1000)))
        .accumulated_usage(Usage::new(Some(600), Some(400), Some(1000)))
        .apply()
        .await?;

    Ok(session)
}

/// Helper: Assert conversation has been compacted with proper message visibility
fn assert_conversation_compacted(conversation: &Conversation) {
    let messages = conversation.messages();
    assert!(!messages.is_empty(), "Conversation should not be empty");

    // Find the summary message (contains "mock summary")
    let summary_index = messages
        .iter()
        .position(|msg| {
            msg.content.iter().any(|content| {
                if let MessageContent::Text(text) = content {
                    text.text.contains("mock summary")
                } else {
                    false
                }
            })
        })
        .expect("Conversation should contain the summary message");

    let summary_msg = &messages[summary_index];

    // Assert summary message visibility
    assert!(
        summary_msg.is_agent_visible(),
        "Summary message should be agent visible"
    );
    assert!(
        !summary_msg.is_user_visible(),
        "Summary message should NOT be user visible"
    );

    // Check messages BEFORE the summary (the compacted original messages)
    // These should be made agent-invisible
    for (idx, msg) in messages.iter().enumerate() {
        if idx < summary_index {
            // Old messages before summary: agent can't see them
            assert!(
                !msg.is_agent_visible(),
                "Message before summary at index {} should be agent-invisible",
                idx
            );
        }
    }

    // Check for continuation message after summary
    // (Should exist and be agent-only)
    if summary_index + 1 < messages.len() {
        let continuation_msg = &messages[summary_index + 1];
        // Continuation message should contain instructions about not mentioning summary
        let has_continuation_text = continuation_msg.content.iter().any(|content| {
            if let MessageContent::Text(text) = content {
                text.text.contains("previous message contains a summary")
                    || text.text.contains("summarization occurred")
            } else {
                false
            }
        });

        if has_continuation_text {
            assert!(
                continuation_msg.is_agent_visible(),
                "Continuation message should be agent visible"
            );
            assert!(
                !continuation_msg.is_user_visible(),
                "Continuation message should NOT be user visible"
            );
        }
    }

    // Any messages AFTER the continuation (e.g., preserved recent user message)
    // should be fully visible to both agent and user
    let continuation_end = summary_index + 2;
    for (idx, msg) in messages.iter().enumerate() {
        if idx >= continuation_end {
            assert!(
                msg.is_agent_visible() && msg.is_user_visible(),
                "Message after compaction at index {} should be fully visible",
                idx
            );
        }
    }
}

#[tokio::test]
#[serial_test::serial]
async fn test_manual_compaction_updates_token_counts_and_conversation() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let agent = Agent::new();

    // Setup session with initial messages
    // Each message ~100 tokens, so 4 messages = ~400 tokens in conversation
    let messages = vec![
        Message::user().with_text("Hello, can you help me with something?"),
        Message::assistant().with_text("Of course! What do you need help with?"),
        Message::user().with_text("I need to understand how compaction works."),
        Message::assistant()
            .with_text("Compaction is a process that summarizes conversation history."),
    ];

    let session = setup_test_session(&agent, &temp_dir, "manual-compact-test", messages).await?;

    // Setup mock provider
    let provider = Arc::new(MockCompactionProvider::new());
    agent
        .update_provider(provider, ModelConfig::new("mock-model"), &session.id)
        .await?;

    // Execute manual compaction
    let result = agent.execute_command("/compact", &session.id).await?;
    assert!(result.is_some(), "Compaction should return a result");

    // Verify token counts
    let updated_session = agent
        .config
        .session_manager
        .get_session(&session.id, true)
        .await?;

    // Expected token calculation for compaction:
    // During compaction, the 4 messages are embedded in the system prompt template
    // - Input: system prompt with embedded conversation + "Please summarize" message
    // - Output: summary (200 tokens)
    //
    // From mock provider calculation:
    // - System prompt (with 4 embedded messages): varies based on template + content
    // - Single "summarize" message: 100 tokens
    // - Total input observed: ~6100 tokens
    //
    // After compaction:
    // - current input_tokens = summary output (200) - the new compact context
    // - current output_tokens = None (compaction doesn't produce new output)
    // - current total_tokens = 200
    // - accumulated_total = initial (1000) + compaction cost
    let expected_summary_output = 200; // compact summary

    // Verify the key invariants after manual compaction:
    // After compaction, the current context is ONLY the summary (200 tokens)
    // This is the new agent-visible input context
    assert_eq!(
        updated_session.usage.input_tokens,
        Some(expected_summary_output),
        "Input tokens should be exactly the summary output (200 tokens)"
    );
    assert_eq!(
        updated_session.usage.output_tokens, None,
        "Output tokens should be None after compaction (no new assistant output)"
    );
    assert_eq!(
        updated_session.usage.total_tokens,
        Some(expected_summary_output),
        "Total should equal input (200 tokens) after compaction"
    );

    // Accumulated tokens increased by the compaction cost
    // Initial: 1000
    // Compaction input: ~6400 (system 6000 + 4 messages ~400)
    // Compaction output: 200
    // Expected accumulated: 1000 + 6400 + 200 = 7600
    let accumulated = updated_session.accumulated_usage.total_tokens.unwrap();
    assert!(
        (7300..=7900).contains(&accumulated),
        "Accumulated should be ~7600 (1000 initial + 6400 input + 200 output). Got: {}",
        accumulated
    );

    // Verify conversation has been compacted
    let compacted_conversation = updated_session
        .conversation
        .expect("Session should have conversation");

    assert_conversation_compacted(&compacted_conversation);

    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_auto_compaction_during_reply() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let agent = Agent::new();

    // Setup session with many messages to have substantial context
    // 20 exchanges = 40 messages * 100 tokens = ~4000 tokens in conversation
    let mut messages = vec![];
    for i in 0..20 {
        messages.push(Message::user().with_text(format!("User message {}", i)));
        messages.push(Message::assistant().with_text(format!("Assistant response {}", i)));
    }

    let session = setup_test_session(&agent, &temp_dir, "auto-compact-test", messages).await?;

    // Capture initial context size before triggering reply
    // Should be: system (6000) + 40 messages (4000) = ~10000 tokens
    let initial_session = agent
        .config
        .session_manager
        .get_session(&session.id, true)
        .await?;
    let initial_input_tokens = initial_session.usage.input_tokens.unwrap_or(0);

    // Setup mock provider (no context limit enforcement)
    let provider = Arc::new(MockCompactionProvider::new());
    agent
        .update_provider(provider, ModelConfig::new("mock-model"), &session.id)
        .await?;

    // Trigger a reply
    // Expected tokens for reply:
    // - Input: system (6000) + 40 messages (4000) + new user message (100) = 10100 tokens
    // - Output: regular response (100 tokens)
    let user_message = Message::user().with_text("Tell me more about compaction");

    let session_config = SessionConfig {
        id: session.id.clone(),
        schedule_id: None,
        max_turns: None,
        retry_config: None,
    };

    let reply_stream = agent.reply(user_message, session_config, None).await?;
    tokio::pin!(reply_stream);

    // Track compaction and context size changes
    let mut compaction_occurred = false;
    let mut input_tokens_after_compaction: Option<i32> = None;

    while let Some(event_result) = reply_stream.next().await {
        match event_result {
            Ok(AgentEvent::HistoryReplaced(_)) => {
                compaction_occurred = true;

                // Capture the input tokens immediately after compaction
                let session_after_compact = agent
                    .config
                    .session_manager
                    .get_session(&session.id, true)
                    .await?;
                input_tokens_after_compaction = session_after_compact.usage.input_tokens;
            }
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }

    let updated_session = agent
        .config
        .session_manager
        .get_session(&session.id, true)
        .await?;

    if compaction_occurred {
        // Verify that current input context decreased after compaction
        let tokens_after =
            input_tokens_after_compaction.expect("Should have captured tokens after compaction");

        // Before compaction: system (6000) + 40 messages (4000) = 10,000 tokens
        // After compaction: only the summary (200 tokens) - this becomes the new input
        assert!(
            tokens_after < initial_input_tokens,
            "Input tokens should decrease after compaction. Before: {}, After: {}",
            initial_input_tokens,
            tokens_after
        );

        // After compaction, input should be exactly the summary: 200 tokens
        assert_eq!(
            tokens_after, 200,
            "Input tokens after compaction should be exactly 200 (summary). Got: {}",
            tokens_after
        );

        // After the subsequent reply, the current window includes:
        // - system (6000) + summary (200) + new user message (100) + reply (100) = 6400
        let final_input = updated_session.usage.input_tokens.unwrap();
        let final_output = updated_session.usage.output_tokens.unwrap();
        let final_total = updated_session.usage.total_tokens.unwrap();

        assert!(
            final_input >= 6000,
            "Final input should include at least system prompt (6000). Got: {}",
            final_input
        );
        assert_eq!(
            final_output, 100,
            "Final output should be 100 tokens (default response). Got: {}",
            final_output
        );
        assert_eq!(
            final_total,
            final_input + final_output,
            "Final total should equal input + output"
        );

        // Accumulated tokens should include:
        // - Initial: 1000
        // - Compaction: ~10,400 input + 200 output = 10,600
        // - Reply: ~6,300 input + 100 output = 6,400
        // Total: 1000 + 10,600 + 6,400 = 18,000
        let accumulated = updated_session.accumulated_usage.total_tokens.unwrap();
        assert!(
            (17000..=19000).contains(&accumulated),
            "Accumulated should be ~18,000 (initial + compaction + reply). Got: {}",
            accumulated
        );
    } else {
        // If no compaction, accumulated should include reply cost
        // - Initial: 1000
        // - Reply: system (6000) + 40 messages (4000) + new message (100) = 10,100 input
        // - Reply output: 100
        // Total: 1000 + 10,100 + 100 = 11,200
        let accumulated = updated_session.accumulated_usage.total_tokens.unwrap();
        assert!(
            (11000..=11500).contains(&accumulated),
            "Accumulated should be ~11,200 (initial + reply). Got: {}",
            accumulated
        );

        // Current window should be: 10,100 input + 100 output = 10,200
        let final_input = updated_session.usage.input_tokens.unwrap();
        let final_output = updated_session.usage.output_tokens.unwrap();

        assert!(
            (10000..=10500).contains(&final_input),
            "Input should be ~10,100. Got: {}",
            final_input
        );
        assert_eq!(
            final_output, 100,
            "Output should be 100. Got: {}",
            final_output
        );
    }

    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_context_limit_recovery_compaction() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let agent = Agent::new();

    // Setup session with messages that will push context over the limit
    // Each message = 100 tokens, but we'll add a large one
    let messages = vec![
        Message::user().with_text("Hello"),
        Message::assistant().with_text("Hi there"),
        Message::user().with_text("Can you process this long_tool_call result?"),
        Message::assistant().with_text("Processing..."),
    ];
    // Token calculation:
    // - 3 regular messages: 300 tokens
    // - 1 message with "long_tool_call": 100 + 15000 = 15100 tokens
    // - Total conversation: ~15400 tokens
    // - With system prompt (6000): 21400 tokens

    let session = setup_test_session(&agent, &temp_dir, "context-limit-test", messages).await?;

    // Setup mock provider with context limit of 20000 tokens
    // Initial context (6000 system + 15400 messages = 21400) exceeds this limit
    let provider = Arc::new(MockCompactionProvider::new());
    agent
        .update_provider(provider, ModelConfig::new("mock-model"), &session.id)
        .await?;

    // Try to send a message - should trigger context limit, then recover via compaction
    let session_config = SessionConfig {
        id: session.id.clone(),
        schedule_id: None,
        max_turns: None,
        retry_config: None,
    };

    let reply_stream = agent
        .reply(
            Message::user().with_text("Tell me more"),
            session_config,
            None,
        )
        .await?;
    tokio::pin!(reply_stream);

    // Track compaction and context size changes
    let mut compaction_occurred = false;
    let mut got_response = false;
    let mut input_tokens_after_compaction: Option<i32> = None;

    while let Some(event_result) = reply_stream.next().await {
        match event_result {
            Ok(AgentEvent::HistoryReplaced(_)) => {
                compaction_occurred = true;

                // Capture the input tokens immediately after compaction
                let session_after_compact = agent
                    .config
                    .session_manager
                    .get_session(&session.id, true)
                    .await?;
                input_tokens_after_compaction = session_after_compact.usage.input_tokens;
            }
            Ok(AgentEvent::Message(msg)) => {
                // Check if we got a real response (not just a notification)
                if msg
                    .content
                    .iter()
                    .any(|c| matches!(c, MessageContent::Text(_)))
                {
                    got_response = true;
                }
            }
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }

    // Verify recovery occurred
    assert!(
        compaction_occurred,
        "Compaction should have occurred due to context limit (>20000 tokens)"
    );
    assert!(
        got_response,
        "Should have received a response after recovery"
    );

    // Verify token counts
    let updated_session = agent
        .config
        .session_manager
        .get_session(&session.id, true)
        .await?;

    // Expected token flow:
    // 1. Initial attempt: >20000 tokens -> Context limit exceeded
    // 2. Compaction triggered:
    //    - Input: system prompt + messages (including long_tool_call with 15k tokens)
    //    - Output: 200 tokens (summary)
    //    - New context size: 200 tokens
    // 3. Retry with compacted context:
    //    - Input: system prompt + summary (200) + new message
    //    - Output: 100 tokens (response)

    // Verify that current input context is dramatically reduced after compaction
    let tokens_after =
        input_tokens_after_compaction.expect("Should have captured tokens after compaction");

    // After compaction, the input context should be ONLY the summary: 200 tokens
    // Before: system (6000) + long_tool_call messages (~15,400) = 21,400 (exceeded limit!)
    // After: only summary (200 tokens)
    assert_eq!(
        tokens_after, 200,
        "Input tokens after compaction should be exactly 200 (summary only). Got: {}",
        tokens_after
    );

    // The compacted context is now well under the 20k limit
    assert!(
        tokens_after < 20000,
        "Compacted context should be under 20k limit. Got: {}",
        tokens_after
    );

    // Check the final token state after recovery
    // Note: The session state reflects the RETRY call (after compaction),
    // which only sees agent-visible messages (summary + continuation + user message)
    let final_input = updated_session.usage.input_tokens.unwrap();
    let final_output = updated_session.usage.output_tokens;
    let final_total = updated_session.usage.total_tokens.unwrap();

    // After compaction, the retry only sees agent-visible messages:
    // Input: system (6000) + summary (~100) + continuation (~100) + user message (~100) = ~6300
    // Output: 200 (mock detects "summarized" in continuation as compaction)
    // Total: ~6500
    assert!(
        (6000..=6600).contains(&final_input),
        "Final input should reflect retry with agent-visible messages (~6300). Got: {}",
        final_input
    );

    assert_eq!(
        final_output,
        Some(200),
        "Final output should be 200 (mock detects continuation as compaction). Got: {:?}",
        final_output
    );

    assert_eq!(
        final_total,
        final_input + final_output.unwrap(),
        "Final total should equal input + output"
    );

    // Accumulated tokens should include all operations:
    // - Initial: 1000
    // - Compaction: ~6400 input (mock uses system_prompt.len()/4) + 200 output = ~6600
    // - Reply: ~6500 input + 200 output = ~6700
    // Total: 1000 + 6600 + 6700 = ~14300
    let accumulated = updated_session.accumulated_usage.total_tokens.unwrap();
    assert!(
        (13000..=16000).contains(&accumulated),
        "Accumulated should be ~14300 (initial + compaction + reply). Got: {}",
        accumulated
    );

    // Verify that the conversation was compacted
    let updated_conversation = updated_session
        .conversation
        .expect("Session should have conversation");
    assert_conversation_compacted(&updated_conversation);

    Ok(())
}

/// K4 both directions: keep-tail preserves the last turns VERBATIM through compaction, and K=0 is
/// byte-identical to the pre-K4 behavior. The measured defect this pins: a long tool loop's tail is
/// all tool content, so nothing survived summarization verbatim and a 40-turn swarm sink lost the
/// exact bytes of the file it was editing.
#[tokio::test]
#[serial_test::serial]
async fn keep_tail_survives_compaction_verbatim_and_zero_is_identity() -> Result<()> {
    use goose::context_mgmt::compact_messages_with_tail;
    use rmcp::model::{AnnotateAble, CallToolRequestParams, CallToolResult, RawContent};
    use rmcp::object;

    let provider = MockCompactionProvider::new();
    let model_config = ModelConfig::new("mock");
    // A tool-heavy conversation: user ask, then two request/response pairs, ending on a response —
    // the exact shape whose tail the old compactor could never preserve.
    let convo = Conversation::new_unvalidated(vec![
        Message::user().with_text("build the thing"),
        Message::assistant().with_tool_request(
            "1",
            Ok(CallToolRequestParams::new("write").with_arguments(object!({"path": "a.py"}))),
        ),
        Message::user().with_tool_response(
            "1",
            Ok(CallToolResult::success(vec![RawContent::text(
                "wrote a.py: def f(): return 41",
            )
            .no_annotation()])),
        ),
        Message::assistant().with_tool_request(
            "2",
            Ok(CallToolRequestParams::new("shell").with_arguments(object!({"cmd": "pytest"}))),
        ),
        Message::user().with_tool_response(
            "2",
            Ok(CallToolResult::success(vec![RawContent::text(
                "1 failed: expected 42 got 41",
            )
            .no_annotation()])),
        ),
    ]);

    // Direction 1 — K=3: the tail returns verbatim among the agent-visible messages, and the kept
    // tail never OPENS on an orphan tool response (the cut extended back to include its request).
    let (compacted, _) =
        compact_messages_with_tail(&provider, &model_config, "s1", &convo, false, 3).await?;
    let visible: Vec<_> = compacted
        .messages()
        .iter()
        .filter(|m| m.is_agent_visible())
        .cloned()
        .collect();
    let visible_text = visible
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolResponse(r) => Some(format!("{:?}", r)),
            MessageContent::Text(t) => Some(t.text.clone()),
            MessageContent::ToolRequest(q) => Some(format!("{:?}", q)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        visible_text.contains("expected 42 got 41"),
        "the tail's exact bytes must survive: {visible_text}"
    );
    // No agent-visible tool RESPONSE may precede its request: walk visibles, track open requests.
    let mut open: std::collections::HashSet<String> = Default::default();
    for m in &visible {
        for c in &m.content {
            match c {
                MessageContent::ToolRequest(r) => {
                    open.insert(r.id.clone());
                }
                MessageContent::ToolResponse(r) => {
                    assert!(
                        open.contains(&r.id),
                        "orphan tool response {} in the kept tail",
                        r.id
                    );
                }
                _ => {}
            }
        }
    }

    // Direction 2 — K=0: identical to the legacy call (same visible set as compact_messages'
    // pre-K4 shape: summary + continuation + preserved user text, and NO verbatim tool tail).
    let (legacy, _) =
        compact_messages_with_tail(&provider, &model_config, "s2", &convo, false, 0).await?;
    let legacy_text = legacy
        .messages()
        .iter()
        .filter(|m| m.is_agent_visible())
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::Text(t) => Some(t.text.clone()),
            MessageContent::ToolResponse(r) => Some(format!("{:?}", r)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !legacy_text.contains("expected 42 got 41"),
        "K=0 must not keep the tool tail (byte-identical legacy): {legacy_text}"
    );
    Ok(())
}

struct ProactiveCompactionProvider {
    main_calls: AtomicUsize,
    observed_context_limits: Mutex<Vec<Option<usize>>>,
    call_sequence: Mutex<Vec<&'static str>>,
    first_main_system_prompt: Mutex<Option<String>>,
    second_turn_messages: Mutex<Option<Vec<Message>>>,
}

impl ProactiveCompactionProvider {
    fn new() -> Self {
        Self {
            main_calls: AtomicUsize::new(0),
            observed_context_limits: Mutex::new(Vec::new()),
            call_sequence: Mutex::new(Vec::new()),
            first_main_system_prompt: Mutex::new(None),
            second_turn_messages: Mutex::new(None),
        }
    }

    fn is_compaction_call(messages: &[Message]) -> bool {
        messages.len() == 1
            && messages[0].content.iter().any(|content| {
                matches!(
                    content,
                    MessageContent::Text(text)
                        if text.text == "Please summarize the conversation history provided in the system prompt."
                )
            })
    }

    fn response(message: Message, usage: Usage) -> MessageStream {
        stream_from_single_message(
            message,
            ProviderUsage::new("forced-low-cap-model".to_string(), usage),
        )
    }
}

#[async_trait]
impl Provider for ProactiveCompactionProvider {
    async fn stream(
        &self,
        model_config: &ModelConfig,
        system_prompt: &str,
        messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        self.observed_context_limits
            .lock()
            .unwrap()
            .push(model_config.context_limit);

        if Self::is_compaction_call(messages) {
            self.call_sequence.lock().unwrap().push("compact");
            return Ok(Self::response(
                Message::assistant().with_text("forced proactive summary"),
                Usage::new(Some(700), Some(40), Some(740)),
            ));
        }

        match self.main_calls.fetch_add(1, Ordering::SeqCst) {
            0 => {
                self.call_sequence.lock().unwrap().push("main-1");
                *self.first_main_system_prompt.lock().unwrap() = Some(system_prompt.to_string());
                let request = CallToolRequestParams::new("developer__shell")
                    .with_arguments(object!({"command": "printf compaction-tail-marker"}));
                Ok(Self::response(
                    Message::assistant().with_tool_request("continuity-call", Ok(request)),
                    Usage::new(Some(850), Some(50), Some(900)),
                ))
            }
            1 => {
                self.call_sequence.lock().unwrap().push("main-2");
                *self.second_turn_messages.lock().unwrap() = Some(messages.to_vec());
                let system_prompt_preserved =
                    self.first_main_system_prompt.lock().unwrap().as_deref() == Some(system_prompt);
                let directive_preserved = messages.iter().any(|message| {
                    message.content.iter().any(|content| {
                        matches!(content, MessageContent::Text(text) if text.text == "Run the marker command, then finish.")
                    })
                });
                let successful_marker_preserved = messages.iter().any(|message| {
                    message.content.iter().any(|content| {
                        let MessageContent::ToolResponse(response) = content else {
                            return false;
                        };
                        if response.id != "continuity-call" {
                            return false;
                        }
                        let Ok(result) = &response.tool_result else {
                            return false;
                        };
                        !result.is_error.unwrap_or(false)
                            && result.content.iter().any(|content| {
                                matches!(&content.raw, RawContent::Text(text) if text.text.contains("compaction-tail-marker"))
                            })
                    })
                });
                if !(system_prompt_preserved && directive_preserved && successful_marker_preserved)
                {
                    return Err(ProviderError::ExecutionError(
                        "post-compaction turn lost required instruction or successful tool state"
                            .to_string(),
                    ));
                }
                Ok(Self::response(
                    Message::assistant().with_text("completed after proactive compaction"),
                    Usage::new(Some(300), Some(30), Some(330)),
                ))
            }
            call => Err(ProviderError::ExecutionError(format!(
                "unexpected main provider call {call}"
            ))),
        }
    }

    fn get_name(&self) -> &str {
        "forced-low-cap-provider"
    }
}

/// The swarm depends on the per-turn proactive hook, not just reactive recovery after a provider
/// rejects an oversized request. Force the first tool turn over a small ModelConfig limit and prove
/// that compaction happens before turn two while the exact request/response tail remains usable.
#[tokio::test]
#[serial_test::serial]
async fn forced_low_cap_compacts_between_tool_turns_and_preserves_exact_tail() -> Result<()> {
    let _env = env_lock::lock_env(
        [
            ("GOOSE_AUTO_COMPACT_THRESHOLD", Some("0.8")),
            ("GOOSE_LOCAL_CONTEXT_CAP", Some("0")),
            ("GOOSE_COMPACT_KEEP_TAIL", Some("3")),
            ("GOOSE_FAST_MODEL", Some("")),
            ("GOOSE_TOOL_PAIR_SUMMARIZATION", Some("false")),
        ]
        .into_iter(),
    );

    let temp_dir = TempDir::new()?;
    let session_manager = Arc::new(SessionManager::new(temp_dir.path().join("sessions")));
    let agent = Agent::with_config(AgentConfig::new(
        Arc::clone(&session_manager),
        Arc::new(PermissionManager::new(temp_dir.path().join("config"))),
        None,
        GooseMode::Auto,
        true,
        GoosePlatform::GooseCli,
    ));
    let session = session_manager
        .create_session(
            temp_dir.path().to_path_buf(),
            "forced-low-cap".to_string(),
            SessionType::Hidden,
            GooseMode::Auto,
        )
        .await?;
    let initial_usage = Usage::new(Some(80), Some(20), Some(100));
    session_manager
        .update(&session.id)
        .usage(initial_usage)
        .accumulated_usage(initial_usage)
        .apply()
        .await?;

    agent
        .add_extension(
            ExtensionConfig::Builtin {
                name: "developer".to_string(),
                description: String::new(),
                display_name: Some("Developer".to_string()),
                timeout: None,
                bundled: Some(true),
                available_tools: vec!["shell".to_string()],
            },
            &session.id,
        )
        .await?;

    const FORCED_CONTEXT_LIMIT: usize = 1_000;
    let provider = Arc::new(ProactiveCompactionProvider::new());
    agent
        .update_provider(
            provider.clone(),
            ModelConfig::new("forced-low-cap-model").with_context_limit(Some(FORCED_CONTEXT_LIMIT)),
            &session.id,
        )
        .await?;

    let reply = agent
        .reply(
            Message::user().with_text("Run the marker command, then finish."),
            SessionConfig {
                id: session.id.clone(),
                schedule_id: None,
                max_turns: Some(3),
                retry_config: None,
            },
            None,
        )
        .await?;
    tokio::pin!(reply);

    let mut history_replacements = 0;
    let mut original_request = None;
    let mut original_response = None;
    let mut final_response_seen = false;
    while let Some(event) = reply.next().await {
        match event? {
            AgentEvent::HistoryReplaced(_) => history_replacements += 1,
            AgentEvent::Message(message) => {
                if message.content.iter().any(|content| {
                    matches!(content, MessageContent::ToolRequest(request) if request.id == "continuity-call")
                }) {
                    original_request = Some(message.clone());
                }
                if message.content.iter().any(|content| {
                    matches!(content, MessageContent::ToolResponse(response) if response.id == "continuity-call")
                }) {
                    original_response = Some(message.clone());
                }
                if message
                    .as_concat_text()
                    .contains("completed after proactive compaction")
                {
                    final_response_seen = true;
                }
            }
            AgentEvent::McpNotification(_) | AgentEvent::Usage(_) => {}
        }
    }

    assert_eq!(
        history_replacements, 1,
        "the first 900-token tool turn must cross 80% of the forced 1,000-token limit exactly once"
    );
    assert!(
        final_response_seen,
        "the agent must continue to a successful final response after compaction"
    );
    assert_eq!(provider.main_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        *provider.call_sequence.lock().unwrap(),
        vec!["main-1", "compact", "main-2"],
        "compaction must occur between the two provider turns"
    );

    let observed_limits = provider.observed_context_limits.lock().unwrap().clone();
    assert_eq!(observed_limits.len(), 3);
    assert!(
        observed_limits
            .iter()
            .all(|limit| *limit == Some(FORCED_CONTEXT_LIMIT)),
        "the forced ModelConfig limit must survive session persistence and the compaction call: {observed_limits:?}"
    );

    let second_turn = provider
        .second_turn_messages
        .lock()
        .unwrap()
        .clone()
        .expect("the post-compaction provider turn must be captured");
    let visible: Vec<&Message> = second_turn
        .iter()
        .filter(|message| message.is_agent_visible())
        .collect();
    assert!(
        visible
            .iter()
            .any(|message| message.as_concat_text() == "forced proactive summary"),
        "turn two must receive the compacted summary"
    );

    let request_index = visible
        .iter()
        .position(|message| {
            message.content.iter().any(|content| {
                matches!(content, MessageContent::ToolRequest(request) if request.id == "continuity-call")
            })
        })
        .expect("the exact kept-tail tool request must reach turn two");
    let response_index = visible
        .iter()
        .position(|message| {
            message.content.iter().any(|content| {
                matches!(content, MessageContent::ToolResponse(response) if response.id == "continuity-call")
            })
        })
        .expect("the exact kept-tail tool response must reach turn two");
    assert!(
        request_index < response_index,
        "the kept tail may not put a tool response before its request"
    );

    let original_request = original_request.expect("the first turn must emit its tool request");
    let original_response = original_response.expect("the tool must emit its response");
    assert_eq!(
        visible[request_index].content, original_request.content,
        "K=3 must preserve the tool request content exactly"
    );
    assert_eq!(
        visible[response_index].content, original_response.content,
        "K=3 must preserve the tool response content exactly"
    );

    let mut open_requests = std::collections::HashSet::new();
    for message in visible {
        for content in &message.content {
            match content {
                MessageContent::ToolRequest(request) => {
                    open_requests.insert(request.id.clone());
                }
                MessageContent::ToolResponse(response) => assert!(
                    open_requests.contains(&response.id),
                    "orphan tool response {} reached the post-compaction turn",
                    response.id
                ),
                _ => {}
            }
        }
    }

    Ok(())
}
