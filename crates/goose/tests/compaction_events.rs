//! Round 5: the benchmark counts flagged summaries from goose's JSON log. Its own test binary,
//! with one test: tracing caches each callsite's interest for the whole process, and the lib's
//! tests, run in parallel, hit these callsites with no subscriber (measured: the same test passed
//! alone and failed 3 of 3 among them).

use async_trait::async_trait;
use goose::context_mgmt::pillars::LedgerRead;
use goose::context_mgmt::{
    compact_chat, ChatCompacted, ChatCompaction, ChatRequestFrame, CompactionTrigger,
    SummaryRequest,
};
use goose::conversation::message::Message;
use goose::conversation::Conversation;
use goose::providers::base::{stream_from_single_message, MessageStream, Provider};
use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use rmcp::model::Tool;
use std::sync::{Arc, Mutex};

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

struct Answers(&'static str);

#[async_trait]
impl Provider for Answers {
    fn get_name(&self) -> &str {
        "answers"
    }

    async fn stream(
        &self,
        _model_config: &ModelConfig,
        _system: &str,
        _messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        Ok(stream_from_single_message(
            Message::assistant().with_text(self.0),
            ProviderUsage::new(
                "answers".to_string(),
                Usage::new(Some(90), Some(10), Some(100)),
            ),
        ))
    }
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn compact(answer: &'static str, chat: Vec<Message>) -> String {
    let frame = SummaryRequest::ExtendsChat(ChatRequestFrame {
        system_prompt: "You are goose.".to_string(),
        tools: vec![Tool::new(
            "developer__shell",
            "Run a shell command",
            rmcp::object!({"type": "object", "properties": {"command": {"type": "string"}}}),
        )],
    });
    let inputs = ChatCompaction {
        trigger: CompactionTrigger::Auto {
            used: Some(143_000),
            limit: Some(178_176),
            threshold: 0.8,
        },
        note: None,
        may_ask: false,
        pins: Vec::new(),
        goal: None,
        ledger: LedgerRead::Entries(Vec::new()),
        context_limit: None,
        working_dir: std::path::PathBuf::from("/Users/someone"),
        progress: None,
        budget: None,
    };
    let ChatCompacted::Compacted { conversation, .. } = compact_chat(
        &Answers(answer),
        &ModelConfig::new("answers"),
        "s",
        &Conversation::new_unvalidated(chat),
        false,
        &frame,
        &inputs,
    )
    .await
    .unwrap() else {
        panic!("compacted");
    };
    conversation.agent_visible_messages()[0].as_concat_text()
}

/// GRADES4's five bold lines (gate-passing, thin): one `compaction_summary_flagged`. A summary
/// claiming a share of the window the conversation never wrote: one `compaction_numbers_unverified`.
#[tokio::test]
async fn a_flagged_summary_logs_an_event_the_benchmark_counts() {
    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let bold = "**Thinking:** I should continue.\n\n**Plan:** look at files.\n\n**Step one:** \
                run ls.\n\n**Step two:** read output.\n\n**Then:** carry on.\n";
    let stored = compact(
        bold,
        vec![
            Message::user().with_text("build the payments console"),
            Message::assistant().with_text(
                "The vendor sync stored the payments and the ledger answered. ".repeat(800),
            ),
            Message::user().with_text("go on"),
        ],
    )
    .await;
    assert!(
        stored.starts_with("[goose could not check this summary"),
        "{stored}"
    );

    let summary = "## Where we are\nThe console runs; context ~60% of 409,600.\n\
                   ## What each file does\n`app/store.py` keeps the ledger.\n\
                   ## Decisions and reasons\nThe canvas is 480 px tall.\n\
                   ## Still to do\nWrite DECISIONS.md.\n\
                   ## Next step\nRun the final checks.\n";
    let stored = compact(
        summary,
        vec![
            Message::user()
                .with_text("context: 239,105 of 409,600 tokens used (58%); span >=60% of 480 px"),
            Message::assistant().with_text("noted"),
            Message::user().with_text("go on"),
        ],
    )
    .await;
    assert!(stored.contains("60% of 409,600 (unverified)"), "{stored}");

    let logs = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    for event in [
        "compaction_summary_flagged",
        "compaction_numbers_unverified",
    ] {
        assert_eq!(
            logs.matches(&format!("\"event\":\"{event}\"")).count(),
            1,
            "{event}: {logs}"
        );
    }
}
