//! A desktop window as goosed sees it: a real websocket to the real ACP router
//! (`create_acp_router` over a TCP listener), raw JSON-RPC frames in and out — so every
//! notification goose sends, its own `_goose/*` ones included, is visible — against a scripted
//! OpenAI-compatible endpoint whose every answer the test decides.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goose::acp::server_factory::{AcpServer, AcpServerFactoryConfig};
use goose::acp::transport::create_acp_router;
use goose::agents::GoosePlatform;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message as WsMessage;

static ROOT: LazyLock<tempfile::TempDir> = LazyLock::new(|| tempfile::tempdir().unwrap());

/// What the scripted model does with one completion request.
#[derive(Debug, Clone)]
pub enum Answer {
    /// Streams `text` and finishes the answer.
    Finish(&'static str),
    /// Streams `text` and never finishes.
    Unfinished(&'static str),
    /// Answers nothing until [`Model::release_held`] closes the socket (the call then errors).
    Hold,
    /// Asks for one tool call — `(tool, JSON arguments)` — and finishes the answer.
    ToolCall(&'static str, &'static str),
    /// Asks for several tool calls in ONE message — each `(tool, JSON arguments)` — and finishes.
    ToolCalls(Vec<(&'static str, String)>),
    /// 503 as goose's distributed engine answers while its watchdog holds admission for memory
    /// (Q-397): the `memory_hold` code, the watchdog's words and the path to wait on. A
    /// `GET /goose/admission` then waits until [`Model::admit`].
    MemoryHold(&'static str),
    /// 503 with no code — any other busy engine.
    Busy(&'static str),
}

/// The words goose's side requests open with: the tool-call label (acp/server/tool_labels.rs) and
/// the end-of-turn fact checker (turn_assessment.rs).
const TOOL_LABEL_REQUEST: &str = "Summarize this tool call in a short lowercase phrase";
/// The label for a message that asked several tools at once.
const TOOL_SEQUENCE_LABEL_REQUEST: &str =
    "Summarize this sequence of tool calls in a short lowercase phrase";
const FACT_CHECK_REQUEST: &str = "You are goose's end-of-turn fact checker";

/// A scripted OpenAI-compatible endpoint: each completion request takes the next [`Answer`];
/// with none left it answers [`Answer::Hold`].
#[derive(Clone)]
pub struct Model {
    pub base_url: String,
    script: Arc<Mutex<VecDeque<Answer>>>,
    held: Arc<Mutex<Vec<TcpStream>>>,
    pub requests: Arc<Mutex<Vec<String>>>,
    /// Set by [`Model::keep_side_requests_off_the_script`].
    side_requests_apart: Arc<std::sync::atomic::AtomicBool>,
    admission: Arc<Mutex<Admission>>,
}

/// Every `GET /goose/admission` received, and the ones still waiting for [`Model::admit`] — one
/// lock, so a wait a test has counted is always one `admit` answers (Q-420: counted under one
/// lock and parked under another, an `admit` between the two answered nobody and the turn hung).
#[derive(Default)]
struct Admission {
    waits: usize,
    waiting: Vec<TcpStream>,
}

fn json_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn chunk(delta: Value, finish: Option<&str>) -> String {
    let chunk = json!({
        "id": "scripted-1",
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "gpt-4o",
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
    });
    format!("data: {chunk}\n\n")
}

async fn read_request(socket: &mut TcpStream) -> String {
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
    String::from_utf8_lossy(&request).to_string()
}

impl Model {
    pub async fn start(script: Vec<Answer>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let model = Model {
            base_url: format!("http://{}", listener.local_addr().unwrap()),
            script: Arc::new(Mutex::new(script.into())),
            held: Arc::default(),
            requests: Arc::default(),
            side_requests_apart: Arc::default(),
            admission: Arc::default(),
        };
        let serving = model.clone();
        tokio::spawn(async move {
            let mut unfinished = Vec::new();
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let request = read_request(&mut socket).await;
                if request.starts_with("GET /goose/admission ") {
                    let mut admission = serving.admission.lock().unwrap();
                    admission.waits += 1;
                    admission.waiting.push(socket);
                    continue;
                }
                if request.starts_with("GET") {
                    let body = include_str!("../acp_test_data/openai_models.json");
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    continue;
                }
                let apart = serving
                    .side_requests_apart
                    .load(std::sync::atomic::Ordering::Relaxed);
                let answer = if apart
                    && (request.contains(TOOL_LABEL_REQUEST)
                        || request.contains(TOOL_SEQUENCE_LABEL_REQUEST))
                {
                    Answer::Finish("running a tool")
                } else if apart && request.contains(FACT_CHECK_REQUEST) {
                    Answer::Hold
                } else {
                    serving.requests.lock().unwrap().push(request);
                    serving
                        .script
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or(Answer::Hold)
                };
                let head =
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
                match answer {
                    Answer::Finish(text) => {
                        let _ = socket.write_all(head.as_bytes()).await;
                        let body = [
                            chunk(json!({"role": "assistant", "content": text}), None),
                            chunk(json!({}), Some("stop")),
                            format!(
                                "data: {}\n\n",
                                json!({"id": "scripted-1", "object": "chat.completion.chunk", "created": 1, "model": "gpt-4o", "choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110}})
                            ),
                            "data: [DONE]\n\n".to_string(),
                        ]
                        .concat();
                        let _ = socket.write_all(body.as_bytes()).await;
                        let _ = socket.flush().await;
                    }
                    Answer::Unfinished(text) => {
                        let _ = socket.write_all(head.as_bytes()).await;
                        let _ = socket
                            .write_all(
                                chunk(json!({"role": "assistant", "content": text}), None)
                                    .as_bytes(),
                            )
                            .await;
                        let _ = socket.flush().await;
                        unfinished.push(socket);
                    }
                    Answer::ToolCall(..) | Answer::ToolCalls(_) => {
                        let calls = match answer {
                            Answer::ToolCall(tool, arguments) => {
                                vec![(tool, arguments.to_string())]
                            }
                            Answer::ToolCalls(calls) => calls,
                            _ => unreachable!(),
                        };
                        let _ = socket.write_all(head.as_bytes()).await;
                        let request = serving.requests.lock().unwrap().len();
                        let tool_calls: Vec<Value> = calls
                            .iter()
                            .enumerate()
                            .map(|(index, (tool, arguments))| {
                                let id = if index == 0 {
                                    format!("call_{request}")
                                } else {
                                    format!("call_{request}_{index}")
                                };
                                json!({
                                    "index": index,
                                    "id": id,
                                    "type": "function",
                                    "function": {"name": tool, "arguments": arguments},
                                })
                            })
                            .collect();
                        let call = json!({"role": "assistant", "tool_calls": tool_calls});
                        let body = [
                            chunk(call, None),
                            chunk(json!({}), Some("tool_calls")),
                            format!(
                                "data: {}\n\n",
                                json!({"id": "scripted-1", "object": "chat.completion.chunk", "created": 1, "model": "gpt-4o", "choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110}})
                            ),
                            "data: [DONE]\n\n".to_string(),
                        ]
                        .concat();
                        let _ = socket.write_all(body.as_bytes()).await;
                        let _ = socket.flush().await;
                    }
                    Answer::Hold => serving.held.lock().unwrap().push(socket),
                    Answer::MemoryHold(reason) => {
                        let body = json!({"error": {
                            "message": format!("goose distributed engine is not admitting new requests: {reason}"),
                            "type": "server_busy",
                            "code": "memory_hold",
                            "reason": reason,
                            "admission": "/goose/admission",
                        }});
                        let response = json_response("503 Service Unavailable", &body.to_string());
                        let _ = socket.write_all(response.as_bytes()).await;
                    }
                    Answer::Busy(message) => {
                        let body = json!({"error": {"message": message, "type": "server_busy"}});
                        let response = json_response("503 Service Unavailable", &body.to_string());
                        let _ = socket.write_all(response.as_bytes()).await;
                    }
                }
            }
        });
        model
    }

    /// goose's side requests — a tool call's label, the end-of-turn fact check — are answered
    /// apart (a label, a hold) and never take, or count as, one of the script's turns.
    pub fn keep_side_requests_off_the_script(&self) {
        self.side_requests_apart
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// The engine admits again: every waiting `GET /goose/admission` is answered.
    pub async fn admit(&self) {
        let waiting = std::mem::take(&mut self.admission.lock().unwrap().waiting);
        let response = json_response("200 OK", r#"{"admission_open": true}"#);
        for mut socket in waiting {
            let _ = socket.write_all(response.as_bytes()).await;
        }
    }

    pub fn admission_waits(&self) -> usize {
        self.admission.lock().unwrap().waits
    }

    pub fn completion_requests(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    /// Replace what the next completion requests get.
    pub fn script(&self, answers: Vec<Answer>) {
        *self.script.lock().unwrap() = answers.into();
    }

    pub fn held(&self) -> usize {
        self.held.lock().unwrap().len()
    }

    /// Close every held request's socket: each call it answers ends in an error.
    pub fn release_held(&self) {
        self.held.lock().unwrap().clear();
    }
}

/// goose's config and data under this test binary's own root, pointed at `model`.
pub fn configure(model: &Model, memory_proposals: bool) {
    std::env::set_var("GOOSE_PATH_ROOT", ROOT.path());
    std::env::set_var("GOOSE_DISABLE_KEYRING", "1");
    std::env::set_var("OPENAI_API_KEY", "test-key");
    let config_dir = goose::config::paths::Paths::config_dir();
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join(goose::config::base::CONFIG_YAML_NAME),
        format!(
            "GOOSE_MODEL: gpt-4o\nGOOSE_PROVIDER: openai\nOPENAI_HOST: {}\nGOOSE_DISABLE_KEYRING: true\nGOOSE_MEMORY_PROPOSALS: {memory_proposals}\nGOOSE_DISABLE_SESSION_NAMING: true\n",
            model.base_url
        ),
    )
    .unwrap();
}

/// A goose serving the ACP router on a loopback port, over the process's own store (so the loop
/// runner, which lives on that store, is installed for its agents).
pub async fn serve() -> std::net::SocketAddr {
    let server = Arc::new(AcpServer::new(AcpServerFactoryConfig {
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

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Window {
    pub socket: Socket,
    next_id: u64,
    pub seen: Vec<Value>,
}

/// How long a test waits for goose to get where it is going before it calls the state a hang: a
/// bound on the TEST, never on the product.
pub const SETTLE: Duration = Duration::from_secs(20);

impl Window {
    /// A window; `hears_goose` = it declares goose's own notifications, as the desktop does.
    pub async fn open(addr: std::net::SocketAddr, hears_goose: bool) -> Self {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/acp"))
            .await
            .expect("the websocket opens");
        let mut window = Window {
            socket,
            next_id: 0,
            seen: Vec::new(),
        };
        let capabilities = if hears_goose {
            json!({"_meta": {"goose": {"customNotifications": true}}})
        } else {
            json!({})
        };
        window
            .request(
                "initialize",
                json!({"protocolVersion": 1, "clientCapabilities": capabilities}),
            )
            .await;
        window
    }

    pub async fn send(&mut self, method: &str, params: Value) -> u64 {
        self.next_id += 1;
        let frame =
            json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params});
        self.socket
            .send(WsMessage::Text(frame.to_string().into()))
            .await
            .unwrap();
        self.next_id
    }

    pub async fn notify(&mut self, method: &str, params: Value) {
        let frame = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.socket
            .send(WsMessage::Text(frame.to_string().into()))
            .await
            .unwrap();
    }

    async fn next_frame(&mut self) -> Value {
        let frame = tokio::time::timeout(SETTLE, async {
            loop {
                match self.socket.next().await {
                    Some(Ok(WsMessage::Text(text))) => {
                        return serde_json::from_str::<Value>(&text).unwrap()
                    }
                    Some(Ok(_)) => continue,
                    other => panic!("the websocket ended: {other:?}"),
                }
            }
        })
        .await;
        frame.unwrap_or_else(|_| panic!("no frame from goose; seen so far: {:#?}", self.seen))
    }

    /// The response to request `id`: its `result`, or its `error` as `Err`.
    pub async fn response(&mut self, id: u64) -> Result<Value, Value> {
        if let Some(at) = self.seen.iter().position(|f| is_response(f, id)) {
            let frame = self.seen.remove(at);
            return outcome(frame);
        }
        loop {
            let frame = self.next_frame().await;
            if is_response(&frame, id) {
                return outcome(frame);
            }
            self.seen.push(frame);
        }
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.send(method, params).await;
        self.response(id)
            .await
            .unwrap_or_else(|error| panic!("{method} failed: {error}"))
    }

    /// The first notification `method` whose params satisfy `matches`, seen already or next.
    pub async fn notification(&mut self, method: &str, matches: impl Fn(&Value) -> bool) -> Value {
        let hit = |f: &Value| f["method"] == method && matches(&f["params"]);
        if let Some(at) = self.seen.iter().position(hit) {
            return self.seen.remove(at)["params"].clone();
        }
        loop {
            let frame = self.next_frame().await;
            if hit(&frame) {
                return frame["params"].clone();
            }
            self.seen.push(frame);
        }
    }

    pub async fn until_streaming(&mut self, session_id: &str) {
        self.notification("session/update", |p| {
            p["sessionId"] == session_id && p["update"]["sessionUpdate"] == "agent_message_chunk"
        })
        .await;
    }

    pub async fn new_chat(&mut self, work: &std::path::Path) -> String {
        self.request("session/new", json!({"cwd": work, "mcpServers": []}))
            .await["sessionId"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// `session/prompt`, not awaited; `meta` is the prompt's `_meta`.
    pub async fn prompt(&mut self, session_id: &str, text: &str, meta: Option<Value>) -> u64 {
        let mut params =
            json!({"sessionId": session_id, "prompt": [{"type": "text", "text": text}]});
        if let Some(meta) = meta {
            params["_meta"] = meta;
        }
        self.send("session/prompt", params).await
    }

    pub async fn cancel(&mut self, session_id: &str) {
        self.notify("session/cancel", json!({"sessionId": session_id}))
            .await;
    }

    pub async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }
}

fn is_response(frame: &Value, id: u64) -> bool {
    frame.get("id").and_then(Value::as_u64) == Some(id) && frame.get("method").is_none()
}

fn outcome(frame: Value) -> Result<Value, Value> {
    match frame.get("error") {
        Some(error) => Err(error.clone()),
        None => Ok(frame["result"].clone()),
    }
}

pub fn user_turns_running() -> usize {
    goose::turn_priority::global().running().running
}

/// Waits (test-side only) until `done` holds.
pub async fn eventually<F, Fut>(what: &str, mut done: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + SETTLE;
    while tokio::time::Instant::now() < deadline {
        if done().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{what}: not within the test's wait");
}
