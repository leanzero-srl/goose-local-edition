//! Q-392: a streamed chat completion whose connection ends or is cut must END the call — an
//! error the turn can show or retry, never a reader parked on a socket nobody will write to.
//!
//! The stub speaks what the split's rank 0 speaks (mlx_lm.server on `http.server`): HTTP/1.0,
//! `text/event-stream`, no Content-Length, the body ended by closing the connection, `: keepalive`
//! comments during prefill, a finish chunk, a usage chunk and `data: [DONE]`.
//!
//! Every wait below is bounded by `BOUND` — a guard of THIS harness only, so a regression shows
//! up as a failed test instead of a hung `cargo test`. Nothing in the engine carries a clock.

use std::net::SocketAddr;
use std::time::Duration;

use futures::StreamExt;
use goose_providers::api_client::{ApiClient, AuthMethod};
use goose_providers::base::{MessageStream, Provider};
use goose_providers::conversation::message::Message;
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use goose_providers::openai::OpenAiProviderBuilder;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const BOUND: Duration = Duration::from_secs(60);
const MODEL: &str = "served";
const SIDE_MARKER: &str = "side-call-marker";

/// What the stub answers a chat POST with.
#[derive(Clone, Copy)]
enum Answer {
    /// Headers, then the connection closes (FIN) with no body at all.
    HeadersThenClose,
    /// Headers, keepalives, two content chunks, then FIN — no finish_reason, no `[DONE]`.
    ChunksThenClose,
    /// Headers, one content chunk, then a RESET (SO_LINGER 0) — the `(CLOSED)` lsof state.
    ChunkThenReset,
    /// A whole answer — finish chunk, usage, `[DONE]` — then FIN, while the client is not reading.
    FinishedThenClosed,
    /// The agent's call streams a whole answer slowly; a call carrying `SIDE_MARKER` is refused
    /// with the admission hold's 503 while it streams.
    AgentStreamsSideRefused,
}

fn chunk(content: &str, finish: Option<&str>) -> String {
    let finish = finish.map_or("null".to_string(), |f| format!("\"{f}\""));
    format!(
        "data: {{\"id\":\"c\",\"object\":\"chat.completion.chunk\",\"model\":\"{MODEL}\",\
         \"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"content\":\"{content}\"}},\
         \"finish_reason\":{finish}}}]}}\n\n"
    )
}

const USAGE: &str = "data: {\"id\":\"c\",\"object\":\"chat.completion\",\"model\":\"served\",\
    \"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n";

const SSE_HEAD: &[u8] =
    b"HTTP/1.0 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n";

async fn read_request(sock: &mut TcpStream) -> (String, String) {
    let mut buf = Vec::new();
    let mut byte = [0u8; 4096];
    let head_end = loop {
        let n = sock.read(&mut byte).await.unwrap();
        assert!(n > 0, "client closed before its request ended");
        buf.extend_from_slice(&byte[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let length = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while buf.len() < head_end + length {
        let n = sock.read(&mut byte).await.unwrap();
        assert!(n > 0, "client closed before its body ended");
        buf.extend_from_slice(&byte[..n]);
    }
    let body = String::from_utf8_lossy(&buf[head_end..head_end + length]).to_string();
    (head, body)
}

async fn answer(mut sock: TcpStream, how: Answer) {
    let (head, body) = read_request(&mut sock).await;
    if head.starts_with("GET") {
        let json = format!(
            "{{\"object\":\"list\",\"data\":[{{\"id\":\"{MODEL}\",\"capabilities\":[\"text\",\"tools\"]}}]}}"
        );
        let reply = format!(
            "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{json}",
            json.len()
        );
        sock.write_all(reply.as_bytes()).await.unwrap();
        return;
    }
    match how {
        Answer::HeadersThenClose => {
            sock.write_all(SSE_HEAD).await.unwrap();
        }
        Answer::ChunksThenClose => {
            sock.write_all(SSE_HEAD).await.unwrap();
            sock.write_all(b": keepalive 1/2\n\n").await.unwrap();
            sock.write_all(chunk("hel", None).as_bytes()).await.unwrap();
            sock.write_all(chunk("lo", None).as_bytes()).await.unwrap();
        }
        Answer::ChunkThenReset => {
            sock.write_all(SSE_HEAD).await.unwrap();
            sock.write_all(chunk("hel", None).as_bytes()).await.unwrap();
            sock.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            // SO_LINGER 0 makes the drop below a RESET instead of a FIN — the whole point here.
            #[allow(deprecated)]
            sock.set_linger(Some(Duration::ZERO)).unwrap();
        }
        Answer::FinishedThenClosed => {
            sock.write_all(SSE_HEAD).await.unwrap();
            sock.write_all(chunk("hel", None).as_bytes()).await.unwrap();
            sock.write_all(chunk("lo", Some("stop")).as_bytes())
                .await
                .unwrap();
            sock.write_all(USAGE.as_bytes()).await.unwrap();
            sock.write_all(b"data: [DONE]\n\n").await.unwrap();
        }
        Answer::AgentStreamsSideRefused if body.contains(SIDE_MARKER) => {
            let json = "{\"error\":{\"message\":\"goose distributed engine is not admitting new \
                        requests: memory is low\",\"type\":\"server_busy\"}}";
            let reply = format!(
                "HTTP/1.0 503 Service Unavailable\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\n\r\n{json}",
                json.len()
            );
            sock.write_all(reply.as_bytes()).await.unwrap();
        }
        Answer::AgentStreamsSideRefused => {
            sock.write_all(SSE_HEAD).await.unwrap();
            for step in 1..=4 {
                sock.write_all(format!(": keepalive {step}/4\n\n").as_bytes())
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
            sock.write_all(chunk("hel", None).as_bytes()).await.unwrap();
            tokio::time::sleep(Duration::from_millis(400)).await;
            sock.write_all(chunk("lo", Some("stop")).as_bytes())
                .await
                .unwrap();
            sock.write_all(USAGE.as_bytes()).await.unwrap();
            sock.write_all(b"data: [DONE]\n\n").await.unwrap();
        }
    }
}

async fn stub(how: Answer) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            tokio::spawn(answer(sock, how));
        }
    });
    addr
}

fn provider(addr: SocketAddr) -> goose_providers::openai::OpenAiProvider {
    let client =
        ApiClient::new_with_tls(format!("http://{addr}"), AuthMethod::NoAuth, None).unwrap();
    OpenAiProviderBuilder::new(client)
        .name("omlx")
        .base_path("v1/chat/completions")
        .supports_streaming(true)
        .build()
}

/// Every item the stream yields until it ends, bounded by the harness guard.
async fn drain(mut stream: MessageStream) -> (String, Option<ProviderError>) {
    let mut text = String::new();
    let mut failure = None;
    let drained = tokio::time::timeout(BOUND, async {
        while let Some(item) = stream.next().await {
            match item {
                Ok((Some(message), _)) => text.push_str(&message.as_concat_text()),
                Ok((None, _)) => {}
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
    })
    .await;
    assert!(
        drained.is_ok(),
        "the stream parked: nothing ended it within the harness bound (read so far: {text:?})"
    );
    (text, failure)
}

async fn open(addr: SocketAddr, system: &str) -> Result<MessageStream, ProviderError> {
    let provider = provider(addr);
    tokio::time::timeout(
        BOUND,
        provider.stream(
            &ModelConfig::new(MODEL),
            system,
            &[Message::user().with_text("hi")],
            &[],
        ),
    )
    .await
    .expect("the request parked before its headers")
}

/// The error an ended stream must end the call with: the transient "Stream decode error" class
/// every retry path keys on, and words naming what happened to the connection.
fn assert_cut(failure: Option<ProviderError>, names: &str) {
    let failure = failure.expect("an ended stream must end the call with an error, not quietly");
    let said = failure.to_string();
    assert!(
        said.contains("Stream decode error"),
        "not the retryable class: {said}"
    );
    assert!(
        said.to_lowercase().contains(names),
        "the error must name the cut ({names}): {said}"
    );
}

const NO_MARKER: &str = "no finish_reason and no [done]";

#[tokio::test(flavor = "multi_thread")]
async fn headers_then_close_ends_the_call_loudly() {
    let addr = stub(Answer::HeadersThenClose).await;
    let (text, failure) = drain(open(addr, "system").await.unwrap()).await;
    assert_eq!(text, "");
    assert_cut(failure, NO_MARKER);
}

#[tokio::test(flavor = "multi_thread")]
async fn chunks_then_close_without_done_ends_the_call_loudly() {
    let addr = stub(Answer::ChunksThenClose).await;
    let (text, failure) = drain(open(addr, "system").await.unwrap()).await;
    assert_eq!(text, "hello");
    assert_cut(failure, NO_MARKER);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reset_mid_stream_ends_the_call_loudly() {
    let addr = stub(Answer::ChunkThenReset).await;
    let (text, failure) = drain(open(addr, "system").await.unwrap()).await;
    assert_eq!(text, "hel");
    assert_cut(failure, "connection reset");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_side_calls_503_never_touches_the_agents_stream() {
    let addr = stub(Answer::AgentStreamsSideRefused).await;
    let agent = open(addr, "system").await.unwrap();
    let side = tokio::spawn(async move { open(addr, SIDE_MARKER).await.err() });
    let (text, failure) = drain(agent).await;
    assert!(failure.is_none(), "the agent's stream failed: {failure:?}");
    assert_eq!(text, "hello");
    let refused = tokio::time::timeout(BOUND, side)
        .await
        .expect("the side call parked")
        .unwrap()
        .expect("the side call was admitted — the stub refuses it");
    assert!(
        refused.to_string().contains("not admitting new requests"),
        "{refused}"
    );
}

/// #3r turn 18 as measured: the agent read the answer up to its last message and ran the tool
/// that message asked for (300 s) without polling the stream; the engine closed the connection
/// meanwhile, so lsof showed goose holding it `(CLOSED)`. Polled again, the stream hands over the
/// rest — the usage frame and the end — and finishes cleanly: a held finished answer is not a
/// lost one.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_held_unread_while_the_server_closes_still_ends_cleanly() {
    let addr = stub(Answer::FinishedThenClosed).await;
    let mut stream = open(addr, "system").await.unwrap();
    let mut text = String::new();
    while !text.ends_with("lo") {
        let item = tokio::time::timeout(BOUND, stream.next())
            .await
            .expect("the answer parked")
            .expect("the stream ended before its last message")
            .unwrap();
        if let (Some(message), _) = item {
            text.push_str(&message.as_concat_text());
        }
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (rest, failure) = drain(stream).await;
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(format!("{text}{rest}"), "hello");
}
