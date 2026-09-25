//! "Measure speed": a fixed, token-counted workload sent to a running engine — a ~2k-token prompt
//! then up to 256 greedy tokens (the shape of our recorded runs, `mlx_lm.benchmark -p 2048 -g 256`),
//! and optionally a ~32k-token document for the long-documents goal. Nothing here is timed out or
//! capped by a clock: the workload ends when the engine finishes the tokens it was asked for.
//!
//! The prompt is the eval bench's document generator (evals/mlx-engine-bench/bench.py `document`,
//! the same vocabulary) opened by a NONCE, so the prefix cache cannot serve a previous run's prompt
//! and the time to the first token is a cold prefill.

use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// bench.py's WORDS: the vocabulary its fixed documents are made of.
const WORDS: &[&str] = &[
    "system",
    "memory",
    "engine",
    "model",
    "token",
    "cache",
    "request",
    "worker",
    "queue",
    "latency",
    "throughput",
    "prefill",
    "decode",
    "kernel",
    "buffer",
    "tensor",
    "layer",
    "attention",
    "state",
    "checkpoint",
    "budget",
    "window",
    "the",
    "a",
    "of",
    "to",
    "and",
    "in",
    "for",
    "on",
    "with",
    "by",
    "from",
    "at",
    "as",
    "into",
    "over",
    "under",
    "between",
    "across",
    "through",
    "reads",
    "writes",
    "measures",
    "holds",
    "returns",
    "stores",
    "computes",
    "loads",
    "keeps",
    "moves",
    "drops",
    "shares",
    "quickly",
    "slowly",
    "carefully",
    "fully",
    "partly",
    "rarely",
    "often",
    "always",
    "never",
    "exactly",
    "roughly",
    "server",
    "client",
    "network",
    "disk",
    "file",
    "record",
    "entry",
    "index",
    "table",
    "column",
    "value",
    "field",
    "schema",
    "first",
    "second",
    "third",
    "final",
    "early",
    "late",
    "fresh",
    "stale",
    "warm",
    "cold",
    "large",
    "small",
    "long",
    "short",
    "user",
    "operator",
    "developer",
    "reviewer",
    "tester",
    "maintainer",
    "owner",
    "service",
    "scheduler",
    "supervisor",
];

// measured: bench.py's document of 24,000 of these words tokenized to 26,838 Qwen tokens
// (1.118 tokens per word); prompts are sized in words by this ratio and the engine's own
// `usage.prompt_tokens` is what gets recorded.
const TOKENS_PER_WORD: f64 = 26_838.0 / 24_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Workload {
    /// ~2k prompt tokens, 256 generated: the chat goal's decode and a short prefill.
    Chat,
    /// ~30k prompt tokens, a short answer: the long-documents goal's prefill.
    LongDocument,
}

impl Workload {
    /// Prompt tokens aimed for (the document is sized to land just under the bucket).
    pub fn prompt_tokens(self) -> u64 {
        match self {
            // ratio: the recorded runs' 2,048-token prompt, less room for the instruction.
            Workload::Chat => 1_900,
            // ratio: bench.py's (b) long document class (~32k), less room for the instruction.
            Workload::LongDocument => 30_000,
        }
    }

    pub fn max_tokens(self) -> u32 {
        match self {
            // ratio: the recorded runs' `-g 256`.
            Workload::Chat => 256,
            // ratio: bench.py (b)'s answer is 200 tokens; prefill is the figure this pass is for.
            Workload::LongDocument => 64,
        }
    }

    /// The answer a typical turn of this shape writes — what a whole turn is timed with when goose
    /// has no recorded turns of the model at this size. (The measurement pass stops at
    /// `max_tokens`: it is after the prompt's rate, not the answer.)
    pub fn answer_tokens(self) -> u64 {
        match self {
            // ratio: the recorded runs' `-g 256`.
            Workload::Chat => 256,
            // ratio: bench.py (b)'s long-document answer is 200 tokens.
            Workload::LongDocument => 200,
        }
    }

    /// The context bucket the workload's prompt falls in.
    pub fn bucket(self) -> u64 {
        super::store::context_bucket(self.prompt_tokens())
    }

    /// Context a placement needs to run this workload at all: its prompt plus its answer.
    pub fn context_needed(self) -> u64 {
        self.bucket() + self.max_tokens() as u64
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Workload::Chat => "chat",
            Workload::LongDocument => "longDocument",
        }
    }
}

/// xorshift64*: a fixed document from a seed without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// bench.py's `document`: sentences of 8–18 words, a paragraph break now and then.
fn document(words: usize, seed: u64) -> String {
    let mut rng = Rng(seed.max(1));
    let mut out = String::new();
    let mut count = 0;
    while count < words {
        let k = 8 + rng.below(11) as usize;
        let mut sentence: Vec<String> = (0..k)
            .map(|_| WORDS[rng.below(WORDS.len() as u64) as usize].to_string())
            .collect();
        if let Some(first) = sentence.first_mut() {
            let mut chars = first.chars();
            if let Some(c) = chars.next() {
                *first = c.to_uppercase().collect::<String>() + chars.as_str();
            }
        }
        out.push_str(&sentence.join(" "));
        out.push_str(". ");
        count += k;
        if rng.below(100) < 12 {
            out.push_str("\n\n");
        }
    }
    out
}

/// The workload's prompt, opened by `nonce`.
pub fn prompt(workload: Workload, nonce: &str) -> String {
    let words = (workload.prompt_tokens() as f64 / TOKENS_PER_WORD) as usize;
    let instruction = match workload {
        Workload::Chat => {
            "Continue the operations log below in the same style, one sentence after another, \
             without lists or headings. Keep writing until you are stopped."
        }
        Workload::LongDocument => {
            "Read the operations log below, then name in one short paragraph the three components \
             it mentions most often."
        }
    };
    format!(
        "Measurement {nonce}.\n{instruction}\n\n{}",
        document(words, 0x9E37_79B9 ^ workload.prompt_tokens())
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchSample {
    pub workload: Workload,
    pub prompt_tokens: u64,
    /// Prompt tokens the engine said its cache served (`usage.prompt_tokens_details.cached_tokens`).
    pub cached_tokens: u64,
    pub completion_tokens: u64,
    pub ttft_ms: f64,
    /// (prompt − cached) ÷ time to first token — conservative: the first token's own step is in it.
    pub prefill_tps: f64,
    /// (completion − 1) ÷ the span between the first and the last output delta; `None` when the
    /// engine produced one token or one delta (no span to divide).
    pub decode_tps: Option<f64>,
    pub finish_reason: Option<String>,
}

fn is_output_delta(choice: &serde_json::Value) -> bool {
    let delta = &choice["delta"];
    ["content", "reasoning_content", "reasoning"]
        .iter()
        .any(|k| delta[k].as_str().is_some_and(|s| !s.is_empty()))
        || delta["tool_calls"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
}

/// Stream one workload request through `base_url`'s `/v1/chat/completions` and time it.
pub async fn run_workload(
    base_url: &str,
    served_model: &str,
    workload: Workload,
    nonce: &str,
) -> Result<BenchSample> {
    let body = serde_json::json!({
        "model": served_model,
        "messages": [{"role": "user", "content": prompt(workload, nonce)}],
        "max_tokens": workload.max_tokens(),
        "temperature": 0,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let started = Instant::now();
    let mut response = reqwest::Client::new()
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&body)?)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        bail!("{url} answered {status}: {}", text.trim());
    }
    let mut buffer = String::new();
    let (mut first, mut last): (Option<Instant>, Option<Instant>) = (None, None);
    let mut usage: Option<serde_json::Value> = None;
    let mut finish_reason = None;
    let mut done = false;
    while let Some(chunk) = response.chunk().await.context("reading the stream")? {
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = buffer.find('\n') {
            let line: String = buffer.drain(..=end).collect();
            let Some(payload) = line.trim().strip_prefix("data:").map(str::trim) else {
                continue;
            };
            if payload == "[DONE]" {
                done = true;
                continue;
            }
            let event: serde_json::Value = serde_json::from_str(payload)
                .with_context(|| format!("an SSE event that is not JSON: {payload}"))?;
            if let Some(error) = event.get("error") {
                bail!("the engine streamed an error: {error}");
            }
            if event["usage"].is_object() {
                usage = Some(event["usage"].clone());
            }
            for choice in event["choices"].as_array().into_iter().flatten() {
                if is_output_delta(choice) {
                    let now = Instant::now();
                    first.get_or_insert(now);
                    last = Some(now);
                }
                if let Some(reason) = choice["finish_reason"].as_str() {
                    finish_reason = Some(reason.to_string());
                }
            }
        }
    }
    if !done {
        bail!("the stream ended without [DONE] (finish reason {finish_reason:?})");
    }
    let usage = usage.context("the engine sent no usage chunk")?;
    let first = first.context("the engine produced no output token")?;
    let tokens = |key: &str| {
        usage[key]
            .as_u64()
            .with_context(|| format!("usage has no `{key}`: {usage}"))
    };
    let prompt_tokens = tokens("prompt_tokens")?;
    let completion_tokens = tokens("completion_tokens")?;
    // An engine that does not report cache hits served none of THIS prompt: its nonce sits at
    // token 0, so no cached prefix can match past the chat template's header.
    let cached_tokens = usage["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .unwrap_or(0);
    let ttft_ms = first.duration_since(started).as_secs_f64() * 1000.0;
    let span_s = last.map(|l| l.duration_since(first).as_secs_f64());
    Ok(BenchSample {
        workload,
        prompt_tokens,
        cached_tokens,
        completion_tokens,
        ttft_ms,
        prefill_tps: prompt_tokens.saturating_sub(cached_tokens) as f64 / (ttft_ms / 1000.0),
        decode_tps: span_s
            .filter(|s| *s > 0.0 && completion_tokens > 1)
            .map(|s| (completion_tokens - 1) as f64 / s),
        finish_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn the_prompt_is_fixed_but_its_nonce_is_not() {
        let a = prompt(Workload::Chat, "n1");
        assert_eq!(
            a,
            prompt(Workload::Chat, "n1"),
            "the document is deterministic"
        );
        assert_ne!(a, prompt(Workload::Chat, "n2"));
        assert!(a.starts_with("Measurement n1."));
        let words = a.split_whitespace().count() as f64;
        let tokens = words * TOKENS_PER_WORD;
        assert!(tokens < 2048.0 && tokens > 1700.0, "{tokens}");
        assert_eq!(Workload::Chat.bucket(), 2048);
        assert_eq!(Workload::LongDocument.bucket(), 32_768);
        assert_eq!(Workload::Chat.context_needed(), 2048 + 256);
    }

    /// A one-shot SSE server: what the engine streams, byte for byte.
    async fn serve_once(body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 65_536];
            let _ = socket.read(&mut request).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n",
                body.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(body.as_bytes()).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn a_stream_is_timed_from_its_own_usage_and_deltas() {
        let base = serve_once(concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"b\"},\"finish_reason\":\"length\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1900,\"completion_tokens\":2,\"prompt_tokens_details\":{\"cached_tokens\":4}}}\n\n",
            "data: [DONE]\n\n"
        ))
        .await;
        let sample = run_workload(&base, "m", Workload::Chat, "x").await.unwrap();
        assert_eq!(sample.prompt_tokens, 1900);
        assert_eq!(sample.cached_tokens, 4);
        assert_eq!(sample.completion_tokens, 2);
        assert_eq!(sample.finish_reason.as_deref(), Some("length"));
        assert!(sample.prefill_tps > 0.0);
    }

    #[tokio::test]
    async fn a_stream_without_usage_or_done_is_a_named_failure() {
        let base =
            serve_once("data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: [DONE]\n\n")
                .await;
        let err = format!(
            "{:#}",
            run_workload(&base, "m", Workload::Chat, "x")
                .await
                .unwrap_err()
        );
        assert!(err.contains("no usage chunk"), "{err}");
        let base = serve_once("data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n").await;
        let err = format!(
            "{:#}",
            run_workload(&base, "m", Workload::Chat, "x")
                .await
                .unwrap_err()
        );
        assert!(err.contains("without [DONE]"), "{err}");
    }
}
