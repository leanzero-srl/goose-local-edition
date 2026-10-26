//! A conversation that outgrows the window OpenRouter serves must end in goose's own compaction,
//! never in OpenRouter's silent middle-out cut (Forge 2.0, 2026-10: Grok 4.7's low build was cut
//! from call #182 on and spent its last ~120 calls without its own history).
//!
//! The real provider runs against a wiremock OpenRouter: without the owner's opt-in the request
//! carries no `transforms` and switches context compression off, the over-long request is refused
//! with OpenRouter's own body, and the agent compacts and retries. With the opt-in, a call whose
//! billed prompt is under half of what goose sent makes the next check compact.

use anyhow::Result;
use futures::StreamExt;
use goose::agents::{Agent, AgentEvent, SessionConfig};
use goose::config::GooseMode;
use goose::conversation::message::{Message, MessageContent};
use goose::conversation::Conversation;
use goose::providers::base::Provider;
use goose::providers::openrouter::OpenRouterProvider;
use goose::session::session_manager::SessionType;
use goose_providers::model::ModelConfig;
use serde_json::{json, Value};
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

const MODEL: &str = "x-ai/grok-4.7";
/// Rides an ASSISTANT message: a chat's compaction keeps the person's own words verbatim (Q-357),
/// so a marker the person wrote would survive it.
const OVER_THE_WINDOW: &str = "OVER-THE-ENDPOINT-WINDOW";

/// OpenRouter's pre-stream refusal of an over-long request, byte for byte as a client received it
/// (hotovo/aider-desk#361): HTTP 400, the endpoint's window, and the output share inside it.
const OPENROUTER_REFUSAL: &str = r#"{"error":{"message":"This endpoint's maximum context length is 131072 tokens. However, you requested about 272464 tokens (258978 of text input, 3486 of tool input, 10000 in the output). Please reduce the length of either one, or use the \"middle-out\" transform to compress your prompt automatically.","code":400,"metadata":{"provider_name":null}}}"#;

fn sse(text: &str, prompt_tokens: i64) -> ResponseTemplate {
    let chunk = |delta: Value, finish: Value, usage: Option<Value>| {
        let mut c = json!({"id": "gen-1", "object": "chat.completion.chunk", "created": 1,
                           "model": MODEL,
                           "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]});
        if let Some(usage) = usage {
            c["usage"] = usage;
        }
        format!("data: {c}\n\n")
    };
    let body = [
        chunk(
            json!({"role": "assistant", "content": text}),
            Value::Null,
            None,
        ),
        chunk(
            json!({}),
            json!("stop"),
            Some(
                json!({"prompt_tokens": prompt_tokens, "completion_tokens": 12,
                        "total_tokens": prompt_tokens + 12}),
            ),
        ),
        "data: [DONE]\n\n".to_string(),
    ]
    .concat();
    ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
}

fn is_compaction(body: &str) -> bool {
    body.contains("Summarize this conversation for yourself")
        || body.contains("Please summarize the conversation history")
}

/// OpenRouter's refusal once the provider accepted the request: HTTP 200, then one chunk with a
/// top-level `error` (its documented MidStreamError shape, `error_type` from its Typed error codes,
/// openrouter.ai/docs/api/reference/errors-and-debugging).
fn mid_stream_refusal() -> ResponseTemplate {
    let chunk = json!({
        "id": "gen-1", "object": "chat.completion.chunk", "created": 1, "model": MODEL,
        "provider": "xAI",
        "error": {"code": 400, "message": "Provider returned error",
                  "metadata": {"error_type": "context_length_exceeded"}},
        "choices": [{"index": 0, "delta": {"content": ""}, "finish_reason": "error"}]
    });
    ResponseTemplate::new(200).set_body_raw(format!("data: {chunk}\n\n"), "text/event-stream")
}

/// The window refusal for the over-long chat, a summary for compaction, an answer otherwise.
struct RefusingOpenRouter {
    mid_stream: bool,
}

impl Respond for RefusingOpenRouter {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body = String::from_utf8_lossy(&request.body);
        if is_compaction(&body) {
            return sse("SUMMARY: the chat is fixing the field write.", 900);
        }
        if body.contains(OVER_THE_WINDOW) {
            return match self.mid_stream {
                true => mid_stream_refusal(),
                false => ResponseTemplate::new(400).set_body_string(OPENROUTER_REFUSAL),
            };
        }
        sse("ok, continuing from the summary", 2_000)
    }
}

/// Bills every chat call `billed` prompt tokens, whatever goose sent.
struct BillingOpenRouter {
    billed: i64,
}

impl Respond for BillingOpenRouter {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body = String::from_utf8_lossy(&request.body);
        if is_compaction(&body) {
            return sse("SUMMARY: the chat is fixing the field write.", 900);
        }
        sse("noted", self.billed)
    }
}

async fn openrouter(responder: impl Respond + 'static, window: u64) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": [{"id": MODEL, "context_length": window}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/chat/completions"))
        .respond_with(responder)
        .mount(&server)
        .await;
    server
}

async fn session_with(agent: &Agent, dir: &TempDir, messages: Vec<Message>) -> Result<String> {
    let session = agent
        .config
        .session_manager
        .create_session(
            dir.path().to_path_buf(),
            "openrouter-overflow".to_string(),
            SessionType::Hidden,
            GooseMode::default(),
        )
        .await?;
    agent
        .config
        .session_manager
        .replace_conversation(&session.id, &Conversation::new_unvalidated(messages))
        .await?;
    Ok(session.id)
}

#[derive(Default)]
struct Reply {
    compacted: bool,
    texts: Vec<String>,
}

async fn reply(agent: &Agent, session_id: &str, text: &str) -> Result<Reply> {
    let config = SessionConfig {
        id: session_id.to_string(),
        schedule_id: None,
        max_turns: None,
        retry_config: None,
    };
    let stream = agent
        .reply(Message::user().with_text(text), config, None)
        .await?;
    tokio::pin!(stream);
    let mut seen = Reply::default();
    while let Some(event) = stream.next().await {
        match event? {
            AgentEvent::HistoryReplaced(_) => seen.compacted = true,
            AgentEvent::Message(message) => {
                for content in &message.content {
                    if let MessageContent::Text(t) = content {
                        seen.texts.push(t.text.clone());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(seen)
}

async fn chat_bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/api/v1/chat/completions")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

fn env<'a>(uri: &'a str, middle_out: Option<&'a str>) -> [(&'a str, Option<&'a str>); 4] {
    [
        ("OPENROUTER_HOST", Some(uri)),
        ("OPENROUTER_API_KEY", Some("or-test")),
        ("OPENROUTER_MIDDLE_OUT", middle_out),
        ("GOOSE_DISABLE_SESSION_NAMING", Some("true")),
    ]
}

/// Runs one reply over an over-long chat that OpenRouter refuses; the provider, for the window
/// assertions.
async fn refused_then_retried(mid_stream: bool) -> Result<(Arc<dyn Provider>, String)> {
    let server = openrouter(RefusingOpenRouter { mid_stream }, 500_000).await;
    let uri = server.uri();
    let _env = env_lock::lock_env(env(&uri, None));
    let dir = TempDir::new()?;
    let agent = Agent::new();
    let session_id = session_with(
        &agent,
        &dir,
        vec![
            Message::user().with_text("Fix the field write so no request touches 200+ issues."),
            Message::assistant().with_text(format!(
                "I read the batching code. {OVER_THE_WINDOW} {}",
                "The field write groups by value. ".repeat(200)
            )),
        ],
    )
    .await?;
    let provider: Arc<dyn Provider> = Arc::new(OpenRouterProvider::from_env(None).await?);
    agent
        .update_provider(provider.clone(), ModelConfig::new(MODEL), &session_id)
        .await?;

    let seen = reply(&agent, &session_id, "Carry on.").await?;

    let chats = chat_bodies(&server).await;
    let over: Vec<_> = chats
        .iter()
        .filter(|b| !is_compaction(&b.to_string()) && b.to_string().contains(OVER_THE_WINDOW))
        .collect();
    assert_eq!(over.len(), 1, "the refused request went once: {chats:#?}");
    assert!(
        chats.iter().any(|b| is_compaction(&b.to_string())),
        "goose compacted after the refusal"
    );
    let retried = chats
        .last()
        .expect("a request after the compaction")
        .to_string();
    assert!(
        !is_compaction(&retried) && !retried.contains(OVER_THE_WINDOW),
        "the retry carried the compacted conversation: {retried}"
    );
    for body in chats.iter().filter(|b| !is_compaction(&b.to_string())) {
        assert_eq!(body.get("transforms"), None, "{body}");
        assert_eq!(
            body["plugins"],
            json!([{"id": "context-compression", "enabled": false}]),
            "{body}"
        );
    }
    assert!(seen.compacted, "the history was replaced by the compaction");
    assert!(
        seen.texts
            .iter()
            .any(|t| t.contains("ok, continuing from the summary")),
        "the turn went on after the retry: {:?}",
        seen.texts
    );
    assert!(
        !seen.texts.iter().any(|t| t.contains("Unable to continue")),
        "{:?}",
        seen.texts
    );
    Ok((provider, session_id))
}

#[tokio::test]
async fn an_over_long_request_is_refused_then_compacted_and_retried() -> Result<()> {
    let (provider, session_id) = refused_then_retried(false).await?;
    // The next request is sized to the window OpenRouter named, less its output share.
    let model = ModelConfig::new(MODEL);
    assert_eq!(
        goose::context_mgmt::effective_context_limit(provider.as_ref(), &model, Some(&session_id))
            .await,
        Some(131_072 - 10_000)
    );
    Ok(())
}

#[tokio::test]
async fn a_refusal_after_the_http_200_is_compacted_and_retried_too() -> Result<()> {
    refused_then_retried(true).await?;
    Ok(())
}

/// What a second reply does after the first was billed `billed` prompt tokens on a history goose
/// counts far larger, with middle-out on.
async fn second_reply_after_billing(billed: i64) -> Result<(Reply, Vec<Value>)> {
    let server = openrouter(BillingOpenRouter { billed }, 1_000_000).await;
    let uri = server.uri();
    let _env = env_lock::lock_env(env(&uri, Some("true")));
    let dir = TempDir::new()?;
    let agent = Agent::new();
    let history = (0..40)
        .flat_map(|i| {
            [
                Message::user().with_text(format!("Step {i}: run the migration batch.")),
                Message::assistant().with_text(format!(
                    "Batch {i} done. {}",
                    "The migration copied the rows and refreshed the field values. ".repeat(250)
                )),
            ]
        })
        .collect();
    let session_id = session_with(&agent, &dir, history).await?;
    let provider: Arc<dyn Provider> = Arc::new(OpenRouterProvider::from_env(None).await?);
    agent
        .update_provider(provider, ModelConfig::new(MODEL), &session_id)
        .await?;

    let first = reply(&agent, &session_id, "Next batch.").await?;
    assert!(!first.compacted, "nothing says the window is short yet");
    let second = reply(&agent, &session_id, "And the next.").await?;
    Ok((second, chat_bodies(&server).await))
}

#[tokio::test]
async fn a_compressed_prompt_makes_the_next_check_compact() -> Result<()> {
    let (second, chats) = second_reply_after_billing(30_000).await?;
    assert!(
        second.compacted,
        "the call billed 30,000 of a history goose counts far larger was compressed; the next check compacts"
    );
    let first = chats
        .iter()
        .find(|b| !is_compaction(&b.to_string()))
        .unwrap();
    assert_eq!(first["transforms"], json!(["middle-out"]), "{first}");
    assert_eq!(first.get("plugins"), None, "{first}");
    Ok(())
}

#[tokio::test]
async fn a_prompt_billed_in_full_does_not_compact() -> Result<()> {
    let (second, _) = second_reply_after_billing(130_000).await?;
    assert!(
        !second.compacted,
        "a prompt billed in full on a 1M window is no reason to compact"
    );
    Ok(())
}
