use lru::LruCache;
use rmcp::model::Tool;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use tiktoken_rs::CoreBPE;
use tokio::sync::OnceCell;

use crate::conversation::message::Message;

static TOKENIZER: OnceCell<Arc<CoreBPE>> = OnceCell::const_new();

const MAX_TOKEN_CACHE_SIZE: usize = 1_024;

// token use for various bits of a tool calls:
const FUNC_INIT: usize = 7;
const PROP_INIT: usize = 3;
const PROP_KEY: usize = 3;
const ENUM_INIT: isize = -3;
const ENUM_ITEM: usize = 3;
const FUNC_END: usize = 12;
const TOKENS_PER_MESSAGE: usize = 4;
const REPLY_PRIMER_TOKENS: usize = 3;

/// One message's tokens as part of a conversation ([`TokenCounter::message_costs`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MessageCost {
    /// What the message adds to the request after the messages before it.
    pub tokens: usize,
    /// Reasoning it repeats from the row before it, which the request sends once: counted only
    /// when a run of rows opens on this message.
    pub repeated_reasoning: usize,
}

/// The tokens of the rows `costs[start..]` sent as their own run of messages: their costs, and
/// the reasoning the first of them repeats from a row the run leaves out.
pub fn run_tokens(costs: &[MessageCost], start: usize) -> usize {
    costs[start..].iter().map(|c| c.tokens).sum::<usize>()
        + costs.get(start).map_or(0, |c| c.repeated_reasoning)
}

// ratio: pixels per image token — Anthropic's published cost of an image is width x height / 750;
// Qwen-VL's 28x28-pixel merged patches cost width x height / 784, within 5% of it. Before
// 2026-10-30 an image counted nothing here and ~18 chars ("[image: image/png]") in a compaction's
// kept tail, while the real image was sent.
const PIXELS_PER_IMAGE_TOKEN: u64 = 750;
/// An image the decoder cannot size: the cost of the largest image Anthropic sends unresized
/// (~1.15 megapixels).
pub const UNSIZED_IMAGE_TOKENS: usize = 1_600;

/// The tokens an image costs a model: its pixels read from the image's own header (base64 `data`),
/// one token per [`PIXELS_PER_IMAGE_TOKEN`]; [`UNSIZED_IMAGE_TOKENS`] when it cannot be read.
pub fn image_tokens(base64_data: &str) -> usize {
    use base64::Engine;
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(base64_data.trim()) else {
        return UNSIZED_IMAGE_TOKENS;
    };
    let Ok(reader) = image::io::Reader::new(std::io::Cursor::new(bytes)).with_guessed_format()
    else {
        return UNSIZED_IMAGE_TOKENS;
    };
    match reader.into_dimensions() {
        Ok((width, height)) => (u64::from(width) * u64::from(height))
            .div_ceil(PIXELS_PER_IMAGE_TOKEN)
            .max(1) as usize,
        Err(_) => UNSIZED_IMAGE_TOKENS,
    }
}

pub struct TokenCounter {
    tokenizer: Arc<CoreBPE>,
    token_cache: Mutex<LruCache<TokenCacheKey, usize>>,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct TokenCacheKey {
    len: usize,
    hash: [u8; 32],
}

impl TokenCacheKey {
    fn from_text(text: &str) -> Self {
        Self {
            len: text.len(),
            hash: *blake3::hash(text.as_bytes()).as_bytes(),
        }
    }
}

impl TokenCounter {
    pub async fn new() -> Result<Self, String> {
        let tokenizer = get_tokenizer().await?;
        let cache_capacity =
            NonZeroUsize::new(MAX_TOKEN_CACHE_SIZE).expect("token cache capacity must be non-zero");
        Ok(Self {
            tokenizer,
            token_cache: Mutex::new(LruCache::new(cache_capacity)),
        })
    }

    pub fn count_tokens(&self, text: &str) -> usize {
        let cache_key = TokenCacheKey::from_text(text);
        if let Some(count) = self
            .token_cache
            .lock()
            .expect("token cache mutex poisoned")
            .get(&cache_key)
            .copied()
        {
            return count;
        }

        let tokens = self.tokenizer.encode_with_special_tokens(text);
        let count = tokens.len();

        self.token_cache
            .lock()
            .expect("token cache mutex poisoned")
            .put(cache_key, count);
        count
    }

    pub fn count_tokens_for_tools(&self, tools: &[Tool]) -> usize {
        let mut func_token_count = 0;
        if !tools.is_empty() {
            for tool in tools {
                func_token_count += FUNC_INIT;
                let name = &tool.name;
                let description = &tool
                    .description
                    .as_deref()
                    .unwrap_or_default()
                    .trim_end_matches('.');

                let line = format!("{}:{}", name, description);
                func_token_count += self.count_tokens(&line);

                if let Some(serde_json::Value::Object(properties)) =
                    tool.input_schema.get("properties")
                {
                    if !properties.is_empty() {
                        func_token_count += PROP_INIT;
                        for (key, value) in properties {
                            func_token_count += PROP_KEY;
                            let p_name = key;
                            let p_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            let p_desc = value
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .trim_end_matches('.');

                            let line = format!("{}:{}:{}", p_name, p_type, p_desc);
                            func_token_count += self.count_tokens(&line);

                            if let Some(enum_values) = value.get("enum").and_then(|v| v.as_array())
                            {
                                func_token_count =
                                    func_token_count.saturating_add_signed(ENUM_INIT);
                                for item in enum_values {
                                    if let Some(item_str) = item.as_str() {
                                        func_token_count += ENUM_ITEM;
                                        func_token_count += self.count_tokens(item_str);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            func_token_count += FUNC_END;
        }

        func_token_count
    }

    pub fn count_chat_tokens(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> usize {
        let mut num_tokens = 0;

        if !system_prompt.is_empty() {
            num_tokens += self.count_tokens(system_prompt) + TOKENS_PER_MESSAGE;
        }

        num_tokens += self
            .message_costs(messages)
            .iter()
            .map(|cost| cost.tokens)
            .sum::<usize>();

        if !tools.is_empty() {
            num_tokens += self.count_tokens_for_tools(tools);
        }

        num_tokens += REPLY_PRIMER_TOKENS;

        num_tokens
    }

    /// Each message's tokens as part of `messages` — the ONE per-row count every caller uses, so a
    /// sum over any of its rows agrees with [`Self::count_chat_tokens`] of the whole. A message
    /// the agent cannot see costs 0.
    ///
    /// The reasoning of the newest assistant tool-call row while only its tool results follow:
    /// the agent stores one answer with N tool calls as N rows, each carrying the answer's
    /// reasoning, and the request formatter sends it once (`merge_split_tool_call_messages`).
    /// Counted per row, the stopped Flash-Next run's 223 rows were 278,867 tokens here and
    /// 239,237 at the engine (its own template and tokenizer): 16 rows repeated 177,693 chars
    /// of reasoning. Before 2026-10-30 (round 4) the compaction guard and the kept tail counted
    /// each message ALONE, so this never fired there: the guard read the 223 rows as 277-281k
    /// and compacted ~14% early.
    ///
    /// A row whose reasoning repeats the row before it costs that reasoning in
    /// [`MessageCost::repeated_reasoning`], not in `tokens`: a run of rows that OPENS on it (a
    /// kept tail cut between the split rows) is sent with it, and adds it back.
    pub fn message_costs(&self, messages: &[Message]) -> Vec<MessageCost> {
        let mut split_reasoning: Option<String> = None;
        let mut costs = Vec::with_capacity(messages.len());
        for message in messages {
            if !message.metadata.agent_visible {
                costs.push(MessageCost::default());
                continue;
            }
            let reasoning: String = message
                .content
                .iter()
                .filter_map(|c| match c {
                    crate::conversation::message::MessageContent::Thinking(t) => {
                        Some(t.thinking.as_str())
                    }
                    _ => None,
                })
                .collect();
            let calls_a_tool = message
                .content
                .iter()
                .any(|c| c.as_tool_request().is_some());
            let says_something = message
                .content
                .iter()
                .any(|c| c.as_text().is_some_and(|t| !t.trim().is_empty()));
            let only_tool_results = !message.content.is_empty()
                && message
                    .content
                    .iter()
                    .all(|c| c.as_tool_response().is_some());
            let repeats_reasoning = message.role == rmcp::model::Role::Assistant
                && calls_a_tool
                && !says_something
                && !reasoning.trim().is_empty()
                && split_reasoning.as_deref() == Some(reasoning.as_str());
            if message.role == rmcp::model::Role::Assistant && calls_a_tool {
                split_reasoning = (!reasoning.trim().is_empty()).then_some(reasoning);
            } else if !only_tool_results {
                split_reasoning = None;
            }
            let mut num_tokens = TOKENS_PER_MESSAGE;
            let mut repeated_reasoning = 0;
            for content in &message.content {
                if let Some(content_text) = content.as_text() {
                    num_tokens += self.count_tokens(content_text);
                } else if let Some(tool_request) = content.as_tool_request() {
                    if let Ok(tool_call) = tool_request.tool_call.as_ref() {
                        let text = format!(
                            "{}:{}:{:?}",
                            tool_request.id, tool_call.name, tool_call.arguments
                        );
                        num_tokens += self.count_tokens(&text);
                    }
                } else if let Some(tool_response) = content.as_tool_response() {
                    if let Some(tool_response_text) = content.as_tool_response_text() {
                        num_tokens += self.count_tokens(&tool_response_text);
                    }
                    if let Ok(result) = &tool_response.tool_result {
                        num_tokens += crate::conversation::message::model_visible_content(result)
                            .filter_map(|item| match &item.raw {
                                rmcp::model::RawContent::Image(image) => {
                                    Some(image_tokens(&image.data))
                                }
                                _ => None,
                            })
                            .sum::<usize>();
                    }
                } else if let crate::conversation::message::MessageContent::Image(image) = content {
                    num_tokens += image_tokens(&image.data);
                } else if let crate::conversation::message::MessageContent::Thinking(thinking) =
                    content
                {
                    // An OpenAI-compatible request carries an assistant turn's reasoning back as
                    // `reasoning_content` (Qwen's templates keep it): the Flash-Next stopped run's
                    // 224 messages were 239,628 prompt tokens at the engine and 112,754 here
                    // while reasoning counted nothing (692k chars of it).
                    if repeats_reasoning {
                        repeated_reasoning += self.count_tokens(&thinking.thinking);
                    } else {
                        num_tokens += self.count_tokens(&thinking.thinking);
                    }
                }
            }
            costs.push(MessageCost {
                tokens: num_tokens,
                repeated_reasoning,
            });
        }
        costs
    }

    pub fn count_everything(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tools: &[Tool],
        resources: &[String],
    ) -> usize {
        let mut num_tokens = self.count_chat_tokens(system_prompt, messages, tools);

        if !resources.is_empty() {
            for resource in resources {
                num_tokens += self.count_tokens(resource);
            }
        }
        num_tokens
    }

    pub fn clear_cache(&self) {
        self.token_cache
            .lock()
            .expect("token cache mutex poisoned")
            .clear();
    }

    pub fn cache_size(&self) -> usize {
        self.token_cache
            .lock()
            .expect("token cache mutex poisoned")
            .len()
    }
}

/// Q-517: building o200k_base is CPU work (~50 ms release, ~400 ms debug on an M-series Mac), paid
/// once per process by whoever counts first — measured as the first turn's compaction check. It runs on
/// the blocking pool so it never stalls an async worker, and [`warm_tokenizer`] starts it with the
/// server so a first chat finds it built.
async fn get_tokenizer() -> Result<Arc<CoreBPE>, String> {
    TOKENIZER
        .get_or_try_init(|| async {
            tokio::task::spawn_blocking(|| {
                Arc::new(
                    tiktoken_rs::o200k_base().expect("Failed to initialize o200k_base tokenizer"),
                )
            })
            .await
            .map_err(|e| format!("the o200k_base tokenizer build did not finish: {e}"))
        })
        .await
        .cloned()
}

/// Builds the tokenizer in the background; a count that arrives first waits for this same build.
pub fn warm_tokenizer() {
    tokio::spawn(async {
        if let Err(error) = get_tokenizer().await {
            tracing::warn!(%error, "token counter: the tokenizer was not built ahead of the first count");
        }
    });
}

pub async fn create_token_counter() -> Result<TokenCounter, String> {
    TokenCounter::new().await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One answer with three parallel tool calls, as the agent stores it: three assistant rows,
    /// each carrying the answer's reasoning, each followed by its result. Sizes as the stopped
    /// Flash-Next run's (2026-10-29): reasoning in the tens of kilobytes, results ~1.5k chars.
    fn split_turns(turns: usize, repeat_reasoning: bool) -> Vec<Message> {
        use rmcp::model::{AnnotateAble, CallToolRequestParams, CallToolResult, RawContent};
        let mut messages = vec![Message::user().with_text("build the console")];
        for turn in 0..turns {
            let reasoning: String = (0..400)
                .map(|i| format!("Turn {turn} step {i}: check app/store.py against the contract. "))
                .collect();
            for call in 0..3 {
                let id = format!("t{turn}c{call}");
                let mut params = CallToolRequestParams::new("developer__shell");
                params.arguments =
                    serde_json::json!({"command": format!("sed -n {call},40p app/store.py")})
                        .as_object()
                        .cloned();
                let mut row = Message::assistant();
                if call == 0 || repeat_reasoning {
                    row = row.with_thinking(reasoning.clone(), "");
                }
                messages.push(row.with_tool_request(id.clone(), Ok(params)));
                messages.push(Message::user().with_tool_response(
                    id,
                    Ok(CallToolResult::success(vec![RawContent::text(
                        format!("{call}: def now_iso(): return datetime.now(UTC)\n").repeat(30),
                    )
                    .no_annotation()])),
                ));
            }
        }
        messages
    }

    /// The reasoning an answer's split rows repeat is sent once (the request formatter merges the
    /// rows back), so it is counted once. Measured 2026-10-30: the stopped Flash-Next run's 223
    /// rows counted 278,867 here and rendered 239,237 with the engine's own template and tokenizer.
    #[tokio::test]
    async fn reasoning_repeated_on_split_tool_call_rows_is_counted_once() {
        let counter = create_token_counter().await.unwrap();
        let split = split_turns(6, true);
        let once = split_turns(6, false);
        assert_eq!(
            counter.count_chat_tokens("", &split, &[]),
            counter.count_chat_tokens("", &once, &[])
        );
        // The formatter sends it once per answer: what the count now follows.
        let sent = goose_providers::formats::openai::format_messages(
            &split,
            &goose_providers::images::ImageFormat::OpenAi,
        );
        let reasoned = sent
            .iter()
            .filter(|m| m.get("reasoning_content").is_some())
            .count();
        assert_eq!(reasoned, 6, "one reasoning per answer on the wire");
        // An answer with words of its own is a new turn: the same reasoning is counted again.
        let mut sequential = once.clone();
        let reasoning = "the same short plan for both turns, repeated".repeat(20);
        sequential.push(
            Message::assistant()
                .with_thinking(reasoning.clone(), "")
                .with_text("done"),
        );
        let before = counter.count_chat_tokens("", &sequential, &[]);
        sequential.push(Message::user().with_text("again"));
        sequential.push(
            Message::assistant()
                .with_thinking(reasoning.clone(), "")
                .with_text("done"),
        );
        let after = counter.count_chat_tokens("", &sequential, &[]);
        assert!(
            after - before > counter.count_tokens(&reasoning),
            "{before} -> {after}"
        );
    }

    #[tokio::test]
    async fn test_token_caching() {
        let counter = create_token_counter().await.unwrap();

        let text = "This is a test for caching functionality";

        let count1 = counter.count_tokens(text);
        assert_eq!(counter.cache_size(), 1);

        let count2 = counter.count_tokens(text);
        assert_eq!(count1, count2);
        assert_eq!(counter.cache_size(), 1);

        let count3 = counter.count_tokens("Different text");
        assert_eq!(counter.cache_size(), 2);
        assert_ne!(count1, count3);
    }

    #[tokio::test]
    async fn test_cache_management() {
        let counter = create_token_counter().await.unwrap();

        counter.count_tokens("First text");
        counter.count_tokens("Second text");
        counter.count_tokens("Third text");

        assert_eq!(counter.cache_size(), 3);

        counter.clear_cache();
        assert_eq!(counter.cache_size(), 0);

        let count = counter.count_tokens("First text");
        assert!(count > 0);
        assert_eq!(counter.cache_size(), 1);
    }

    #[tokio::test]
    async fn a_count_during_the_warm_up_waits_for_that_build() {
        warm_tokenizer();
        let (a, b) = tokio::join!(create_token_counter(), create_token_counter());
        let (a, b) = (a.unwrap(), b.unwrap());
        assert!(Arc::ptr_eq(&a.tokenizer, &b.tokenizer));
        assert!(Arc::ptr_eq(&a.tokenizer, &get_tokenizer().await.unwrap()));
        assert_eq!(a.count_tokens("hello world"), 2);
    }

    #[tokio::test]
    async fn test_concurrent_token_counter_creation() {
        let handles: Vec<_> = (0..10)
            .map(|_| tokio::spawn(async { create_token_counter().await.unwrap() }))
            .collect();

        let counters: Vec<_> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        let text = "Test concurrent creation";
        let expected_count = counters[0].count_tokens(text);

        for counter in &counters {
            assert_eq!(counter.count_tokens(text), expected_count);
        }
    }

    #[tokio::test]
    async fn test_cache_eviction_behavior() {
        let counter = create_token_counter().await.unwrap();

        let mut cached_texts = Vec::new();
        for i in 0..=MAX_TOKEN_CACHE_SIZE {
            let text = format!("Test string number {}", i);
            counter.count_tokens(&text);
            cached_texts.push(text);
        }

        assert_eq!(counter.cache_size(), MAX_TOKEN_CACHE_SIZE);

        let recent_text = &cached_texts[cached_texts.len() - 1];
        let start_size = counter.cache_size();

        counter.count_tokens(recent_text);
        assert_eq!(counter.cache_size(), start_size);
    }

    #[tokio::test]
    async fn test_concurrent_cache_operations() {
        let counter = std::sync::Arc::new(create_token_counter().await.unwrap());

        let handles: Vec<_> = (0..20)
            .map(|i| {
                let counter_clone = counter.clone();
                tokio::spawn(async move {
                    let text = format!("Concurrent test {}", i % 5);
                    counter_clone.count_tokens(&text)
                })
            })
            .collect();

        let results: Vec<_> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        for result in results {
            assert!(result > 0);
        }

        assert!(counter.cache_size() > 0);
        assert!(counter.cache_size() <= MAX_TOKEN_CACHE_SIZE);
    }
}
