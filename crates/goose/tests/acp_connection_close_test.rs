//! Session loops Q9 (DESIGN-SESSION-LOOPS §5.1, "A renderer reload mid-tick"): what this ACP stack
//! does to a prompt that is still running when its window's websocket closes. Measured over the
//! real router the desktop connects to (`create_acp_router`, a real TCP websocket), against a
//! model endpoint that streams its first words and never finishes.
//!
//! Measured answer (b): the websocket's close makes `agent-client-protocol-http` abort the
//! connection's task, and that drops every future the connection spawned — `on_prompt` included —
//! mid-await. Nothing after the await runs: no `clear_active_run`, no end-of-turn record. So the
//! state a prompt holds for its turn must be released by guards that drop with the future.

use std::sync::LazyLock;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goose::acp::server_factory::{AcpServer, AcpServerFactoryConfig};
use goose::acp::transport::create_acp_router;
use goose::agents::GoosePlatform;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message as WsMessage;

const WRITTEN: &str = "The first words of an answer that never finishes";

static ROOT: LazyLock<tempfile::TempDir> = LazyLock::new(|| tempfile::tempdir().unwrap());

/// An OpenAI-compatible endpoint whose answer streams its first words and then never finishes.
async fn never_finishing_openai() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = socket.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                if let Some(head_end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let length = String::from_utf8_lossy(&request[..head_end])
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if request.len() >= head_end + 4 + length {
                        break;
                    }
                }
            }
            if String::from_utf8_lossy(&request).starts_with("GET") {
                let body = include_str!("acp_test_data/openai_models.json");
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                continue;
            }
            let chunk = json!({
                "id": "unfinished-1",
                "object": "chat.completion.chunk",
                "created": 1,
                "model": "gpt-4o",
                "choices": [{"index": 0, "delta": {"role": "assistant", "content": WRITTEN}, "finish_reason": null}],
            });
            let head =
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket
                .write_all(format!("data: {chunk}\n\n").as_bytes())
                .await;
            let _ = socket.flush().await;
            held.push(socket);
        }
    });
    base_url
}

fn write_config(openai: &str) {
    std::env::set_var("GOOSE_PATH_ROOT", ROOT.path());
    std::env::set_var("GOOSE_DISABLE_KEYRING", "1");
    std::env::set_var("OPENAI_API_KEY", "test-key");
    let config_dir = goose::config::paths::Paths::config_dir();
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join(goose::config::base::CONFIG_YAML_NAME),
        format!(
            "GOOSE_MODEL: gpt-4o\nGOOSE_PROVIDER: openai\nOPENAI_HOST: {openai}\nGOOSE_DISABLE_KEYRING: true\nGOOSE_MEMORY_PROPOSALS: false\nGOOSE_DISABLE_SESSION_NAMING: true\n"
        ),
    )
    .unwrap();
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Window {
    socket: Socket,
    next_id: u64,
    seen: Vec<Value>,
}

impl Window {
    async fn open(addr: std::net::SocketAddr) -> Self {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/acp"))
            .await
            .expect("the websocket opens");
        let mut window = Window {
            socket,
            next_id: 0,
            seen: Vec::new(),
        };
        window
            .request(
                "initialize",
                json!({"protocolVersion": 1, "clientCapabilities": {}}),
            )
            .await;
        window
    }

    async fn send(&mut self, method: &str, params: Value) -> u64 {
        self.next_id += 1;
        let frame =
            json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params});
        self.socket
            .send(WsMessage::Text(frame.to_string().into()))
            .await
            .unwrap();
        self.next_id
    }

    async fn next_frame(&mut self) -> Value {
        loop {
            match self.socket.next().await {
                Some(Ok(WsMessage::Text(text))) => return serde_json::from_str(&text).unwrap(),
                Some(Ok(_)) => continue,
                other => panic!("the websocket ended: {other:?}"),
            }
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.send(method, params).await;
        loop {
            let frame = self.next_frame().await;
            if frame.get("id").and_then(Value::as_u64) == Some(id) && frame.get("method").is_none()
            {
                assert!(frame.get("error").is_none(), "{method} failed: {frame}");
                return frame["result"].clone();
            }
            self.seen.push(frame);
        }
    }

    async fn until_streaming(&mut self) {
        loop {
            let frame = self.next_frame().await;
            let streaming = frame["method"] == "session/update"
                && frame["params"]["update"]["sessionUpdate"] == "agent_message_chunk";
            self.seen.push(frame);
            if streaming {
                return;
            }
        }
    }
}

async fn serve() -> std::net::SocketAddr {
    let server = std::sync::Arc::new(AcpServer::new(AcpServerFactoryConfig {
        builtins: vec![],
        data_dir: goose::config::paths::Paths::data_dir(),
        config_dir: goose::config::paths::Paths::config_dir(),
        goose_platform: GoosePlatform::GooseDesktop,
        additional_source_roots: Vec::new(),
        scheduler: None,
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = create_acp_router(server);
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    addr
}

async fn user_turns_running() -> usize {
    goose::turn_priority::global().running().running
}

/// Waits (test-side only) until `done` holds, or says what it last saw.
async fn eventually(what: &str, mut done: impl AsyncFnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline {
        if done().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{what}: not within the test's wait");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_websocket_drops_the_prompt_it_was_running() {
    let openai = never_finishing_openai().await;
    write_config(&openai);
    let addr = serve().await;

    let mut window = Window::open(addr).await;
    let work = tempfile::tempdir().unwrap();
    let session = window
        .request("session/new", json!({"cwd": work.path(), "mcpServers": []}))
        .await;
    let session_id = session["sessionId"].as_str().unwrap().to_string();

    assert_eq!(user_turns_running().await, 0);
    window
        .send(
            "session/prompt",
            json!({"sessionId": session_id, "prompt": [{"type": "text", "text": "Plan the migration"}]}),
        )
        .await;
    window.until_streaming().await;
    assert_eq!(
        user_turns_running().await,
        1,
        "the prompt holds its user turn while it streams"
    );

    // The window reloads: its websocket closes with the answer still streaming.
    window.socket.close(None).await.unwrap();
    drop(window);

    // (b): the turn's guards drop with the prompt's future — the prompt did not run on unseen.
    eventually("the closed connection's prompt was dropped", async || {
        user_turns_running().await == 0
    })
    .await;

    // A new window on the same goose serves the same chat at once: nothing of the old
    // connection's run holds the session busy.
    let mut reloaded = Window::open(addr).await;
    reloaded
        .request(
            "session/load",
            json!({"sessionId": session_id, "cwd": work.path(), "mcpServers": []}),
        )
        .await;
    reloaded
        .send(
            "session/prompt",
            json!({"sessionId": session_id, "prompt": [{"type": "text", "text": "Go on"}]}),
        )
        .await;
    reloaded.until_streaming().await;
    assert_eq!(user_turns_running().await, 1);
}
