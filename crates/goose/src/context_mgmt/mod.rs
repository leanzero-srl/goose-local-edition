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
pub mod facts;
pub mod pillars;
pub mod state;
pub mod summary_check;

use pillars::{KeptSources, LedgerRead, NoteVerdict, Pillars};

pub const DEFAULT_COMPACTION_THRESHOLD: f64 = 0.8;

// ratio: one tool result plus the summary instruction, as a share of the window — the largest
// single-call growth of the Flash-Next Gauntlet sb-7.2 run (ple.6, 2026-10-30) was 22,944 prompt
// tokens on a 360,000 window (6.4%); the instruction and a second ask add ~2k more (8.3% in all).
const TOOL_RESULT_WINDOW_SHARE: f64 = 1.0 / 12.0;

/// tokens: the next answer's room when the model config sets no `max_tokens`. goose sends none to a
/// local engine on purpose (goose-sidecar engine.rs, lz.5), and `ModelConfig::max_output_tokens`
/// then answers a fictitious 4,096: on Flash-Next's 360,000 window that put the point at 325,904
/// and left ~9k for a thinking model's summary. 32,768 is the serve default of the engines that cap
/// such a request (rapid-mlx "Default max tokens: 32768"; Jundot Flash-Next ple.6 scheduled every
/// unset request at max_tokens=32768) and the largest answer the Flash-Next Gauntlet sb-7.2 run
/// measured (attempt-2, 154 answers: p50 469, p90 4,316, p99 15,607, max 32,768 — that cap).
pub const UNSET_OUTPUT_RESERVE_TOKENS: usize = 32_768;

/// A reserve the person set (`GOOSE_COMPACTION_RESERVE_TOKENS`) may move the point below the 0.8
/// floor of the derived rule — a 32,768 cap on a 128,000 window wants 0.66 — but never below half
/// the window: a compacted chat must fit well under its point, or every call compacts again.
const SET_RESERVE_FLOOR: f64 = 0.5;

/// tokens: the room a summary request must leave for its own answer, thinking included. The
/// Flash-Next summary call (ple.6, 289,943 prompt) answered 4,316 tokens in 299 s; the run's p99
/// answer was 15,607. A quarter of the window on a small one.
pub const SUMMARY_ANSWER_TOKENS: usize = 16_384;

/// The room a summary request leaves for its answer on a `window`-token window.
pub fn summary_answer_room(window: usize) -> usize {
    SUMMARY_ANSWER_TOKENS.min(window / 4)
}

/// The output the next call may take: the model config's `max_tokens` when set, else
/// [`UNSET_OUTPUT_RESERVE_TOKENS`] — never `max_output_tokens()`'s 4,096 stand-in.
pub fn output_reserve_tokens(model_config: &ModelConfig) -> usize {
    match model_config.max_tokens {
        Some(tokens) if tokens > 0 => tokens as usize,
        _ => UNSET_OUTPUT_RESERVE_TOKENS,
    }
}

/// The model config a summary request is sent with: `max_tokens` set to the reserve the
/// compaction point was computed with ([`output_reserve_tokens`]), cut to what the window leaves
/// after the request's `prompt` when both are measured. Before 2026-10-30 a model config without
/// `max_tokens` (how goose talks to local engines) sent none, so the engine's own default decided
/// how long the summary could be, whatever room the point had kept for it.
pub(crate) fn summary_request_config(
    model_config: &ModelConfig,
    window: Option<usize>,
    prompt: Option<usize>,
) -> ModelConfig {
    let reserve = output_reserve_tokens(model_config);
    let room = match (window, prompt) {
        (Some(window), Some(prompt)) => window.saturating_sub(prompt),
        _ => reserve,
    };
    let mut config = model_config.clone();
    config.max_tokens = Some(reserve.min(room).clamp(1, i32::MAX as usize) as i32);
    config
}

/// The ratio the person set (`GOOSE_AUTO_COMPACT_THRESHOLD`), when they set one.
fn configured_compaction_threshold() -> Option<f64> {
    Config::global()
        .get_param::<f64>("GOOSE_AUTO_COMPACT_THRESHOLD")
        .ok()
}

/// The threshold a caller that does not know the window reports: the person's ratio, else 0.8.
pub fn configured_or_default_threshold() -> f64 {
    configured_compaction_threshold().unwrap_or(DEFAULT_COMPACTION_THRESHOLD)
}

/// The compaction point of a `context_limit`-token window, as a share of it. ONE rule for the
/// guard, the compaction card, the turn-context countdown and the tool-pair cutoff.
///
/// A ratio the person set wins. Otherwise the point is the window minus a reserve — the next
/// answer at the model's output cap ([`output_reserve_tokens`]) plus one tool result — and never
/// earlier than 0.8; a reserve the person set is honoured down to half the window. Why
/// (Flash-Next, Gauntlet sb-7.2, 2026-10-30): the window was the MEASURED 360,000 the engine holds,
/// 0.8 compacted at 289,943 and left 70k of it unused, while the room the next call can take is
/// its output (cap 32,768) and one tool result (largest 22,944) with the summary instruction. `GOOSE_COMPACTION_RESERVE_TOKENS`
/// replaces the derived reserve.
pub fn compaction_threshold(context_limit: usize, model_config: &ModelConfig) -> f64 {
    let reserve = Config::global()
        .get_param::<usize>("GOOSE_COMPACTION_RESERVE_TOKENS")
        .ok();
    compaction_threshold_from(
        configured_compaction_threshold(),
        reserve,
        context_limit,
        output_reserve_tokens(model_config),
    )
}

/// [`compaction_threshold`] with its inputs given — the testable form.
pub fn compaction_threshold_from(
    configured: Option<f64>,
    reserve_tokens: Option<usize>,
    context_limit: usize,
    max_output_tokens: usize,
) -> f64 {
    if let Some(threshold) = configured {
        return threshold;
    }
    if context_limit == 0 {
        return DEFAULT_COMPACTION_THRESHOLD;
    }
    let reserve = reserve_tokens.unwrap_or_else(|| {
        max_output_tokens + (context_limit as f64 * TOOL_RESULT_WINDOW_SHARE) as usize
    });
    // At least one token of reserve: a point at the whole window reads as auto-compaction off.
    let point = context_limit.saturating_sub(reserve.max(1)) as f64 / context_limit as f64;
    match reserve_tokens {
        Some(_) => point.max(SET_RESERVE_FLOOR),
        None => point.max(DEFAULT_COMPACTION_THRESHOLD),
    }
}

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
/// the summary's parts while it streams. "What each file does" and "Still to do" since 2026-10-30:
/// the kept block names the files only by their line counts, and after the Flash-Next compaction
/// the model re-grepped its own modules (`grep -n "def \|class "`) to learn what it had built.
pub const WRITTEN_PARTS: [(&str, &str); 5] = [
    (
        "Where we are",
        "What is done and what is in progress at this moment — the files, commands and results \
         (counts, test outcomes) as they now stand.",
    ),
    (
        "What each file does",
        "Each file the work wrote or relies on, one line each: what it does now — its main \
         functions, routes or exports — and what is still missing in it.",
    ),
    (
        "Decisions and reasons",
        "Each decision this conversation made that the ledger does not already hold — the value \
         chosen (a seed, a threshold, a rule, an exception) and why — one line each.",
    ),
    (
        "Still to do",
        "Each part of the person's request that is not done or not yet verified, one line each, \
         with the check that will show it done.",
    ),
    (
        "Next step",
        "The one next step, as the person's latest request asks for it.",
    ),
];

/// `text` as its letters and digits, lowercased: what a section's own words are compared by.
fn bare_words(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The sections a summary needs, and how many of them must hold something.
const MIN_FILLED_SECTIONS: usize = WRITTEN_PARTS.len().div_ceil(2);

/// Whether a section's body holds anything of its own: a letter or digit once the instruction's
/// heading and ask for it (echoed back) are taken out. "None.", "Keine.", "- none", "n/a" hold
/// something — saying a list is empty is an answer.
fn section_filled(body: &str, part: Option<usize>) -> bool {
    let mut own = bare_words(body);
    if let Some(part) = part {
        let (heading, ask) = WRITTEN_PARTS[part];
        for template in [bare_words(ask), bare_words(heading)] {
            own = own.replace(&template, "");
        }
    }
    !own.is_empty()
}

/// A line that heads a section in any language: a markdown heading (its level), or a line that
/// opens in bold (`**Wo wir stehen**`, `1. **User Intent** – …`), or a short line ending in a
/// colon (`Prochaine étape :`). Returns the kind (the markdown level, else 0) and the words after
/// the heading on its line.
fn generic_heading(line: &str) -> Option<(usize, &str)> {
    let line = line.trim();
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && line.get(hashes..).is_some_and(|rest| rest.starts_with(' ')) {
        return Some((hashes, ""));
    }
    let unnumbered = match line.split_once(['.', ')']) {
        Some((number, after))
            if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) =>
        {
            after.trim_start()
        }
        _ => line,
    };
    if let Some(rest) = unnumbered.strip_prefix("**") {
        let (title, after) = rest.split_once("**")?;
        if title.trim().is_empty() || title.split_whitespace().count() > 8 {
            return None;
        }
        return Some((
            0,
            after.trim_start_matches([':', ' ', '–', '—', '-']).trim(),
        ));
    }
    let short = line.split_whitespace().count() <= 6;
    if short && line.ends_with(':') && !line.starts_with(['-', '*', '+', '>', '`', '|']) {
        return Some((0, ""));
    }
    None
}

/// The answer's sections by [`generic_heading`], outside code fences: the markdown headings of
/// the highest level used, else the bold or colon-ended lines. Each is its body (the words on the
/// heading's line, then the lines up to the next such heading).
fn generic_sections(text: &str) -> Vec<String> {
    let mut lines: Vec<(Option<(usize, String)>, &str)> = Vec::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(marker) = fence {
            if trimmed.starts_with(marker) {
                fence = None;
            }
            lines.push((None, line));
            continue;
        }
        if let Some(marker) = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m)) {
            fence = Some(marker);
            lines.push((None, line));
            continue;
        }
        lines.push((
            generic_heading(line).map(|(kind, inline)| (kind, inline.to_string())),
            line,
        ));
    }
    let top = lines
        .iter()
        .filter_map(|(heading, _)| heading.as_ref().map(|(kind, _)| *kind))
        .filter(|kind| *kind > 0)
        .min();
    let kind = match top {
        Some(level)
            if lines
                .iter()
                .filter(|(h, _)| h.as_ref().is_some_and(|(k, _)| *k == level))
                .count()
                > 1 =>
        {
            level
        }
        _ => 0,
    };
    let mut sections: Vec<String> = Vec::new();
    for (heading, line) in lines {
        match heading {
            Some((k, inline)) if k == kind => sections.push(inline),
            _ => {
                if let Some(body) = sections.last_mut() {
                    body.push('\n');
                    body.push_str(line);
                }
            }
        }
    }
    sections
}

/// Why `answer` is not the summary — `None` when it is one. STRUCTURAL ONLY (round 4): the
/// sections are there and most of them hold something. The five headings are read in their
/// usual markdown forms (`pillars::written_sections`), and an answer in another language or with
/// its own headings counts its own sections ([`generic_sections`]). A section that says its list
/// is empty ("None.", "Keine.", "- none") holds something.
///
/// Why: the round-2/3 gate judged the WORDS — a list of vacant words, a "concrete name the
/// conversation used" anchor — and an independent review of round 3 found it fragile both ways.
/// What a thin summary leaves out the kept block's facts carry (`facts`), so the gate refuses
/// only an answer that is not a summary at all: the Flash-Next stopped run's one line ("I will
/// now proceed to document the design decisions…"), a template echoed back, five empty headings.
pub(crate) fn summary_shortfall(answer: &str) -> Option<String> {
    let text = pillars::strip_scratch(answer);
    if !text.chars().any(char::is_alphanumeric) {
        return Some("is empty".to_string());
    }
    let named = pillars::written_sections(&text);
    let mut filled = [false; WRITTEN_PARTS.len()];
    let mut seen = [false; WRITTEN_PARTS.len()];
    for (part, body) in &named {
        seen[*part] = true;
        filled[*part] |= section_filled(body, Some(*part));
    }
    let (count, filled) = if seen.iter().all(|s| *s) {
        (seen.len(), filled.iter().filter(|f| **f).count())
    } else {
        let sections = generic_sections(&text);
        if sections.len() < WRITTEN_PARTS.len() {
            let missing: Vec<String> = WRITTEN_PARTS
                .iter()
                .zip(seen)
                .filter(|(_, seen)| !seen)
                .map(|((heading, _), _)| format!("`## {heading}`"))
                .collect();
            return Some(if named.is_empty() && sections.is_empty() {
                "has none of the sections".to_string()
            } else {
                format!(
                    "has {} of the {} sections (no {})",
                    sections.len().max(seen.iter().filter(|s| **s).count()),
                    WRITTEN_PARTS.len(),
                    missing.join(", ")
                )
            });
        }
        let filled = sections.iter().filter(|b| section_filled(b, None)).count();
        (sections.len(), filled)
    };
    (filled < MIN_FILLED_SECTIONS)
        .then(|| format!("leaves {} of its {count} sections empty", count - filled))
}

/// What an answer that is not the summary lacks, said to the model that wrote it.
fn summary_correction(shortfall: &str) -> String {
    format!(
        "Your answer {shortfall}: it is not the summary. Write the summary now: only the sections \
         the instruction above lists, in its order, from `## {}` on, each with what this \
         conversation did. A section with nothing to say says so (\"None.\"). Call no tool.",
        WRITTEN_PARTS[0].0
    )
}

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
    /// The chat's window in tokens (`effective_context_limit`, the guard's own); `None` when it is
    /// unknown. It sizes the kept block ([`kept_budget_chars`]), the kept tail
    /// ([`tail_budget_tokens`]) and the room the summary request leaves for its answer.
    pub context_limit: Option<usize>,
    /// The session's folder, which the kept block resolves relative paths against (Q-394).
    pub working_dir: std::path::PathBuf,
    pub progress: Option<CompactionObserver>,
    /// The turn's call budget when the compaction happened between its calls (the kept block says
    /// it); `None` at a reply's start and for `/compact`.
    pub budget: Option<pillars::CallBudget>,
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

/// The kept block fitted to its sixteenth of a `context_limit`-token window as goose's tokenizer
/// counts it. `Pillars::fit` cuts by chars at `CHARS_PER_TOKEN`; the person's words in CJK, or
/// JSON and hex in a failed call's output, run 2-4x more tokens than that, so a block that fits
/// its chars is fitted again to the chars its own tokens allow.
fn fit_kept_to_tokens(
    built: Pillars,
    context_limit: usize,
    counter: &crate::token_counter::TokenCounter,
) -> Pillars {
    let budget_tokens = tail_budget_tokens(context_limit);
    let mut budget_chars = kept_budget_chars(Some(context_limit)).unwrap_or(usize::MAX);
    let mut kept = built.clone().fit(Some(budget_chars));
    for _ in 0..3 {
        let rendered = kept.render();
        let tokens = counter.count_tokens(&rendered);
        if tokens <= budget_tokens {
            break;
        }
        let chars = rendered.chars().count();
        budget_chars = (chars as f64 * budget_tokens as f64 / tokens as f64 * 0.95) as usize;
        kept = built.clone().fit(Some(budget_chars));
    }
    kept
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
        usage: Box<ProviderUsage>,
        note: NoteOutcome,
        /// What goose kept word for word; `None` for a caller with no chat inputs.
        kept: Option<Box<Pillars>>,
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
        keep_tail_from_config().unwrap_or(0),
        request,
    )
    .await
}

fn keep_tail_from_config() -> Option<usize> {
    Config::global()
        .get_param::<usize>("GOOSE_COMPACT_KEEP_TAIL")
        .ok()
}

/// The kept tail's budget in tokens for a window of `context_limit` tokens: the same sixteenth as
/// the kept block.
pub fn tail_budget_tokens(context_limit: usize) -> usize {
    (context_limit as f64 * KEPT_WINDOW_SHARE) as usize
}

fn opens_on_a_tool_result(message: &Message) -> bool {
    message
        .content
        .iter()
        .any(|c| matches!(c, MessageContent::ToolResponse(_)))
}

/// A chat's kept tail: the newest messages that fit `budget_tokens` as goose's tokenizer counts
/// them (images at their real cost: `token_counter::image_tokens`), counted back from the end and
/// stopping at the first message that is not an ordinary turn or goose's own turn-context block
/// or harness nudge (an earlier summary, or a row an earlier compaction hid). Why: before 2026-10-30 a chat kept no
/// tail (`GOOSE_COMPACT_KEEP_TAIL` 0), so the model woke from a compaction with no exact bytes of
/// what it had just done — the Flash-Next stopped run spent 13 of its next 17 calls re-reading.
/// Counted in tokens since 2026-10-30: in chars an image was ~18 ("[image: image/png]") while the
/// image itself was sent, and CJK, JSON and hex run 2-4x more tokens than chars/4.
///
/// A tail never opens on a tool result whose request it leaves out (an orphan `role:tool` message
/// that OpenAI-compatible servers refuse): the request is kept with it when both fit the budget,
/// else the result is left out with its request.
pub fn tail_that_fits(
    messages: &[Message],
    budget_tokens: usize,
    counter: &crate::token_counter::TokenCounter,
) -> usize {
    // goose's own rows the model reads go into the tail too: its turn-context blocks and its
    // harness nudges (a length-cut continuation is the newest row of a long tool loop, and a tail
    // that stopped at it kept nothing).
    let ordinary = |message: &Message| {
        message.is_agent_visible()
            && (message.is_user_visible()
                || crate::conversation::is_turn_context_message(message)
                || is_harness_nudge(message))
    };
    // Each row as part of the conversation (round 4): an answer's reasoning its split rows
    // repeat is counted once, and added back only for the row a tail opens on.
    let costs = counter.message_costs(messages);
    // The person's `/compact` row: stored for the person alone just before a manual compaction
    // reads the conversation, it is the newest row and is passed over, not stopped at. Before
    // 2026-10-30 a manual compaction kept no tail at all, where an automatic one kept the newest
    // turns. It is never re-sent: `compact_core` copies only rows the model saw.
    let compact_command = |message: &Message| {
        message.role == Role::User
            && !message.is_agent_visible()
            && message.is_user_visible()
            && crate::agents::compaction_run::compact_command_note(&message.as_concat_text())
                .is_some()
    };
    let mut used = 0usize;
    let mut start = messages.len();
    while start > 0 {
        let message = &messages[start - 1];
        if compact_command(message) {
            start -= 1;
            continue;
        }
        if !ordinary(message) {
            break;
        }
        let tokens = costs[start - 1].tokens;
        if used + tokens + costs[start - 1].repeated_reasoning > budget_tokens {
            break;
        }
        used += tokens;
        start -= 1;
    }
    // The walk stopped at a message that did not fit (or was not an ordinary turn): when the tail
    // opens on a tool result, that message is its request, so the result goes with it.
    while start < messages.len() && opens_on_a_tool_result(&messages[start]) {
        start += 1;
    }
    // A tail of nothing but passed-over rows keeps nothing.
    if messages[start..].iter().all(|m| !m.is_agent_visible()) {
        return 0;
    }
    messages.len() - start
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
        } => Ok((conversation, *usage)),
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
    // A recovery compaction keeps no tail: the provider just refused a request goose had sized to
    // this window, so the window is not to be trusted with one.
    let keep_tail = match (keep_tail_from_config(), chat.context_limit) {
        (Some(keep_tail), _) => keep_tail,
        (None, _) if chat.trigger == CompactionTrigger::Recovery => 0,
        (None, Some(limit)) => {
            let counter = create_token_counter()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create token counter: {e}"))?;
            tail_that_fits(conversation.messages(), tail_budget_tokens(limit), &counter)
        }
        (None, None) => 0,
    };
    compact_core(
        provider,
        model_config,
        session_id,
        conversation,
        manual_compact,
        keep_tail,
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

    // A chat's kept turn-context blocks are goose's, not the person's words: they are never the
    // request a compaction preserves.
    let persons_text = |c: &MessageContent| match c {
        MessageContent::Text(text) => !crate::conversation::is_turn_context_text(&text.text),
        _ => false,
    };
    let has_text_only = |msg: &Message| {
        let has_text = msg.content.iter().any(persons_text);
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
            .filter(|c| persons_text(c))
            .filter_map(|c| c.as_text().map(str::to_string))
            .collect();

        if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join("\n"))
        }
    };

    // Find and preserve the most recent user message for non-manual compacts
    let (preserved_user_message, preserved_idx, is_most_recent) = if !manual_compact {
        let found_msg = messages.iter().enumerate().rev().find(|(_, msg)| {
            msg.is_agent_visible()
                && matches!(msg.role, rmcp::model::Role::User)
                && has_text_only(msg)
                && !is_harness_nudge(msg)
        });

        if let Some((idx, msg)) = found_msg {
            let is_last = messages[idx + 1..]
                .iter()
                .all(crate::conversation::is_turn_context_message);
            (Some(msg.clone()), Some(idx), is_last)
        } else {
            (None, None, false)
        }
    } else {
        (None, None, false)
    };

    // K4 KEEP-TAIL: summarize everything EXCEPT the last `keep_tail` messages, which survive
    // VERBATIM. The measured defect: a long tool loop's recent tail is all tool content, so
    // `has_text_only` preserves nothing from it and the summary is the ONLY survivor — a 40-turn
    // swarm sink loses the exact bytes of the file it is mid-way through editing and must re-read
    // it, which is precisely the re-reading spiral the swarm polices. A strong model survives
    // prose-only recall; a 27B does not. Default 0 => byte-identical for every non-swarm session.
    // At least two of the rows the model saw are summarized; rows hidden from it at the end (the
    // person's `/compact`) are not counted against that.
    // The swarm's workers keep the golden bound (the last two messages, seen or not).
    let trailing_hidden = match chat {
        Some(_) => messages
            .iter()
            .rev()
            .take_while(|m| !m.is_agent_visible())
            .count(),
        None => 0,
    };
    let keep_tail =
        keep_tail.min((messages.len() - trailing_hidden).saturating_sub(2) + trailing_hidden);
    let mut cut = messages.len() - keep_tail;
    // A kept tail may not OPEN on a tool response whose request was summarized away — that is an
    // orphan `role:tool` message and OpenAI-compatible servers reject the request outright. A
    // chat's tail (`tail_that_fits`) already keeps the pair within its budget or leaves both out;
    // here a chat drops the response with its request rather than drag a request of any size in
    // (a small result whose request was a big write pulled the whole write past the budget). The
    // swarm workers keep the golden rule: extend the tail backward until its first message carries
    // no ToolResponse; `cut == 0` degrades to keeping everything except the summary.
    if chat.is_some() {
        while cut < messages.len() && opens_on_a_tool_result(&messages[cut]) {
            cut += 1;
        }
    } else {
        while cut > 0 && cut < messages.len() && opens_on_a_tool_result(&messages[cut]) {
            cut -= 1;
        }
    }
    let messages_to_compact = &messages[..cut];

    // With a kept tail, the most recent user text is usually INSIDE it already — appending the
    // preserved copy too would duplicate the instruction the model most attends to.
    // A harness nudge in the tail is goose's, not the person's request: counted as it, the
    // person's real request was never re-sent before the tail.
    // The swarm's workers keep the golden test (any user text row in the tail).
    let preserved_in_tail = keep_tail > 0
        && messages[cut..].iter().any(|m| {
            matches!(m.role, rmcp::model::Role::User)
                && has_text_only(m)
                && (chat.is_none() || (m.is_agent_visible() && !is_harness_nudge(m)))
        });
    let preserved_text = preserved_user_message
        .as_ref()
        .filter(|_| !preserved_in_tail)
        .and_then(extract_text);

    let counter = match chat.and_then(|chat| chat.context_limit) {
        Some(_) => Some(
            create_token_counter()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create token counter: {e}"))?,
        ),
        None => None,
    };
    let kept = chat.map(|chat| {
        let built = Pillars::build(
            messages,
            &KeptSources {
                note: chat.note.as_deref(),
                pins: &chat.pins,
                preserved: preserved_text.as_deref(),
                working_dir: Some(&chat.working_dir),
                budget: chat.budget,
            },
            chat.ledger.clone(),
        );
        let mut kept = match (chat.context_limit, &counter) {
            (Some(limit), Some(counter)) => fit_kept_to_tokens(built, limit, counter),
            _ => built.fit(None),
        };
        kept.tail_messages = messages[cut..]
            .iter()
            .filter(|m| m.is_agent_visible())
            .count();
        kept
    });

    // A summary request that extends the chat sends the WHOLE conversation even when a tail is
    // kept: its prompt is then the prefix the chat's last call cached, while a shorter one cannot be
    // served from a hybrid model's non-trimmable cache (qwen4_exp: LCP unavailable, full prefill).
    let summarized = match request {
        SummaryRequest::ExtendsChat(_) if chat.is_some() => messages,
        _ => messages_to_compact,
    };
    let (summary_message, summarization_usage, note, flag) = match do_compact(
        provider,
        model_config,
        session_id,
        summarized,
        request,
        chat.zip(kept.as_ref()),
    )
    .await?
    {
        Summary::Written {
            message,
            usage,
            note,
            flag,
        } => (*message, usage, note, flag),
        Summary::Asked { question } => return Ok(ChatCompacted::Asked { question }),
    };
    let summary_message = match &kept {
        Some(kept) => {
            // Round 5: a summary the gate passed is still FLAGGED when it is thin for what it
            // replaces, mostly under a sentence a section, or mostly what the model will do next;
            // a flagged one is stored after goose's facts. Numbers the conversation never showed
            // are marked "(unverified)" (`summary_check`).
            let written = summary_message.as_concat_text();
            let flag = flag.or_else(|| {
                summary_check::thin_summary(
                    &written,
                    summary_check::replaced_chars(messages_to_compact),
                )
                .map(|why| format!("the model's answer passed the gate, but {why}"))
            });
            if let Some(why) = &flag {
                tracing::warn!(
                    event = "compaction_summary_flagged",
                    reason = %why,
                    "compaction: the summary is stored flagged, after goose's facts"
                );
            }
            let rendered = kept.render();
            let system = match request {
                SummaryRequest::ExtendsChat(frame) => frame.system_prompt.as_str(),
                SummaryRequest::Transcript => "",
            };
            let known = summary_check::known_text(messages, &[&rendered, system]);
            let written = summary_check::mark_unverified_numbers(&written, &known);
            let written = summary_check::mark_missing_paths(
                &written,
                kept.chat_folder.as_deref().map(std::path::Path::new),
            );
            let unverified = written.matches(summary_check::UNVERIFIED).count();
            if unverified > 0 {
                tracing::warn!(
                    event = "compaction_numbers_unverified",
                    count = unverified,
                    "compaction: {unverified} numbers in the summary are not in the conversation"
                );
            }
            let stored = pillars::stored_summary(&written, kept, flag.as_deref());
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
            && preserved_idx == Some(idx)
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

    // A chat's person's request goes BEFORE its kept tail: the tail is the work that request
    // asked for, and re-sent after it the request reads as new work. The swarm workers keep the
    // golden order (tail, then the request).
    let mut preserved_text = preserved_text;
    if chat.is_some() && cut < messages.len() {
        if let Some(text) = preserved_text.take() {
            final_messages.push(Message::user().with_text(&text));
        }
    }

    // K4: the kept tail returns VERBATIM after the summary, so the agent reads "summary of the
    // early conversation, then the last turns exactly as they happened" — same shape as the
    // preserved-user mechanism, generalized to tool content.
    // Only rows the model saw: a row hidden from it (the person's `/compact`, which
    // `tail_that_fits` passes over) stays hidden.
    // Each copy is dated now: the session store reads a conversation back ordered by its rows'
    // `created` (then insertion), so a copy keeping its original's time was read back BEFORE the
    // summary — the model got the newest turns, then the summary of what came before them.
    // The swarm's workers copy every tail row as their golden run did. Their copies are dated now
    // too, and that is the one change to them: no request carries a row's date, so a worker's
    // requests within the reply that compacted are unchanged byte for byte, and a reply that reads
    // the conversation back from the store now gets the order the worker had in memory (summary,
    // then tail) where it got the tail first.
    let now = chrono::Utc::now().timestamp();
    for msg in messages[cut..]
        .iter()
        .filter(|m| chat.is_none() || m.is_agent_visible())
    {
        let mut copy = msg.clone().with_metadata(MessageMetadata::agent_only());
        copy.created = copy.created.max(now);
        final_messages.push(copy);
    }

    if let Some(text) = preserved_text {
        final_messages.push(Message::user().with_text(&text));
    }

    Ok(ChatCompacted::Compacted {
        conversation: Conversation::new_unvalidated(final_messages),
        usage: Box::new(summarization_usage),
        note,
        kept: kept.map(Box::new),
    })
}

/// A row the agent loop added to keep going (`crate::agents::harness_nudge`: the length-cut and
/// empty-turn continuations, goal and stop-hook reminders) is not the person's request. Preserved
/// after a compaction it became the chat's last word: Nex-N2.5-mini (laptop, 2026-10-19, run
/// openai-cloud-21a3f68b) read "continue in smaller steps" under a summary and answered with prose,
/// so the session ended at call 54 of 300. Decided by the row's marker, never by its words: a
/// person who types the same sentence is still the person. The person's own message still reaches
/// the model inside the kept block.
fn is_harness_nudge(msg: &Message) -> bool {
    msg.metadata.harness_nudge
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
///
/// `session_id` is the chat the window is for: a routed model (`strategy:<id>`, `node:<id>`)
/// answers the window of the node that serves THAT chat (Q-463), which the provider reads from
/// the session scope — and every consumer asks outside the reply's provider calls, where no scope
/// is set.
pub async fn effective_context_limit(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: Option<&str>,
) -> Option<usize> {
    let measured = match session_id {
        Some(session_id) => {
            crate::session_context::with_session_id(
                Some(session_id.to_string()),
                provider.get_context_limit(model_config),
            )
            .await
        }
        None => provider.get_context_limit(model_config).await,
    };
    let context_limit = match measured {
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
    let context_limit = match session_id.and_then(|id| refused_window(id, &model_config.model_name))
    {
        Some(window) => context_limit.min(window),
        None => context_limit,
    };
    let context_limit = match session_id
        .and_then(compressed)
        .and_then(|prompt| prompt.window)
    {
        Some(window) => context_limit.min(window),
        None => context_limit,
    };
    Some(
        match Config::global().get_param::<usize>("GOOSE_LOCAL_CONTEXT_CAP") {
            Ok(cap) if cap > 0 => context_limit.min(cap),
            _ => context_limit,
        },
    )
}

type RefusedWindows = std::collections::HashMap<(String, String), usize>;

/// I-9: the window the provider's own last context-length refusal named, per session and model.
/// The probe reads a served window once; a memory-guarded engine can serve less later (Xing on the
/// Studio, 2026-10-18: probed 262,144, refused at "maximum context here is 35328 tokens"), and
/// compaction kept sizing to the probe until the session ended.
static REFUSED_WINDOWS: std::sync::LazyLock<std::sync::Mutex<RefusedWindows>> =
    std::sync::LazyLock::new(Default::default);

fn refused_window(session_id: &str, model: &str) -> Option<usize> {
    REFUSED_WINDOWS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&(session_id.to_string(), model.to_string()))
        .copied()
}

/// Remembers the window a context-length refusal names, so `effective_context_limit` (and with it
/// proactive compaction and the recovery summary) sizes to it from now on. The latest refusal wins:
/// it says what the engine can hold now. A refusal that counts an output reservation inside the
/// window (OpenRouter's "… 10000 in the output") leaves the prompt only the rest of it: sized to
/// the whole window, the next request would be refused again at the same size.
pub fn note_refused_window(session_id: &str, model: &str, error: &ProviderError) -> Option<usize> {
    let ProviderError::ContextLengthExceeded(text) = error else {
        return None;
    };
    let named = goose_providers::errors::refused_context_window(text)?;
    let window = goose_providers::errors::refused_output_reservation(text)
        .filter(|output| *output < named)
        .map_or(named, |output| named - output);
    remember_window(session_id, model, window);
    Some(window)
}

fn remember_window(session_id: &str, model: &str, window: usize) {
    REFUSED_WINDOWS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert((session_id.to_string(), model.to_string()), window);
}

/// A provider-reported prompt below this share of goose's own count of the request it sent means
/// the provider dropped part of it (OpenRouter's middle-out). The share is far below any
/// tokenizer disagreement — goose counts o200k, a provider its own vocabulary — measured on the
/// Grok 4.7 Forge 2.0 build: billed 207,145 at call #181 against goose's count of what it sent,
/// then 76,843 at #182 while goose kept sending the whole history (~273k).
pub const COMPRESSED_PROMPT_SHARE: f64 = 0.5;

/// Whether a call's reported prompt says the provider compressed what goose sent.
pub fn prompt_was_compressed(sent_tokens: usize, reported_prompt_tokens: usize) -> bool {
    sent_tokens > 0
        && (reported_prompt_tokens as f64) < sent_tokens as f64 * COMPRESSED_PROMPT_SHARE
}

/// A call the provider compressed: what goose sent, and the window it showed it would hold.
#[derive(Clone, Copy)]
struct CompressedPrompt {
    sent: usize,
    window: Option<usize>,
}

/// Per session, until the next uncompressed call or a compaction ends it. Kept apart from the
/// refused windows, which stand for the session: one compressed call (one OpenRouter routed to a
/// small host) says what that call was served, not what the model can hold.
static COMPRESSED_PROMPTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, CompressedPrompt>>,
> = std::sync::LazyLock::new(Default::default);

fn compressed(session_id: &str) -> Option<CompressedPrompt> {
    COMPRESSED_PROMPTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(session_id)
        .copied()
}

pub(crate) fn compressed_prompt(session_id: &str) -> Option<usize> {
    compressed(session_id).map(|prompt| prompt.sent)
}

/// A provider compressed this call's prompt (`prompt_was_compressed`): the usage it reports is
/// what it kept, not what the conversation holds, and reading it as the context hides the very
/// truncation from the compaction guard. From now on the guard counts `sent_tokens`, and the
/// window is at most `delivered_tokens` — what the provider showed it would hold — so the next
/// check compacts the conversation instead of letting the provider cut its middle again.
///
/// `fixed_tokens` is the system prompt and tools, which compaction cannot shrink: a provider that
/// held less than twice that has no room a compacted chat could fit in, and sizing to it would
/// compact on every call, so only the count is kept and the warning says why.
pub fn note_compressed_prompt(
    session_id: &str,
    model: &str,
    sent_tokens: usize,
    delivered_tokens: usize,
    fixed_tokens: usize,
) {
    let sized = delivered_tokens >= fixed_tokens.saturating_mul(2);
    COMPRESSED_PROMPTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            session_id.to_string(),
            CompressedPrompt {
                sent: sent_tokens,
                window: sized.then_some(delivered_tokens),
            },
        );
    tracing::warn!(
        session_id,
        model,
        sent_tokens,
        delivered_tokens,
        fixed_tokens,
        window_sized_to_delivered = sized,
        "prompt_compressed_by_provider: the provider reported a prompt under {:.0}% of what goose \
         sent, so it dropped part of the conversation; compaction now counts what goose sent{}",
        COMPRESSED_PROMPT_SHARE * 100.0,
        if sized {
            " and sizes to what the provider held"
        } else {
            " (the provider held less than twice the system prompt and tools, so the window is not \
             sized to it)"
        }
    );
}

/// A call the provider did not compress, or a compaction, ends the compressed reading.
pub fn forget_compressed_prompt(session_id: &str) {
    COMPRESSED_PROMPTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(session_id);
}

/// The context the compaction guard reads: the provider's reported usage, or what goose sent when
/// the provider is known to have compressed the last call.
fn guarded_context_tokens(reported: Option<i32>, compressed_sent: Option<usize>) -> Option<usize> {
    let reported = reported.map(|tokens| tokens.max(0) as usize);
    match (reported, compressed_sent) {
        (Some(reported), Some(sent)) => Some(reported.max(sent)),
        (reported, sent) => reported.or(sent),
    }
}

/// The last chat request goose sent and the prompt its provider reported for it: the base the
/// compaction guard adds the rows written since to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SentPrompt {
    prompt_tokens: usize,
    /// The conversation's rows the request carried (before `fix_conversation`: the rows the
    /// guard reads back), and the newest of them, by [`row_fingerprint`].
    rows: usize,
    last_row: Option<[u8; 32]>,
}

static SENT_PROMPTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, SentPrompt>>,
> = std::sync::LazyLock::new(Default::default);

/// A row as the session store gives it back: its role, time and content. Not its id (the store
/// gives a row without one an id of its own) and not its metadata (a compaction rewrites it).
fn row_fingerprint(message: &Message) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(format!("{:?}|{}|", message.role, message.created).as_bytes());
    hasher.update(&serde_json::to_vec(&message.content).unwrap_or_default());
    *hasher.finalize().as_bytes()
}

/// Notes what a chat's call carried — `sent`, the conversation's rows — and the prompt tokens its
/// provider reported (`Usage::input_tokens`, cache reads included). A call that reported none
/// leaves no base: the guard then reads the session's stored usage, or counts.
pub fn note_sent_prompt(session_id: &str, sent: &[Message], prompt_tokens: Option<i32>) {
    let mut prompts = SENT_PROMPTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match prompt_tokens.filter(|tokens| *tokens > 0) {
        Some(tokens) => {
            prompts.insert(
                session_id.to_string(),
                SentPrompt {
                    prompt_tokens: tokens as usize,
                    rows: sent.len(),
                    last_row: sent.last().map(row_fingerprint),
                },
            );
        }
        None => {
            prompts.remove(session_id);
        }
    }
}

/// The reported prompt of the chat's last call and where the rows written since begin in
/// `messages` — `None` when no call is noted, or `messages` no longer open with what it sent (a
/// compaction rewrote them, or the session was reloaded and its rows differ).
fn prompt_and_rows_since(session_id: &str, messages: &[Message]) -> Option<(usize, usize)> {
    let sent = SENT_PROMPTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(session_id)
        .cloned()?;
    if sent.rows > messages.len() {
        return None;
    }
    let last = sent
        .rows
        .checked_sub(1)
        .map(|i| row_fingerprint(&messages[i]));
    (last == sent.last_row).then_some((sent.prompt_tokens, sent.rows))
}

/// Whether the guard will COUNT this chat's whole request (no call it can start from): the
/// caller then passes the request's frame, which the count includes.
pub fn guard_counts_the_whole_request(
    session: &crate::session::Session,
    messages: &[Message],
) -> bool {
    prompt_and_rows_since(&session.id, messages).is_none() && session.usage.total_tokens.is_none()
}

/// How the compaction guard counts a conversation.
#[derive(Debug, Clone, Copy)]
pub enum GuardCount<'a> {
    /// The swarm's workers, as their golden run measured: the session's stored total, else each
    /// agent-visible message counted alone.
    Worker,
    /// A chat ([`chat_context_tokens`]): the provider's prompt for its last call plus the rows
    /// written since. The frame (system prompt, tools) is counted when no call was reported.
    Chat(Option<(&'a str, &'a [Tool])>),
}

/// Where a chat's count came from, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountSource {
    /// The provider's prompt for the chat's last call, plus goose's count of the rows since.
    LastCall,
    /// The session's stored usage (the last call before a reload), plus the rows after its answer.
    StoredUsage,
    /// goose's tokenizer over the whole request: no call has reported a prompt.
    Estimated,
}

/// The tokens a chat's next request carries, as the compaction guard decides on them. The
/// provider's own count is trusted: the prompt its last call reported (`note_sent_prompt`) plus
/// goose's count of only the rows written since (sequence-counted, [`TokenCounter::message_costs`]).
/// After a reload the stored usage of the last call stands in, plus the rows after its answer.
/// Only with neither is the whole request counted, its frame included.
///
/// Why (round 4, 2026-10-30): the guard counted each message alone, so the split rows' repeated
/// reasoning was counted N times — the stopped Flash-Next run's 223 rows read 277-281k against the
/// 243,299 its engine reported for them with the summary instruction — and it counted no system
/// prompt or tools at all.
pub fn chat_context_tokens(
    session_id: &str,
    messages: &[Message],
    stored_total: Option<i32>,
    frame: Option<(&str, &[Tool])>,
    counter: &crate::token_counter::TokenCounter,
) -> (usize, CountSource) {
    let costs = counter.message_costs(messages);
    let since =
        |start: usize| goose_counted(costs[start..].iter().map(|c| c.tokens).sum::<usize>());
    if let Some((prompt, start)) = prompt_and_rows_since(session_id, messages) {
        return (prompt + since(start), CountSource::LastCall);
    }
    if let Some(total) = stored_total.filter(|t| *t > 0) {
        // The stored total is the last call's prompt and its answer: what follows the answer is new.
        let after_answer = messages
            .iter()
            .rposition(|m| m.is_agent_visible() && m.role == Role::Assistant)
            .map_or(0, |i| i + 1);
        return (
            total as usize + since(after_answer),
            CountSource::StoredUsage,
        );
    }
    let (system_prompt, tools) = frame.unwrap_or(("", &[]));
    (
        goose_counted(counter.count_chat_tokens(system_prompt, messages, tools)),
        CountSource::Estimated,
    )
}

// measured (round 4, the Flash-Next stopped run's rows, frame and tools): goose's o200k count
// ran under the engines' own — -3.4% at 223 rows on Flash-Next (234,979 vs 243,299), -6.1% to
// -5.2% at 93-181 rows on Qwen3.5-9B (128,932 vs 137,879 ... 204,418 vs 215,606). 1.065 puts
// every one of those within -0.6%..+2.9%: a count the guard decides on may run over the
// engine's, not under it.
const GOOSE_COUNT_MARGIN: f64 = 1.065;

/// goose's own count of tokens the guard decides on, raised by [`GOOSE_COUNT_MARGIN`]: only the
/// rows a provider has not counted yet, or the whole request when none has.
fn goose_counted(tokens: usize) -> usize {
    (tokens as f64 * GOOSE_COUNT_MARGIN).ceil() as usize
}

pub async fn check_if_compaction_needed(
    provider: &dyn Provider,
    conversation: &Conversation,
    threshold_override: Option<f64>,
    session: &crate::session::Session,
    count: GuardCount<'_>,
) -> Result<bool> {
    if provider.manages_own_context() {
        return Ok(false);
    }

    let messages = conversation.messages();

    let model_config = session
        .model_config
        .clone()
        .unwrap_or_else(|| ModelConfig::new("unknown"));
    let Some(context_limit) =
        effective_context_limit(provider, &model_config, Some(&session.id)).await
    else {
        return Ok(false);
    };
    let threshold =
        threshold_override.unwrap_or_else(|| compaction_threshold(context_limit, &model_config));

    let current_tokens = match count {
        GuardCount::Worker => {
            match guarded_context_tokens(session.usage.total_tokens, compressed_prompt(&session.id))
            {
                Some(tokens) => tokens,
                None => {
                    let token_counter = create_token_counter()
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to create token counter: {}", e))?;

                    let token_counts: Vec<_> = messages
                        .iter()
                        .filter(|m| m.is_agent_visible())
                        .map(|msg| {
                            token_counter.count_chat_tokens("", std::slice::from_ref(msg), &[])
                        })
                        .collect();

                    token_counts.iter().sum()
                }
            }
        }
        GuardCount::Chat(frame) => {
            let counter = create_token_counter()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create token counter: {}", e))?;
            let (tokens, source) = chat_context_tokens(
                &session.id,
                messages,
                session.usage.total_tokens,
                frame,
                &counter,
            );
            tracing::debug!(tokens, ?source, "compaction guard: the chat's next request");
            // A provider that compressed the last call reported less than it was sent: the guard
            // reads what goose sent.
            match compressed_prompt(&session.id) {
                Some(sent) => tokens.max(sent),
                None => tokens,
            }
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
#[allow(clippy::large_enum_variant)] // one per compaction, moved once: boxing buys nothing
enum Summary {
    Written {
        message: Box<Message>,
        usage: ProviderUsage,
        note: NoteOutcome,
        /// Why the model's answer could not be checked as the summary; it is stored flagged,
        /// beside goose's facts (round 4).
        flag: Option<String>,
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
    let Some((chat, _)) = chat else {
        let (message, usage) =
            summarize_a_transcript_as_the_swarm(provider, model_config, session_id, messages)
                .await?;
        return Ok(Summary::Written {
            message: Box::new(message),
            usage,
            note: NoteOutcome::NoNote,
            flag: None,
        });
    };
    let (message, usage) = summarize_a_transcript(
        provider,
        model_config,
        session_id,
        messages,
        chat.context_limit
            .or_else(|| refused_window(session_id, &model_config.model_name)),
    )
    .await?;
    // The transcript answer is checked as the chat's is, and stored flagged when it fails: no
    // second transcript request (another full prefill buys the same answer).
    let flag = summary_shortfall(&message.as_concat_text())
        .map(|why| format!("the model's answer to the transcript request {why}"));
    let note = match chat.note {
        Some(_) => NoteOutcome::NotSent,
        None => NoteOutcome::NoNote,
    };
    Ok(Summary::Written {
        message: Box::new(message),
        usage,
        note,
        flag,
    })
}

enum ChatSummary {
    Written(Box<Summary>),
    /// The model asked about the person's note instead of summarizing.
    Asked(String),
    /// The provider refused the request as too long, or goose measured it would not fit: which,
    /// for the log. The one case a chat's summary falls to the transcript request.
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
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
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
/// the instruction. The chat's messages are `fix_conversation`'s output (`moim::fixed_for_request`
/// fixes every request, its kept turn-context blocks included), so these are too — which is also what places the instruction: it
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
    // The summary's answer needs room of its own, thinking included. A request that would leave it
    // less is not sent: the answer would be cut at `length` and fail the gate after a full prefill.
    // The transcript request, without the tools and the kept tail, is measured to fit instead.
    let mut measured_prompt = None;
    if let (Some(window), Some(counter)) = (chat.context_limit, &counter) {
        let prompt = counter.count_chat_tokens(&frame.system_prompt, sent.messages(), &frame.tools);
        measured_prompt = Some(prompt);
        let room = summary_answer_room(window);
        if prompt + room > window {
            return Ok(ChatSummary::NotWritten(format!(
                "would leave its answer {} of the {room} tokens it needs ({prompt} prompt tokens \
                 on a {window}-token window)",
                window.saturating_sub(prompt)
            )));
        }
    }
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
    let answer_config = summary_request_config(model_config, chat.context_limit, measured_prompt);
    let answer = crate::model_config::stream_as_the_chat(
        crate::background_work::BackgroundWorkKind::Compaction,
        provider,
        &answer_config,
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
    let called_a_tool = |response: &Message| {
        response.content.iter().any(|c| {
            matches!(
                c,
                MessageContent::ToolRequest(_) | MessageContent::FrontendToolRequest(_)
            )
        })
    };
    let shortfall = |response: &Message| {
        if called_a_tool(response) {
            Some("called a tool".to_string())
        } else {
            summary_shortfall(&response.as_concat_text())
        }
    };
    // A thinking model may write the whole summary in its reasoning and leave the answer empty
    // (or a stray `<tool_call></function></tool_call>`): the summary is taken from the
    // reasoning, without a second ask.
    if shortfall(&response).is_some() {
        if let Some(summary) = summary_in_reasoning(&response) {
            warn!(
                "compaction: the summary was written in the model's reasoning ({} characters); \
                 storing it from there",
                summary.chars().count()
            );
            response = Message::assistant().with_text(summary);
        }
    }
    let mut flag = None;
    if let Some(first_shortfall) = shortfall(&response) {
        // The model went on with the work instead of summarizing it. Asked once more on the same
        // cached prefix — its answer and a correction are all that is prefilled. Never a third
        // time, and never the transcript request: on a local model that is a full prefill of the
        // conversation (6-8 minutes at 250-300k); the kept block's facts back what is stored.
        let correction = summary_correction(&first_shortfall);
        warn!("compaction: the summary answer was not a summary ({correction}); asking once more");
        let mut answered = response.clone();
        strip_reasoning_content(&mut answered);
        answered
            .content
            .retain(|c| matches!(c, MessageContent::Text(_)));
        if answered.content.is_empty() {
            answered = answered.with_text("(no text)");
        }
        let mut retry = conversation.messages().clone();
        retry.push(answered);
        retry.push(Message::user().with_text(&correction));
        let (retry, _) = fix_conversation(Conversation::new_unvalidated(retry));
        let retry_sent = crate::agents::reply_parts::messages_for_provider(
            retry.messages(),
            model_config.toolshim,
        );
        let mut retry_prompt = None;
        let mut second_ask = Ok(());
        if let (Some(window), Some(counter)) = (chat.context_limit, &counter) {
            let prompt = counter.count_chat_tokens(
                &frame.system_prompt,
                retry_sent.messages(),
                &frame.tools,
            );
            retry_prompt = Some(prompt);
            if prompt + summary_answer_room(window) > window {
                second_ask = Err(format!(
                    "a second ask would leave its answer {} tokens",
                    window.saturating_sub(prompt)
                ));
            }
        }
        let second = match second_ask {
            Err(why) => Err(why),
            Ok(()) => {
                let retry_config =
                    summary_request_config(model_config, chat.context_limit, retry_prompt);
                let retry_answer = crate::model_config::stream_as_the_chat(
                    crate::background_work::BackgroundWorkKind::Compaction,
                    provider,
                    &retry_config,
                    session_id,
                    &frame.system_prompt,
                    retry_sent.messages(),
                    &frame.tools,
                    |_, text| {
                        if let Some(observe) = &chat.progress {
                            observe(WritingProgress {
                                written_tokens: None,
                                parts: pillars::written_parts(text),
                            });
                        }
                        true
                    },
                )
                .await;
                match retry_answer {
                    Ok((answer, _)) => Ok(answer),
                    Err(ProviderError::ContextLengthExceeded(detail)) => {
                        Err(format!("the second ask was refused as too long ({detail})"))
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };
        match second {
            Ok((second, second_usage)) => {
                provider_usage = provider_usage.combine_with(&second_usage);
                match shortfall(&second) {
                    None => response = second,
                    Some(second_shortfall) => {
                        flag = Some(format!(
                            "the model's answer {first_shortfall}; asked once more, it {second_shortfall}"
                        ));
                        response = best_answer(response, second);
                    }
                }
            }
            Err(why) => {
                flag = Some(format!("the model's answer {first_shortfall}, and {why}"));
                response = best_answer(response, Message::assistant());
            }
        }
        if let Some(flag) = &flag {
            warn!("compaction: storing goose's facts with an unchecked summary: {flag}");
        }
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
        message: Box::new(response),
        usage: provider_usage,
        note,
        flag,
    })))
}

/// The summary a model wrote in its reasoning: the last block of it, from a line that heads a
/// section to the end, that passes [`summary_shortfall`]. Measured (verifier, Flash-Next with
/// thinking on, 2026-10-30): 1 of 3 real compactions on c8d908a85f wrote all five sections in
/// its reasoning and 0 visible characters; the code before it stored
/// "<tool_call></function></tool_call>" as the summary.
fn summary_in_reasoning(response: &Message) -> Option<String> {
    let reasoning: String = response
        .content
        .iter()
        .filter_map(|c| match c {
            MessageContent::Thinking(t) => Some(t.thinking.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let starts: Vec<usize> = reasoning
        .match_indices('\n')
        .map(|(at, _)| at + 1)
        .chain(std::iter::once(0))
        .filter(|&at| {
            let line = reasoning
                .get(at..)
                .and_then(|rest| rest.lines().next())
                .unwrap_or("");
            // Round 6: only a line that heads one of the five NAMED sections starts it. Reasoning
            // is full of colon lines ("Constraints:", "Approach:", "Risks:"): the verifier's
            // planning notes passed the gate as five sections of their own and were stored as
            // the summary, never re-asked. A visible answer keeps the round-4 reading.
            !pillars::written_sections(line).is_empty()
        })
        .collect();
    let mut starts = starts;
    starts.sort_unstable();
    starts
        .into_iter()
        .rev()
        .filter_map(|at| reasoning.get(at..))
        .map(without_trailing_thought)
        .find(|block| summary_shortfall(block).is_none())
}

/// A summary drafted in reasoning, without the thinking that goes on after it: the last section
/// (`## Next step`, one step) ends at its first blank line once it holds something, so "Wait, I
/// should double-check…" written after the draft is not stored as part of the step. Structural:
/// a paragraph break, never the words. Only for a summary taken from reasoning — a visible answer
/// is the model's answer as written.
fn without_trailing_thought(block: &str) -> String {
    let block = block.trim();
    let last_heading = block
        .match_indices('\n')
        .map(|(at, _)| at + 1)
        .rfind(|&at| {
            let line = block.get(at..).and_then(|rest| rest.lines().next());
            !pillars::written_sections(line.unwrap_or("")).is_empty()
        })
        .unwrap_or(0);
    let mut lines = block
        .get(last_heading..)
        .unwrap_or("")
        .split_inclusive('\n');
    let heading = lines.next().unwrap_or("");
    // `**Next step:** fix the framing` holds its step on the heading's own line.
    let mut written = pillars::written_sections(heading)
        .first()
        .is_some_and(|(_, inline)| !inline.trim().is_empty());
    let mut kept = last_heading + heading.len();
    for line in lines {
        let blank = line.trim().is_empty();
        if blank && written {
            break;
        }
        written |= !blank;
        kept += line.len();
    }
    block.get(..kept).unwrap_or(block).trim_end().to_string()
}

/// Of two answers that were not the summary, the one with more written in it, as text only (a
/// tool call is not part of a summary).
fn best_answer(first: Message, second: Message) -> Message {
    let written = |m: &Message| {
        pillars::strip_scratch(&m.as_concat_text())
            .chars()
            .filter(|c| c.is_alphanumeric())
            .count()
    };
    let best = if written(&second) >= written(&first) {
        second
    } else {
        first
    };
    let mut text_only = Message::user().with_text(best.as_concat_text());
    text_only.role = Role::Assistant;
    text_only
}

/// [`SummaryRequest::Transcript`], trying progressively more of the tool responses removed from
/// the middle while the provider refuses the request as too long, and then (I-9) the middle of the
/// transcript itself cut to a share of the window the refusal names.
///
/// `window` is a window goose has MEASURED (the chat's effective limit, or one a refusal named) —
/// never a model config's declared one, which can be wrong either way. With it, each request is
/// measured by goose's tokenizer before it is sent, and one that would leave its answer less than
/// [`summary_answer_room`] is not sent: a refusal (or an answer cut at `length`) is certain, and
/// each costs a full prefill. A refusal naming a smaller window narrows it for the steps after.
/// Without one, the removal ladder is three steps (0, 50, 100%) instead of five. At most
/// [`MAX_TRANSCRIPT_CALLS`] requests are sent: before 2026-10-30 up to eight were, each
/// prefilling the whole transcript.
async fn summarize_a_transcript(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    messages: &[Message],
    window: Option<usize>,
) -> Result<(Message, ProviderUsage), anyhow::Error> {
    let agent_visible_messages: Vec<Message> = messages
        .iter()
        .filter(|msg| msg.is_agent_visible())
        .map(|msg| msg.agent_visible_content())
        .collect();
    let counter = create_token_counter().await.ok();
    let mut window = window;
    let fits = |transcript: &str, window: Option<usize>| match (window, &counter) {
        (Some(window), Some(counter)) => {
            counter.count_tokens(transcript)
                + TRANSCRIPT_PROMPT_TOKENS
                + summary_answer_room(window)
                <= window
        }
        _ => true,
    };
    let mut calls = 0usize;

    // Try progressively removing more tool response messages from the middle to reduce context length
    let removal_percentages: &[u32] = if window.is_some() && counter.is_some() {
        &[0, 10, 20, 50, 100]
    } else {
        &[0, 50, 100]
    };
    let mut refusal = String::new();
    let mut transcript = String::new();

    for &remove_percent in removal_percentages.iter() {
        let filtered_messages = filter_tool_responses(&agent_visible_messages, remove_percent);
        transcript = filtered_messages
            .iter()
            .map(|&msg| format_message_for_compacting(msg))
            .collect::<Vec<_>>()
            .join("\n");
        if !fits(&transcript, window) || calls == MAX_TRANSCRIPT_CALLS {
            continue;
        }
        calls += 1;
        let answer_config = summary_request_config(
            model_config,
            window,
            counter
                .as_ref()
                .map(|c| c.count_tokens(&transcript) + TRANSCRIPT_PROMPT_TOKENS),
        );
        match summarize_transcript_text(provider, &answer_config, session_id, transcript.clone())
            .await?
        {
            Ok(summary) => return Ok(summary),
            Err(ProviderError::ContextLengthExceeded(why)) => {
                if let Some(named) = goose_providers::errors::refused_context_window(&why) {
                    window = Some(window.map_or(named, |window| window.min(named)));
                }
                refusal = why;
            }
            Err(e) => return Err(e.into()),
        }
    }

    // Still too long with every tool response gone: the conversation's own text is over the
    // window. Before I-9 the session ended here ("Unable to continue").
    let named = goose_providers::errors::refused_context_window(&refusal)
        .or_else(|| refused_window(session_id, &model_config.model_name));
    let Some(mut window) = named
        .into_iter()
        .chain(window)
        .min()
        .or(model_config.context_limit)
    else {
        return Err(anyhow::anyhow!(
            "Failed to compact: context limit exceeded even after removing all tool responses"
        ));
    };
    for share in TRANSCRIPT_WINDOW_SHARES {
        if calls == MAX_TRANSCRIPT_CALLS {
            break;
        }
        let budget = (window as f64 * share * TRANSCRIPT_CHARS_PER_TOKEN) as usize;
        let cut = cut_transcript_middle(&transcript, budget);
        if !fits(&cut, Some(window)) {
            continue;
        }
        warn!(
            "compaction: the transcript is over the {window}-token window even without tool \
             responses; summarizing it with its middle cut to {} of {} characters",
            cut.len(),
            transcript.len()
        );
        calls += 1;
        let answer_config = summary_request_config(
            model_config,
            Some(window),
            counter
                .as_ref()
                .map(|c| c.count_tokens(&cut) + TRANSCRIPT_PROMPT_TOKENS),
        );
        match summarize_transcript_text(provider, &answer_config, session_id, cut).await? {
            Ok(summary) => return Ok(summary),
            Err(ProviderError::ContextLengthExceeded(why)) => {
                if let Some(named) = goose_providers::errors::refused_context_window(&why) {
                    window = window.min(named);
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(anyhow::anyhow!(
        "Failed to compact: context limit exceeded even after removing all tool responses and \
         cutting the transcript to fit a {window}-token window ({calls} summary requests sent)"
    ))
}

/// The swarm workers' transcript summary, as their golden run measured it (round 4 restored it):
/// the five removal steps whatever the window, every refusal tried, no `max_tokens`, no gate.
async fn summarize_a_transcript_as_the_swarm(
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
    let mut refusal = String::new();
    let mut transcript = String::new();

    for &remove_percent in removal_percentages.iter() {
        let filtered_messages = filter_tool_responses(&agent_visible_messages, remove_percent);
        transcript = filtered_messages
            .iter()
            .map(|&msg| format_message_for_compacting(msg))
            .collect::<Vec<_>>()
            .join("\n");
        match summarize_transcript_text(provider, model_config, session_id, transcript.clone())
            .await?
        {
            Ok(summary) => return Ok(summary),
            Err(ProviderError::ContextLengthExceeded(why)) => refusal = why,
            Err(e) => return Err(e.into()),
        }
    }

    // Still too long with every tool response gone: the conversation's own text is over the
    // window. Before I-9 the session ended here ("Unable to continue").
    let named = goose_providers::errors::refused_context_window(&refusal)
        .or_else(|| refused_window(session_id, &model_config.model_name));
    let Some(mut window) = named.or(model_config.context_limit) else {
        return Err(anyhow::anyhow!(
            "Failed to compact: context limit exceeded even after removing all tool responses"
        ));
    };
    for share in TRANSCRIPT_WINDOW_SHARES {
        let budget = (window as f64 * share * TRANSCRIPT_CHARS_PER_TOKEN) as usize;
        let cut = cut_transcript_middle(&transcript, budget);
        warn!(
            "compaction: the transcript is over the {window}-token window even without tool \
             responses; summarizing it with its middle cut to {} of {} characters",
            cut.len(),
            transcript.len()
        );
        match summarize_transcript_text(provider, model_config, session_id, cut).await? {
            Ok(summary) => return Ok(summary),
            Err(ProviderError::ContextLengthExceeded(why)) => {
                if let Some(named) = goose_providers::errors::refused_context_window(&why) {
                    window = window.min(named);
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(anyhow::anyhow!(
        "Failed to compact: context limit exceeded even after removing all tool responses and \
         cutting the transcript to fit a {window}-token window"
    ))
}

/// The most transcript summary requests one compaction sends: three removal steps and two cuts
/// when goose has no measured window; with one, the steps that cannot fit are never sent and a
/// cut transcript usually goes second.
const MAX_TRANSCRIPT_CALLS: usize = 5;

/// tokens: the transcript instruction around the transcript (compaction.md's transcript branch,
/// ~2.4k chars) and its one user line, rounded up.
const TRANSCRIPT_PROMPT_TOKENS: usize = 1_024;

/// Shares of the window a cut transcript may fill, tried in order: the rest is the summary
/// prompt and the summary itself.
const TRANSCRIPT_WINDOW_SHARES: [f64; 3] = [0.5, 0.25, 0.125];
/// Characters per token assumed when cutting a transcript to a window: low on purpose (code and
/// JSON tokenize near 3), so a cut errs short.
const TRANSCRIPT_CHARS_PER_TOKEN: f64 = 3.0;

/// The transcript with its middle replaced by a marker so it is at most `budget` characters: the
/// first quarter keeps the task, the rest is the newest work.
fn cut_transcript_middle(transcript: &str, budget: usize) -> String {
    if transcript.len() <= budget {
        return transcript.to_string();
    }
    let boundary = |at: usize| {
        let mut at = at.min(transcript.len());
        while !transcript.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    let head = boundary(budget / 4);
    let tail = boundary(transcript.len() - (budget - budget / 4));
    let tail = if tail < head { head } else { tail };
    format!(
        "{}\n[... {} characters of the middle of the conversation omitted to fit the model's \
         context window ...]\n{}",
        transcript.get(..head).unwrap_or_default(),
        tail - head,
        transcript.get(tail..).unwrap_or_default()
    )
}

/// One transcript summary request: the provider's refusal is the inner error, so the caller can
/// shrink the transcript and ask again.
async fn summarize_transcript_text(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    messages_text: String,
) -> Result<std::result::Result<(Message, ProviderUsage), ProviderError>, anyhow::Error> {
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

            Ok(Ok((response, provider_usage)))
        }
        Err(e) => Ok(Err(e)),
    }
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
            MessageContent::Media(media) => Some(media.placeholder()),
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

/// The tool-pair cutoff of a chat: [`compute_tool_call_cutoff`] on the window the compaction guard
/// reads ([`effective_context_limit`]: a refused or compressed window and GOOSE_LOCAL_CONTEXT_CAP
/// included) and its compaction point; `worker_threshold` is a swarm worker's own ratio. An
/// unknown window sizes no cutoff (`usize::MAX`). Before 2026-10-30 the cutoff read the provider's
/// raw limit while the guard read the effective one, so the two sized to different windows.
pub async fn tool_call_cutoff(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    worker_threshold: Option<f64>,
) -> usize {
    effective_context_limit(provider, model_config, Some(session_id))
        .await
        .map_or(usize::MAX, |limit| {
            compute_tool_call_cutoff(
                limit,
                worker_threshold.unwrap_or_else(|| compaction_threshold(limit, model_config)),
            )
        })
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

/// Q-516: whether the provider serves THIS chat's prompt from its cache — the last agent call's
/// cache read, or, when that call read nothing, what the chat's agent calls have read in total
/// (`Session::accumulated_usage`, which only agent and compaction calls add to). One cold call on a
/// caching provider is a miss, not a provider without a cache: it re-read the prompt and so put
/// it back in the cache, and the next call reads it warm — unless the pairs were condensed in the
/// meantime. E2E #3y (27B split, session 20260929_19): turn 22's only agent call re-read 196,384
/// tokens from 0 after a side call evicted its prefix (Q-514), so turn 23 started with a last-call
/// read of 0 and condensed 10 pairs into the stored conversation; turn 23's own calls still read
/// 195,011–195,875 warm, and the needs-you answer turn that loaded the condensed conversation
/// re-read ~195k cold. A chat whose agent calls never read from a cache is condensed as before.
pub fn chat_prompt_cache_read(last_agent_call: &Usage, chat_agent_calls: &Usage) -> Option<i32> {
    prompt_cache_read(last_agent_call).or_else(|| prompt_cache_read(chat_agent_calls))
}

/// `faithful_records`: chat agents get `record_tool_call` (facts only, no model call); the swarm's
/// workers keep the model-written summary their golden benchmark was measured with.
/// `cached_prompt`: `chat_prompt_cache_read` of the chat's agent calls — when the provider serves
/// the prompt from its cache, the pairs stay as they are (Q-294, Q-516).
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

    /// A transcript request's answer as a model that summarizes writes it: its own sections, then
    /// the names the conversation it was given used (paths, numbers).
    fn transcript_answer(system: &str) -> String {
        let history = system
            .split_once("**Conversation History:**")
            .map(|(_, rest)| rest.split_once("Wrap reasoning").map_or(rest, |(h, _)| h))
            .unwrap_or_default();
        format!(
            "<transcript summary>\n1. **User Intent** – the person asked for the work this \
             conversation shows.\n2. **Files + Code** – each file the work touched.\n\
             3. **Errors + Fixes** – None.\n4. **Pending Tasks** – what remains of the request.\n\
             5. **Current Work** – the newest turn.\n{}",
            history
                .split_whitespace()
                .filter(|t| t.len() <= 40
                    && t.contains(|c: char| c.is_ascii_digit() || matches!(c, '/' | '.' | '_')))
                .take(40)
                .collect::<Vec<_>>()
                .join(" ")
        )
    }

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

    /// I-13 x I-31: every harness nudge (the empty-turn continuation too, which tells the model it
    /// may "give your final answer") is skipped by the compaction's preserved request, through a
    /// stored round trip; the same words typed by the person are still the person's request.
    #[tokio::test]
    async fn every_harness_nudge_stays_out_of_the_preserved_request_and_the_persons_words_stay_in()
    {
        for nudge_text in [
            crate::agents::LENGTH_CUT_CONTINUATION,
            crate::agents::EMPTY_TURN_CONTINUATION,
        ] {
            let stored: Message = serde_json::from_str(
                &serde_json::to_string(&crate::agents::harness_nudge(nudge_text)).unwrap(),
            )
            .unwrap();
            assert!(stored.metadata.harness_nudge, "the marker is persisted");
            for (last, nudge_kept) in [
                (stored.clone(), false),
                (
                    Message::user()
                        .with_text(nudge_text)
                        .with_visibility(false, true),
                    true,
                ),
            ] {
                let provider =
                    MockProvider::new(Message::assistant().with_text("<mock summary>"), 1);
                let conversation = Conversation::new_unvalidated(vec![
                    Message::user().with_text("ship v2 of the app"),
                    Message::assistant()
                        .with_tool_request("tool_0", Ok(CallToolRequestParams::new("read_file"))),
                    Message::user().with_tool_response(
                        "tool_0",
                        Ok(rmcp::model::CallToolResult::success(vec![
                            RawContent::text("v1 source").no_annotation(),
                        ])),
                    ),
                    last,
                ]);
                let model_config = provider.config.clone();
                let (compacted, _usage) = compact_messages(
                    &provider,
                    &model_config,
                    "test-session-id",
                    &conversation,
                    false,
                    &SummaryRequest::Transcript,
                )
                .await
                .unwrap();
                let last_text = compacted
                    .agent_visible_messages()
                    .last()
                    .expect("a compacted conversation")
                    .as_concat_text();
                assert_eq!(
                    last_text.contains(nudge_text),
                    nudge_kept,
                    "{nudge_text:?} -> {last_text:?}"
                );
                if !nudge_kept {
                    assert!(last_text.contains("ship v2 of the app"), "{last_text:?}");
                }
            }
        }
    }

    /// I-31: goose's length-cut nudge is the newest agent-visible user text in a long tool loop,
    /// and a compaction preserved IT as the chat's latest request. The person's message must be
    /// the one carried after the summary. NEGATIVE CONTROL: without `is_harness_nudge` the last
    /// message is the nudge.
    #[tokio::test]
    async fn a_compaction_never_preserves_goose_s_own_length_cut_nudge_as_the_request() {
        let provider = MockProvider::new(Message::assistant().with_text("<mock summary>"), 1);
        let conversation = Conversation::new_unvalidated(vec![
            Message::user().with_text("ship v2 of the app"),
            Message::assistant()
                .with_tool_request("tool_0", Ok(CallToolRequestParams::new("read_file"))),
            Message::user().with_tool_response(
                "tool_0",
                Ok(rmcp::model::CallToolResult::success(vec![
                    RawContent::text("v1 source").no_annotation(),
                ])),
            ),
            crate::agents::harness_nudge(crate::agents::LENGTH_CUT_CONTINUATION),
        ]);
        let model_config = provider.config.clone();
        let (compacted, _usage) = compact_messages(
            &provider,
            &model_config,
            "test-session-id",
            &conversation,
            false,
            &SummaryRequest::Transcript,
        )
        .await
        .unwrap();
        let visible = compacted.agent_visible_messages();
        let texts: Vec<String> = visible
            .iter()
            .flat_map(|m| {
                m.content
                    .iter()
                    .filter_map(|c| c.as_text().map(str::to_string))
            })
            .collect();
        assert!(
            !texts
                .iter()
                .any(|t| t.contains("Your last reply was cut off")),
            "the nudge survived the compaction: {texts:?}"
        );
        let last = visible.last().expect("a compacted conversation");
        assert!(matches!(last.role, rmcp::model::Role::User));
        assert_eq!(
            last.content
                .iter()
                .filter_map(|c| c.as_text())
                .collect::<String>(),
            "ship v2 of the app"
        );
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
            true,
        )
        .await
        .unwrap()
        .stream;
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

        // The engine answers every request with "reading project configuration" — no summary
        // sections — so the chat's summary is asked once more on the same prefix and then stored
        // flagged (round 4: no transcript prefill).
        let bodies = engine.bodies().await;
        assert_eq!(
            bodies.len(),
            4,
            "chat, summary, its second ask, the worker's transcript: {bodies:?}"
        );
        let (chat, summary, second_ask, transcript) =
            (&bodies[0], &bodies[1], &bodies[2], &bodies[3]);
        let cached = stable_messages(chat, &block);
        let sent = summary["messages"].as_array().unwrap();
        let resent = second_ask["messages"].as_array().unwrap();
        assert_eq!(
            &resent[..sent.len()],
            sent.as_slice(),
            "the second ask extends the summary request"
        );
        assert_eq!(
            resent[sent.len()],
            serde_json::json!({"role": "assistant", "content": "reading project configuration"})
        );
        assert!(resent[sent.len() + 1]["content"]
            .as_str()
            .is_some_and(|c| c.contains("it is not the summary")));
        assert_eq!(second_ask["tools"], chat["tools"]);
        assert!(bodies[3]
            .get("tools")
            .is_none_or(|t| t.as_array().is_some_and(|t| t.is_empty())));
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
            .find(|m| m.as_concat_text().starts_with(pillars::FLAG_LEAD))
            .expect("the answer was no summary twice, so it is stored flagged beside the facts");
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
            context_limit: None,
            working_dir: std::path::PathBuf::from("/Users/someone"),
            progress: None,
            budget: None,
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
                "NOTE CONCERN: the chat says 24, the note says 12.\n## Where we are\nplanning the users by the 24-month rule\n## What each file does\nno files written yet\n## Decisions and reasons\n24 months\n## Still to do\nthe plan for the users\n## Next step\nrun the user plan",
                NoteOutcome::Read(NoteVerdict::Concern(
                    "the chat says 24, the note says 12.".to_string(),
                )),
            ),
            (
                "NOTE OK\n## Where we are\nplanning the users by the 24-month rule\n## What each file does\nno files written yet\n## Decisions and reasons\n24 months\n## Still to do\nthe plan for the users\n## Next step\nrun the user plan",
                NoteOutcome::Read(NoteVerdict::Ok),
            ),
            (
                "<analysis>long scratch</analysis>\n## Where we are\nplanning the users by the 24-month rule\n## What each file does\nno files written yet\n## Decisions and reasons\n24 months\n## Still to do\nthe plan for the users\n## Next step\nrun the user plan",
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
                stored.starts_with("## Where we are\nplanning the users by the 24-month rule\n")
                    && stored.contains("run the user plan\n\n<kept-by-goose>"),
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
            "## Where we are\nplanning the users by the 24-month rule\n## What each file does\n\
             no files written yet\n## Decisions and reasons\n24 months\n## Still to do\n\
             the plan for the users\n## Next step\nrun the user plan",
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
            [
                "Where we are",
                "What each file does",
                "Decisions and reasons",
                "Still to do",
                "Next step"
            ]
        );
        assert!(last.written_tokens.is_some_and(|t| t > 0));
        let counts: Vec<usize> = seen.iter().map(|p| p.parts.len()).collect();
        assert!(
            counts.windows(2).all(|w| w[0] <= w[1]) && counts[0] < WRITTEN_PARTS.len(),
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
                Message::assistant().with_text(transcript_answer(system)),
                usage,
            ))
        }
    }

    /// Refused as too long, the chat-extending request falls to the transcript (tools gone, the
    /// conversation inside the system prompt): the window refused it. Answered with a tool call
    /// twice, it is NOT asked as a transcript (round 4): the answer is stored flagged, and the
    /// tool call never becomes part of the summary.
    #[tokio::test]
    async fn a_refused_summary_request_is_asked_as_a_transcript_and_a_tool_call_is_flagged() {
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
            let refused = provider.chat_shaped_answer.is_none();
            let calls = provider.calls.lock().unwrap();
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0].0, "You are goose.");
            assert_eq!(calls[0].2, 1, "the first request extends the chat");
            let visible: Vec<String> = compacted
                .agent_visible_messages()
                .iter()
                .map(|m| m.as_concat_text())
                .collect();
            if refused {
                assert_eq!(calls[1].2, 0, "the second is the transcript request");
                assert!(calls[1].0.contains("**Conversation History:**"));
                assert!(
                    visible[0].starts_with("<transcript summary>\n")
                        && visible[0].contains("notes/kickoff.md")
                        && visible[0].contains("\n\n<kept-by-goose>"),
                    "{}",
                    visible[0]
                );
            } else {
                assert_eq!(calls[1].2, 1, "asked once more on the chat's prefix");
                assert!(visible[0].starts_with(pillars::FLAG_LEAD), "{}", visible[0]);
                assert!(visible[0].contains("called a tool"), "{}", visible[0]);
                assert!(visible[0].contains("<kept-by-goose>"), "{}", visible[0]);
            }
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
                working_dir: Some(&inputs.working_dir),
                budget: None,
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
        assert!(
            instruction.contains("Name every file by its absolute path"),
            "Q-394: a relative file name loses the folder the work is in: {instruction}"
        );

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

    /// Q-516, E2E #3y's own calls (session 20260929_19): turn 22's only agent call re-read
    /// 196,384 prompt tokens from 0 after a side call evicted its prefix, and the chat's agent
    /// calls had read 19,236,521 tokens from cache by then. A caching chat keeps its pairs through
    /// that one cold call; a chat whose agent calls never read from a cache is condensed as before.
    #[test]
    fn a_cold_call_on_a_caching_chat_does_not_reopen_condensation() {
        let turn22_cold =
            Usage::new(Some(196_384), Some(154), None).with_cache_tokens(Some(0), None);
        let chat_so_far =
            Usage::new(Some(19_500_000), None, None).with_cache_tokens(Some(19_236_521), None);
        assert_eq!(
            chat_prompt_cache_read(&turn22_cold, &chat_so_far),
            Some(19_236_521)
        );

        let turn23_first =
            Usage::new(Some(195_660), Some(197), None).with_cache_tokens(Some(195_011), None);
        assert_eq!(
            chat_prompt_cache_read(&turn23_first, &chat_so_far),
            Some(195_011)
        );

        let never_cached = Usage::new(Some(41_020), None, None).with_cache_tokens(Some(0), None);
        assert_eq!(
            chat_prompt_cache_read(&never_cached, &never_cached),
            None,
            "a chat whose agent calls never read from a cache is condensed as before"
        );
        assert_eq!(
            chat_prompt_cache_read(&Usage::default(), &Usage::default()),
            None
        );
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

    /// Refuses every request whose system prompt is over `window` tokens (3 characters each), the
    /// way Rapid-MLX's memory guard does, and records what it accepted.
    struct WindowProvider {
        window: usize,
        accepted: std::sync::Mutex<Vec<usize>>,
    }

    #[async_trait]
    impl Provider for WindowProvider {
        fn get_name(&self) -> &str {
            "window"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            let tokens = system.len() / 3;
            if tokens > self.window {
                return Err(ProviderError::ContextLengthExceeded(format!(
                    "This prompt ({tokens} tokens) needs 131.8 GB but this Mac can hold 75.1 GB; \
                     the maximum context here is {} tokens.",
                    self.window
                )));
            }
            self.accepted.lock().unwrap().push(tokens);
            let usage = ProviderUsage::new("window".to_string(), Usage::default());
            Ok(stream_from_single_message(
                Message::assistant().with_text("<summary>"),
                usage,
            ))
        }

        async fn get_context_limit(
            &self,
            _model_config: &ModelConfig,
        ) -> Result<usize, ProviderError> {
            Ok(262_144)
        }
    }

    /// I-9 (Xing on the Studio, 2026-10-18): the conversation's own text is over the engine's
    /// window even with every tool response removed. Before, compaction failed ("even after
    /// removing all tool responses") and the session ended; now the transcript's middle is cut to
    /// the window the refusal names.
    #[tokio::test]
    async fn a_transcript_over_the_named_window_is_cut_to_fit() {
        let provider = WindowProvider {
            window: 20_000,
            accepted: std::sync::Mutex::new(Vec::new()),
        };
        let conversation = Conversation::new_unvalidated(vec![
            Message::user().with_text(format!("Build the app. {}", "spec ".repeat(30_000))),
            Message::assistant().with_text("plan ".repeat(30_000)),
            Message::user().with_text("continue"),
        ]);
        let model_config = ModelConfig::new("xing").with_context_limit(Some(262_144));
        let compacted = compact_messages(
            &provider,
            &model_config,
            "i9-window-session",
            &conversation,
            false,
            &SummaryRequest::Transcript,
        )
        .await;
        assert!(compacted.is_ok(), "{:?}", compacted.err());
        let accepted = provider.accepted.lock().unwrap().clone();
        assert_eq!(accepted.len(), 1, "{accepted:?}");
        assert!(accepted[0] <= 20_000, "{accepted:?}");
    }

    /// I-9: once the provider has named a smaller window, the session's effective limit is that
    /// window, so proactive compaction fires at 0.8 x it instead of at the probed window.
    #[tokio::test]
    async fn the_named_window_bounds_the_sessions_effective_limit() {
        let provider = MockProvider::new(Message::assistant().with_text("x"), 262_144);
        let model_config = provider.config.clone();
        let session = "i9-effective-session";
        assert_eq!(
            effective_context_limit(&provider, &model_config, Some(session)).await,
            Some(262_144)
        );
        let refusal = ProviderError::ContextLengthExceeded(
            "This prompt (82684 tokens) needs 131.8 GB but this Mac can hold 75.1 GB; the \
             maximum context here is 35328 tokens."
                .to_string(),
        );
        assert_eq!(
            note_refused_window(session, &model_config.model_name, &refusal),
            Some(35_328)
        );
        assert_eq!(
            effective_context_limit(&provider, &model_config, Some(session)).await,
            Some(35_328)
        );
        assert_eq!(
            effective_context_limit(&provider, &model_config, Some("another-session")).await,
            Some(262_144)
        );
    }

    /// OpenRouter's refusal counts the output it reserved inside the window it names: the prompt
    /// a later request may carry is the window minus that share.
    #[tokio::test]
    async fn an_openrouter_refusal_sizes_the_prompt_to_the_window_minus_its_output_share() {
        let provider = MockProvider::new(Message::assistant().with_text("x"), 500_000);
        let model_config = provider.config.clone();
        let session = "openrouter-refusal-session";
        let refusal = ProviderError::ContextLengthExceeded(
            "This endpoint's maximum context length is 131072 tokens. However, you requested \
             about 272464 tokens (258978 of text input, 3486 of tool input, 10000 in the \
             output). Please reduce the length of either one, or use the \"middle-out\" \
             transform to compress your prompt automatically."
                .to_string(),
        );
        assert_eq!(
            note_refused_window(session, &model_config.model_name, &refusal),
            Some(121_072)
        );
        assert_eq!(
            effective_context_limit(&provider, &model_config, Some(session)).await,
            Some(121_072)
        );
    }

    #[test]
    fn a_reported_prompt_under_half_of_what_goose_sent_was_compressed() {
        // Grok 4.7's Forge 2.0 low build: billed 76,843 at call #182 while goose sent ~273k.
        assert!(prompt_was_compressed(273_000, 76_843));
        // The call before it was billed in full.
        assert!(!prompt_was_compressed(207_000, 207_145));
        // A provider tokenizer that counts a fifth fewer tokens than o200k is not compression.
        assert!(!prompt_was_compressed(100_000, 80_000));
        assert!(!prompt_was_compressed(0, 0));
    }

    #[test]
    fn the_guard_counts_what_goose_sent_while_the_provider_compresses() {
        assert_eq!(guarded_context_tokens(Some(80_000), None), Some(80_000));
        assert_eq!(
            guarded_context_tokens(Some(80_000), Some(273_000)),
            Some(273_000)
        );
        assert_eq!(guarded_context_tokens(None, Some(273_000)), Some(273_000));
        assert_eq!(guarded_context_tokens(None, None), None);
    }

    /// A compressed call sizes the window to what the provider held, so the guard (0.8 x window)
    /// compacts on the next check; a provider that held less than twice the fixed prompt is not
    /// sized to, or every compacted call would compact again.
    #[tokio::test]
    async fn a_compressed_call_sizes_the_window_to_what_the_provider_held() {
        let provider = MockProvider::new(Message::assistant().with_text("x"), 500_000);
        let model_config = provider.config.clone();
        let model = model_config.model_name.clone();

        let sized = "compressed-sized-session";
        note_compressed_prompt(sized, &model, 273_000, 76_843, 9_000);
        assert_eq!(compressed_prompt(sized), Some(273_000));
        assert_eq!(
            effective_context_limit(&provider, &model_config, Some(sized)).await,
            Some(76_843)
        );
        forget_compressed_prompt(sized);
        assert_eq!(compressed_prompt(sized), None);

        let tiny = "compressed-tiny-session";
        note_compressed_prompt(tiny, &model, 95_000, 7_000, 9_000);
        assert_eq!(compressed_prompt(tiny), Some(95_000));
        assert_eq!(
            effective_context_limit(&provider, &model_config, Some(tiny)).await,
            Some(500_000)
        );
    }

    /// One compressed call — the compaction's own summary call, or one OpenRouter routed to a
    /// small endpoint — does not shrink the session's window for good: the next uncompressed call
    /// or a compaction gives the listed window back.
    #[tokio::test]
    async fn a_compressed_window_lasts_only_until_the_compression_ends() {
        let provider = MockProvider::new(Message::assistant().with_text("x"), 500_000);
        let model_config = provider.config.clone();
        let session = "compressed-then-clear-session";
        note_compressed_prompt(session, &model_config.model_name, 273_000, 76_843, 9_000);
        assert_eq!(
            effective_context_limit(&provider, &model_config, Some(session)).await,
            Some(76_843)
        );
        forget_compressed_prompt(session);
        assert_eq!(
            effective_context_limit(&provider, &model_config, Some(session)).await,
            Some(500_000)
        );
    }

    #[test]
    fn a_cut_transcript_keeps_its_ends() {
        let transcript = format!("TASK {} NEWEST", "x".repeat(10_000));
        let cut = cut_transcript_middle(&transcript, 1_000);
        assert!(cut.starts_with("TASK") && cut.ends_with("NEWEST"), "{cut}");
        assert!(cut.len() < 1_200, "{}", cut.len());
        assert_eq!(cut_transcript_middle("short", 1_000), "short");
        let wide = "é".repeat(5_000);
        assert!(cut_transcript_middle(&wide, 999).len() < 1_200);
    }

    /// Answers the chat-shaped requests with `answers` in turn (the last one repeats) and a
    /// transcript request with "<transcript summary>"; records each request.
    struct SequenceProvider {
        answers: Vec<String>,
        calls: std::sync::Mutex<Vec<(Vec<Message>, usize)>>,
    }

    #[async_trait]
    impl Provider for SequenceProvider {
        fn get_name(&self) -> &str {
            "sequence"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            system: &str,
            messages: &[Message],
            tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            let mut calls = self.calls.lock().unwrap();
            let chat_calls = calls.iter().filter(|(_, tools)| *tools > 0).count();
            calls.push((messages.to_vec(), tools.len()));
            let usage = ProviderUsage::new(
                "sequence".to_string(),
                Usage::new(Some(90), Some(10), Some(100)),
            );
            let text = if tools.is_empty() {
                transcript_answer(system)
            } else {
                self.answers[chat_calls.min(self.answers.len() - 1)].clone()
            };
            Ok(stream_from_single_message(
                Message::assistant().with_text(text),
                usage,
            ))
        }
    }

    const EVERY_PART: &str = "## Where we are\nsync done, 41 tests pass, 24 months kept\n## What each file does\n\
        app/sync.py — the documented walk\n## Decisions and reasons\nBerlin days, not UTC\n## Still to do\n\
        the 3D field and its test\n## Next step\nwrite web/viz.js";

    /// The words the Flash-Next stopped run (2026-10-29) stored as the summary of 241k tokens.
    const FLASH_NEXT_NON_SUMMARY: &str =
        "I will now proceed to document the design decisions and run the final verification sequence.";

    #[test]
    fn an_answer_without_the_sections_is_not_a_summary() {
        assert!(summary_shortfall(FLASH_NEXT_NON_SUMMARY).is_some());
        assert!(summary_shortfall("## Where we are\nx\n## Next step\ny").is_some());
        assert_eq!(summary_shortfall(EVERY_PART), None);
        assert_eq!(
            summary_shortfall(&format!(
                "<analysis>scratch</analysis>\nNOTE OK\n{EVERY_PART}"
            )),
            None
        );
        let shortfall = summary_shortfall("## Where we are\nsync done, 41 tests pass").unwrap();
        assert!(
            shortfall.contains("`## What each file does`"),
            "{shortfall}"
        );
        assert!(!shortfall.contains("`## Where we are`"), "{shortfall}");
        assert!(summary_correction(&shortfall).contains("it is not the summary"));
    }

    /// The Flash-Next shape: the model answers the summary instruction by going on with the work.
    /// goose asks once more on the same prefix — the non-answer and a correction naming the
    /// missing sections — and stores the second answer, never the one-liner.
    #[tokio::test]
    async fn a_non_summary_is_asked_once_more_on_the_same_prefix() {
        let provider = SequenceProvider {
            answers: vec![FLASH_NEXT_NON_SUMMARY.to_string(), EVERY_PART.to_string()],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let ChatCompacted::Compacted { conversation, .. } = compact_chat(
            &provider,
            &ModelConfig::new("sequence"),
            "s",
            &two_turn_chat(),
            false,
            &frame(),
            &chat_inputs(None, false),
        )
        .await
        .unwrap() else {
            panic!("compacted");
        };
        let calls = provider.calls.lock().unwrap();
        assert_eq!(calls.len(), 2, "one more ask, no transcript request");
        let (first, second) = (&calls[0].0, &calls[1].0);
        assert_eq!(
            &second[..first.len()],
            &first[..],
            "the second ask extends the first request"
        );
        assert_eq!(second[first.len()].as_concat_text(), FLASH_NEXT_NON_SUMMARY);
        assert!(second
            .last()
            .unwrap()
            .as_concat_text()
            .contains("it is not the summary"));
        let stored = conversation.agent_visible_messages()[0].as_concat_text();
        assert!(stored.starts_with("## Where we are\nsync done"), "{stored}");
        assert!(!stored.contains("I will now proceed"), "{stored}");
    }

    /// Two non-answers: no third ask and no transcript request (a full prefill on a local model).
    /// The better of the two answers is stored FLAGGED, beside goose's facts.
    #[tokio::test]
    async fn two_non_summaries_are_stored_flagged_beside_the_facts_not_asked_as_a_transcript() {
        let provider = SequenceProvider {
            answers: vec![
                FLASH_NEXT_NON_SUMMARY.to_string(),
                "Continuing with the verification now.".to_string(),
            ],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let ChatCompacted::Compacted { conversation, .. } = compact_chat(
            &provider,
            &ModelConfig::new("sequence"),
            "s",
            &two_turn_chat(),
            false,
            &frame(),
            &chat_inputs(None, false),
        )
        .await
        .unwrap() else {
            panic!("compacted");
        };
        let calls = provider.calls.lock().unwrap();
        let tools: Vec<usize> = calls.iter().map(|(_, tools)| *tools).collect();
        assert_eq!(tools.len(), 2, "{tools:?}");
        assert!(
            tools.iter().all(|t| *t > 0),
            "no transcript request: {tools:?}"
        );
        let stored = conversation.agent_visible_messages()[0].as_concat_text();
        assert!(stored.starts_with(pillars::FLAG_LEAD), "{stored}");
        assert!(stored.contains("asked once more"), "{stored}");
        assert!(
            stored.contains(FLASH_NEXT_NON_SUMMARY),
            "the longer answer: {stored}"
        );
        assert!(stored.contains(pillars::KEPT_OPEN), "{stored}");
        assert!(stored.contains("The person's messages"), "{stored}");
    }

    fn tool_loop(turns: usize) -> Conversation {
        let mut messages = vec![Message::user().with_text("build the console")];
        for i in 0..turns {
            messages.push(Message::assistant().with_tool_request(
                format!("t{i}"),
                Ok(CallToolRequestParams::new("developer__shell")),
            ));
            messages.push(Message::user().with_tool_response(
                format!("t{i}"),
                Ok(rmcp::model::CallToolResult::success(vec![
                    RawContent::text(format!("output {i} {}", "x".repeat(200))).no_annotation(),
                ])),
            ));
        }
        Conversation::new_unvalidated(messages)
    }

    /// A chat keeps its newest turns word for word after the summary, sized to its share of the
    /// window, and the summary request still sends the whole conversation (the cached prefix).
    #[tokio::test]
    async fn a_chat_keeps_its_newest_turns_and_the_summary_request_keeps_the_whole_prefix() {
        let chat = tool_loop(12);
        let counter = create_token_counter().await.unwrap();
        let per_message_tokens =
            counter.count_chat_tokens("", std::slice::from_ref(&chat.messages()[2]), &[]);
        let mut inputs = chat_inputs(None, false);
        inputs.context_limit = Some((per_message_tokens * 6 + per_message_tokens / 2) * 16);
        let provider = SequenceProvider {
            answers: vec![EVERY_PART.to_string()],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let ChatCompacted::Compacted {
            conversation, kept, ..
        } = compact_chat(
            &provider,
            &ModelConfig::new("sequence"),
            "s",
            &chat,
            false,
            &frame(),
            &inputs,
        )
        .await
        .unwrap()
        else {
            panic!("compacted");
        };
        let sent = &provider.calls.lock().unwrap()[0].0;
        assert_eq!(
            sent.len(),
            chat.messages().len() + 1,
            "every message, then the ask"
        );
        let tail = kept.unwrap().tail_messages;
        assert!(tail > 0 && tail < chat.messages().len() / 2, "{tail}");
        let visible = conversation.agent_visible_messages();
        let last_tool = visible
            .iter()
            .rev()
            .find(|m| {
                m.content
                    .iter()
                    .any(|c| matches!(c, MessageContent::ToolResponse(_)))
            })
            .unwrap();
        assert!(format_message_for_compacting(last_tool).contains("output 11"));
        assert!(!visible
            .iter()
            .any(|m| format_message_for_compacting(m).contains("output 0 ")));
    }

    #[tokio::test]
    async fn a_tail_stops_at_a_row_an_earlier_compaction_hid() {
        let mut messages = tool_loop(3).messages().clone();
        messages[2] = messages[2]
            .clone()
            .with_metadata(MessageMetadata::user_only());
        let counter = create_token_counter().await.unwrap();
        assert_eq!(tail_that_fits(&messages, usize::MAX, &counter), 4);
        assert_eq!(tail_that_fits(&messages, 0, &counter), 0);
    }

    /// The compaction point: the person's ratio wins; otherwise the window minus the output cap
    /// and one tool result, never earlier than 0.8. The Flash-Next numbers: 360,000 with a 32,768
    /// cap compacts at 297,232 instead of 288,000.
    #[test]
    fn the_compaction_point_reserves_one_answer_and_one_tool_result() {
        let at = |configured, reserve, limit: usize, output| {
            (compaction_threshold_from(configured, reserve, limit, output) * limit as f64).round()
                as usize
        };
        assert_eq!(at(None, None, 360_000, 32_768), 297_232);
        assert_eq!(at(Some(0.8), None, 360_000, 32_768), 288_000);
        assert_eq!(at(None, Some(20_000), 360_000, 32_768), 340_000);
        assert_eq!(
            at(None, None, 128_000, 32_768),
            102_400,
            "never earlier than 0.8"
        );
        assert_eq!(at(None, None, 1_048_576, 32_768), 928_427);
        assert!(compaction_threshold_from(None, Some(0), 360_000, 32_768) < 1.0);
        assert_eq!(compaction_threshold_from(None, None, 0, 4_096), 0.8);
    }

    // ---- review 2026-10-30 (compaction-fix-2): each test below fails on 430d85f003 ----

    /// Defect 1: goose sends a local engine no `max_tokens`, and the point read
    /// `max_output_tokens()`'s 4,096 stand-in: 325,904 on Flash-Next's 360,000 instead of 297,232,
    /// leaving ~9k for a thinking model's summary.
    #[test]
    fn an_unset_output_cap_reserves_the_engines_default_not_4096() {
        let flash = ModelConfig::new("flash-next");
        assert_eq!(flash.max_tokens, None);
        assert_eq!(
            flash.max_output_tokens(),
            4_096,
            "the stand-in this must not read"
        );
        let point = (compaction_threshold(360_000, &flash) * 360_000.0).round() as usize;
        assert_eq!(point, 297_232);
        // At the point: one tool result (the run's largest, 22,944) and the summary's answer room
        // (16,384, ~3.8x the measured 4,316) still fit.
        assert!(360_000 - point >= 22_944 + summary_answer_room(360_000));
        let capped = ModelConfig::new("flash-next").with_max_tokens(Some(8_192));
        assert_eq!(output_reserve_tokens(&capped), 8_192);
    }

    /// Plausible item: the 0.8 floor applies to the DERIVED rule only; a reserve the person set
    /// moves the point below it (a 32,768 cap plus a tool result on a 128,000 window), down to
    /// half the window.
    #[test]
    fn a_reserve_the_person_set_moves_the_point_below_the_default_floor() {
        let at = |reserve: Option<usize>, limit: usize| {
            (compaction_threshold_from(None, reserve, limit, 32_768) * limit as f64).round()
                as usize
        };
        assert_eq!(at(Some(43_434), 128_000), 84_566);
        assert_eq!(
            at(Some(100_000), 128_000),
            64_000,
            "never below half the window"
        );
        assert_eq!(
            at(None, 128_000),
            102_400,
            "the derived rule keeps its 0.8 floor"
        );
    }

    /// The guard compacts at the reserve's point, not at the floor: on Flash-Next's 360,000 with no
    /// output cap, 295,000 used is under the point (297,232) and 298,000 is over it. Catches a
    /// guard that ignores the reserve (0.8 would compact at 288,000) and the 4,096 stand-in (it
    /// would not compact until 325,904).
    #[tokio::test]
    async fn the_guard_compacts_at_the_reserve_point() {
        let provider = MockProvider::new(Message::assistant().with_text("x"), 360_000);
        let conversation = Conversation::new_unvalidated(vec![Message::user().with_text("go")]);
        let mut session = crate::session::Session {
            id: "reserve-guard-session".to_string(),
            model_config: Some(ModelConfig::new("flash-next")),
            ..Default::default()
        };
        for (used, compacts) in [(287_000, false), (295_000, false), (298_000, true)] {
            session.usage = Usage::new(Some(used - 1_000), Some(1_000), Some(used));
            let needed = check_if_compaction_needed(
                &provider,
                &conversation,
                None,
                &session,
                GuardCount::Chat(None),
            )
            .await
            .unwrap();
            assert_eq!(needed, compacts, "{used} used of 360,000");
        }
    }

    fn sections(parts: [&str; 5]) -> String {
        WRITTEN_PARTS
            .iter()
            .zip(parts)
            .map(|((heading, _), body)| format!("## {heading}\n{body}\n"))
            .collect()
    }

    const BODIES: [&str; 5] = [
        "sync done, 41 tests pass, 24 months kept",
        "app/sync.py — the documented walk",
        "Berlin days, not UTC",
        "the 3D field and its test",
        "write web/viz.js",
    ];

    /// The gate is STRUCTURAL (round 4): an answer is refused only when it is not a summary at all
    /// — no sections, empty sections, the instruction's own template echoed back, the summary
    /// inside a code fence. It never judges the words in a section.
    #[test]
    fn the_gate_refuses_only_a_missing_or_mostly_empty_summary() {
        assert_eq!(summary_shortfall(&sections(BODIES)), None);
        let empty = sections(["", "", "", "", ""]);
        assert!(summary_shortfall(&empty).is_some(), "{empty}");
        let parroted: String = WRITTEN_PARTS
            .iter()
            .map(|(heading, ask)| format!("## {heading}\n{ask}\n"))
            .collect();
        assert!(summary_shortfall(&parroted).is_some(), "{parroted}");
        let fenced = format!(
            "Here is the format:\n```markdown\n{}```\n",
            sections(BODIES)
        );
        assert!(summary_shortfall(&fenced).is_some(), "{fenced}");
        // Three of five sections empty is mostly empty; two is not.
        let three_empty = sections(["", BODIES[1], "", "", BODIES[4]]);
        assert!(
            summary_shortfall(&three_empty)
                .is_some_and(|why| why.contains("3 of its 5 sections empty")),
            "{three_empty}"
        );
        assert_eq!(
            summary_shortfall(&sections(["", BODIES[1], BODIES[2], "", BODIES[4]])),
            None
        );
        // A section may hold a code block of its own.
        let with_code = sections([
            "the build:\n```\ncargo test -p goose\n```",
            BODIES[1],
            BODIES[2],
            BODIES[3],
            BODIES[4],
        ]);
        assert_eq!(summary_shortfall(&with_code), None, "{with_code}");
    }

    /// Round 3's word heuristics are gone: a short answer that says a list is empty, a placeholder,
    /// another language, bullets, and words the round-3 vacant list refused all pass. Every one of
    /// these failed the round-3 gate, or would with its vacant list or its anchor.
    #[test]
    fn short_non_english_and_bulleted_answers_pass_the_gate() {
        for bodies in [
            [BODIES[0], "None.", "Keine.", "- none", BODIES[4]],
            ["N/A", BODIES[1], BODIES[2], "TBD", BODIES[4]],
            [
                "ongoing",
                "see above.",
                "same as before",
                "as discussed",
                "keep going",
            ],
            [
                "- sync done\n- 41 tests pass",
                "- app/sync.py: walk\n- app/api.py: routes",
                "- Berlin days",
                "- none",
                "- run the 3D check",
            ],
        ] {
            let answer = sections(bodies);
            assert_eq!(summary_shortfall(&answer), None, "{answer}");
        }
        let german = "## Wo wir stehen\nSync fertig, 41 Tests grün.\n## Was jede Datei tut\n\
            Keine.\n## Entscheidungen und Gründe\nKeine.\n## Noch zu tun\n3D-Feld.\n\
            ## Nächster Schritt\nweb/viz.js schreiben.";
        assert_eq!(summary_shortfall(german), None, "{german}");
        let bold =
            "**Où nous en sommes :** synchro faite\n**Ce que fait chaque fichier :** aucun\n\
            **Décisions :** aucune\n**Reste à faire :** le champ 3D\n**Prochaine étape :** viz.js";
        assert_eq!(summary_shortfall(bold), None, "{bold}");
        // Another language still needs its sections: one line is not a summary in any language.
        assert!(summary_shortfall("Ich mache jetzt mit der Prüfung weiter.").is_some());
        assert!(summary_shortfall("## Wo wir stehen\nfertig\n## Nächster Schritt\nx").is_some());
    }

    /// Defect 3: real summaries head their sections in other markdown forms; each is the section.
    #[test]
    fn a_section_heading_is_read_in_its_usual_markdown_forms() {
        let forms: [fn(&str, &str) -> String; 6] = [
            |h, b| format!("## {h}:\n{b}\n"),
            |h, b| format!("### {h}\n{b}\n"),
            |h, b| format!("**{h}**\n{b}\n"),
            |h, b| format!("**{h}:** {b}\n"),
            |h, b| format!("{}:\n{b}\n", h.to_uppercase()),
            |h, b| format!("## 1. {h}\n{b}\n"),
        ];
        for (i, form) in forms.iter().enumerate() {
            let answer: String = WRITTEN_PARTS
                .iter()
                .zip(BODIES)
                .map(|((heading, _), body)| form(heading, body))
                .collect();
            assert_eq!(summary_shortfall(&answer), None, "form {i}:\n{answer}");
            assert_eq!(
                pillars::written_parts(&answer),
                WRITTEN_PARTS.iter().map(|(h, _)| *h).collect::<Vec<_>>(),
                "form {i}"
            );
        }
        // A sentence that opens with a heading's words is not the heading.
        assert!(pillars::written_parts("Where we are now is unclear\n## Next steps\n").is_empty());
    }

    /// Hex: ~2-3 chars per token, so a write of it is far more tokens than its chars / 4.
    fn hex_text(chars: usize) -> String {
        let mut text: String = (0u64..chars as u64 / 16 + 1)
            .map(|i| format!("{:016x}", i.wrapping_mul(0x9e37_79b9_7f4a_7c15)))
            .collect();
        text.truncate(chars);
        text
    }

    fn write_then_ok(content: &str) -> [Message; 2] {
        let mut params = CallToolRequestParams::new("developer__write");
        params.arguments = serde_json::json!({"path": "/w/data.hex", "content": content})
            .as_object()
            .cloned();
        [
            Message::assistant().with_tool_request("w1", Ok(params)),
            Message::user().with_tool_response(
                "w1",
                Ok(rmcp::model::CallToolResult::success(vec![
                    RawContent::text("Created /w/data.hex").no_annotation(),
                ])),
            ),
        ]
    }

    /// Defect 4: a small tool result whose request is a big write dragged the whole write into the
    /// kept tail past its budget. The pair is kept within the budget or left out together.
    #[tokio::test]
    async fn a_tool_result_whose_request_is_over_the_budget_leaves_the_tail_with_it() {
        let big = hex_text(40_000);
        let mut messages = vec![Message::user().with_text("store the dump")];
        messages.extend(write_then_ok(&big));
        let chat = Conversation::new_unvalidated(messages);
        let mut inputs = chat_inputs(None, false);
        inputs.context_limit = Some(64_000);
        let counter = create_token_counter().await.unwrap();
        let budget = tail_budget_tokens(64_000);
        assert!(counter.count_chat_tokens("", &chat.messages()[1..2], &[]) > budget);
        assert_eq!(tail_that_fits(chat.messages(), budget, &counter), 0);
        let provider = SequenceProvider {
            answers: vec![sections(BODIES)],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let ChatCompacted::Compacted {
            conversation, kept, ..
        } = compact_chat(
            &provider,
            &ModelConfig::new("sequence"),
            "s",
            &chat,
            false,
            &frame(),
            &inputs,
        )
        .await
        .unwrap()
        else {
            panic!("compacted");
        };
        assert_eq!(kept.unwrap().tail_messages, 0);
        let after = conversation.agent_visible_messages();
        let tokens = counter.count_chat_tokens("", &after, &[]);
        // The facts quote the write's first 160 characters among the last calls; no row after
        // the summary carries it.
        assert!(
            !after[1..]
                .iter()
                .any(|m| m.as_concat_text().contains(big.get(..64).unwrap())
                    || format_message_for_compacting(m).contains(big.get(..64).unwrap())),
            "the write stays out of the compacted chat"
        );
        assert!(tokens < 64_000 / 4, "{tokens}");
        // Room for both: the pair is kept whole, never the result alone.
        let small = write_then_ok("hello");
        let kept_pair = tail_that_fits(&small, budget, &counter);
        assert_eq!(kept_pair, 2);
    }

    fn png(width: u32, height: u32) -> String {
        use base64::Engine;
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::new(width, height))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageOutputFormat::Png,
            )
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// Defect 5: the tail was counted in chars — an image as ~18 while the real image is sent,
    /// CJK at 2-4x the tokens of chars / 4. It is counted by goose's tokenizer, images at their
    /// pixel cost.
    #[tokio::test]
    async fn the_tail_counts_tokens_and_images_at_their_real_cost() {
        assert_eq!(crate::token_counter::image_tokens(&png(1024, 1024)), 1_399);
        assert_eq!(
            crate::token_counter::image_tokens("not an image"),
            crate::token_counter::UNSIZED_IMAGE_TOKENS
        );
        let counter = create_token_counter().await.unwrap();
        let screenshot = Message::user()
            .with_text("see the screen")
            .with_image(png(1024, 1024), "image/png");
        assert!(counter.count_chat_tokens("", std::slice::from_ref(&screenshot), &[]) > 1_399);
        let window = 16_384;
        let budget = tail_budget_tokens(window);
        let messages = vec![
            Message::user().with_text("build it"),
            Message::assistant().with_text("ok"),
            screenshot,
        ];
        assert_eq!(
            tail_that_fits(&messages, budget, &counter),
            0,
            "the image is over 1,024"
        );
        let cjk = "数据同步完成测试通过".repeat(120);
        let messages = vec![
            Message::user().with_text("build it"),
            Message::assistant().with_text(&cjk),
            Message::user().with_text(&cjk),
        ];
        let kept = tail_that_fits(&messages, budget, &counter);
        let kept_tokens = counter.count_chat_tokens("", &messages[messages.len() - kept..], &[]);
        assert!(kept_tokens <= budget, "{kept_tokens} > {budget}");
        assert!(
            format_message_for_compacting(&messages[2]).chars().count() < budget * 4,
            "in chars it fit"
        );
    }

    /// Defect 5, the loop: on a 16k or 32k window a compaction's own part — summary, kept block,
    /// kept tail — is well under the point, so the next guard check does not compact again. The
    /// chat is CJK and hex tool output with screenshots, the shapes chars / 4 under-counted.
    #[tokio::test]
    async fn a_small_window_is_left_well_under_its_point_after_a_compaction() {
        let counter = create_token_counter().await.unwrap();
        for window in [16_384usize, 32_768] {
            let mut messages = vec![Message::user().with_text("同步所有用户并画出图表")];
            for i in 0..30 {
                let mut params = CallToolRequestParams::new("developer__shell");
                params.arguments = serde_json::json!({"command": format!("run {i}")})
                    .as_object()
                    .cloned();
                messages.push(Message::assistant().with_tool_request(format!("c{i}"), Ok(params)));
                let mut result = rmcp::model::CallToolResult::success(vec![RawContent::text(
                    format!("{} {}", "数据同步完成".repeat(40), hex_text(400)),
                )
                .no_annotation()]);
                if i % 5 == 0 {
                    result
                        .content
                        .push(RawContent::image(png(800, 600), "image/png").no_annotation());
                }
                messages.push(Message::user().with_tool_response(format!("c{i}"), Ok(result)));
            }
            let chat = Conversation::new_unvalidated(messages);
            let mut inputs = chat_inputs(None, false);
            inputs.context_limit = Some(window);
            let provider = SequenceProvider {
                answers: vec![sections(BODIES)],
                calls: std::sync::Mutex::new(Vec::new()),
            };
            let ChatCompacted::Compacted { conversation, .. } = compact_chat(
                &provider,
                &ModelConfig::new("sequence"),
                "s",
                &chat,
                false,
                &SummaryRequest::Transcript,
                &inputs,
            )
            .await
            .unwrap() else {
                panic!("compacted");
            };
            let after = counter.count_chat_tokens("", &conversation.agent_visible_messages(), &[]);
            let point = (0.8 * window as f64) as usize;
            // The summary here is short; a real one gets the answer room at most. A quarter of
            // the point is left for the system prompt and tools.
            let own = after + summary_answer_room(window);
            assert!(
                own < point * 3 / 4,
                "{window}: {after} tokens kept + {} answer room vs point {point}",
                summary_answer_room(window)
            );
        }
    }

    /// Defect 6 and the request-before-tail order: a harness nudge in the kept tail is not the
    /// person's request; the request is re-sent BEFORE the tail (the tail is the work it asked for).
    #[tokio::test]
    async fn the_persons_request_goes_before_the_tail_even_with_a_nudge_in_it() {
        for with_nudge in [false, true] {
            let mut messages = tool_loop(12).messages().clone();
            if with_nudge {
                messages.push(crate::agents::harness_nudge(
                    crate::agents::LENGTH_CUT_CONTINUATION,
                ));
            }
            let chat = Conversation::new_unvalidated(messages);
            let counter = create_token_counter().await.unwrap();
            let per = counter.count_chat_tokens("", std::slice::from_ref(&chat.messages()[2]), &[]);
            let mut inputs = chat_inputs(None, false);
            inputs.context_limit = Some((per * 6 + per / 2) * 16);
            let provider = SequenceProvider {
                answers: vec![sections(BODIES)],
                calls: std::sync::Mutex::new(Vec::new()),
            };
            let ChatCompacted::Compacted {
                conversation, kept, ..
            } = compact_chat(
                &provider,
                &ModelConfig::new("sequence"),
                "s",
                &chat,
                false,
                &frame(),
                &inputs,
            )
            .await
            .unwrap()
            else {
                panic!("compacted");
            };
            let tail = kept.unwrap().tail_messages;
            assert!(tail > 0, "nudge {with_nudge}");
            let visible = conversation.agent_visible_messages();
            let request = visible
                .iter()
                .position(|m| m.as_concat_text() == "build the console")
                .unwrap_or_else(|| panic!("nudge {with_nudge}: the request is re-sent"));
            let first_tail = visible.len() - tail;
            assert!(
                request < first_tail,
                "nudge {with_nudge}: request at {request}, tail from {first_tail}"
            );
        }
    }

    /// Defect 8: the tool-pair cutoff reads the guard's window — a refused window narrows both.
    #[tokio::test]
    async fn the_tool_pair_cutoff_reads_the_guards_window() {
        let provider = MockProvider::new(Message::assistant().with_text("x"), 1_000_000);
        let model_config = provider.config.clone();
        let session = "cutoff-window-session";
        let wide = tool_call_cutoff(&provider, &model_config, session, Some(0.8)).await;
        assert_eq!(wide, compute_tool_call_cutoff(1_000_000, 0.8));
        remember_window(session, &model_config.model_name, 100_000);
        let narrowed = tool_call_cutoff(&provider, &model_config, session, Some(0.8)).await;
        assert_eq!(narrowed, compute_tool_call_cutoff(100_000, 0.8));
        assert!(narrowed < wide);
    }

    /// Plausible item: a summary request that would leave its answer no room is not sent; the
    /// transcript request goes instead, and the transcript fallback sends at most
    /// MAX_TRANSCRIPT_CALLS requests (it sent up to eight).
    #[tokio::test]
    async fn a_summary_request_without_room_for_its_answer_is_not_sent() {
        let provider = SequenceProvider {
            answers: vec![sections(BODIES)],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let chat = Conversation::new_unvalidated(vec![
            Message::user().with_text(hex_text(36_000)),
            Message::assistant().with_text("noted"),
            Message::user().with_text("go on"),
        ]);
        let mut inputs = chat_inputs(None, false);
        inputs.context_limit = Some(16_384);
        let ChatCompacted::Compacted { conversation, .. } = compact_chat(
            &provider,
            &ModelConfig::new("sequence"),
            "s",
            &chat,
            false,
            &frame(),
            &inputs,
        )
        .await
        .unwrap() else {
            panic!("compacted");
        };
        let calls = provider.calls.lock().unwrap();
        assert!(
            calls.iter().all(|(_, tools)| *tools == 0),
            "no chat-shaped request was sent: {:?}",
            calls.iter().map(|(_, t)| *t).collect::<Vec<_>>()
        );
        assert!(calls.len() <= MAX_TRANSCRIPT_CALLS);
        let stored = conversation.agent_visible_messages()[0].as_concat_text();
        // Round 5: 146 characters for 36,026 of conversation is stored flagged, after the facts.
        assert!(
            pillars::model_part(&stored).contains("<transcript summary>"),
            "{stored}"
        );
    }

    struct RefusingProvider {
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl Provider for RefusingProvider {
        fn get_name(&self) -> &str {
            "refusing"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(ProviderError::ContextLengthExceeded("too long".to_string()))
        }
    }

    /// Plausible item: a provider that keeps refusing got up to eight full-prefill transcript
    /// requests (five removal steps, three cuts); a CHAT's recovery sends at most
    /// MAX_TRANSCRIPT_CALLS. A swarm worker keeps its golden ladder (round 4, C): all eight.
    #[tokio::test]
    async fn the_refused_transcript_ladder_is_bounded() {
        let model_config = ModelConfig::new("refusing").with_context_limit(Some(262_144));
        let provider = RefusingProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let mut inputs = chat_inputs(None, false);
        inputs.trigger = CompactionTrigger::Recovery;
        let result = compact_chat(
            &provider,
            &model_config,
            "refusing-session",
            &tool_loop(10),
            false,
            &SummaryRequest::Transcript,
            &inputs,
        )
        .await;
        assert!(result.is_err());
        let calls = provider.calls.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            (3..=MAX_TRANSCRIPT_CALLS).contains(&calls),
            "every refusal was read and the ladder stopped at its bound: {calls}"
        );
        let worker = RefusingProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let result = compact_messages(
            &worker,
            &model_config,
            "refusing-worker",
            &tool_loop(10),
            false,
            &SummaryRequest::Transcript,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(worker.calls.load(std::sync::atomic::Ordering::SeqCst), 8);
    }

    /// E2E (2026-10-30, compaction-fix-2): a real long transcript compacted by a real local model
    /// through the chat-shaped request. Env: E2E_SESSION_JSON (an array of goose Messages),
    /// E2E_WINDOW, E2E_PROMPT_TOKENS (the conversation is cut from its end to this many tokens),
    /// GOOSE_MODEL = GOOSE_FAST_MODEL, OPENAI_HOST, OPENAI_API_KEY. Prints what the gate read.
    #[tokio::test]
    #[ignore = "needs a local engine and a session export"]
    async fn e2e_a_real_model_writes_a_summary_the_gate_reads() {
        let var = |name: &str| std::env::var(name).unwrap();
        // A session export (`[{id, role, created, content, metadata}]`) or a fixture object
        // (`{system_prompt, tools, messages}`, as the real-transcript guard test reads).
        let input: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(var("E2E_SESSION_JSON")).unwrap())
                .unwrap();
        let (mut messages, request): (Vec<Message>, SummaryRequest) = match input {
            serde_json::Value::Array(_) => (serde_json::from_value(input).unwrap(), frame()),
            fixture => (
                serde_json::from_value(fixture["messages"].clone()).unwrap(),
                SummaryRequest::ExtendsChat(ChatRequestFrame {
                    system_prompt: fixture["system_prompt"].as_str().unwrap().to_string(),
                    tools: serde_json::from_value(fixture["tools"].clone()).unwrap(),
                }),
            ),
        };
        let window: usize = var("E2E_WINDOW").parse().unwrap();
        let prompt_tokens: usize = var("E2E_PROMPT_TOKENS").parse().unwrap();
        let counter = create_token_counter().await.unwrap();
        while counter.count_chat_tokens("", &messages, &[]) > prompt_tokens {
            messages.pop();
        }
        let provider = crate::providers::create("openai", Vec::new())
            .await
            .unwrap();
        let model_config = ModelConfig::new(var("GOOSE_MODEL")).with_context_limit(Some(window));
        let point = compaction_threshold(window, &model_config);
        let written = Arc::new(std::sync::Mutex::new(None));
        let mut inputs = chat_inputs(None, false);
        inputs.context_limit = Some(window);
        inputs.trigger = CompactionTrigger::Auto {
            used: Some(prompt_tokens),
            limit: Some(window),
            threshold: point,
        };
        let seen = written.clone();
        inputs.progress = Some(Arc::new(move |progress: WritingProgress| {
            *seen.lock().unwrap() = Some(progress);
        }));
        let started = std::time::Instant::now();
        if let Ok(dir) = std::env::var("E2E_WORKING_DIR") {
            inputs.working_dir = std::path::PathBuf::from(dir);
        }
        let compacted = compact_chat(
            provider.as_ref(),
            &model_config,
            "e2e-compaction",
            &Conversation::new_unvalidated(messages.clone()),
            false,
            &request,
            &inputs,
        )
        .await
        .unwrap();
        let ChatCompacted::Compacted {
            conversation,
            usage,
            kept,
            ..
        } = compacted
        else {
            panic!("asked");
        };
        let stored = conversation.agent_visible_messages()[0].as_concat_text();
        let model_text = pillars::model_part(&stored).to_string();
        let sections = pillars::written_sections(&model_text);
        // What the model reads on its next call: every row it sees after the compaction.
        if let Ok(out) = std::env::var("E2E_OUT") {
            let seen: Vec<String> = conversation
                .agent_visible_messages()
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    format!(
                        "===== row {i} [{:?}] =====\n{}",
                        m.role,
                        format_message_for_compacting(m)
                    )
                })
                .collect();
            std::fs::write(out, seen.join("\n")).unwrap();
        }
        println!(
            "E2E messages={} counted_prompt={} window={window} point={:.4} ({}) elapsed={:.0}s",
            messages.len(),
            counter.count_chat_tokens("", &messages, &[]),
            point,
            (point * window as f64) as usize,
            started.elapsed().as_secs_f64()
        );
        println!(
            "E2E usage input={:?} output={:?} streamed_tokens={:?} tail={} shortfall={:?}",
            usage.usage.input_tokens,
            usage.usage.output_tokens,
            written
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|p| p.written_tokens),
            kept.map_or(0, |k| k.tail_messages),
            summary_shortfall(&model_text)
        );
        for (part, body) in &sections {
            println!(
                "E2E section {} = {} chars",
                WRITTEN_PARTS[*part].0,
                body.len()
            );
        }
        println!(
            "E2E model text ({} chars):\n{model_text}\nE2E END",
            model_text.len()
        );
        let junk = [
            FLASH_NEXT_NON_SUMMARY.to_string(),
            WRITTEN_PARTS
                .iter()
                .map(|(heading, ask)| format!("## {heading}\n{ask}\n"))
                .collect(),
            format!("```\n{model_text}\n```"),
            model_text
                .split_once("## What each file does")
                .map_or(String::new(), |(first, _)| first.to_string()),
        ];
        for (i, junk) in junk.iter().enumerate() {
            println!("E2E junk {i} shortfall={:?}", summary_shortfall(junk));
        }
    }

    /// Answers chat-shaped requests with the Flash-Next non-summary and transcript requests with a
    /// one-liner; counts both.
    struct OneLinerProvider {
        calls: std::sync::Mutex<Vec<usize>>,
    }

    #[async_trait]
    impl Provider for OneLinerProvider {
        fn get_name(&self) -> &str {
            "one-liner"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.calls.lock().unwrap().push(tools.len());
            let text = if tools.is_empty() {
                "I will continue with the task."
            } else {
                FLASH_NEXT_NON_SUMMARY
            };
            Ok(stream_from_single_message(
                Message::assistant().with_text(text),
                ProviderUsage::new("one-liner".to_string(), Usage::default()),
            ))
        }
    }

    /// One answer with three tool calls as the agent stores it: three assistant rows, each
    /// carrying the answer's reasoning, each followed by its result.
    fn split_answer(turn: usize, reasoning_chars: usize) -> Vec<Message> {
        let reasoning: String = format!("Turn {turn}: check app/store.py against the contract. ")
            .repeat(reasoning_chars / 50 + 1);
        let mut rows = Vec::new();
        for call in 0..3 {
            let id = format!("t{turn}c{call}");
            let mut params = CallToolRequestParams::new("shell");
            params.arguments =
                serde_json::json!({"command": format!("sed -n {call},40p app/store.py")})
                    .as_object()
                    .cloned();
            rows.push(
                Message::assistant()
                    .with_thinking(reasoning.clone(), "")
                    .with_tool_request(id.clone(), Ok(params)),
            );
            rows.push(Message::user().with_tool_response(
                id,
                Ok(shell_result(
                    &format!("{call}: def now_iso()\n").repeat(20),
                    "",
                    0,
                )),
            ));
        }
        rows
    }

    /// Round 4, A: the guard trusts the provider. With a call noted, the count is the prompt its
    /// provider reported plus goose's count of ONLY the rows written since; once the rows no longer
    /// open with what that call carried (a compaction rewrote them), the session's stored usage
    /// stands in, plus the rows after its answer; with neither, the whole request is counted, its
    /// frame included.
    #[tokio::test]
    async fn the_guard_reads_the_reported_prompt_plus_only_the_rows_written_since() {
        let counter = create_token_counter().await.unwrap();
        let session = "guard-reported-prompt";
        let mut messages = vec![Message::user().with_text("build the console")];
        for turn in 0..4 {
            messages.extend(split_answer(turn, 4_000));
        }
        let sent = messages.len() - 6;
        note_sent_prompt(session, &messages[..sent], Some(200_000));
        let costs = counter.message_costs(&messages);
        let since: usize = costs[sent..].iter().map(|c| c.tokens).sum();
        assert!(since > 0);
        assert_eq!(
            chat_context_tokens(session, &messages, Some(150_000), None, &counter),
            (200_000 + goose_counted(since), CountSource::LastCall)
        );
        // The rows the call carried were rewritten: the base no longer holds.
        let mut rewritten = messages.clone();
        rewritten[sent - 1] = Message::user().with_text("a summary");
        let after_answer = rewritten
            .iter()
            .rposition(|m| m.role == Role::Assistant)
            .unwrap()
            + 1;
        let tail: usize = counter.message_costs(&rewritten)[after_answer..]
            .iter()
            .map(|c| c.tokens)
            .sum();
        assert_eq!(
            chat_context_tokens(session, &rewritten, Some(150_000), None, &counter),
            (150_000 + goose_counted(tail), CountSource::StoredUsage)
        );
        let tools = chat_tools();
        let frame = Some(("You are goose.", tools.as_slice()));
        assert_eq!(
            chat_context_tokens(session, &rewritten, None, frame, &counter),
            (
                goose_counted(counter.count_chat_tokens("You are goose.", &rewritten, &tools)),
                CountSource::Estimated
            )
        );
        note_sent_prompt(session, &messages, None);
        assert_eq!(
            chat_context_tokens(session, &messages, None, None, &counter).1,
            CountSource::Estimated,
            "a call that reported no prompt leaves no base"
        );
    }

    /// Round 4, A: the guard's own count follows the request, so an answer's reasoning that its
    /// split tool-call rows repeat is counted ONCE on the guard's path too. Before round 4 the
    /// guard counted each row alone: these 25 split answers read ~2x their size and compacted at
    /// half the point.
    #[tokio::test]
    async fn the_guard_counts_an_answer_s_reasoning_once() {
        let counter = create_token_counter().await.unwrap();
        let mut messages = vec![Message::user().with_text("build the console")];
        for turn in 0..25 {
            messages.extend(split_answer(turn, 8_000));
        }
        let whole = counter.count_chat_tokens("", &messages, &[]);
        let alone: usize = messages
            .iter()
            .map(|m| counter.count_chat_tokens("", std::slice::from_ref(m), &[]))
            .sum();
        assert!(alone as f64 > whole as f64 * 1.5, "{alone} vs {whole}");
        let (guarded, _) =
            chat_context_tokens("guard-split-reasoning", &messages, None, None, &counter);
        assert_eq!(guarded, goose_counted(whole));
        // Through the guard itself: a window whose point sits between the two counts.
        let provider = MockProvider::new(
            Message::assistant().with_text("x"),
            (whole + alone) / 2 * 10 / 8,
        );
        let session = crate::session::Session {
            id: "guard-split-reasoning-2".to_string(),
            model_config: Some(ModelConfig::new("split")),
            ..Default::default()
        };
        let conversation = Conversation::new_unvalidated(messages.clone());
        let needed = check_if_compaction_needed(
            &provider,
            &conversation,
            Some(0.8),
            &session,
            GuardCount::Chat(None),
        )
        .await
        .unwrap();
        assert!(
            !needed,
            "{whole} tokens on a {}-token window",
            (whole + alone) / 2 * 10 / 8
        );
        // A kept tail that opens between the split rows is sent with the reasoning: it adds it.
        let costs = counter.message_costs(&messages);
        let opens_on_a_repeat = messages.len() - 4;
        assert!(costs[opens_on_a_repeat].repeated_reasoning > 0);
        assert_eq!(
            crate::token_counter::run_tokens(&costs, opens_on_a_repeat),
            counter.count_chat_tokens("", &messages[opens_on_a_repeat..], &[]) - 3
        );
    }

    /// Round 4, A: the kept tail counts rows as the request sends them — an answer's reasoning
    /// once, and added back only for the row the tail opens on.
    #[tokio::test]
    async fn the_kept_tail_counts_an_answer_s_reasoning_once() {
        let counter = create_token_counter().await.unwrap();
        let mut messages = vec![Message::user().with_text("build the console")];
        for turn in 0..3 {
            messages.extend(split_answer(turn, 8_000));
        }
        let answer = messages.len() - 6;
        let budget = counter.count_chat_tokens("", &messages[answer..], &[]) + 16;
        assert_eq!(tail_that_fits(&messages, budget, &counter), 6);
        let alone: usize = messages[answer..]
            .iter()
            .map(|m| counter.count_chat_tokens("", std::slice::from_ref(m), &[]))
            .sum();
        assert!(alone > budget * 2, "{alone} vs {budget}");
    }

    /// Round 4, A, on the real transcript: the verifier's 223-row Flash-Next stopped run and the
    /// 243,299 prompt tokens its engine reported for them with the summary instruction. Ignored:
    /// the transcript is a benchmark's work and stays out of this public repo — run with
    /// GOOSE_COMPACTION_FIXTURE=<flash-223.json> (`{system_prompt, tools, messages, instruction,
    /// recorded: [{rows, with_instruction, prompt_tokens}]}`).
    #[tokio::test]
    #[ignore]
    async fn real_transcript_the_guard_is_within_the_engine_s_count() {
        let path = std::env::var("GOOSE_COMPACTION_FIXTURE").expect("GOOSE_COMPACTION_FIXTURE");
        let fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let system = fixture["system_prompt"].as_str().unwrap().to_string();
        let tools: Vec<Tool> = serde_json::from_value(fixture["tools"].clone()).unwrap();
        let messages: Vec<Message> = serde_json::from_value(fixture["messages"].clone()).unwrap();
        let instruction = Message::user().with_text(fixture["instruction"].as_str().unwrap());
        let counter = create_token_counter().await.unwrap();
        let engine_of = |r: &serde_json::Value| {
            r.get("engine")
                .or(fixture.get("engine"))
                .and_then(|e| e.as_str())
                .unwrap_or_default()
                .to_string()
        };
        let alone: usize = messages
            .iter()
            .map(|m| counter.count_chat_tokens("", std::slice::from_ref(m), &[]))
            .sum();
        println!("REAL per-row-alone (round 3 guard) = {alone}");
        let recorded = fixture["recorded"].as_array().unwrap();
        let mut checked = 0;
        let mut failures: Vec<String> = Vec::new();
        for later in recorded {
            let rows = later["rows"].as_u64().unwrap() as usize;
            let engine = later["prompt_tokens"].as_u64().unwrap() as f64;
            let mut sent = messages[..rows].to_vec();
            if later["with_instruction"].as_bool().unwrap_or(false) {
                sent.push(instruction.clone());
            }
            let (estimated, source) = chat_context_tokens(
                &format!("real-{rows}"),
                &sent,
                None,
                Some((&system, &tools)),
                &counter,
            );
            assert_eq!(source, CountSource::Estimated);
            let off = estimated as f64 / engine - 1.0;
            println!(
                "REAL estimate {} rows={rows} engine={engine} goose={estimated} off={:+.2}%",
                engine_of(later),
                off * 100.0
            );
            // A prompt far under any compaction point decides nothing: the estimate is held to
            // the bound where a decision can be near (100k tokens and up).
            if engine >= 100_000.0
                && !later["image_not_counted"].as_bool().unwrap_or(false)
                && !(-0.01..=0.03).contains(&off)
            {
                failures.push(format!("estimate rows {rows}: {off:+.4}"));
            }
            // The provider path: an earlier call's reported prompt plus the rows since.
            for earlier in recorded {
                let earlier_rows = earlier["rows"].as_u64().unwrap() as usize;
                if earlier_rows >= rows
                    || earlier["with_instruction"].as_bool().unwrap_or(false)
                    || engine_of(earlier) != engine_of(later)
                {
                    continue;
                }
                let session = format!("real-{earlier_rows}-{rows}");
                note_sent_prompt(
                    &session,
                    &messages[..earlier_rows],
                    Some(earlier["prompt_tokens"].as_i64().unwrap() as i32),
                );
                let (decided, source) = chat_context_tokens(&session, &sent, None, None, &counter);
                assert_eq!(source, CountSource::LastCall);
                let off = decided as f64 / engine - 1.0;
                println!(
                    "REAL reported {} rows {earlier_rows}->{rows}: engine={engine} decided={decided} off={:+.2}%",
                    engine_of(later),
                    off * 100.0
                );
                // An engine that did not count an image goose counts is over-counted, never under.
                let bound = if later["image_not_counted"].as_bool().unwrap_or(false) {
                    -0.01..=0.06
                } else {
                    -0.01..=0.03
                };
                if !bound.contains(&off) {
                    failures.push(format!("reported {earlier_rows}->{rows}: {off:+.4}"));
                }
                checked += 1;
            }
        }
        println!("REAL provider-path pairs checked: {checked}");
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// Records what a real engine reports as the prompt of goose's own request at several points
    /// of the real transcript (max_tokens 1), into the fixture the guard test reads. Ignored: needs
    /// an engine (OPENAI_HOST, GOOSE_MODEL) and GOOSE_COMPACTION_FIXTURE; writes E2E_RECORD_OUT.
    #[tokio::test]
    #[ignore]
    async fn real_transcript_record_engine_prompts() {
        use futures::StreamExt;
        let path = std::env::var("GOOSE_COMPACTION_FIXTURE").unwrap();
        let mut fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let system = fixture["system_prompt"].as_str().unwrap().to_string();
        let tools: Vec<Tool> = serde_json::from_value(fixture["tools"].clone()).unwrap();
        let messages: Vec<Message> = serde_json::from_value(fixture["messages"].clone()).unwrap();
        // Points a request may end on: right after a tool result whose next row is the model's.
        let ends: Vec<usize> = (1..messages.len())
            .filter(|&k| {
                opens_on_a_tool_result(&messages[k - 1])
                    && messages[k].role == Role::Assistant
                    && !opens_on_a_tool_result(&messages[k])
            })
            .collect();
        let picks: Vec<usize> = match std::env::var("E2E_RECORD_ROWS") {
            Ok(rows) => rows.split(',').map(|r| r.parse().unwrap()).collect(),
            Err(_) => (1..=6).map(|i| ends[ends.len() * i / 6 - 1]).collect(),
        };
        let provider = crate::providers::create("openai", Vec::new())
            .await
            .unwrap();
        let mut model_config = ModelConfig::new(std::env::var("GOOSE_MODEL").unwrap());
        model_config.max_tokens = Some(1);
        let mut recorded = Vec::new();
        for rows in picks {
            let sent = crate::agents::reply_parts::messages_for_provider(
                fix_conversation(Conversation::new_unvalidated(messages[..rows].to_vec()))
                    .0
                    .messages(),
                false,
            );
            let mut stream = provider
                .stream(&model_config, &system, sent.messages(), &tools)
                .await
                .unwrap();
            let mut prompt = None;
            while let Some(item) = stream.next().await {
                if let Ok((_, Some(usage))) = item {
                    prompt = usage.usage.input_tokens.or(prompt);
                }
            }
            println!("RECORD rows={rows} prompt_tokens={prompt:?}");
            if let Some(prompt) = prompt {
                recorded.push(serde_json::json!({
                    "rows": rows, "with_instruction": false, "prompt_tokens": prompt,
                    "engine": model_config.model_name,
                }));
            }
        }
        let mut all = fixture["recorded"].as_array().cloned().unwrap_or_default();
        all.extend(recorded);
        fixture["recorded"] = serde_json::Value::Array(all);
        std::fs::write(
            std::env::var("E2E_RECORD_OUT").unwrap(),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();
    }

    /// Answers every chat-shaped request with one fixed message; counts them.
    struct FixedProvider {
        answer: Message,
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl Provider for FixedProvider {
        fn get_name(&self) -> &str {
            "fixed"
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(stream_from_single_message(
                self.answer.clone(),
                ProviderUsage::new("fixed".to_string(), Usage::default()),
            ))
        }
    }

    /// The verifier's real-model shape (Flash-Next, thinking on, 2026-10-30): the five sections
    /// written in the REASONING, the visible answer empty or a stray
    /// `<tool_call></function></tool_call>` (what the code before round 3 stored, graded 0/52).
    /// The summary is taken from the reasoning — its last complete block — with no second ask.
    #[tokio::test]
    async fn a_summary_written_in_the_reasoning_is_taken_from_there() {
        let reasoning = format!(
            "The person wants a summary. Draft:\n## Where we are\nan early draft\n\nLet me \
             redo it properly.\n\n{EVERY_PART}\n"
        );
        for visible in ["", "<tool_call></function></tool_call>"] {
            let mut answer = Message::assistant().with_thinking(reasoning.clone(), "");
            if !visible.is_empty() {
                answer = answer.with_text(visible);
            }
            let provider = FixedProvider {
                answer,
                calls: std::sync::atomic::AtomicUsize::new(0),
            };
            let ChatCompacted::Compacted { conversation, .. } = compact_chat(
                &provider,
                &ModelConfig::new("fixed"),
                "s",
                &two_turn_chat(),
                false,
                &frame(),
                &chat_inputs(None, false),
            )
            .await
            .unwrap() else {
                panic!("compacted");
            };
            assert_eq!(
                provider.calls.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "no second ask"
            );
            let stored = conversation.agent_visible_messages()[0].as_concat_text();
            assert!(stored.starts_with("## Where we are\nsync done"), "{stored}");
            assert!(
                !stored.contains("an early draft"),
                "the last complete block: {stored}"
            );
            assert!(!stored.contains("tool_call"), "{stored}");
            assert!(!stored.starts_with(pillars::FLAG_LEAD), "{stored}");
        }
    }

    /// A `write` call and its result as the developer tool leaves them: the diff in `_meta`.
    fn written(id: &str, path: &str, content: &str) -> [Message; 2] {
        use crate::agents::platform_extensions::developer::file_diff::{
            diff_texts, with_file_diff, Before,
        };
        let mut params = CallToolRequestParams::new("write");
        params.arguments = serde_json::json!({"path": path, "content": content})
            .as_object()
            .cloned();
        let result = with_file_diff(
            rmcp::model::CallToolResult::success(vec![
                RawContent::text(format!("Created {path}")).no_annotation()
            ]),
            path,
            Before::None,
            &diff_texts(path, None, content),
        );
        [
            Message::assistant().with_tool_request(id, Ok(params)),
            Message::user().with_tool_response(id, Ok(result)),
        ]
    }

    /// A shell call and its result with its exit code.
    fn shelled(id: &str, command: &str, stdout: &str, exit_code: i64) -> [Message; 2] {
        let mut params = CallToolRequestParams::new("shell");
        params.arguments = serde_json::json!({"command": command}).as_object().cloned();
        [
            Message::assistant().with_tool_request(id, Ok(params)),
            Message::user().with_tool_response(id, Ok(shell_result(stdout, "", exit_code))),
        ]
    }

    /// Round 6 (2026-10-30, verifier's FIX4 defect 3, mocks m4-reason-plan and
    /// m4-reason-trailing): planning notes in the reasoning ("Constraints:", "Approach:", … five
    /// colon lines) with an empty visible answer were stored as the summary, unflagged and never
    /// re-asked; a summary drafted in the reasoning was stored with the thinking after it ("Wait,
    /// I should double-check…").
    #[tokio::test]
    async fn planning_notes_in_the_reasoning_are_not_the_summary_and_trailing_thought_is_cut() {
        let plan =
            "The user wants a summary of this conversation.\nConstraints:\n- five sections, \
            no tools.\nWhat I know:\n- lots of files were written.\nApproach:\n- go section by \
            section.\nOutput format:\n- markdown headings.\nRisks:\n- running long.\n";
        let provider = FixedProvider {
            answer: Message::assistant().with_thinking(plan, ""),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let ChatCompacted::Compacted { conversation, .. } = compact_chat(
            &provider,
            &ModelConfig::new("fixed"),
            "s",
            &two_turn_chat(),
            false,
            &frame(),
            &chat_inputs(None, false),
        )
        .await
        .unwrap() else {
            panic!("compacted");
        };
        assert_eq!(
            provider.calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "re-asked once"
        );
        let stored = conversation.agent_visible_messages()[0].as_concat_text();
        assert!(stored.starts_with(pillars::FLAG_LEAD), "{stored}");

        let trailing = "Let me draft the summary.\n\n## Where we are\n\nServices on 8121.\n\n\
            ## What each file does\n\napp/ledgerd.py serves the API.\n\n## Decisions and reasons\n\n\
            SQLite DELETE.\n\n## Still to do\n\nframing.\n\n## Next step\n\nFix the camera \
            framing.\n\nWait, I should double-check whether fitcheck.py really crashed. Hmm.";
        let summary = summary_in_reasoning(&Message::assistant().with_thinking(trailing, ""))
            .expect("taken from the reasoning");
        assert!(summary.ends_with("Fix the camera framing."), "{summary}");
        assert!(summary.starts_with("## Where we are"), "{summary}");
        let inline = "**Where we are:** a\n**What each file does:** b\n**Decisions and reasons:** \
            c\n**Still to do:** d\n**Next step:** fix it\n\nWait, let me re-check.";
        let summary =
            summary_in_reasoning(&Message::assistant().with_thinking(inline, "")).unwrap();
        assert!(summary.ends_with("**Next step:** fix it"), "{summary}");
    }

    /// Round 6 check of the verifier's FIX4 defect 2 (five bold headings of planning,
    /// "**Thinking:** I should continue …"): round 5 already stores it FLAGGED, on a small chat as
    /// on a big one (most sections under a sentence) — pinned here, no new rule needed. A
    /// translated summary stays unflagged (round 5's design).
    #[tokio::test]
    async fn five_bold_planning_lines_are_flagged_on_a_small_chat_too() {
        let bold = "**Thinking:** I should continue.\n\n**Plan:** look at files.\n\n**Step one:** \
                    run ls.\n\n**Step two:** read output.\n\n**Then:** carry on.\n";
        let stored = compacted_with(bold, &two_turn_chat()).await;
        assert!(stored.starts_with(pillars::FLAG_LEAD), "{stored}");
        assert!(stored.contains("under one sentence"), "{stored}");
    }

    /// Round 6 (2026-10-30, verifier's FIX4 defect 4, the real Flash-Next transcript): of 3
    /// "Errors still open" 2 were noise — `wc -c app/**/*.py` exiting 1 on a glob that matched
    /// nothing, and `cd /tmp && curl …` that later worked as `cd . && curl …`. A look-up finding
    /// nothing is not an error, a later success of the same command from any folder settles it;
    /// real failures (a crash, a compile error, a pipe into a script, `sed -i`) stay open.
    #[test]
    fn a_look_up_finding_nothing_or_a_command_rerun_elsewhere_is_not_an_open_error() {
        let mut messages = vec![Message::user().with_text("build the ledger")];
        for (id, command, out, code) in [
            (
                "p1",
                "find app web -type f | head -50; echo \"---\"; wc -c app/**/*.py 2>/dev/null",
                "app/transport.py",
                1,
            ),
            ("p2", "ls /w/DECISIONS.md", "", 2),
            ("p3", "test -f /w/out.json && cat /w/out.json", "", 1),
            ("p4", "grep -n TODO /w/app/sync.py", "", 1),
            (
                "c1",
                "cd /tmp && curl -s http://127.0.0.1:8850/v3 | python3 -c 'print(1)'",
                "Traceback",
                1,
            ),
            (
                "c2",
                "cd . && curl -s http://127.0.0.1:8850/v3 | python3 -c 'print(1)'",
                "1",
                0,
            ),
            (
                "f1",
                "cd . && python3 fitcheck.py",
                "Traceback (most recent call last):",
                1,
            ),
            ("f2", "cargo build", "error[E0425]: cannot find value", 101),
            ("f3", "cat data.json | python3 parse.py", "KeyError", 1),
            ("f4", "sed -i 's/a/b/' /w/app/missing.py", "No such file", 1),
        ] {
            messages.extend(shelled(id, command, out, code));
        }
        let kept = Pillars::build(
            &messages,
            &KeptSources::default(),
            LedgerRead::Entries(Vec::new()),
        );
        let rendered = kept.render();
        let open = rendered
            .split_once("## Errors still open (4)")
            .unwrap_or_else(|| panic!("{rendered}"))
            .1;
        let open = open.split_once("\n## ").map_or(open, |(open, _)| open);
        for real in ["fitcheck.py", "cargo build", "parse.py", "sed -i"] {
            assert!(open.contains(real), "{real}: {open}");
        }
        for noise in ["wc -c", "DECISIONS.md", "test -f", "grep -n", "cd /tmp"] {
            assert!(!open.contains(noise), "{noise}: {open}");
        }
        assert!(
            open.contains("1 earlier failed calls were settled"),
            "{open}"
        );
        assert!(open.contains("4 failed look-ups"), "{open}");
    }

    /// Steward 2026-10-31 (the live MiMo run 8985a86d, second compaction at call ~121): a call
    /// that wrote a probe and ran it failed in its run; the next plain run of the probe worked,
    /// yet the kept block listed the first as an open error and a to-do. A probe line ending on
    /// `grep … /proc/cpuinfo` (no /proc on macOS) exited 2 from that look-up alone, listed too.
    /// The exit code is the LAST top-level step's: that step is what a later success settles and
    /// what decides a look-up. A write to a file is never a look-up; a real failure stays open.
    #[test]
    fn a_failed_call_is_judged_by_the_step_its_exit_code_came_from() {
        let mut messages = vec![Message::user().with_text("build the console")];
        for (id, command, out, code) in [
            (
                "w1",
                "cd /w && cat > test/perf2.mjs <<'EOF'\nconst r = 1; console.log(r);\nEOF\nnode test/perf2.mjs",
                "TimeoutError",
                1,
            ),
            ("w2", "cd /w && node test/perf2.mjs", "{\"setCam\":[1694]}", 0),
            (
                "g1",
                "cd /w && cat > test/fill.mjs <<'EOF'\nlet x;\nEOF\nnode test/fill.mjs; nproc; grep -m1 \"model name\" /proc/cpuinfo",
                "{\"full\":[1714]}\n14\ngrep: /proc/cpuinfo: No such file or directory",
                2,
            ),
            (
                "r1",
                "cd /w && cat > test/dump.py <<'EOF'\nprint(1)\nEOF\npython3 test/dump.py",
                "Traceback (most recent call last):",
                1,
            ),
            ("r2", "cd /w && python3 - <<'EOF'\nprint(2)\nEOF\nls -la test/", "2", 0),
            ("x1", "echo done > /ro/state.txt", "Permission denied", 1),
        ] {
            messages.extend(shelled(id, command, out, code));
        }
        let kept = Pillars::build(
            &messages,
            &KeptSources::default(),
            LedgerRead::Entries(Vec::new()),
        );
        let rendered = kept.render();
        let open = rendered
            .split_once("## Errors still open (2)")
            .unwrap_or_else(|| panic!("{rendered}"))
            .1;
        let open = open.split_once("\n## ").map_or(open, |(open, _)| open);
        // A different program that later worked does not settle the dump; a failed write is real.
        for real in ["test/dump.py", "/ro/state.txt"] {
            assert!(open.contains(real), "{real}: {open}");
        }
        for noise in ["perf2.mjs", "fill.mjs"] {
            assert!(!open.contains(noise), "{noise}: {open}");
        }
        assert!(
            open.contains("1 earlier failed calls were settled"),
            "{open}"
        );
        assert!(open.contains("1 failed look-ups"), "{open}");
    }

    const SYNC_PY: &str =
        "import json\n\nclass Store:\n    def get(self, key):\n        return key\n\n\
        def sync_all(store):\n    return []\n\nROUTES = {\"GET\": \"/api/payments\"}\n\
        def handle(path):\n    if path == \"/api/sync\":\n        return sync_all(None)\n";

    /// A chat whose summary is the facts' to carry: the request, a file written, a test that
    /// failed and was then fixed, a run that still fails, and a check that worked.
    fn worked_chat(request: &str) -> Conversation {
        let mut messages = vec![Message::user().with_text(request)];
        messages.extend(written("w1", "/w/app/sync.py", SYNC_PY));
        // An edit that left out its path fails; the next edit, naming it, settles it.
        for (id, args, ok) in [
            (
                "e1",
                serde_json::json!({"before": "[]", "after": "[1]"}),
                false,
            ),
            (
                "e2",
                serde_json::json!({"path": "/w/app/sync.py", "before": "[]", "after": "[1]"}),
                true,
            ),
        ] {
            let mut params = CallToolRequestParams::new("edit");
            params.arguments = args.as_object().cloned();
            let text = vec![
                RawContent::text(if ok { "Edited" } else { "Error: no path" }).no_annotation(),
            ];
            let result = if ok {
                rmcp::model::CallToolResult::success(text)
            } else {
                rmcp::model::CallToolResult::error(text)
            };
            messages.push(Message::assistant().with_tool_request(id, Ok(params)));
            messages.push(Message::user().with_tool_response(id, Ok(result)));
        }
        messages.extend(shelled(
            "s1",
            "cd /w && python3 -m pytest -q",
            "1 failed",
            1,
        ));
        messages.extend(shelled(
            "s2",
            "cd /w && python3 -m pytest -q",
            "41 passed",
            0,
        ));
        messages.extend(shelled(
            "s3",
            "cd /w && python3 app/boot.py",
            "ImportError: boot",
            1,
        ));
        messages.extend(shelled("s4", "cat /w/SPEC.md", "# SPEC", 0));
        messages.push(Message::assistant().with_text("app/sync.py written"));
        Conversation::new_unvalidated(messages)
    }

    /// The facts goose builds itself back every stored summary — the request word for word, each
    /// written file with its size and what it defines, the errors still open (a failure a later
    /// identical call settled is not one), the commands that worked, the newest calls with their
    /// outcome — whatever the model wrote.
    #[tokio::test]
    async fn every_stored_summary_carries_goose_s_facts() {
        let request = "write app/sync.py from SPEC.md: inactive means no login in 24 months";
        let provider = SequenceProvider {
            answers: vec![EVERY_PART.to_string()],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let ChatCompacted::Compacted { conversation, .. } = compact_chat(
            &provider,
            &ModelConfig::new("sequence"),
            "s",
            &worked_chat(request),
            false,
            &frame(),
            &chat_inputs(None, false),
        )
        .await
        .unwrap() else {
            panic!("compacted");
        };
        let stored = conversation.agent_visible_messages()[0].as_concat_text();
        assert!(stored.starts_with("## Where we are\nsync done"), "{stored}");
        let facts = stored.split_once(pillars::KEPT_OPEN).unwrap().1;
        // The request is kept once: re-sent word for word right after the summary, and the
        // facts say so.
        assert!(
            facts.contains("re-sent word for word right after"),
            "{facts}"
        );
        let rows = conversation.agent_visible_messages();
        assert_eq!(rows.last().unwrap().as_concat_text(), request);
        assert!(facts.contains("/w/app/sync.py — created"), "{facts}");
        assert!(
            facts.contains("13 lines")
                && facts.contains("as last written in the conversation")
                && facts.contains("class Store")
                && facts.contains("  def get")
                && facts.contains("def sync_all")
                && facts.contains("def handle")
                && facts.contains("routes /api/payments /api/sync"),
            "{facts}"
        );
        let open = facts.split_once("## Errors still open (1)").unwrap().1;
        assert!(open.contains("python3 app/boot.py"), "{facts}");
        assert!(
            open.contains("2 earlier failed calls were settled"),
            "{facts}"
        );
        let worked = facts.split_once("## Commands that worked").unwrap().1;
        let worked = worked.split_once("##").unwrap().0;
        assert!(worked.contains("cd /w && python3 -m pytest -q"), "{facts}");
        assert!(
            !worked.contains("cat /w/SPEC.md"),
            "a look is not a command that worked"
        );
        assert!(!worked.contains("boot.py"), "{facts}");
        let calls = facts.split_once("tool calls (oldest first)").unwrap().1;
        assert!(
            calls.contains("shell")
                && calls.contains("succeeded (exit 0)")
                && calls.contains("41 passed"),
            "{facts}"
        );
        assert!(calls.contains("failed (exit 1)"), "{facts}");
    }

    /// A one-liner from either request is stored FLAGGED with the facts, after at most one more
    /// ask on the chat's prefix and never a second transcript prefill: the chat-shaped request
    /// sends 2 requests and no transcript, the recovery transcript request sends 1.
    #[tokio::test]
    async fn a_one_liner_is_stored_flagged_with_the_facts_without_another_prefill() {
        let request = "write app/sync.py from SPEC.md: inactive means no login in 24 months";
        for (request_shape, expected) in
            [(frame(), vec![1, 1]), (SummaryRequest::Transcript, vec![0])]
        {
            let provider = OneLinerProvider {
                calls: std::sync::Mutex::new(Vec::new()),
            };
            let mut inputs = chat_inputs(None, false);
            if matches!(request_shape, SummaryRequest::Transcript) {
                inputs.trigger = CompactionTrigger::Recovery;
            }
            let ChatCompacted::Compacted { conversation, .. } = compact_chat(
                &provider,
                &ModelConfig::new("one-liner"),
                "s",
                &worked_chat(request),
                false,
                &request_shape,
                &inputs,
            )
            .await
            .unwrap() else {
                panic!("compacted");
            };
            let calls: Vec<usize> = provider
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|t| (*t > 0) as usize)
                .collect();
            assert_eq!(
                calls, expected,
                "requests (1 = chat-shaped, 0 = transcript)"
            );
            let rows = conversation.agent_visible_messages();
            let stored = rows[0].as_concat_text();
            assert!(stored.starts_with(pillars::FLAG_LEAD), "{stored}");
            assert!(
                rows.iter().any(|m| m.as_concat_text() == request),
                "the request, word for word"
            );
            assert!(stored.contains("def sync_all"), "{stored}");
            assert!(stored.contains("python3 app/boot.py"), "{stored}");
        }
    }

    /// Fix 3: a manual compaction keeps the tail an automatic one keeps — the person's `/compact`
    /// row (stored for the person alone just before) is passed over, not stopped at — and the
    /// command never becomes the agent's.
    #[tokio::test]
    async fn a_manual_compaction_keeps_the_tail_without_the_command() {
        let good = sections([
            "app/sync.py written, 41 tests pass",
            "app/sync.py — the 24-month rule",
            "Nothing.",
            "nothing, the task is complete",
            "report the 41 passing tests",
        ]);
        let messages = vec![
            Message::user().with_text("read SPEC.md"),
            Message::assistant().with_text("SPEC.md asks for app/sync.py"),
            Message::user().with_text("write app/sync.py with the 24-month rule"),
            Message::assistant().with_text("app/sync.py written, 41 tests pass"),
        ];
        let mut with_command = messages.clone();
        with_command.push(
            Message::user()
                .with_text("/compact")
                .with_visibility(true, false),
        );
        let counter = create_token_counter().await.unwrap();
        assert_eq!(
            tail_that_fits(&with_command, usize::MAX, &counter),
            tail_that_fits(&messages, usize::MAX, &counter) + 1
        );
        let mut inputs = chat_inputs(None, true);
        inputs.context_limit = Some(64_000);
        let provider = SequenceProvider {
            answers: vec![good],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let ChatCompacted::Compacted {
            conversation, kept, ..
        } = compact_chat(
            &provider,
            &ModelConfig::new("sequence"),
            "s",
            &Conversation::new_unvalidated(with_command),
            true,
            &frame(),
            &inputs,
        )
        .await
        .unwrap()
        else {
            panic!("compacted");
        };
        let visible: Vec<String> = conversation
            .agent_visible_messages()
            .iter()
            .map(|m| m.as_concat_text())
            .collect();
        assert!(
            visible[2..].contains(&"app/sync.py written, 41 tests pass".to_string()),
            "the newest turn is kept word for word: {visible:?}"
        );
        assert!(
            visible.iter().all(|text| !text.contains("/compact")),
            "{visible:?}"
        );
        assert_eq!(kept.unwrap().tail_messages, 2);
        let command = conversation
            .messages()
            .iter()
            .find(|m| m.as_concat_text() == "/compact");
        assert!(command.is_some_and(|m| m.is_user_visible() && !m.is_agent_visible()));
    }

    /// Records each request's `max_tokens` and whether it carried tools; answers with a summary.
    struct MaxTokensProvider {
        calls: std::sync::Mutex<Vec<(usize, Option<i32>)>>,
    }

    #[async_trait]
    impl Provider for MaxTokensProvider {
        fn get_name(&self) -> &str {
            "max-tokens"
        }

        async fn stream(
            &self,
            model_config: &ModelConfig,
            _system: &str,
            _messages: &[Message],
            tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.calls
                .lock()
                .unwrap()
                .push((tools.len(), model_config.max_tokens));
            Ok(stream_from_single_message(
                Message::assistant().with_text(sections(BODIES)),
                ProviderUsage::new("max-tokens".to_string(), Usage::default()),
            ))
        }
    }

    /// Fix 5: a summary request carries `max_tokens` — the reserve the compaction point kept for
    /// the answer (32,768 when the model config sets none, else its own), cut to what the window
    /// leaves after the prompt — so the engine's default never decides the summary's room.
    #[tokio::test]
    async fn a_summary_request_carries_the_reserve_as_its_max_tokens() {
        let mut capped = ModelConfig::new("max-tokens");
        capped.max_tokens = Some(8_192);
        let cases: [(ModelConfig, Option<usize>, SummaryRequest); 4] = [
            (ModelConfig::new("max-tokens"), Some(200_000), frame()),
            (capped, Some(200_000), frame()),
            (ModelConfig::new("max-tokens"), Some(20_000), frame()),
            (
                ModelConfig::new("max-tokens"),
                None,
                SummaryRequest::Transcript,
            ),
        ];
        let mut seen = Vec::new();
        for (config, window, request) in cases {
            let provider = MaxTokensProvider {
                calls: std::sync::Mutex::new(Vec::new()),
            };
            let mut inputs = chat_inputs(None, false);
            inputs.context_limit = window;
            compact_chat(
                &provider,
                &config,
                "s",
                &two_turn_chat(),
                false,
                &request,
                &inputs,
            )
            .await
            .unwrap();
            let calls = provider.calls.lock().unwrap().clone();
            assert_eq!(calls.len(), 1, "{calls:?}");
            seen.push(calls[0].1);
        }
        assert_eq!(seen[0], Some(32_768), "{seen:?}");
        assert_eq!(seen[1], Some(8_192), "{seen:?}");
        let small = seen[2].expect("a cap on the small window") as usize;
        assert!(
            small < 20_000 && small >= summary_answer_room(20_000),
            "what the 20,000-token window leaves after the prompt: {seen:?}"
        );
        assert_eq!(seen[3], Some(32_768), "{seen:?}");
    }

    /// Round 4, C: the swarm's workers send their transcript request as the golden run did — no
    /// `max_tokens` (round 3 gave it the chat's reserve). Its removal ladder is the golden one
    /// again too (`summarize_a_transcript_as_the_swarm`; `test_progressive_removal_on_context_exceeded`).
    #[tokio::test]
    async fn a_worker_s_transcript_request_is_the_golden_one() {
        let provider = MaxTokensProvider {
            calls: std::sync::Mutex::new(Vec::new()),
        };
        compact_messages_with_tail(
            &provider,
            &ModelConfig::new("max-tokens"),
            "worker",
            &two_turn_chat(),
            false,
            0,
            &SummaryRequest::Transcript,
        )
        .await
        .unwrap();
        assert_eq!(provider.calls.lock().unwrap().clone(), vec![(0, None)]);
    }

    /// Round 4, C: a worker's kept tail is its golden one — every tail row copied (a row the model
    /// did not see included), and the person's request not re-sent when a user text row is in the
    /// tail. Round 3 had changed both for workers.
    #[tokio::test]
    async fn a_worker_s_kept_tail_is_the_golden_one() {
        let provider = MockProvider::new(Message::assistant().with_text("the summary"), 10_000);
        let messages = vec![
            Message::user().with_text("build it"),
            Message::assistant().with_text("planning"),
            Message::user()
                .with_text("a note the model did not see")
                .with_visibility(true, false),
            Message::assistant().with_text("built app/x.py"),
        ];
        let (compacted, _) = compact_messages_with_tail(
            &provider,
            &provider.config.clone(),
            "worker-tail",
            &Conversation::new_unvalidated(messages),
            false,
            2,
            &SummaryRequest::Transcript,
        )
        .await
        .unwrap();
        let visible: Vec<String> = compacted
            .agent_visible_messages()
            .iter()
            .map(|m| m.as_concat_text())
            .collect();
        assert_eq!(visible.len(), 4, "{visible:?}");
        assert_eq!(visible[2], "a note the model did not see", "{visible:?}");
        assert_eq!(visible[3], "built app/x.py", "{visible:?}");
        assert!(!visible.iter().any(|v| v == "build it"), "{visible:?}");
    }

    /// The kept tail's copies are dated now, chat and worker alike: the session store reads a
    /// conversation back by its rows' dates (then insertion), and a copy keeping its original's
    /// date was read back BEFORE the summary. No request carries a date.
    #[tokio::test]
    async fn the_kept_tail_reads_back_after_the_summary() {
        let dated = |m: Message| {
            let mut m = m;
            m.created = 1_000;
            m
        };
        let messages: Vec<Message> = vec![
            dated(Message::user().with_text("build it")),
            dated(Message::assistant().with_text("planning")),
            dated(Message::user().with_text("now the tests")),
            dated(Message::assistant().with_text("41 tests pass")),
        ];
        let provider = MockProvider::new(Message::assistant().with_text("the summary"), 10_000);
        let (compacted, _) = compact_messages_with_tail(
            &provider,
            &provider.config.clone(),
            "dated-tail",
            &Conversation::new_unvalidated(messages),
            false,
            2,
            &SummaryRequest::Transcript,
        )
        .await
        .unwrap();
        let in_memory: Vec<String> = compacted
            .agent_visible_messages()
            .iter()
            .map(|m| m.as_concat_text())
            .collect();
        // `ORDER BY created_timestamp, id`: a stable sort by date over the insertion order.
        let mut stored: Vec<&Message> = compacted.messages().iter().collect();
        stored.sort_by_key(|m| m.created);
        let read_back: Vec<String> = stored
            .into_iter()
            .filter(|m| m.is_agent_visible())
            .map(|m| m.as_concat_text())
            .collect();
        assert_eq!(read_back, in_memory);
        assert_eq!(in_memory[0], "the summary", "{in_memory:?}");
    }

    /// Round 4, B: a second compaction carries the first one's summary word for word, whether the
    /// new answer passes the gate or is stored flagged (the round-3 fallback dropped it).
    #[tokio::test]
    async fn a_second_compaction_carries_the_first_summary_and_the_facts() {
        let first_summary = sections([
            "the walk is done: 12,288 payments loaded",
            "app/sync.py: the documented walk",
            "Berlin days",
            "the webhooks",
            "write app/hooks.py",
        ]);
        let provider = SequenceProvider {
            answers: vec![first_summary.clone()],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let ChatCompacted::Compacted {
            conversation: once, ..
        } = compact_chat(
            &provider,
            &ModelConfig::new("sequence"),
            "s",
            &worked_chat("build the payments console"),
            false,
            &frame(),
            &chat_inputs(None, false),
        )
        .await
        .unwrap()
        else {
            panic!("compacted");
        };
        let mut later = once.messages().clone();
        later.push(Message::user().with_text("now the webhooks"));
        later.push(Message::assistant().with_text("app/hooks.py started"));
        for second in [EVERY_PART.to_string(), FLASH_NEXT_NON_SUMMARY.to_string()] {
            let provider = SequenceProvider {
                answers: vec![second.clone()],
                calls: std::sync::Mutex::new(Vec::new()),
            };
            let ChatCompacted::Compacted { conversation, .. } = compact_chat(
                &provider,
                &ModelConfig::new("sequence"),
                "s",
                &Conversation::new_unvalidated(later.clone()),
                false,
                &frame(),
                &chat_inputs(None, false),
            )
            .await
            .unwrap() else {
                panic!("compacted");
            };
            let stored = conversation.agent_visible_messages()[0].as_concat_text();
            let facts = stored.split_once(pillars::KEPT_OPEN).unwrap().1;
            assert!(
                facts.contains("## The summaries earlier compactions stored (1")
                    // Round 5: the fixture's 12,288 is in no row of the conversation.
                    && facts.contains("12,288 (unverified) payments loaded")
                    && facts.contains("write app/hooks.py"),
                "{stored}"
            );
            assert!(facts.contains("build the payments console"), "{facts}");
            let rows = conversation.agent_visible_messages();
            assert_eq!(rows.last().unwrap().as_concat_text(), "now the webhooks");
            assert!(facts.contains("def sync_all"), "{facts}");
            assert_eq!(
                facts.matches("12,288 (unverified) payments loaded").count(),
                1,
                "carried once, not nested: {facts}"
            );
        }
    }

    // ---- Round 5: flag instead of reject, facts first, numbers checked, still to do ----

    /// The FIX4-2 real-model summary (verifier GRADES4, Flash-Next, 2026-10-30), cut to what the
    /// tests read: its "Where we are" invents "context ~60% of 409,600"; its "Still to do" drops
    /// the framing check (`fitcheck.py` crashed and never ran clean) and lists the file only for
    /// cleanup.
    const FIX4_2_SUMMARY: &str = "## Where we are\nBuilding the Meridian Payments Console in the \
        workspace. Budget: 150 model calls; roughly 45 used, context ~60% of 409,600. The backend \
        is functional and smoke-tested against the real vendor on port 8131.\n\
        ## What each file does\n- `app/store.py` keeps the ledger, the outbox and the drafts.\n\
        - `web/viz.js` draws the towers with raw WebGL2 and picks on the GPU.\n\
        ## Decisions and reasons\n- The canvas is 480 px tall so the field spans the frame.\n\
        ## Still to do\n- Write `DECISIONS.md` with D1, D2 and D3 as implemented.\n\
        - Kill test processes and remove `tmpdb`, `selfcheck.mjs`, `fitcheck.py` before handoff.\n\
        ## Next step\nWrite DECISIONS.md, then run the final checks.\n";

    /// Rows from the Flash-Next stopped run (verifier pre.db, rows 0, 12, 205-213): the task with
    /// the turn context, the contract's framing rule, the framing check that crashed, and the
    /// inline probe that ran after it ("outside=3").
    fn d4_chat() -> Conversation {
        let shell = |id: &str, command: &str| {
            let mut params = CallToolRequestParams::new("shell");
            params.arguments = serde_json::json!({ "command": command })
                .as_object()
                .cloned();
            Message::assistant().with_tool_request(id, Ok(params))
        };
        let result = |text: &str, exit: i64| {
            let mut r = if exit == 0 {
                rmcp::model::CallToolResult::success(vec![RawContent::text(text).no_annotation()])
            } else {
                rmcp::model::CallToolResult::error(vec![RawContent::text(text).no_annotation()])
            };
            r.structured_content = Some(serde_json::json!({ "exit_code": exit }));
            r
        };
        let mut write = CallToolRequestParams::new("write");
        write.arguments = serde_json::json!({
            "path": "fitcheck.py",
            "content": "import json, math, urllib.request\nd = json.load(urllib.request.urlopen('http://127.0.0.1:8131/api/viz/records'))\n",
        })
        .as_object()
        .cloned();
        Conversation::new_unvalidated(vec![
            Message::user().with_text(
                "Build and finish the Meridian Payments Console in this workspace. \
                 VISUAL-CONTRACT.md: the overview spans >=60% of the canvas width and height, \
                 none within 4 CSS px of the canvas edge.\ncontext: 239,105 of 409,600 tokens \
                 used (58%)",
            ),
            Message::assistant().with_tool_request("w1", Ok(write)),
            Message::user()
                .with_tool_response("w1", Ok(result("Created fitcheck.py (48 lines)", 0))),
            shell("s1", "cd . && python3 fitcheck.py"),
            Message::user().with_tool_response(
                "s1",
                Ok(result(
                    "Traceback (most recent call last):\n  File \"fitcheck.py\", line 12, in \
                     daynum\nAttributeError: 'datetime.date' object has no attribute 'split'\n\n\
                     Command exited with code 1",
                    1,
                )),
            ),
            shell("s2", "cd . && python3 - <<'PY'\nprint('framing')\nPY"),
            Message::user().with_tool_response(
                "s2",
                Ok(result(
                    "H=480 aspect=1.93 W=926: x 191..784 (64% of W)  y 101..432 (69% of H) \
                     outside=3",
                    0,
                )),
            ),
            Message::user().with_text("go on"),
        ])
    }

    async fn compacted_with(answer: &str, chat: &Conversation) -> String {
        let provider = SequenceProvider {
            answers: vec![answer.to_string()],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let ChatCompacted::Compacted { conversation, .. } = compact_chat(
            &provider,
            &ModelConfig::new("sequence"),
            "s",
            chat,
            false,
            &frame(),
            &chat_inputs(None, false),
        )
        .await
        .unwrap() else {
            panic!("compacted");
        };
        conversation.agent_visible_messages()[0].as_concat_text()
    }

    /// D4 (GRADES4: every real summary dropped it; run 2 said the crashed `fitcheck.py` "proved"
    /// the framing): the summary below lists `fitcheck.py` only for cleanup. goose's "Still to
    /// do" carries the check as open work.
    #[tokio::test]
    async fn the_still_to_do_list_carries_the_framing_check_the_summary_dropped() {
        let stored = compacted_with(FIX4_2_SUMMARY, &d4_chat()).await;
        let facts = stored.split_once(pillars::KEPT_OPEN).unwrap().1;
        let todo = facts
            .split_once("## Still to do, by the tool calls")
            .unwrap_or_else(|| panic!("a Still to do list: {facts}"))
            .1;
        let todo = todo.split("\n## ").next().unwrap();
        assert!(
            todo.contains("make `shell {\"command\":\"cd . && python3 fitcheck.py\"}` succeed"),
            "{todo}"
        );
        assert!(todo.contains("failed (exit 1)"), "{todo}");
        assert!(
            !todo.contains("outside=3"),
            "a call that worked is not to do: {todo}"
        );
    }

    /// GRADES4 run 2: "context ~60% of 409,600". Both numbers are in the conversation, the share
    /// is not: it is marked. What the conversation said (409,600 alone, 150 is not) is checked
    /// number by number.
    #[tokio::test]
    async fn a_number_the_conversation_never_showed_is_marked_unverified() {
        let stored = compacted_with(FIX4_2_SUMMARY, &d4_chat()).await;
        let model = pillars::model_part(&stored);
        assert!(
            model.contains("context ~60% of 409,600 (unverified)."),
            "{model}"
        );
        assert!(model.contains("150 (unverified) model calls"), "{model}");
        assert!(model.contains("roughly 45 (unverified) used"), "{model}");
        assert!(
            model.contains("port 8131."),
            "8131 is in the conversation: {model}"
        );
        assert!(
            model.contains("480 px tall"),
            "480 is in the conversation: {model}"
        );
        assert!(
            !stored.starts_with(pillars::FLAG_LEAD),
            "a real summary: {stored}"
        );
    }

    /// A conversation big enough that a summary of it says something (the floor is 1% of its
    /// characters): the GRADES4 junk shapes pass the structural gate and are stored FLAGGED,
    /// goose's facts first.
    fn big_chat() -> Conversation {
        Conversation::new_unvalidated(vec![
            Message::user().with_text("build the payments console"),
            Message::assistant().with_text(
                "The vendor sync stored the payments and the ledger answered. ".repeat(800),
            ),
            Message::user().with_text("go on"),
        ])
    }

    #[tokio::test]
    async fn a_gate_passing_non_summary_is_stored_flagged_after_the_facts() {
        let bold = "**Thinking:** I should continue.\n\n**Plan:** look at files.\n\n**Step one:** \
                    run ls.\n\n**Step two:** read output.\n\n**Then:** carry on.\n";
        let notes = "## Where we are\nI will now read the files again.\n## What each file does\n\
                     Let me list them first.\n## Decisions and reasons\nNext I will decide.\n\
                     ## Still to do\nI will check the rest.\n## Next step\nNow I start.\n";
        for answer in [bold, notes] {
            assert!(
                summary_shortfall(answer).is_none(),
                "the gate passes it: {answer}"
            );
            let stored = compacted_with(answer, &big_chat()).await;
            assert!(stored.starts_with(pillars::FLAG_LEAD), "{stored}");
            assert!(stored.contains("passed the gate, but"), "{stored}");
            let facts_at = stored.find(pillars::KEPT_OPEN).expect("facts");
            let answer_at = stored.find(answer.lines().next().unwrap()).expect("answer");
            assert!(facts_at < answer_at, "facts first: {stored}");
            assert!(stored.contains(pillars::FLAGGED_ANSWER), "{stored}");
            // The model's part is still read back as the flag and the answer (earlier summaries).
            let model = pillars::model_part(&stored);
            assert!(model.starts_with(pillars::FLAG_LEAD), "{model}");
            assert!(model.contains(answer.lines().next().unwrap()), "{model}");
            assert!(!model.contains(pillars::KEPT_OPEN), "{model}");
        }
    }

    /// The counter-case on the same conversation: a summary that says what was done is stored as
    /// before — the model's text, then the facts.
    #[tokio::test]
    async fn a_real_summary_is_stored_first_and_unflagged() {
        let mut answer = String::new();
        for (heading, _) in WRITTEN_PARTS {
            answer.push_str(&format!(
                "## {heading}\n{}\n",
                "The vendor sync stored the payments and the ledger answered. ".repeat(4)
            ));
        }
        let stored = compacted_with(&answer, &big_chat()).await;
        assert!(stored.starts_with("## Where we are"), "{stored}");
        assert!(!stored.contains(pillars::FLAGGED_ANSWER), "{stored}");
    }

    /// The real case, on the real transcript (fixtures outside the repo; workhorse
    /// ~/scratch/compaction5: flash-next-223.json from verifier pre.db, real-fix4-{1,2,3}.md the
    /// stored model parts). Run: GOOSE_COMPACTION5_DIR=~/scratch/compaction5 cargo test -p goose
    /// real_flash_next_summaries -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_flash_next_summaries_flags_marks_and_still_to_do() {
        let dir = std::path::PathBuf::from(
            std::env::var("GOOSE_COMPACTION5_DIR").expect("GOOSE_COMPACTION5_DIR"),
        );
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("flash-next-223.json")).unwrap(),
        )
        .unwrap();
        let messages: Vec<Message> = serde_json::from_value(fixture["messages"].clone()).unwrap();
        let kept = Pillars::build(
            &messages,
            &KeptSources::default(),
            LedgerRead::Entries(vec![]),
        );
        let rendered = kept.render();
        let known = summary_check::known_text(&messages, &[&rendered]);
        let replaced = summary_check::replaced_chars(&messages);
        println!(
            "replaced chars {replaced}, floor {}",
            summary_check::thin_floor(replaced)
        );
        println!("still to do:\n{}", kept.todo.join("\n"));
        assert!(kept.todo.iter().any(|t| t.contains("python3 fitcheck.py")));
        for run in ["real-fix4-1", "real-fix4-2", "real-fix4-3"] {
            let text = std::fs::read_to_string(dir.join(format!("{run}.md"))).unwrap();
            assert_eq!(summary_check::thin_summary(&text, replaced), None, "{run}");
            let marked = summary_check::mark_unverified_numbers(&text, &known);
            let marks: Vec<&str> = marked
                .match_indices(summary_check::UNVERIFIED)
                .map(|(at, _)| {
                    let before = marked.get(..at).unwrap_or_default();
                    let from = before.char_indices().rev().nth(24).map_or(0, |(i, _)| i);
                    before.get(from..).unwrap_or_default()
                })
                .collect();
            println!("{run}: {} marks: {marks:#?}", marks.len());
            if run == "real-fix4-2" {
                assert!(marked.contains("~60% of 409,600 (unverified)"), "{marked}");
            }
        }
    }
}
