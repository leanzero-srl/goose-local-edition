use crate::conversation::message::{
    audience_includes, model_visible_content, model_visible_texts, Message, MessageContent,
};
use crate::conversation::message::{ActionRequiredData, MessageMetadata};
use crate::conversation::{fix_conversation, merge_consecutive_messages, Conversation};
use crate::prompt_template::render_template;
use crate::providers::base::Provider;
#[cfg(test)]
use crate::providers::base::{stream_from_single_message, MessageStream};
use crate::{config::Config, token_counter::create_token_counter};
use anyhow::Result;
use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use indoc::indoc;
use rmcp::model::{Role, Tool};
use serde::Serialize;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::info;
use tracing::log::warn;

pub mod acp;
pub mod context_line;
pub mod pillars;
pub mod state;

use pillars::{KeptSources, LedgerRead, NoteVerdict, Pillars};

pub const DEFAULT_COMPACTION_THRESHOLD: f64 = 0.8;

const TOOLCALL_SUMMARIZATION_BATCH_SIZE: usize = 10;

fn tool_pair_summarization_enabled() -> bool {
    Config::global()
        .get_param::<bool>("GOOSE_TOOL_PAIR_SUMMARIZATION")
        .unwrap_or(true)
}

const CONVERSATION_CONTINUATION_TEXT: &str =
    "Your context was compacted. The previous message contains a summary of the conversation so far.
Do not mention that you read a summary or that conversation summarization occurred.
Just continue the conversation naturally based on the summarized context.";

const TOOL_LOOP_CONTINUATION_TEXT: &str =
    "Your context was compacted. The previous message contains a summary of the conversation so far.
Do not mention that you read a summary or that conversation summarization occurred.
Continue calling tools as necessary to complete the task.";

const MANUAL_COMPACT_CONTINUATION_TEXT: &str =
    "Your context was compacted at the user's request. The previous message contains a summary of the conversation so far.
Do not mention that you read a summary or that conversation summarization occurred.
Just continue the conversation naturally based on the summarized context.";

/// `compaction.md`'s context: the conversation as text for a [`SummaryRequest::Transcript`], `None`
/// for a [`SummaryRequest::ExtendsChat`], whose conversation is the request's own messages — and
/// then `chat`, the instruction's facts about THIS chat.
#[derive(Serialize)]
struct SummarizeContext {
    messages: Option<String>,
    chat: Option<ChatInstruction>,
}

/// The sections the model writes (Q-357: P6 and P7 — what code cannot know). The card counts them as
/// the summary's parts while it streams.
pub const WRITTEN_PARTS: [(&str, &str); 3] = [
    (
        "Where we are",
        "What is done and what is in progress at this moment — the files, commands and results \
         (counts, test outcomes) as they now stand.",
    ),
    (
        "Next step",
        "The one next step, as the person's latest request asks for it.",
    ),
    (
        "Decisions and reasons",
        "Each decision this conversation made that the ledger does not already hold — the value \
         chosen (a seed, a threshold, a rule, an exception) and why — one line each.",
    ),
];

#[derive(Serialize)]
struct WrittenPart {
    heading: &'static str,
    ask: &'static str,
}

/// What the summary instruction says about this chat: why it is compacted now, what goose keeps
/// itself, the goal, the person's note and how to answer it.
#[derive(Serialize)]
struct ChatInstruction {
    trigger: Option<String>,
    kept: Vec<String>,
    earlier_summary: bool,
    goal: Option<String>,
    note: Option<String>,
    may_ask: bool,
    parts: Vec<WrittenPart>,
}

/// Why a chat is compacted now.
#[derive(Debug, Clone, PartialEq)]
pub enum CompactionTrigger {
    /// The person asked (`/compact`, the meter menu, the Context tab).
    Manual,
    /// The conversation passed the chat's compaction point: `used` of `limit` tokens, each absent
    /// when goose could not measure it.
    Auto {
        used: Option<usize>,
        limit: Option<usize>,
        threshold: f64,
    },
    /// The chat's own request was refused as too long.
    Recovery,
}

/// What the summary has written so far, for the chat's compaction card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritingProgress {
    /// Output tokens streamed so far, reasoning included, by goose's tokenizer; `None` when the
    /// tokenizer could not be built.
    pub written_tokens: Option<u64>,
    /// The section headings written so far.
    pub parts: Vec<String>,
}

pub type CompactionObserver = Arc<dyn Fn(WritingProgress) + Send + Sync>;

/// Q-357: a CHAT's compaction inputs beyond its conversation. The swarm's workers pass none and
/// keep their golden-measured compaction byte for byte.
#[derive(Clone)]
pub struct ChatCompaction {
    pub trigger: CompactionTrigger,
    /// The person's note for this compaction.
    pub note: Option<String>,
    /// The model may stop at a question about the note instead of summarizing: only when the
    /// person is there to answer (a manual compaction not already told to follow the note as
    /// written).
    pub may_ask: bool,
    pub pins: Vec<String>,
    pub goal: Option<String>,
    pub ledger: LedgerRead,
    /// The kept block's share of the window, in chars; `None` when the window is unknown.
    pub kept_budget_chars: Option<usize>,
    pub progress: Option<CompactionObserver>,
}

// ratio: the kept block may take a sixteenth of the window — E2E #3p's model-written summary was
// 8,821 chars (~2.2k tokens) on a 178,176-token window; a sixteenth (11.1k tokens) leaves the
// person's words, the files, errors and ledger five times that room before anything is cut.
pub(crate) const KEPT_WINDOW_SHARE: f64 = 1.0 / 16.0;

/// The kept block's budget in chars for a window of `context_limit` tokens.
pub fn kept_budget_chars(context_limit: Option<usize>) -> Option<usize> {
    context_limit.map(|limit| {
        (limit as f64
            * crate::agents::platform_extensions::recall::CHARS_PER_TOKEN
            * KEPT_WINDOW_SHARE) as usize
    })
}

/// How the person's note fared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteOutcome {
    NoNote,
    Read(NoteVerdict),
    /// The summary was asked for as a transcript (the chat's own request was refused as too long),
    /// whose request carries no note; the note is in the kept block word for word.
    NotSent,
}

pub enum ChatCompacted {
    Compacted {
        conversation: Conversation,
        usage: ProviderUsage,
        note: NoteOutcome,
        /// What goose kept word for word; `None` for a caller with no chat inputs.
        kept: Option<Pillars>,
    },
    /// The model asked about the note instead of summarizing; the conversation is unchanged.
    Asked { question: String },
}

/// What the chat's provider calls carry besides the conversation: the system prompt and the tools
/// exactly as the provider is sent them (after tool disclosure).
#[derive(Debug, Clone)]
pub struct ChatRequestFrame {
    pub system_prompt: String,
    pub tools: Vec<Tool>,
}

/// The shape of the request that asks a model for the summary.
#[derive(Debug, Clone)]
pub enum SummaryRequest {
    /// Q-342: the chat's own next request — its system prompt, its tools, its messages as its
    /// provider calls render them, its model config — with the summary instruction as the one
    /// message after the conversation. A provider that caches prompt prefixes reads the whole
    /// conversation from the entry the chat's last call left and prefills only the instruction.
    /// E2E #3p (3.0.69, 27B split): the chat call before the compaction read 138,665 of 139,503
    /// prompt tokens from cache; the transcript request after it prefilled 97,590 cold for 441 s
    /// before its first token, 678 s in all.
    ExtendsChat(ChatRequestFrame),
    /// The conversation rendered as text inside the system prompt of a request with no tools, sent
    /// with reasoning off. The swarm workers' golden-measured shape, and the shape a compaction
    /// takes when the chat's own request was just refused as too long — extending it cannot fit.
    Transcript,
}

/// Summarizer responses from reasoning models carry Thinking/RedactedThinking
/// blocks. Those must not survive the re-role to `Role::User` — providers such
/// as Bedrock reject user messages containing reasoning content, which kills
/// the session at the first compaction.
fn strip_reasoning_content(message: &mut Message) {
    message.content.retain(|c| {
        !matches!(
            c,
            MessageContent::Thinking(_) | MessageContent::RedactedThinking(_)
        )
    });
}

/// Compact messages by summarizing them
///
/// This function performs the actual compaction by summarizing messages and updating
/// their visibility metadata. It does not check thresholds - use `check_if_compaction_needed`
/// first to determine if compaction is necessary.
///
/// # Arguments
/// * `provider` - The provider to use for summarization
/// * `session_id` - The session to use for summarization
/// * `conversation` - The current conversation history
/// * `manual_compact` - If true, this is a manual compaction (don't preserve user message)
///
/// # Returns
/// * A tuple containing:
///   - `Conversation`: The compacted messages
///   - `ProviderUsage`: Provider usage from summarization
pub async fn compact_messages(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    conversation: &Conversation,
    manual_compact: bool,
    request: &SummaryRequest,
) -> Result<(Conversation, ProviderUsage)> {
    compact_messages_with_tail(
        provider,
        model_config,
        session_id,
        conversation,
        manual_compact,
        keep_tail_from_config(),
        request,
    )
    .await
}

fn keep_tail_from_config() -> usize {
    Config::global()
        .get_param::<usize>("GOOSE_COMPACT_KEEP_TAIL")
        .unwrap_or(0)
}

/// `compact_messages` with the keep-tail injected — the testable form (no env/config read). The
/// compaction of a caller with no chat inputs (the swarm's workers): a summary request that extends
/// the chat is a chat's (`compact_chat`) and is refused here.
pub async fn compact_messages_with_tail(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    conversation: &Conversation,
    manual_compact: bool,
    keep_tail: usize,
    request: &SummaryRequest,
) -> Result<(Conversation, ProviderUsage)> {
    if matches!(request, SummaryRequest::ExtendsChat(_)) {
        return Err(anyhow::anyhow!(
            "a summary request that extends the chat is a chat's compaction (compact_chat), \
             which carries the chat's inputs"
        ));
    }
    match compact_core(
        provider,
        model_config,
        session_id,
        conversation,
        manual_compact,
        keep_tail,
        request,
        None,
    )
    .await?
    {
        ChatCompacted::Compacted {
            conversation,
            usage,
            ..
        } => Ok((conversation, usage)),
        ChatCompacted::Asked { question } => Err(anyhow::anyhow!(
            "a compaction without a note was answered with a question about one: {question}"
        )),
    }
}

/// Q-357: a chat's compaction — the summary request `request`, the note and the kept block from
/// `chat`. The stored summary is the model's text with its scratch stripped, then what goose kept
/// word for word (`pillars`).
pub async fn compact_chat(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    conversation: &Conversation,
    manual_compact: bool,
    request: &SummaryRequest,
    chat: &ChatCompaction,
) -> Result<ChatCompacted> {
    compact_core(
        provider,
        model_config,
        session_id,
        conversation,
        manual_compact,
        keep_tail_from_config(),
        request,
        Some(chat),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn compact_core(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    conversation: &Conversation,
    manual_compact: bool,
    keep_tail: usize,
    request: &SummaryRequest,
    chat: Option<&ChatCompaction>,
) -> Result<ChatCompacted> {
    info!("Performing message compaction");

    let messages = conversation.messages();

    let has_text_only = |msg: &Message| {
        let has_text = msg
            .content
            .iter()
            .any(|c| matches!(c, MessageContent::Text(_)));
        let has_tool_content = msg.content.iter().any(|c| {
            matches!(
                c,
                MessageContent::ToolRequest(_) | MessageContent::ToolResponse(_)
            )
        });
        has_text && !has_tool_content
    };

    let extract_text = |msg: &Message| -> Option<String> {
        let text_parts: Vec<String> = msg
            .content
            .iter()
            .filter_map(|c| {
                if let MessageContent::Text(text) = c {
                    Some(text.text.clone())
                } else {
                    None
                }
            })
            .collect();

        if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join("\n"))
        }
    };

    // Find and preserve the most recent user message for non-manual compacts
    let (preserved_user_message, is_most_recent) = if !manual_compact {
        let found_msg = messages.iter().enumerate().rev().find(|(_, msg)| {
            msg.is_agent_visible()
                && matches!(msg.role, rmcp::model::Role::User)
                && has_text_only(msg)
        });

        if let Some((idx, msg)) = found_msg {
            let is_last = idx == messages.len() - 1;
            (Some(msg.clone()), is_last)
        } else {
            (None, false)
        }
    } else {
        (None, false)
    };

    // K4 KEEP-TAIL: summarize everything EXCEPT the last `keep_tail` messages, which survive
    // VERBATIM. The measured defect: a long tool loop's recent tail is all tool content, so
    // `has_text_only` preserves nothing from it and the summary is the ONLY survivor — a 40-turn
    // swarm sink loses the exact bytes of the file it is mid-way through editing and must re-read
    // it, which is precisely the re-reading spiral the swarm polices. A strong model survives
    // prose-only recall; a 27B does not. Default 0 => byte-identical for every non-swarm session.
    let keep_tail = keep_tail.min(messages.len().saturating_sub(2));
    let mut cut = messages.len() - keep_tail;
    // A kept tail may not OPEN on a tool response whose request was summarized away — that is an
    // orphan `role:tool` message and OpenAI-compatible servers reject the request outright. Extend
    // the tail backward until its first message carries no ToolResponse (the paired request then
    // rides along); `cut == 0` degrades to keeping everything except the summary, which is safe.
    while cut > 0
        && cut < messages.len()
        && messages[cut]
            .content
            .iter()
            .any(|c| matches!(c, MessageContent::ToolResponse(_)))
    {
        cut -= 1;
    }
    let messages_to_compact = &messages[..cut];

    // With a kept tail, the most recent user text is usually INSIDE it already — appending the
    // preserved copy too would duplicate the instruction the model most attends to.
    let preserved_in_tail = keep_tail > 0
        && messages[cut..]
            .iter()
            .any(|m| matches!(m.role, rmcp::model::Role::User) && has_text_only(m));
    let preserved_text = preserved_user_message
        .as_ref()
        .filter(|_| !preserved_in_tail)
        .and_then(extract_text);

    let kept = chat.map(|chat| {
        Pillars::build(
            messages,
            &KeptSources {
                note: chat.note.as_deref(),
                pins: &chat.pins,
                preserved: preserved_text.as_deref(),
            },
            chat.ledger.clone(),
        )
        .fit(chat.kept_budget_chars)
    });

    let (summary_message, summarization_usage, note) = match do_compact(
        provider,
        model_config,
        session_id,
        messages_to_compact,
        request,
        chat.zip(kept.as_ref()),
    )
    .await?
    {
        Summary::Written {
            message,
            usage,
            note,
        } => (message, usage, note),
        Summary::Asked { question } => return Ok(ChatCompacted::Asked { question }),
    };
    let summary_message = match &kept {
        Some(kept) => {
            let stored = pillars::stored_summary(&summary_message.as_concat_text(), kept);
            let mut message = summary_message;
            message
                .content
                .retain(|c| !matches!(c, MessageContent::Text(_)));
            message.content.insert(0, MessageContent::text(stored));
            message
        }
        None => summary_message,
    };

    // Create the final message list with updated visibility metadata:
    // 1. Original messages become user_visible but not agent_visible
    // 2. Summary message becomes agent_visible but not user_visible
    // 3. Assistant messages to continue the conversation are also agent_visible but not user_visible
    let mut final_messages = Vec::new();

    for (idx, msg) in messages.iter().enumerate() {
        let updated_metadata = if is_most_recent
            && idx == messages.len() - 1
            && preserved_user_message.is_some()
            && keep_tail == 0
        {
            // This is the most recent message and we're preserving it by adding a fresh copy
            MessageMetadata::invisible()
        } else {
            msg.metadata.clone().with_agent_invisible()
        };
        let updated_msg = msg.clone().with_metadata(updated_metadata);
        final_messages.push(updated_msg);
    }

    let summary_msg = summary_message.with_metadata(MessageMetadata::agent_only());

    let mut continuation_messages = vec![summary_msg];

    let continuation_text = if manual_compact {
        MANUAL_COMPACT_CONTINUATION_TEXT
    } else if is_most_recent {
        CONVERSATION_CONTINUATION_TEXT
    } else {
        TOOL_LOOP_CONTINUATION_TEXT
    };

    let continuation_msg = Message::assistant()
        .with_text(continuation_text)
        .with_metadata(MessageMetadata::agent_only());
    continuation_messages.push(continuation_msg);

    let (merged_continuation, _issues) = merge_consecutive_messages(continuation_messages);
    final_messages.extend(merged_continuation);

    // K4: the kept tail returns VERBATIM after the summary, so the agent reads "summary of the
    // early conversation, then the last turns exactly as they happened" — same shape as the
    // preserved-user mechanism, generalized to tool content.
    for msg in &messages[cut..] {
        final_messages.push(msg.clone().with_metadata(MessageMetadata::agent_only()));
    }

    if let Some(text) = preserved_text {
        final_messages.push(Message::user().with_text(&text));
    }

    Ok(ChatCompacted::Compacted {
        conversation: Conversation::new_unvalidated(final_messages),
        usage: summarization_usage,
        note,
        kept,
    })
}

/// Check if messages exceed the auto-compaction threshold
/// The context window as the compaction guard compares it: the provider's limit for this model
/// (a probed n_ctx where the provider has one, else the model config's) under the optional
/// GOOSE_LOCAL_CONTEXT_CAP. ONE derivation for `check_if_compaction_needed`, the MOIM turn-context
/// block and the swarm's per-lane digest — VA-107's receipt is a lane reading a budget computed
/// from a limit other than the one this guard used, so the number a lane reads is THIS one.
///
/// local-edition: the cap is an optional HARD cap on the effective window (quality-first lean
/// context for hybrid local models like Qwen3.6). Read via Config — the same path that reads the
/// threshold — so it gates the auto-compaction trigger regardless of any provider wrapping. No-op
/// when GOOSE_LOCAL_CONTEXT_CAP is unset or 0.
///
/// `None` = the window is UNKNOWN: the provider could not measure it (it answered an error) and the
/// model config declares none. Never the default for an unknown model standing in for it (Q-18:
/// the swarm's first turn ran on 128,000 until a pick had measured the pool) — the consumers say
/// "unknown" (the MOIM line), skip what needs a window (proactive compaction, the skill autoload
/// budget), and the provider's own context-length error still triggers recovery compaction.
pub async fn effective_context_limit(
    provider: &dyn Provider,
    model_config: &ModelConfig,
) -> Option<usize> {
    let context_limit = match provider.get_context_limit(model_config).await {
        Ok(limit) => limit,
        Err(err) => match model_config.context_limit {
            Some(declared) => declared,
            None => {
                tracing::warn!(
                    model = %model_config.model_name,
                    reason = %err,
                    "context_window_unknown: the provider could not measure the window and the model \
                     declares none; compaction waits for the provider's own context-length error"
                );
                return None;
            }
        },
    };
    Some(
        match Config::global().get_param::<usize>("GOOSE_LOCAL_CONTEXT_CAP") {
            Ok(cap) if cap > 0 => context_limit.min(cap),
            _ => context_limit,
        },
    )
}

pub async fn check_if_compaction_needed(
    provider: &dyn Provider,
    conversation: &Conversation,
    threshold_override: Option<f64>,
    session: &crate::session::Session,
) -> Result<bool> {
    if provider.manages_own_context() {
        return Ok(false);
    }

    let messages = conversation.messages();
    let config = Config::global();
    let threshold = threshold_override.unwrap_or_else(|| {
        config
            .get_param::<f64>("GOOSE_AUTO_COMPACT_THRESHOLD")
            .unwrap_or(DEFAULT_COMPACTION_THRESHOLD)
    });

    let model_config = session
        .model_config
        .clone()
        .unwrap_or_else(|| ModelConfig::new("unknown"));
    let Some(context_limit) = effective_context_limit(provider, &model_config).await else {
        return Ok(false);
    };

    let (current_tokens, _token_source) = match session.usage.total_tokens {
        Some(tokens) => (tokens as usize, "session metadata"),
        None => {
            let token_counter = create_token_counter()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create token counter: {}", e))?;

            let token_counts: Vec<_> = messages
                .iter()
                .filter(|m| m.is_agent_visible())
                .map(|msg| token_counter.count_chat_tokens("", std::slice::from_ref(msg), &[]))
                .collect();

            (token_counts.iter().sum(), "estimated")
        }
    };

    let usage_ratio = current_tokens as f64 / context_limit as f64;

    let needs_compaction = if threshold <= 0.0 || threshold >= 1.0 {
        false // Auto-compact is disabled.
    } else {
        usage_ratio > threshold
    };
    Ok(needs_compaction)
}

fn filter_tool_responses(messages: &[Message], remove_percent: u32) -> Vec<&Message> {
    fn has_tool_response(msg: &Message) -> bool {
        msg.content
            .iter()
            .any(|c| matches!(c, MessageContent::ToolResponse(_)))
    }

    if remove_percent == 0 {
        return messages.iter().collect();
    }

    let tool_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, msg)| has_tool_response(msg))
        .map(|(i, _)| i)
        .collect();

    if tool_indices.is_empty() {
        return messages.iter().collect();
    }

    let num_to_remove = ((tool_indices.len() * remove_percent as usize) / 100).max(1);

    let middle = tool_indices.len() / 2;
    let mut indices_to_remove = Vec::new();

    // Middle out
    for i in 0..num_to_remove {
        if i % 2 == 0 {
            let offset = i / 2;
            if middle > offset {
                indices_to_remove.push(tool_indices[middle - offset - 1]);
            }
        } else {
            let offset = i / 2;
            if middle + offset < tool_indices.len() {
                indices_to_remove.push(tool_indices[middle + offset]);
            }
        }
    }

    messages
        .iter()
        .enumerate()
        .filter(|(i, _)| !indices_to_remove.contains(i))
        .map(|(_, msg)| msg)
        .collect()
}

/// What the summary call produced.
enum Summary {
    Written {
        message: Message,
        usage: ProviderUsage,
        note: NoteOutcome,
    },
    /// The model asked about the person's note (`NOTE QUESTION: …`) and was stopped there.
    Asked { question: String },
}

async fn do_compact(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    messages: &[Message],
    request: &SummaryRequest,
    chat: Option<(&ChatCompaction, &Pillars)>,
) -> Result<Summary, anyhow::Error> {
    if let SummaryRequest::ExtendsChat(frame) = request {
        let Some((chat, kept)) = chat else {
            return Err(anyhow::anyhow!(
                "a summary request that extends the chat needs the chat's compaction inputs"
            ));
        };
        let summary_model =
            crate::model_config::get_fast_model(provider.get_name(), model_config).await?;
        if summary_model.model_name == model_config.model_name {
            match summarize_as_the_chat(
                provider,
                model_config,
                session_id,
                messages,
                frame,
                chat,
                kept,
            )
            .await?
            {
                ChatSummary::Written(summary) => return Ok(*summary),
                ChatSummary::Asked(question) => return Ok(Summary::Asked { question }),
                ChatSummary::NotWritten(why) => warn!(
                    "compaction: the summary request that extends the chat {why}; summarizing \
                     from a transcript of the conversation instead"
                ),
            }
        } else {
            tracing::debug!(
                "compaction: the summary model {} is not the chat's {}, so nothing of the chat is \
                 cached for it; summarizing from a transcript of the conversation",
                summary_model.model_name,
                model_config.model_name
            );
        }
    }
    let (message, usage) =
        summarize_a_transcript(provider, model_config, session_id, messages).await?;
    let note = match chat {
        Some((chat, _)) if chat.note.is_some() => NoteOutcome::NotSent,
        _ => NoteOutcome::NoNote,
    };
    Ok(Summary::Written {
        message,
        usage,
        note,
    })
}

enum ChatSummary {
    Written(Box<Summary>),
    /// The model asked about the person's note instead of summarizing.
    Asked(String),
    /// The provider refused the request as too long, or the model answered it with a tool call:
    /// which, for the log.
    NotWritten(String),
}

/// The continuation text every compaction leaves after its summary begins with this.
pub(crate) const COMPACTED_PREFIX: &str = "Your context was compacted";

/// The summary the latest compaction stored: the agent-only message before the continuation line
/// every compaction leaves after it.
pub fn latest_summary(messages: &[Message]) -> Option<String> {
    let continuation = messages.iter().rposition(|m| {
        m.role == Role::Assistant
            && m.is_agent_visible()
            && !m.is_user_visible()
            && m.as_concat_text().starts_with(COMPACTED_PREFIX)
    })?;
    let summary = messages.get(continuation.checked_sub(1)?)?;
    (summary.role == Role::User && summary.is_agent_visible() && !summary.is_user_visible())
        .then(|| summary.as_concat_text())
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The instruction's facts about this chat.
fn chat_instruction(
    chat: &ChatCompaction,
    kept: &Pillars,
    messages: &[Message],
) -> ChatInstruction {
    let trigger = match &chat.trigger {
        CompactionTrigger::Manual => {
            Some("The person asked goose to compact this conversation now.".to_string())
        }
        CompactionTrigger::Auto {
            used,
            limit,
            threshold,
        } => {
            let point = (threshold * 100.0).round() as u32;
            Some(match (used, limit) {
                (Some(used), Some(limit)) if *limit > 0 => format!(
                    "The conversation reached {} of {} tokens ({}%), past this chat's compaction \
                     point of {point}%.",
                    thousands(*used),
                    thousands(*limit),
                    (*used as f64 * 100.0 / *limit as f64).round() as u32,
                ),
                _ => format!("The conversation passed this chat's compaction point of {point}%."),
            })
        }
        CompactionTrigger::Recovery => None,
    };
    let earlier_summary = messages.iter().any(|m| {
        m.role == Role::Assistant
            && m.is_agent_visible()
            && !m.is_user_visible()
            && m.as_concat_text().starts_with(COMPACTED_PREFIX)
    });
    ChatInstruction {
        trigger,
        kept: kept.kept_lines(),
        earlier_summary,
        goal: chat.goal.clone(),
        note: kept.note.clone(),
        may_ask: chat.may_ask,
        parts: WRITTEN_PARTS
            .iter()
            .map(|(heading, ask)| WrittenPart { heading, ask })
            .collect(),
    }
}

/// [`SummaryRequest::ExtendsChat`]: the conversation as the chat's next call would send it, then
/// the instruction. The chat's messages are `fix_conversation`'s output (`inject_moim` fixes the
/// conversation it extends), so these are too — which is also what places the instruction: it
/// joins a trailing user message, as the turn-context block does, and follows tool results as a
/// user message of its own (a strict chat template refuses two user turns in a row). The call is
/// the chat's, not a helper's (`complete_as_the_chat`: the thinking switch can change the system
/// block the cache holds). Q-357: the call streams, so the chat's card counts what is written and
/// the note's verdict is read from the first line as it arrives — a question stops the call there.
/// The person's note, like every fact about this chat, is in the instruction alone: the messages
/// before it are the chat's own, byte for byte, and stay the cached prefix.
#[allow(clippy::too_many_arguments)]
async fn summarize_as_the_chat(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    messages: &[Message],
    frame: &ChatRequestFrame,
    chat: &ChatCompaction,
    kept: &Pillars,
) -> Result<ChatSummary> {
    let instruction = render_template(
        "compaction.md",
        &SummarizeContext {
            messages: None,
            chat: Some(chat_instruction(chat, kept, messages)),
        },
    )?;
    let mut conversation = messages.to_vec();
    conversation.push(Message::user().with_text(instruction));
    let (conversation, _) = fix_conversation(Conversation::new_unvalidated(conversation));
    let sent = crate::agents::reply_parts::messages_for_provider(
        conversation.messages(),
        model_config.toolshim,
    );

    let counter = match create_token_counter().await {
        Ok(counter) => Some(counter),
        Err(e) => {
            warn!("compaction: the tokenizer could not be built ({e}); the card counts no tokens");
            None
        }
    };
    let watch_note = kept.note.is_some() && chat.may_ask;
    let mut written_tokens = 0u64;
    let mut asked: Option<String> = None;
    let watch = |delta: &str, text: &str| {
        if let Some(counter) = &counter {
            written_tokens += counter.count_tokens(delta) as u64;
        }
        if let Some(observe) = &chat.progress {
            observe(WritingProgress {
                written_tokens: counter.as_ref().map(|_| written_tokens),
                parts: pillars::written_parts(text),
            });
        }
        if watch_note {
            if let Some(NoteVerdict::Question(question)) = pillars::streaming_note_verdict(text) {
                asked = Some(question);
                return false;
            }
        }
        true
    };
    let answer = crate::model_config::stream_as_the_chat(
        crate::background_work::BackgroundWorkKind::Compaction,
        provider,
        model_config,
        session_id,
        &frame.system_prompt,
        sent.messages(),
        &frame.tools,
        watch,
    )
    .await;
    let (mut response, mut provider_usage) = match answer {
        Ok((answer, _)) => answer,
        Err(ProviderError::ContextLengthExceeded(detail)) => {
            return Ok(ChatSummary::NotWritten(format!(
                "was refused as too long ({detail})"
            )))
        }
        Err(e) => return Err(e.into()),
    };
    if let Some(question) = asked {
        return Ok(ChatSummary::Asked(question));
    }
    if response.content.iter().any(|c| {
        matches!(
            c,
            MessageContent::ToolRequest(_) | MessageContent::FrontendToolRequest(_)
        )
    }) {
        return Ok(ChatSummary::NotWritten(
            "was answered with a tool call instead of the summary".to_string(),
        ));
    }
    let note = match &kept.note {
        None => NoteOutcome::NoNote,
        Some(_) => match pillars::note_verdict(&response.as_concat_text()) {
            NoteVerdict::Question(question) if chat.may_ask => {
                return Ok(ChatSummary::Asked(question))
            }
            // Told a turn cannot wait for an answer, the model asked anyway: its words stand as
            // the concern the summary was written under.
            NoteVerdict::Question(question) => NoteOutcome::Read(NoteVerdict::Concern(question)),
            verdict => NoteOutcome::Read(verdict),
        },
    };
    response.role = Role::User;
    strip_reasoning_content(&mut response);
    crate::providers::usage_estimator::ensure_usage_tokens(
        &mut provider_usage,
        &frame.system_prompt,
        sent.messages(),
        &response,
        &frame.tools,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to ensure usage tokens: {}", e))?;
    Ok(ChatSummary::Written(Box::new(Summary::Written {
        message: response,
        usage: provider_usage,
        note,
    })))
}

/// [`SummaryRequest::Transcript`], trying progressively more of the tool responses removed from
/// the middle while the provider refuses the request as too long.
async fn summarize_a_transcript(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    messages: &[Message],
) -> Result<(Message, ProviderUsage), anyhow::Error> {
    let agent_visible_messages: Vec<Message> = messages
        .iter()
        .filter(|msg| msg.is_agent_visible())
        .map(|msg| msg.agent_visible_content())
        .collect();

    // Try progressively removing more tool response messages from the middle to reduce context length
    let removal_percentages = [0, 10, 20, 50, 100];

    for (attempt, &remove_percent) in removal_percentages.iter().enumerate() {
        let filtered_messages = filter_tool_responses(&agent_visible_messages, remove_percent);

        let messages_text = filtered_messages
            .iter()
            .map(|&msg| format_message_for_compacting(msg))
            .collect::<Vec<_>>()
            .join("\n");

        let context = SummarizeContext {
            messages: Some(messages_text),
            chat: None,
        };

        let system_prompt = render_template("compaction.md", &context)?;

        let user_message = Message::user()
            .with_text("Please summarize the conversation history provided in the system prompt.");
        let summarization_request = vec![user_message];

        match crate::model_config::complete_fast(
            crate::background_work::BackgroundWorkKind::Compaction,
            provider,
            model_config,
            session_id,
            &system_prompt,
            &summarization_request,
            &[],
        )
        .await
        {
            Ok((mut response, mut provider_usage)) => {
                response.role = Role::User;
                strip_reasoning_content(&mut response);

                crate::providers::usage_estimator::ensure_usage_tokens(
                    &mut provider_usage,
                    &system_prompt,
                    &summarization_request,
                    &response,
                    &[],
                )
                .await
                .map_err(|e| anyhow::anyhow!("Failed to ensure usage tokens: {}", e))?;

                return Ok((response, provider_usage));
            }
            Err(e) => {
                if matches!(e, ProviderError::ContextLengthExceeded(_)) {
                    if attempt < removal_percentages.len() - 1 {
                        continue;
                    } else {
                        return Err(anyhow::anyhow!(
                            "Failed to compact: context limit exceeded even after removing all tool responses"
                        ));
                    }
                }
                return Err(e.into());
            }
        }
    }

    Err(anyhow::anyhow!(
        "Unexpected: exhausted all attempts without returning"
    ))
}

/// One message as text for a summarizing model. Content marked for the user alone never enters it
/// (Q-211): `summarize_tool_call` and the orchestrator's summary pass messages the provider path
/// never filtered.
pub fn format_message_for_compacting(msg: &Message) -> String {
    let content_parts: Vec<String> = msg
        .content
        .iter()
        .filter_map(|content| match content {
            MessageContent::Text(text) => {
                audience_includes(text.audience(), &Role::Assistant).then(|| text.text.clone())
            }
            MessageContent::Image(img) => audience_includes(img.audience(), &Role::Assistant)
                .then(|| format!("[image: {}]", img.mime_type)),
            MessageContent::ToolRequest(req) => {
                if let Ok(call) = &req.tool_call {
                    Some(format!(
                        "tool_request({}): {}",
                        call.name,
                        serde_json::to_string(&call.arguments)
                            .unwrap_or_else(|_| "<<invalid json>>".to_string())
                    ))
                } else {
                    Some("tool_request: [error]".to_string())
                }
            }
            MessageContent::ToolResponse(res) => {
                if let Ok(result) = &res.tool_result {
                    let text_items: Vec<&str> = model_visible_texts(result).collect();

                    if !text_items.is_empty() {
                        Some(format!("tool_response: {}", text_items.join("\n")))
                    } else {
                        Some("tool_response: [non-text content]".to_string())
                    }
                } else {
                    Some("tool_response: [error]".to_string())
                }
            }
            MessageContent::ToolConfirmationRequest(req) => {
                Some(format!("tool_confirmation_request: {}", req.tool_name))
            }
            MessageContent::ActionRequired(action) => match &action.data {
                ActionRequiredData::ToolConfirmation { tool_name, .. } => {
                    Some(format!("action_required(tool_confirmation): {}", tool_name))
                }
                ActionRequiredData::Elicitation { message, .. } => {
                    Some(format!("action_required(elicitation): {}", message))
                }
                ActionRequiredData::ElicitationResponse { id, .. } => {
                    Some(format!("action_required(elicitation_response): {}", id))
                }
            },
            MessageContent::FrontendToolRequest(req) => {
                if let Ok(call) = &req.tool_call {
                    Some(format!("frontend_tool_request: {}", call.name))
                } else {
                    Some("frontend_tool_request: [error]".to_string())
                }
            }
            MessageContent::Thinking(_) => None,
            MessageContent::RedactedThinking(_) => None,
            MessageContent::SystemNotification(notification) => {
                Some(format!("system_notification: {}", notification.msg))
            }
        })
        .collect();

    let role_str = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };

    if content_parts.is_empty() {
        format!("[{}]: <empty message>", role_str)
    } else {
        format!("[{}]: {}", role_str, content_parts.join("\n"))
    }
}

pub fn compute_tool_call_cutoff(context_limit: usize, compaction_threshold: f64) -> usize {
    let threshold = if compaction_threshold > 0.0 && compaction_threshold <= 1.0 {
        compaction_threshold
    } else {
        DEFAULT_COMPACTION_THRESHOLD
    };
    let effective_limit = (context_limit as f64 * threshold) as usize;
    (3 * effective_limit / 20_000).clamp(10, 500)
}

pub fn tool_ids_to_summarize(
    conversation: &Conversation,
    cutoff: usize,
    protect_last_n: usize,
) -> Vec<String> {
    let messages = conversation.messages();

    let mut tool_call_ids: Vec<String> = Vec::new();

    for msg in messages.iter() {
        if !msg.is_agent_visible() {
            continue;
        }

        for content in &msg.content {
            if let MessageContent::ToolRequest(req) = content {
                tool_call_ids.push(req.id.clone());
            }
        }
    }

    // Never summarize the last N tool calls (current turn)
    let eligible = tool_call_ids.len().saturating_sub(protect_last_n);
    if eligible <= cutoff.saturating_add(TOOLCALL_SUMMARIZATION_BATCH_SIZE) {
        return Vec::new();
    }

    tool_call_ids
        .into_iter()
        .take(TOOLCALL_SUMMARIZATION_BATCH_SIZE)
        .collect()
}

pub async fn summarize_tool_call(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    conversation: &Conversation,
    tool_id: &str,
) -> Result<Message> {
    let messages = conversation.messages();

    let matching_messages: Vec<&Message> = messages
        .iter()
        .filter(|m| {
            m.content.iter().any(|c| match c {
                MessageContent::ToolRequest(req) => req.id == tool_id,
                MessageContent::ToolResponse(resp) => resp.id == tool_id,
                _ => false,
            })
        })
        .collect();

    if matching_messages.is_empty() {
        return Err(anyhow::anyhow!(
            "No messages found for tool id: {}",
            tool_id
        ));
    }

    let formatted = matching_messages
        .iter()
        .map(|msg| format_message_for_compacting(msg))
        .collect::<Vec<_>>()
        .join("\n");

    let user_message = Message::user().with_text(formatted);
    let summarization_request = vec![user_message];

    let system_prompt = indoc! {r#"
                Your task is to summarize a tool call & response pair to save tokens.

                Reply with a single message that describes what happened. Typically a tool call
                asks for something using a bunch of parameters and then the result is also some
                structured output. So the tool might ask to look up something on github and the
                reply might be a json document. So you could reply with something like:

                "A call to github was made to get the project status"

                if that is what it was.
            "#};

    let (mut response, _) = crate::model_config::complete_fast(
        crate::background_work::BackgroundWorkKind::ToolDigest,
        provider,
        model_config,
        session_id,
        system_prompt,
        &summarization_request,
        &[],
    )
    .await?;

    response.role = Role::User;
    strip_reasoning_content(&mut response);
    response.created = matching_messages.last().unwrap().created;
    response.metadata = MessageMetadata::agent_only();

    Ok(response.with_generated_id())
}

/// Opens every condensed tool pair, so the model reads it as goose's bookkeeping about a call it
/// made — not as a new message from the user (the pair is stored with role `user`).
pub const TOOL_RECORD_HEADER: &str =
    "[goose's record of an earlier tool call, condensed to save context; not a message from the user]";

// measured: round 4's ten model-written pair summaries (sessions.db 764259-68) ran 84–716 chars; one
// excerpt per argument and per output end at this length keeps a record inside that band.
pub const RECORD_EXCERPT_CHARS: usize = 160;

fn excerpt(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(RECORD_EXCERPT_CHARS).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn quoted(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

fn arguments_excerpt(arguments: Option<&rmcp::model::JsonObject>) -> String {
    let Some(arguments) = arguments else {
        return "{}".to_string();
    };
    let cut: serde_json::Map<String, serde_json::Value> = arguments
        .iter()
        .map(|(key, value)| {
            let text = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let kept = if text.chars().count() > RECORD_EXCERPT_CHARS {
                serde_json::Value::String(excerpt(&text))
            } else {
                value.clone()
            };
            (key.clone(), kept)
        })
        .collect();
    serde_json::Value::Object(cut).to_string()
}

fn outcome_and_output(result: &rmcp::model::CallToolResult) -> (String, String) {
    let exit_code = result
        .structured_content
        .as_ref()
        .and_then(|s| s.get("exit_code"))
        .and_then(serde_json::Value::as_i64);
    let verb = if result.is_error == Some(true) {
        "failed"
    } else {
        "succeeded"
    };
    let outcome = match exit_code {
        Some(code) => format!("{verb} (exit {code})"),
        None => verb.to_string(),
    };

    let text = model_visible_texts(result).collect::<Vec<_>>().join("\n");
    let non_text = model_visible_content(result)
        .filter(|c| c.as_text().is_none())
        .count();
    let text = text.trim_end();
    let mut output = if text.is_empty() {
        "No text output.".to_string()
    } else if text.chars().count() <= 2 * RECORD_EXCERPT_CHARS {
        format!("Output: {}", quoted(text))
    } else {
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let first = lines.first().copied().unwrap_or(text);
        let last = lines.last().copied().unwrap_or(text);
        format!(
            "Output, {} lines; first: {}; last: {}",
            text.lines().count(),
            quoted(&excerpt(first)),
            quoted(&excerpt(last))
        )
    };
    if non_text > 0 {
        output.push_str(&format!(" Plus {non_text} non-text item(s)."));
    }
    (outcome, output)
}

/// A condensed tool pair built only from what the call and its result actually carry: the tool,
/// its arguments, the outcome and excerpts of the output. Round 4 measured the model-written
/// summaries it replaces: of ten, one ended in an order ("Write the E2E test artifacts … into this
/// file.") and two stated file contents the outputs contradicted ("still ends with an em dash";
/// the od dump ended with "!"). Checking such prose against the output cannot catch a narrative
/// claim, so no model writes any of this.
pub fn record_tool_call(conversation: &Conversation, tool_id: &str) -> Result<Message> {
    let messages = conversation.messages();
    let request = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .find_map(|c| match c {
            MessageContent::ToolRequest(req) if req.id == tool_id => Some(req),
            _ => None,
        });
    let response = messages.iter().find_map(|m| {
        m.content.iter().find_map(|c| match c {
            MessageContent::ToolResponse(resp) if resp.id == tool_id => Some((m.created, resp)),
            _ => None,
        })
    });
    let (Some(request), Some((response_created, response))) = (request, response) else {
        return Err(anyhow::anyhow!(
            "No request/response pair found for tool id: {}",
            tool_id
        ));
    };

    let call = match &request.tool_call {
        Ok(call) => format!(
            "{} {}",
            call.name,
            arguments_excerpt(call.arguments.as_ref())
        ),
        Err(e) => format!(
            "A tool call goose could not parse ({})",
            excerpt(&e.message)
        ),
    };
    let (outcome, output) = match &response.tool_result {
        Ok(result) => outcome_and_output(result),
        Err(e) => (
            format!("failed: {}", quoted(&excerpt(&e.message))),
            String::new(),
        ),
    };
    let mut record = format!("{TOOL_RECORD_HEADER}\n{call} {outcome}.");
    if !output.is_empty() {
        record.push('\n');
        record.push_str(&output);
    }

    let mut message = Message::user().with_text(record);
    message.created = response_created;
    message.metadata = MessageMetadata::agent_only();
    Ok(message.with_generated_id())
}

/// Q-294: the prompt tokens the provider's last call read from its own cache, when it read any. A
/// condensed pair replaces messages near the START of the conversation, so the next request stops
/// agreeing with the cached prompt at the first condensed pair and re-reads everything after it.
/// E2E #3o (the 27B split, 262k window): from turn 4 on every reply condensed ten pairs, and the
/// next turn's first call read 0 of 109,655 / 40,244 of 102,210 / 0 of 108,801 tokens from cache —
/// about 15 minutes of prefill each — to save 4k–12k tokens of a window auto-compaction already
/// guards. A provider that reports no cache reads keeps the condensation as before.
pub fn prompt_cache_read(last_call: &Usage) -> Option<i32> {
    last_call.cache_read_input_tokens.filter(|read| *read > 0)
}

/// `faithful_records`: chat agents get `record_tool_call` (facts only, no model call); the swarm's
/// workers keep the model-written summary their golden benchmark was measured with.
/// `cached_prompt`: `prompt_cache_read` of the latest call — when the provider serves the prompt
/// from its cache, the pairs stay as they are (Q-294).
#[allow(clippy::too_many_arguments)]
pub fn maybe_summarize_tool_pairs(
    provider: Arc<dyn Provider>,
    model_config: ModelConfig,
    session_id: String,
    conversation: Conversation,
    cutoff: usize,
    protect_last_n: usize,
    faithful_records: bool,
    cached_prompt: Option<i32>,
) -> Option<JoinHandle<Vec<(Message, String)>>> {
    if !tool_pair_summarization_enabled() || provider.manages_own_context() {
        return None;
    }

    let tool_ids = tool_ids_to_summarize(&conversation, cutoff, protect_last_n);
    if tool_ids.is_empty() {
        return None;
    }

    if let Some(read) = cached_prompt {
        info!(
            session_id = %session_id,
            cached_prompt_tokens = read,
            pairs = tool_ids.len(),
            "tool pairs kept whole: the provider serves this conversation's prompt from its cache, \
             and condensing its oldest pairs would make the next turn re-read everything after them"
        );
        return None;
    }

    if faithful_records {
        return Some(tokio::spawn(async move {
            tool_ids
                .into_iter()
                .filter_map(|tool_id| match record_tool_call(&conversation, &tool_id) {
                    Ok(record) => Some((record, tool_id)),
                    Err(e) => {
                        warn!("Failed to record tool pair: {}", e);
                        None
                    }
                })
                .collect()
        }));
    }

    Some(tokio::spawn(async move {
        let mut results = Vec::new();
        for tool_id in tool_ids {
            match summarize_tool_call(
                provider.as_ref(),
                &model_config,
                &session_id,
                &conversation,
                &tool_id,
            )
            .await
            {
                Ok(summary) => results.push((summary, tool_id)),
                Err(e) => {
                    warn!("Failed to summarize tool pair: {}", e);
                }
            }
        }
        results
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use goose_providers::conversation::token_usage::Usage;
    use rmcp::model::{AnnotateAble, CallToolRequestParams, RawContent, Tool};

    /// Compaction and the tool-pair digest ask `complete_fast` for ThinkingEffort::Off, as every
    /// other provider has honoured; on an MLX engine the Off now arrives as the template switch.
    /// Compaction's own prompt carries its reasoning step in-band (`<analysis>`), so the engine's
    /// thinking block ran that step twice.
    #[tokio::test]
    async fn compaction_and_the_tool_pair_digest_reach_the_mlx_engine_with_thinking_off() {
        use crate::model_config::mlx_endpoint::{thinking_off, MlxEndpoint, SERVED};
        let session = ModelConfig::new(SERVED);
        let mut messages = vec![Message::user().with_text("list the notes")];
        messages.extend(create_tool_pair(
            "call1",
            "resp1",
            "developer__shell",
            "notes/kickoff.md",
        ));
        messages.push(Message::assistant().with_text("one file: notes/kickoff.md"));
        let conversation = Conversation::new_unvalidated(messages);

        let engine = MlxEndpoint::start().await;
        compact_messages_with_tail(
            engine.provider.as_ref(),
            &session,
            "s",
            &conversation,
            true,
            0,
            &SummaryRequest::Transcript,
        )
        .await
        .unwrap();
        let bodies = engine.bodies().await;
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Wrap reasoning in `<analysis>` tags"));
        assert_eq!(bodies[0]["chat_template_kwargs"], thinking_off());

        let engine = MlxEndpoint::start().await;
        summarize_tool_call(
            engine.provider.as_ref(),
            &session,
            "s",
            &conversation,
            "call1",
        )
        .await
        .unwrap();
        let bodies = engine.bodies().await;
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("summarize a tool call & response pair"));
        assert_eq!(bodies[0]["chat_template_kwargs"], thinking_off());
    }

    #[test]
    fn strip_reasoning_content_removes_thinking_keeps_text() {
        let mut msg = Message::assistant()
            .with_thinking("chain of thought", "sig")
            .with_text("the summary");
        msg.role = Role::User;
        strip_reasoning_content(&mut msg);
        assert_eq!(msg.content.len(), 1);
        assert!(matches!(msg.content[0], MessageContent::Text(_)));
    }

    fn create_tool_pair(
        call_id: &str,
        response_id: &str,
        tool_name: &str,
        response_text: &str,
    ) -> Vec<Message> {
        vec![
            Message::assistant()
                .with_tool_request(
                    call_id,
                    Ok(CallToolRequestParams::new(tool_name.to_string())),
                )
                .with_id(call_id),
            Message::user()
                .with_tool_response(
                    call_id,
                    Ok(rmcp::model::CallToolResult::success(vec![
                        RawContent::text(response_text).no_annotation(),
                    ])),
                )
                .with_id(response_id),
        ]
    }

    struct MockProvider {
        message: Message,
        config: ModelConfig,
        max_tool_responses: Option<usize>,
    }

    impl MockProvider {
        fn new(message: Message, context_limit: usize) -> Self {
            Self {
                message,
                config: ModelConfig {
                    model_name: "test".to_string(),
                    context_limit: Some(context_limit),
                    temperature: None,
                    max_tokens: None,
                    toolshim: false,
                    toolshim_model: None,
                    request_params: None,
                    reasoning: None,
                },
                max_tool_responses: None,
            }
        }

        fn with_max_tool_responses(mut self, max: usize) -> Self {
            self.max_tool_responses = Some(max);
            self
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn get_name(&self) -> &str {
            "mock"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            // If max_tool_responses is set, fail if we have too many
            if let Some(max) = self.max_tool_responses {
                let tool_response_count = messages
                    .iter()
                    .filter(|m| {
                        m.content
                            .iter()
                            .any(|c| matches!(c, MessageContent::ToolResponse(_)))
                    })
                    .count();

                if tool_response_count > max {
                    return Err(ProviderError::ContextLengthExceeded(format!(
                        "Too many tool responses: {} > {}",
                        tool_response_count, max
                    )));
                }
            }

            let message = self.message.clone();
            let usage = ProviderUsage::new("mock-model".to_string(), Usage::default());
            Ok(stream_from_single_message(message, usage))
        }

        async fn get_context_limit(
            &self,
            _model_config: &ModelConfig,
        ) -> Result<usize, ProviderError> {
            Ok(self.config.context_limit())
        }
    }

    #[tokio::test]
    async fn test_keeps_tool_request() {
        let response_message = Message::assistant().with_text("<mock summary>");
        let provider = MockProvider::new(response_message, 1);
        let basic_conversation = vec![
            Message::user().with_text("read hello.txt"),
            Message::assistant()
                .with_tool_request("tool_0", Ok(CallToolRequestParams::new("read_file"))),
            Message::user().with_tool_response(
                "tool_0",
                Ok(rmcp::model::CallToolResult::success(vec![
                    RawContent::text("hello, world").no_annotation(),
                ])),
            ),
        ];

        let conversation = Conversation::new_unvalidated(basic_conversation);
        let model_config = provider.config.clone();
        let (compacted_conversation, _usage) = compact_messages(
            &provider,
            &model_config,
            "test-session-id",
            &conversation,
            false,
            &SummaryRequest::Transcript,
        )
        .await
        .unwrap();

        let agent_conversation = compacted_conversation.agent_visible_messages();

        let _ = Conversation::new(agent_conversation)
            .expect("compaction should produce a valid conversation");
    }

    fn chat_tools() -> Vec<Tool> {
        vec![Tool::new(
            "developer__shell",
            "Run a shell command",
            rmcp::object!({"type": "object", "properties": {"command": {"type": "string"}}}),
        )]
    }

    /// goose's turn-context block as `inject_moim` composes it (the chat's volatile tail).
    fn turn_context_block() -> String {
        "<turn-context>\n<current-time>2026-09-28 12:20:00</current-time>\n<working-directory>\
         /Users/mihaiperdum</working-directory>\n</turn-context>"
            .to_string()
    }

    /// The request body as the engine caches it: the turn-context block the next request drops
    /// removed from the end of the last user or tool message, a user message that held nothing
    /// else dropped whole — rank_boundary.py `stable_messages`, the key the conversation prefix
    /// is cut at (less the replay margin).
    fn stable_messages(body: &serde_json::Value, block: &str) -> Vec<serde_json::Value> {
        let mut messages = body["messages"].as_array().unwrap().clone();
        let at = messages
            .iter()
            .rposition(|m| m["role"] == "user" || m["role"] == "tool")
            .unwrap();
        let content = messages[at]["content"].as_str().unwrap().to_string();
        let stable = content
            .strip_suffix(&format!("\n{block}"))
            .or_else(|| content.strip_suffix(block))
            .unwrap_or_else(|| panic!("the chat request ends on its turn-context block: {content}"))
            .to_string();
        if stable.is_empty() && messages[at]["role"] == "user" {
            messages.remove(at);
        } else {
            messages[at]["content"] = serde_json::Value::String(stable);
        }
        messages
    }

    /// Q-342 on the wire: the summary request a compaction sends starts with the whole request the
    /// chat's last call sent — its system message, its tools, its messages up to the turn-context
    /// block the next request drops, its template switches — and ends on ONE user message
    /// carrying the instruction, so the engine reads the conversation from the entry that call
    /// left. The chat call goes through the agent's own provider path. NEGATIVE CONTROL: the
    /// transcript request (every compaction before Q-342; E2E #3p's) shares not even the system
    /// message with it, carries no tools, and switches thinking off.
    #[tokio::test]
    async fn the_summary_request_extends_the_chats_last_request_on_the_wire() {
        use crate::agents::Agent;
        use crate::model_config::mlx_endpoint::{thinking_off, MlxEndpoint, SERVED};
        use futures::StreamExt;

        let session = ModelConfig::new(SERVED);
        let system = "You are goose, a general-purpose agent.";
        let tools = chat_tools();
        let mut asked = vec![Message::user().with_text("list the notes")];
        asked.extend(create_tool_pair(
            "call1",
            "resp1",
            "developer__shell",
            "notes/kickoff.md",
        ));
        // The chat's last call: the conversation with the turn-context block on the newest tool
        // results (inject_moim's chat placement, then its fix_conversation).
        let block = turn_context_block();
        let mut with_block = asked.clone();
        with_block
            .last_mut()
            .unwrap()
            .content
            .push(MessageContent::text(&block));
        let (with_block, _) = fix_conversation(Conversation::new_unvalidated(with_block));

        let engine = MlxEndpoint::start().await;
        let mut stream = Agent::stream_response_from_provider(
            engine.provider.clone(),
            session.clone(),
            "s",
            system,
            with_block.messages(),
            &tools,
            &[],
        )
        .await
        .unwrap();
        while stream.next().await.is_some() {}

        // The turn ended on the model's answer and the person's next message arrived; the
        // auto-compaction at that reply's start summarizes it all.
        let mut conversation = asked.clone();
        conversation.push(Message::assistant().with_text("one file: notes/kickoff.md"));
        conversation.push(Message::user().with_text("now read it"));
        let conversation = Conversation::new_unvalidated(conversation);
        let frame = ChatRequestFrame {
            system_prompt: system.to_string(),
            tools: tools.clone(),
        };
        // Q-357: the person's note rides the instruction and nothing else.
        let note = "keep the 24-month cutoff exactly";
        let mut inputs = chat_inputs(Some(note), false);
        inputs.pins = vec!["seed 20260928".to_string()];
        let ChatCompacted::Compacted {
            conversation: compacted,
            ..
        } = compact_chat(
            engine.provider.as_ref(),
            &session,
            "s",
            &conversation,
            false,
            &SummaryRequest::ExtendsChat(frame),
            &inputs,
        )
        .await
        .unwrap()
        else {
            panic!("an automatic compaction never stops at a question");
        };
        compact_messages_with_tail(
            engine.provider.as_ref(),
            &session,
            "s",
            &conversation,
            false,
            0,
            &SummaryRequest::Transcript,
        )
        .await
        .unwrap();

        let bodies = engine.bodies().await;
        assert_eq!(bodies.len(), 3, "chat, summary, transcript: {bodies:?}");
        let (chat, summary, transcript) = (&bodies[0], &bodies[1], &bodies[2]);
        let cached = stable_messages(chat, &block);
        let sent = summary["messages"].as_array().unwrap();
        assert_eq!(
            &sent[..cached.len()],
            cached.as_slice(),
            "the summary request starts with the chat's cached request, byte for byte"
        );
        assert_eq!(summary["tools"], chat["tools"], "the chat's tools, as sent");
        assert_eq!(
            summary.get("chat_template_kwargs"),
            chat.get("chat_template_kwargs"),
            "the chat's template switches: a thinking switch can change the system block"
        );
        assert_eq!(
            sent[cached.len()],
            serde_json::json!({"role": "assistant", "content": "one file: notes/kickoff.md"})
        );
        assert_eq!(sent.len(), cached.len() + 2, "{sent:?}");
        let instruction = &sent[cached.len() + 1];
        assert_eq!(instruction["role"], "user");
        let instruction = instruction["content"].as_str().unwrap();
        assert!(
            instruction.starts_with("now read it\n## Summarize this conversation for yourself"),
            "the instruction joins the person's trailing message (no two user turns in a row): \
             {instruction}"
        );
        assert!(instruction.contains("Call no tool."));
        assert!(!instruction.contains("Conversation History"));
        assert!(!sent.iter().any(|m| m["content"]
            .as_str()
            .is_some_and(|c| c.contains("<turn-context>"))));
        // The note and the pins are in the final instruction message alone: every message before
        // it — the cached prefix — is the chat's own.
        assert!(instruction.contains(&format!(
            "The person's note for this compaction: \"{note}\""
        )));
        assert!(instruction.contains("`NOTE CONCERN: <one sentence>`"));
        assert!(
            !instruction.contains("NOTE QUESTION"),
            "a turn cannot wait for an answer"
        );
        for earlier in &sent[..cached.len() + 1] {
            let text = earlier.to_string();
            assert!(!text.contains(note) && !text.contains("20260928"), "{text}");
        }
        assert!(
            !summary.to_string().contains("seed 20260928"),
            "pins are kept, never sent"
        );

        // The summary is read back as before — the answer, re-roled to the user, agent-only — and
        // what goose kept follows it.
        let summary_message = compacted
            .messages()
            .iter()
            .find(|m| {
                m.as_concat_text()
                    .starts_with("reading project configuration")
            })
            .expect("the summary is in the compacted conversation");
        assert_eq!(summary_message.role, Role::User);
        assert!(summary_message.is_agent_visible() && !summary_message.is_user_visible());
        let stored = summary_message.as_concat_text();
        assert!(stored.contains("<kept-by-goose>"), "{stored}");
        assert!(stored.contains("[1] list the notes"), "{stored}");
        assert!(stored.contains(&format!("- Note for this compaction: {note}")));
        assert!(stored.contains("- Pinned: seed 20260928"));

        assert_ne!(transcript["messages"][0], chat["messages"][0]);
        assert!(transcript
            .get("tools")
            .is_none_or(|t| t.as_array().is_some_and(|t| t.is_empty())));
        assert_eq!(transcript["chat_template_kwargs"], thinking_off());
    }

    /// A summary request that extends the chat and ends mid tool loop: the instruction follows the
    /// tool results as a user message of its own (merged into them, the formatter would put it
    /// before the results).
    #[tokio::test]
    async fn the_instruction_follows_tool_results_as_its_own_user_message() {
        use crate::model_config::mlx_endpoint::{MlxEndpoint, SERVED};
        let session = ModelConfig::new(SERVED);
        let mut messages = vec![Message::user().with_text("list the notes")];
        messages.extend(create_tool_pair(
            "call1",
            "resp1",
            "developer__shell",
            "notes/kickoff.md",
        ));
        let engine = MlxEndpoint::start().await;
        compact_chat(
            engine.provider.as_ref(),
            &session,
            "s",
            &Conversation::new_unvalidated(messages),
            false,
            &SummaryRequest::ExtendsChat(ChatRequestFrame {
                system_prompt: "You are goose.".to_string(),
                tools: chat_tools(),
            }),
            &chat_inputs(None, false),
        )
        .await
        .unwrap();
        let bodies = engine.bodies().await;
        let sent = bodies[0]["messages"].as_array().unwrap();
        let roles: Vec<&str> = sent.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "tool", "user"]);
        assert!(sent[4]["content"]
            .as_str()
            .unwrap()
            .starts_with("## Summarize this conversation for yourself"));
    }

    /// A chat's compaction inputs with nothing of the chat's own but the note.
    fn chat_inputs(note: Option<&str>, may_ask: bool) -> ChatCompaction {
        ChatCompaction {
            trigger: if may_ask {
                CompactionTrigger::Manual
            } else {
                CompactionTrigger::Auto {
                    used: Some(143_000),
                    limit: Some(178_176),
                    threshold: 0.8,
                }
            },
            note: note.map(str::to_string),
            may_ask,
            pins: Vec::new(),
            goal: None,
            ledger: LedgerRead::Entries(Vec::new()),
            kept_budget_chars: None,
            progress: None,
        }
    }

    /// Answers every request with `answer`, streamed a few characters at a time, and records how
    /// many chunks the reader took before it stopped reading.
    struct ScriptedProvider {
        answer: String,
        taken: Arc<std::sync::atomic::AtomicUsize>,
        requests: std::sync::Mutex<Vec<Vec<Message>>>,
    }

    impl ScriptedProvider {
        fn new(answer: &str) -> Self {
            Self {
                answer: answer.to_string(),
                taken: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                requests: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        fn get_name(&self) -> &str {
            "scripted"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            use futures::StreamExt;
            self.requests.lock().unwrap().push(messages.to_vec());
            let chars: Vec<char> = self.answer.chars().collect();
            let chunks: Vec<String> = chars.chunks(4).map(|c| c.iter().collect()).collect();
            let last = chunks.len() - 1;
            let taken = self.taken.clone();
            let usage = ProviderUsage::new(
                "scripted".to_string(),
                Usage::new(Some(90), Some(10), Some(100)),
            );
            Ok(Box::pin(
                futures::stream::iter(chunks.into_iter().enumerate()).map(move |(i, chunk)| {
                    taken.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok((
                        Some(Message::assistant().with_text(chunk)),
                        (i == last).then(|| usage.clone()),
                    ))
                }),
            ))
        }
    }

    fn two_turn_chat() -> Conversation {
        Conversation::new_unvalidated(vec![
            Message::user().with_text("inactive means no login in 24 months"),
            Message::assistant().with_text("noted: 24 months"),
            Message::user().with_text("now plan the users"),
        ])
    }

    fn frame() -> SummaryRequest {
        SummaryRequest::ExtendsChat(ChatRequestFrame {
            system_prompt: "You are goose.".to_string(),
            tools: chat_tools(),
        })
    }

    /// Q-357 STEER: a manual compaction under a note the conversation contradicts stops at the
    /// model's question — the stream is dropped at the first line, nothing is replaced, and the
    /// question comes back word for word for the person to answer.
    #[tokio::test]
    async fn a_question_about_the_note_stops_the_compaction_at_its_first_line() {
        let provider = ScriptedProvider::new(
            "NOTE QUESTION: The chat says 24 months; do you mean to change it to 12?\n## Where we are\nplanning the users, a long summary that must never be read",
        );
        let outcome = compact_chat(
            &provider,
            &ModelConfig::new("scripted"),
            "s",
            &two_turn_chat(),
            true,
            &frame(),
            &chat_inputs(Some("use the 12-month cutoff"), true),
        )
        .await
        .unwrap();
        let ChatCompacted::Asked { question } = outcome else {
            panic!("the question stops the compaction");
        };
        assert_eq!(
            question,
            "The chat says 24 months; do you mean to change it to 12?"
        );
        let taken = provider.taken.load(std::sync::atomic::Ordering::SeqCst);
        let chunks = provider.answer.chars().count().div_ceil(4);
        assert!(
            taken < chunks,
            "the stream is dropped once the first line is whole: {taken} of {chunks} chunks read"
        );
    }

    /// Q-357: an automatic compaction cannot wait for an answer — its note is read as OK or a
    /// concern and the summary is written under it; a summary with no NOTE line says so plainly
    /// (Missing), never passes as OK; and the stored summary keeps neither the NOTE line nor any
    /// `<analysis>` scratch.
    #[tokio::test]
    async fn the_notes_verdict_is_read_and_the_stored_summary_drops_its_scratch() {
        for (answer, expected) in [
            (
                "NOTE CONCERN: the chat says 24, the note says 12.\n## Where we are\nplanning",
                NoteOutcome::Read(NoteVerdict::Concern(
                    "the chat says 24, the note says 12.".to_string(),
                )),
            ),
            (
                "NOTE OK\n## Where we are\nplanning",
                NoteOutcome::Read(NoteVerdict::Ok),
            ),
            (
                "<analysis>long scratch</analysis>\n## Where we are\nplanning",
                NoteOutcome::Read(NoteVerdict::Missing),
            ),
        ] {
            let provider = ScriptedProvider::new(answer);
            let ChatCompacted::Compacted {
                conversation, note, ..
            } = compact_chat(
                &provider,
                &ModelConfig::new("scripted"),
                "s",
                &two_turn_chat(),
                false,
                &frame(),
                &chat_inputs(Some("use the 12-month cutoff"), false),
            )
            .await
            .unwrap()
            else {
                panic!("an automatic compaction is never asked");
            };
            assert_eq!(note, expected, "{answer}");
            let stored = conversation.agent_visible_messages()[0].as_concat_text();
            assert!(
                stored.starts_with("## Where we are\nplanning\n\n<kept-by-goose>"),
                "{stored}"
            );
            assert!(
                !stored.contains("NOTE ") && !stored.contains("<analysis>"),
                "{stored}"
            );
        }
    }

    /// Q-357: the card reads what is written while it streams — the tokens and the section
    /// headings, in order.
    #[tokio::test]
    async fn the_summary_reports_its_parts_while_it_streams() {
        let provider = ScriptedProvider::new(
            "## Where we are\nplanning\n## Next step\nrun it\n## Decisions and reasons\n24 months",
        );
        let seen: Arc<std::sync::Mutex<Vec<WritingProgress>>> = Arc::default();
        let mut inputs = chat_inputs(None, false);
        let sink = seen.clone();
        inputs.progress = Some(Arc::new(move |p| sink.lock().unwrap().push(p)));
        compact_chat(
            &provider,
            &ModelConfig::new("scripted"),
            "s",
            &two_turn_chat(),
            false,
            &frame(),
            &inputs,
        )
        .await
        .unwrap();
        let seen = seen.lock().unwrap();
        let last = seen.last().unwrap();
        assert_eq!(
            last.parts,
            ["Where we are", "Next step", "Decisions and reasons"]
        );
        assert!(last.written_tokens.is_some_and(|t| t > 0));
        let counts: Vec<usize> = seen.iter().map(|p| p.parts.len()).collect();
        assert!(
            counts.windows(2).all(|w| w[0] <= w[1]) && counts[0] < 3,
            "{counts:?}"
        );
    }

    /// Q-357: a chat whose own request was refused as too long is summarized from a transcript,
    /// whose request carries no note — said as NotSent, with the note kept word for word.
    #[tokio::test]
    async fn a_note_the_transcript_request_cannot_carry_is_said_not_sent() {
        let provider = RecordingProvider {
            calls: std::sync::Mutex::new(Vec::new()),
            chat_shaped_answer: None,
        };
        let ChatCompacted::Compacted {
            conversation, note, ..
        } = compact_chat(
            &provider,
            &ModelConfig::new("recording-model"),
            "s",
            &two_turn_chat(),
            false,
            &frame(),
            &chat_inputs(Some("use the 12-month cutoff"), false),
        )
        .await
        .unwrap()
        else {
            panic!("compacted");
        };
        assert_eq!(note, NoteOutcome::NotSent);
        let calls = provider.calls.lock().unwrap();
        assert!(
            !calls[1].0.contains("12-month"),
            "the transcript request is byte-identical"
        );
        let stored = conversation.agent_visible_messages()[0].as_concat_text();
        assert!(stored.contains("- Note for this compaction: use the 12-month cutoff"));
    }

    /// Records every request; refuses the ones carrying tools as too long, or answers them with a
    /// tool call, as configured.
    struct RecordingProvider {
        calls: std::sync::Mutex<Vec<(String, Vec<Message>, usize)>>,
        chat_shaped_answer: Option<Message>,
    }

    #[async_trait]
    impl Provider for RecordingProvider {
        fn get_name(&self) -> &str {
            "recording"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            system: &str,
            messages: &[Message],
            tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.calls
                .lock()
                .unwrap()
                .push((system.to_string(), messages.to_vec(), tools.len()));
            let usage = ProviderUsage::new("recording".to_string(), Usage::default());
            if !tools.is_empty() {
                return match &self.chat_shaped_answer {
                    Some(answer) => Ok(stream_from_single_message(answer.clone(), usage)),
                    None => Err(ProviderError::ContextLengthExceeded(
                        "prompt of 180,000 tokens exceeds the 178,176 window".to_string(),
                    )),
                };
            }
            Ok(stream_from_single_message(
                Message::assistant().with_text("<transcript summary>"),
                usage,
            ))
        }
    }

    /// The chat-extending request is a first rung, never the only one: refused as too long, or
    /// answered with a tool call instead of a summary, the summary is asked for as a transcript
    /// (tools gone, the conversation inside the system prompt) — and the tool call never becomes
    /// the summary.
    #[tokio::test]
    async fn a_chat_shaped_summary_that_cannot_be_used_is_asked_again_as_a_transcript() {
        let conversation = Conversation::new_unvalidated(vec![
            Message::user().with_text("list the notes"),
            Message::assistant().with_text("one file: notes/kickoff.md"),
        ]);
        let frame = ChatRequestFrame {
            system_prompt: "You are goose.".to_string(),
            tools: chat_tools(),
        };
        for answer in [
            None,
            Some(
                Message::assistant()
                    .with_tool_request("t1", Ok(CallToolRequestParams::new("developer__shell"))),
            ),
        ] {
            let provider = RecordingProvider {
                calls: std::sync::Mutex::new(Vec::new()),
                chat_shaped_answer: answer,
            };
            let ChatCompacted::Compacted {
                conversation: compacted,
                ..
            } = compact_chat(
                &provider,
                &ModelConfig::new("recording-model"),
                "s",
                &conversation,
                false,
                &SummaryRequest::ExtendsChat(frame.clone()),
                &chat_inputs(None, false),
            )
            .await
            .unwrap()
            else {
                panic!("compacted");
            };
            let calls = provider.calls.lock().unwrap();
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0].0, "You are goose.");
            assert_eq!(calls[0].2, 1, "the first request extends the chat");
            assert_eq!(calls[1].2, 0, "the second is the transcript request");
            assert!(calls[1].0.contains("**Conversation History:**"));
            let visible: Vec<String> = compacted
                .agent_visible_messages()
                .iter()
                .map(|m| m.as_concat_text())
                .collect();
            assert!(
                visible[0].starts_with("<transcript summary>\n\n<kept-by-goose>"),
                "{}",
                visible[0]
            );
            assert!(compacted.agent_visible_messages().iter().all(|m| !m
                .content
                .iter()
                .any(|c| matches!(c, MessageContent::ToolRequest(_)))));
        }
    }

    /// `compaction.md` before Q-342, verbatim: the transcript request — the swarm workers' golden
    /// shape — must render byte-identically from the template that now also carries the
    /// chat-extending instruction.
    const COMPACTION_BEFORE_Q342: &str = "## Task Context\n- An llm context limit was reached when a user was in a working session with an agent (you)\n- Generate a version of the below messages with only the most verbose parts removed\n- Include user requests, your responses, all technical content, and as much of the original context as possible\n- This will be used to let the user continue the working session\n- Use framing and tone knowing the content will be read an agent (you) on a next exchange to allow for continuation of the session\n\n**Conversation History:**\n{{ messages }}\n\nWrap reasoning in `<analysis>` tags:  \n- Review conversation chronologically\n- For each part, log:  \n  - User goals and requests  \n  - Your method and solution  \n  - Key decisions and designs  \n  - File names, code, signatures, errors, fixes  \n- Highlight user feedback and revisions  \n- Confirm completeness and accuracy  \n- This summary will only be read by you so it is ok to make it much longer than a normal summary you would show to a human\n- Do not exclude any information that might be important to continuing a session working with you\n\n### Include the Following Sections:\n1. **User Intent** – All goals and requests  \n2. **Technical Concepts** – All discussed tools, methods  \n3. **Files + Code** – Viewed/edited files, full code, change justifications  \n4. **Errors + Fixes** – Bugs, resolutions, user-driven changes  \n5. **Problem Solving** – Issues solved or in progress  \n6. **User Messages** – All user messages including tool calls, but truncate long tool call arguments or results\n7. **Pending Tasks** – All unresolved user requests  \n8. **Current Work** – Active work at summary request time: filenames, code, alignment to latest instruction  \n9. **Next Step** – *Include only if* directly continues user instruction  \n\n> No new ideas unless user confirmed\n";

    /// The transcript request — the swarm workers' golden shape — renders byte-identically from the
    /// template that now also carries the chat's instruction. Q-357 replaced that instruction
    /// deliberately: it asks for the model's part alone (no `<analysis>` scratch, no nine
    /// sections), names what goose keeps from THIS chat's facts, and carries the note.
    #[test]
    fn the_transcript_prompt_is_unchanged_and_the_chat_instruction_is_built_from_its_facts() {
        let transcript = SummarizeContext {
            messages: Some("[user]: list the notes\n[assistant]: one file".to_string()),
            chat: None,
        };
        assert_eq!(
            render_template("compaction.md", &transcript).unwrap(),
            crate::prompt_template::render_string(COMPACTION_BEFORE_Q342, &transcript).unwrap()
        );

        let conversation = two_turn_chat();
        let mut inputs = chat_inputs(Some("use the 12-month cutoff"), true);
        inputs.goal = Some("a readiness report Aoife can sign off".to_string());
        let kept = Pillars::build(
            conversation.messages(),
            &KeptSources {
                note: inputs.note.as_deref(),
                pins: &[],
                preserved: None,
            },
            LedgerRead::Entries(vec!["2026-09-28 12:20 [fact] 10 pass / 0 fail".to_string()]),
        );
        let instruction = render_template(
            "compaction.md",
            &SummarizeContext {
                messages: None,
                chat: Some(chat_instruction(&inputs, &kept, conversation.messages())),
            },
        )
        .unwrap();
        assert!(!instruction.contains("Conversation History"));
        assert!(!instruction.contains("Wrap reasoning"));
        assert!(instruction.contains("The person asked goose to compact this conversation now."));
        assert!(instruction.contains("- the person's 2 messages, word for word"));
        assert!(instruction.contains("- the 1 ledger entry"));
        assert!(instruction.contains("The goal the person set with /goal: a readiness report"));
        assert!(instruction.contains("\"use the 12-month cutoff\""));
        assert!(instruction.contains("`NOTE QUESTION: <one question>`"));
        for (heading, _) in WRITTEN_PARTS {
            assert!(
                instruction.contains(&format!("## {heading}\n")),
                "{instruction}"
            );
        }
        assert!(
            instruction.ends_with("nothing before the NOTE line."),
            "{instruction}"
        );
        assert!(!instruction.contains("earlier summary"));

        let auto = chat_instruction(&chat_inputs(None, false), &kept, conversation.messages());
        assert_eq!(
            auto.trigger.as_deref(),
            Some(
                "The conversation reached 143,000 of 178,176 tokens (80%), past this chat's \
                 compaction point of 80%."
            )
        );
    }

    /// The swarm workers' compaction (no chat inputs): the request is the transcript shape and the
    /// stored summary is the model's answer as written — no kept block, nothing stripped.
    #[tokio::test]
    async fn a_compaction_without_chat_inputs_stores_the_answer_as_written() {
        let answer = "<analysis>scratch</analysis>\nNOTE OK\nthe summary";
        let provider = MockProvider::new(Message::assistant().with_text(answer), 1000);
        let (compacted, _) = compact_messages_with_tail(
            &provider,
            &provider.config.clone(),
            "s",
            &two_turn_chat(),
            false,
            0,
            &SummaryRequest::Transcript,
        )
        .await
        .unwrap();
        assert_eq!(
            compacted.agent_visible_messages()[0].as_concat_text(),
            answer
        );
        assert!(compact_messages_with_tail(
            &provider,
            &provider.config.clone(),
            "s",
            &two_turn_chat(),
            false,
            0,
            &frame(),
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn test_progressive_removal_on_context_exceeded() {
        let response_message = Message::assistant().with_text("<mock summary>");
        // Set max to 2 tool responses - will trigger progressive removal
        let provider = MockProvider::new(response_message, 1000).with_max_tool_responses(2);

        // Create a conversation with many tool responses
        let mut messages = vec![Message::user().with_text("start")];
        for i in 0..10 {
            messages.push(Message::assistant().with_tool_request(
                format!("tool_{}", i),
                Ok(CallToolRequestParams::new("read_file")),
            ));
            messages.push(Message::user().with_tool_response(
                format!("tool_{}", i),
                Ok(rmcp::model::CallToolResult::success(vec![
                    RawContent::text(format!("response{}", i)).no_annotation(),
                ])),
            ));
        }

        let conversation = Conversation::new_unvalidated(messages);
        let model_config = provider.config.clone();
        let result = compact_messages(
            &provider,
            &model_config,
            "test-session-id",
            &conversation,
            false,
            &SummaryRequest::Transcript,
        )
        .await;

        assert!(
            result.is_ok(),
            "Should succeed with progressive removal: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_compute_tool_call_cutoff_scales_with_context() {
        // Default threshold (0.8)
        assert_eq!(compute_tool_call_cutoff(128_000, 0.8), 15); // 102K effective
        assert_eq!(compute_tool_call_cutoff(200_000, 0.8), 24); // 160K effective
        assert_eq!(compute_tool_call_cutoff(1_000_000, 0.8), 120); // 800K effective
                                                                   // Clamp at minimum
        assert_eq!(compute_tool_call_cutoff(50_000, 0.8), 10);
        assert_eq!(compute_tool_call_cutoff(10_000, 0.8), 10);
        // Clamp at maximum (500)
        assert_eq!(compute_tool_call_cutoff(10_000_000, 0.8), 500);
        // Lower compaction threshold means earlier summarization
        assert_eq!(compute_tool_call_cutoff(200_000, 0.3), 10); // 60K effective
        assert_eq!(compute_tool_call_cutoff(1_000_000, 0.5), 75); // 500K effective
                                                                  // Invalid threshold falls back to default 0.8
        assert_eq!(compute_tool_call_cutoff(200_000, 0.0), 24); // falls back to 0.8
        assert_eq!(compute_tool_call_cutoff(200_000, -1.0), 24); // falls back to 0.8
    }

    #[test]
    fn test_tool_ids_to_summarize_triggers_at_cutoff_plus_batch() {
        // cutoff=5, so we need >5+10=15 to trigger. 15 exactly should NOT trigger.
        let mut messages = vec![Message::user().with_text("hello")];
        for i in 0..15 {
            messages.extend(create_tool_pair(
                &format!("call{}", i),
                &format!("resp{}", i),
                "read_file",
                "content",
            ));
        }
        let conversation = Conversation::new_unvalidated(messages);
        let result = tool_ids_to_summarize(&conversation, 5, 0);
        assert!(result.is_empty(), "Exactly cutoff+batch should not trigger");

        // 16 tool calls: now exceeds cutoff+10, should return a batch of 10
        let mut messages = vec![Message::user().with_text("hello")];
        for i in 0..16 {
            messages.extend(create_tool_pair(
                &format!("call{}", i),
                &format!("resp{}", i),
                "read_file",
                "content",
            ));
        }
        let conversation = Conversation::new_unvalidated(messages);
        let result = tool_ids_to_summarize(&conversation, 5, 0);
        assert_eq!(result.len(), TOOLCALL_SUMMARIZATION_BATCH_SIZE);
        assert_eq!(result[0], "call0");
        assert_eq!(result[9], "call9");
    }

    fn recorded_pair(
        name: &str,
        arguments: serde_json::Value,
        result: Result<rmcp::model::CallToolResult, rmcp::model::ErrorData>,
    ) -> String {
        let call = CallToolRequestParams::new(name.to_string())
            .with_arguments(arguments.as_object().unwrap().clone());
        let conversation = Conversation::new_unvalidated(vec![
            Message::user().with_text("go"),
            Message::assistant().with_tool_request("c1", Ok(call)),
            Message::user().with_tool_response("c1", result),
        ]);
        let record = record_tool_call(&conversation, "c1").unwrap();
        assert!(!record.is_user_visible() && record.is_agent_visible());
        record.as_concat_text()
    }

    fn shell_result(stdout: &str, stderr: &str, exit_code: i64) -> rmcp::model::CallToolResult {
        let text = if stderr.is_empty() {
            stdout.to_string()
        } else {
            format!("{stdout}\n{stderr}\n\nCommand exited with code {exit_code}")
        };
        let mut result = if exit_code == 0 {
            rmcp::model::CallToolResult::success(vec![RawContent::text(text).no_annotation()])
        } else {
            rmcp::model::CallToolResult::error(vec![RawContent::text(text).no_annotation()])
        };
        result.structured_content = Some(serde_json::json!({
            "stdout": stdout, "stderr": stderr, "exit_code": exit_code
        }));
        result
    }

    /// Every quoted excerpt in a record is text the tool actually returned.
    fn assert_excerpts_come_from(record: &str, source: &str) {
        for line in record.lines().filter(|l| l.starts_with("Output")) {
            let mut rest = line;
            while let Some(start) = rest.find('"') {
                let tail = rest.get(start..).unwrap();
                let quoted: String = serde_json::Deserializer::from_str(tail)
                    .into_iter::<String>()
                    .next()
                    .unwrap()
                    .unwrap();
                let consumed = serde_json::Value::String(quoted.clone()).to_string().len();
                let bare = quoted.trim_end_matches('…');
                assert!(source.contains(bare), "{bare:?} is not in the output");
                rest = tail.get(consumed..).unwrap();
            }
        }
    }

    /// Round 4, sessions.db 764255/764256 (#2b): a shell call that failed with exit 2.
    #[test]
    fn a_failed_shell_call_is_recorded_as_failed_with_its_own_stderr() {
        let stderr = "bash: -c: line 1: syntax error near unexpected token `newline'\nbash: -c: line 1: `</parameter>'";
        let record = recorded_pair(
            "shell",
            serde_json::json!({"command": "curl -s --max-time 20 -A \"Mozilla/5.0\" \"https://support.atlassian.com/totally-bogus-page-12345/\" | wc -c\n</parameter>\n!\n</parameter>\n!"}),
            Ok(shell_result("   69272", stderr, 2)),
        );
        assert!(record.starts_with(TOOL_RECORD_HEADER), "{record}");
        assert!(record.contains("shell {\"command\":"), "{record}");
        assert!(record.contains(" failed (exit 2)."), "{record}");
        assert!(record.contains("69272") && record.contains("unexpected token `newline'"));
        assert_excerpts_come_from(
            &record,
            &format!("   69272\n{stderr}\n\nCommand exited with code 2"),
        );
    }

    /// Round 4, 764166/764167: the model-written summary said the file "still ends with an em dash";
    /// the output's last lines are the od dump ending in "!" and a smart-quote count of 0.
    #[test]
    fn a_long_output_is_quoted_from_its_own_ends_never_paraphrased() {
        let output = r#"--- last 60 chars (od) ---
0000000    n   e   x   t       c   a   l   l       —  **  **       n   o
0000020    t   e   d       a   s       *   *   F   r   i       2   /   1
0000040    0   *   *       (   O   c   t       2   )   ?  \n  \n   <   /
0000060    p   a   r   a   m   e   t   e   r   >  \n   !
0000074
--- smart-quote count (expect 0) ---
0"#;
        let record = recorded_pair(
            "shell",
            serde_json::json!({"command": "cd work && echo '--- last 60 chars (od) ---' && tail -c 60 notes/kickoff.md | od -c | tail -8"}),
            Ok(shell_result(output, "", 0)),
        );
        assert!(record.contains(" succeeded (exit 0)."), "{record}");
        assert!(
            record.contains("Output, 8 lines; first: \"--- last 60 chars (od) ---\"; last: \"0\""),
            "{record}"
        );
        assert!(!record.contains("em dash"));
        assert_excerpts_come_from(&record, output);
    }

    /// Round 4, 764160/764161: a write that lost its `path`; 764261's summary of the pair before it
    /// ended "Write the E2E test artifacts … into this file." — a record states, it never asks.
    #[test]
    fn a_rejected_write_is_recorded_with_the_error_and_the_content_cut() {
        let content =
            "# Kickoff — Harbourline Freight Jira DC→Cloud Migration Readiness\n\n".repeat(20);
        let record = recorded_pair(
            "write",
            serde_json::json!({"content": content}),
            Ok(rmcp::model::CallToolResult::error(vec![RawContent::text(
                "Error: Failed to parse arguments: missing field `path`",
            )
            .no_annotation()])),
        );
        assert!(record.contains(" failed."), "{record}");
        assert!(
            record.contains("Output: \"Error: Failed to parse arguments: missing field `path`\"")
        );
        assert!(record.contains("…"), "a long argument is cut: {record}");
        assert!(record.len() < content.len(), "{record}");
        let body = record.trim_start_matches(TOOL_RECORD_HEADER);
        assert!(!body.contains("\nWrite ") && !body.to_lowercase().contains("please"));
    }

    #[test]
    fn a_protocol_error_is_recorded_as_the_error() {
        let record = recorded_pair(
            "fetch",
            serde_json::json!({"url": "https://support.atlassian.com/x"}),
            Err(rmcp::model::ErrorData::internal_error(
                "Resource not found: Request failed with status code 404",
                None,
            )),
        );
        assert!(
            record.contains(
                "fetch {\"url\":\"https://support.atlassian.com/x\"} failed: \"Resource not found: Request failed with status code 404\"."
            ),
            "{record}"
        );
    }

    #[tokio::test]
    async fn chat_agents_record_pairs_without_asking_the_model() {
        let mock = MockProvider::new(
            Message::assistant().with_text("Write the E2E test artifacts into this file."),
            1000,
        );
        let model_config = mock.config.clone();
        let provider: Arc<dyn Provider> = Arc::new(mock);
        let mut messages = vec![Message::user().with_text("hello")];
        for i in 0..16 {
            messages.extend(create_tool_pair(
                &format!("call{i}"),
                &format!("resp{i}"),
                "read_file",
                "content",
            ));
        }
        let conversation = Conversation::new_unvalidated(messages);
        let faithful = maybe_summarize_tool_pairs(
            provider.clone(),
            model_config.clone(),
            "s".to_string(),
            conversation.clone(),
            5,
            0,
            true,
            None,
        )
        .unwrap()
        .await
        .unwrap();
        assert_eq!(faithful.len(), TOOLCALL_SUMMARIZATION_BATCH_SIZE);
        for (record, _) in &faithful {
            let text = record.as_concat_text();
            assert!(text.starts_with(TOOL_RECORD_HEADER), "{text}");
            assert!(!text.contains("E2E"), "{text}");
        }
    }

    /// Q-294, E2E #3o's own calls: the last call of turn 4 read 112,257 of 113,451 prompt tokens
    /// from the split's cache, and turn 4's condensation made turn 5's first call read 0 of
    /// 109,655. The same conversation past the cutoff keeps its pairs while the provider serves
    /// the prompt from cache, and is condensed as before when it reports no cache reads.
    #[tokio::test]
    async fn a_prompt_served_from_cache_keeps_its_tool_pairs() {
        let turn4_last =
            Usage::new(Some(113_451), Some(167), None).with_cache_tokens(Some(112_257), None);
        assert_eq!(prompt_cache_read(&turn4_last), Some(112_257));
        assert_eq!(
            prompt_cache_read(&Usage::new(Some(41_020), None, None)),
            None
        );
        assert_eq!(
            prompt_cache_read(
                &Usage::new(Some(41_020), None, None).with_cache_tokens(Some(0), None)
            ),
            None,
            "a provider that read nothing from cache has no prefix to keep"
        );

        let mock = MockProvider::new(Message::assistant().with_text("unused"), 1000);
        let model_config = mock.config.clone();
        let provider: Arc<dyn Provider> = Arc::new(mock);
        let mut messages = vec![Message::user().with_text("hello")];
        for i in 0..16 {
            messages.extend(create_tool_pair(
                &format!("call{i}"),
                &format!("resp{i}"),
                "read_file",
                "content",
            ));
        }
        let conversation = Conversation::new_unvalidated(messages);
        let condense = |cached| {
            maybe_summarize_tool_pairs(
                provider.clone(),
                model_config.clone(),
                "s".to_string(),
                conversation.clone(),
                5,
                0,
                true,
                cached,
            )
        };
        assert!(condense(prompt_cache_read(&turn4_last)).is_none());
        let condensed = condense(None).unwrap().await.unwrap();
        assert_eq!(condensed.len(), TOOLCALL_SUMMARIZATION_BATCH_SIZE);
    }

    #[test]
    fn test_tool_ids_to_summarize_protects_current_turn() {
        // 20 tool pairs, cutoff=2 → 20 > 12, would normally trigger
        let mut messages = vec![Message::user().with_text("hello")];
        for i in 0..20 {
            messages.extend(create_tool_pair(
                &format!("call{}", i),
                &format!("resp{}", i),
                "read_file",
                "content",
            ));
        }
        let conversation = Conversation::new_unvalidated(messages);

        // No protection: 20 eligible, 20 > 12 → batch of 10
        let result = tool_ids_to_summarize(&conversation, 2, 0);
        assert_eq!(result.len(), TOOLCALL_SUMMARIZATION_BATCH_SIZE);

        // Protect last 8: 12 eligible, 12 <= 12 → nothing
        let result = tool_ids_to_summarize(&conversation, 2, 8);
        assert!(
            result.is_empty(),
            "Should not summarize when protected count leaves eligible <= cutoff + batch"
        );

        // Protect last 7: 13 eligible, 13 > 12 → batch of 10
        let result = tool_ids_to_summarize(&conversation, 2, 7);
        assert_eq!(result.len(), TOOLCALL_SUMMARIZATION_BATCH_SIZE);
        assert_eq!(result[0], "call0");
    }

    /// Q-211: what a summarizing model reads of a tool result is its model-visible content only;
    /// goose's own (unannotated) results read exactly as before.
    mod audience {
        use super::*;
        use rmcp::model::{CallToolResult, Content, RawTextContent};

        const SECRET: &str = "SECRET-for-the-person-only";

        fn with_user_only(visible: &str) -> CallToolResult {
            CallToolResult::success(vec![
                Content::text(visible),
                Content::text(SECRET).with_audience(vec![Role::User]),
                Content::image("aGk=", "image/png").with_audience(vec![Role::User]),
            ])
        }

        fn unannotated() -> CallToolResult {
            CallToolResult::success(vec![
                Content::text("a"),
                Content::text("b"),
                Content::image("aGk=", "image/png"),
            ])
        }

        fn pair(result: CallToolResult) -> Conversation {
            Conversation::new_unvalidated(vec![
                Message::user().with_text("go"),
                Message::assistant()
                    .with_tool_request("c1", Ok(CallToolRequestParams::new("probe".to_string()))),
                Message::user().with_tool_response("c1", Ok(result)),
            ])
        }

        #[test]
        fn the_compaction_text_of_an_unannotated_message_is_unchanged() {
            let msg = Message::user()
                .with_text("hi")
                .with_image("aGk=", "image/png")
                .with_tool_response("c1", Ok(unannotated()));
            assert_eq!(
                format_message_for_compacting(&msg),
                "[user]: hi\n[image: image/png]\ntool_response: a\nb"
            );
        }

        #[test]
        fn the_compaction_text_never_carries_user_only_content() {
            let msg = Message::user()
                .with_content(MessageContent::Text(
                    RawTextContent {
                        text: SECRET.to_string(),
                        meta: None,
                    }
                    .no_annotation()
                    .with_audience(vec![Role::User]),
                ))
                .with_tool_response("c1", Ok(with_user_only("seen")));
            assert_eq!(
                format_message_for_compacting(&msg),
                "[user]: tool_response: seen"
            );
        }

        #[test]
        fn the_tool_call_record_of_an_unannotated_result_is_unchanged() {
            let record = record_tool_call(&pair(unannotated()), "c1")
                .unwrap()
                .as_concat_text();
            assert_eq!(
                record,
                format!(
                    "{TOOL_RECORD_HEADER}\nprobe {{}} succeeded.\nOutput: \"a\\nb\" Plus 1 non-text item(s)."
                )
            );
        }

        #[test]
        fn the_tool_call_record_never_carries_user_only_content() {
            let record = record_tool_call(&pair(with_user_only("seen")), "c1")
                .unwrap()
                .as_concat_text();
            assert_eq!(
                record,
                format!("{TOOL_RECORD_HEADER}\nprobe {{}} succeeded.\nOutput: \"seen\"")
            );
        }

        #[tokio::test]
        async fn the_tool_pair_digest_never_sends_user_only_content() {
            use crate::model_config::mlx_endpoint::{MlxEndpoint, SERVED};
            let engine = MlxEndpoint::start().await;
            summarize_tool_call(
                engine.provider.as_ref(),
                &ModelConfig::new(SERVED),
                "s",
                &pair(with_user_only("seen")),
                "c1",
            )
            .await
            .unwrap();
            let bodies = engine.bodies().await;
            assert_eq!(bodies.len(), 1);
            let sent = bodies[0].to_string();
            assert!(sent.contains("tool_response: seen"), "{sent}");
            assert!(!sent.contains(SECRET), "{sent}");
        }
    }
}
