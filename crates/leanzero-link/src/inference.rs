//! The chat INFERENCE PROXY: a same-account device's chat answered by THIS node's MLX engine.
//!
//! Two halves, one wire:
//!
//! - SERVING — the control service's routes (bearer / `?token=`, constant time, any `Origin`
//!   refused; see [`crate::control`]):
//!   `POST /v1/swarm/inference/v1/chat/completions`, `GET /v1/swarm/inference/v1/models`,
//!   `GET /v1/swarm/inference/v1/status`. Each is forwarded to the SAME path on this node's own
//!   engine at [`ChatServing::engine_base_url`] (loopback — the engine never binds anything
//!   else). Request and response bodies are streamed through byte for byte: the engine's status
//!   code, `content-type` and every SSE byte arrive unchanged, nothing is appended — so a stream
//!   the engine ends without `finish_reason`/`[DONE]` still ends without them, and the
//!   requester's parser names the cut (50ca4247d). An engine that dies mid-stream aborts the
//!   proxied body (the chunked response is never terminated), which the requester reads as a
//!   body error, never as a clean end. A requester that disconnects drops the proxied body,
//!   which drops the engine connection — the engine sees its client leave.
//!   Gate order: no [`ChatServing`] injected → `501`; the owner's switch off → `403`
//!   [`chat_serving_disabled`] (read per request); the engine's base URL unnamable → `503`;
//!   nothing answers there → `502` [`ENGINE_UNREACHABLE`] (the proxy NEVER mounts a model).
//!
//! - REQUESTING — [`InferenceRelay`]: a loopback listener in the REQUESTER's process that its
//!   OpenAI-compatible provider talks to as if it were an engine. It forwards
//!   `<base>/v1/{chat/completions,models,status}` to the peer's routes above through the mesh
//!   proxy ([`crate::peer_dial`]) with the node token. Its base URL carries a random capability
//!   segment (`http://127.0.0.1:<port>/relay/<64 hex>`): a local process that does not hold the
//!   URL gets `404`, and an `Origin`-bearing (browser) request gets `403`. The peer is resolved
//!   on EVERY request, so a peer whose mesh IP changed is followed and a peer that left is a
//!   named `502` [`RELAY_FAILED`], never a stale dial.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures::TryStreamExt;
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::peer_dial::{peer_http_client, MeshProxy, PeerTimeout};
use crate::state::ChatServing;

/// The control-service prefix the engine's OpenAI paths are served under.
pub const INFERENCE_ROUTE_PREFIX: &str = "/v1/swarm/inference";

/// The `502` text prefix when the serving node's engine does not answer — nothing is mounted
/// there, or it is still loading. The proxy never mounts on its own.
pub const ENGINE_UNREACHABLE: &str = "engineUnreachable";

/// The `502` text prefix when the requester's relay could not reach the peer at all (Link not
/// connected, the peer left the mesh, the SOCKS dial failed).
pub const RELAY_FAILED: &str = "linkRelayFailed";

/// The response headers carried through both hops. Everything else (hop-by-hop framing,
/// `content-length` of a re-chunked body) is the transport's own.
const PASSED_RESPONSE_HEADERS: [header::HeaderName; 2] =
    [header::CONTENT_TYPE, header::CACHE_CONTROL];
const PASSED_REQUEST_HEADERS: [header::HeaderName; 2] = [header::CONTENT_TYPE, header::ACCEPT];

/// The engine paths the proxy serves — and nothing else of the engine's surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnginePath {
    ChatCompletions,
    Models,
    Status,
}

impl EnginePath {
    pub const ALL: [EnginePath; 3] = [Self::ChatCompletions, Self::Models, Self::Status];

    /// The engine-relative path (`v1/...`), identical on the engine, under
    /// [`INFERENCE_ROUTE_PREFIX`], and under a relay's base URL.
    pub fn path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "v1/chat/completions",
            Self::Models => "v1/models",
            Self::Status => "v1/status",
        }
    }

    pub fn method(self) -> Method {
        match self {
            Self::ChatCompletions => Method::POST,
            Self::Models | Self::Status => Method::GET,
        }
    }

    pub fn control_route(self) -> String {
        format!("{INFERENCE_ROUTE_PREFIX}/{}", self.path())
    }
}

/// The `403` a node answers while its owner's chat switch is off, naming the node (the requester
/// shows it verbatim).
pub fn chat_serving_disabled(hostname: &str) -> String {
    format!(
        "chatServingDisabled: \"Let my other Macs use this Mac › Answer chat\" is off on {hostname}"
    )
}

/// The client the serving node uses to reach its OWN engine: loopback only, so no proxy may
/// divert it, and only a CONNECT timeout — a generation runs as long as it runs (gate 5).
pub(crate) fn engine_client(connect_timeout: Duration) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(connect_timeout)
        .build()
}

/// Copy the passed headers and stream the body through unchanged.
fn passthrough(response: reqwest::Response) -> Response {
    let mut builder = Response::builder().status(response.status().as_u16());
    for name in PASSED_RESPONSE_HEADERS {
        if let Some(value) = response.headers().get(&name) {
            builder = builder.header(name, value.clone());
        }
    }
    let body = Body::from_stream(response.bytes_stream());
    builder.body(body).unwrap_or_else(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("could not frame the proxied response: {err}"),
        )
            .into_response()
    })
}

fn forward_request(
    client: &reqwest::Client,
    url: String,
    path: EnginePath,
    headers: &HeaderMap,
    body: Body,
) -> reqwest::RequestBuilder {
    let mut request = client.request(path.method(), url);
    for name in PASSED_REQUEST_HEADERS {
        if let Some(value) = headers.get(&name) {
            request = request.header(name, value.clone());
        }
    }
    if path.method() == Method::POST {
        request = request.body(reqwest::Body::wrap_stream(
            body.into_data_stream().map_err(std::io::Error::other),
        ));
    }
    request
}

/// The serving half: one proxied request to this node's engine. `hostname` is asked for only on
/// the refusal path (it names the node in the `403`).
pub(crate) async fn serve<F>(
    serving: Option<&Arc<dyn ChatServing>>,
    engine_http: &reqwest::Client,
    hostname: F,
    path: EnginePath,
    headers: HeaderMap,
    body: Body,
) -> Response
where
    F: std::future::Future<Output = String>,
{
    let Some(serving) = serving else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "chat serving is not wired on this node".to_string(),
        )
            .into_response();
    };
    if !serving.serving_allowed() {
        return (
            StatusCode::FORBIDDEN,
            chat_serving_disabled(&hostname.await),
        )
            .into_response();
    }
    let base = match serving.engine_base_url() {
        Ok(base) => base,
        Err(why) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("this node cannot name its engine's address: {why}"),
            )
                .into_response()
        }
    };
    let url = format!("{}/{}", base.trim_end_matches('/'), path.path());
    match forward_request(engine_http, url, path, &headers, body)
        .send()
        .await
    {
        Ok(response) => passthrough(response),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            format!(
                "{ENGINE_UNREACHABLE}: no MLX engine answers at {base} on {} — mount a model there first ({err})",
                hostname.await
            ),
        )
            .into_response(),
    }
}

/// How the relay reaches its peer right now: the peer's control base URL, the bearer, and the
/// mesh proxy to dial through.
#[derive(Debug, Clone)]
pub struct PeerCall {
    pub base_url: String,
    pub token: String,
    pub proxy: Option<MeshProxy>,
    pub connect_timeout: Duration,
}

/// Resolves a peer (node id or mesh hostname) to a [`PeerCall`] on every relayed request. The
/// `LinkManager` implements it over its live peer registry; `Err(text)` is the named reason.
#[async_trait::async_trait]
pub trait PeerCallResolver: Send + Sync + 'static {
    async fn peer_call(&self, peer: &str) -> Result<PeerCall, String>;
}

struct RelayCtx {
    peer: String,
    secret: String,
    resolver: Arc<dyn PeerCallResolver>,
    /// One client per mesh proxy address (the daemon's SOCKS port changes only across
    /// reconnects); rebuilt when the resolved proxy differs.
    client: StdMutex<Option<(Option<MeshProxy>, reqwest::Client)>>,
}

impl RelayCtx {
    fn client_for(&self, call: &PeerCall) -> Result<reqwest::Client, String> {
        let mut cached = self.client.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((proxy, client)) = cached.as_ref() {
            if *proxy == call.proxy {
                return Ok(client.clone());
            }
        }
        let client = peer_http_client(call.proxy, PeerTimeout::ConnectOnly(call.connect_timeout))
            .map_err(|e| e.to_string())?;
        *cached = Some((call.proxy, client.clone()));
        Ok(client)
    }
}

/// The requester's loopback relay to ONE peer's inference routes. Dropping it (or
/// [`Self::shutdown`]) stops the listener; streams already in flight end with it.
pub struct InferenceRelay {
    peer: String,
    base_url: String,
    local_addr: SocketAddr,
    task: JoinHandle<()>,
}

impl InferenceRelay {
    pub async fn start(peer: String, resolver: Arc<dyn PeerCallResolver>) -> std::io::Result<Self> {
        let secret = crate::replica::hex(&rand::random::<[u8; 32]>());
        let ctx = Arc::new(RelayCtx {
            peer: peer.clone(),
            secret: secret.clone(),
            resolver,
            client: StdMutex::new(None),
        });
        let mut router = Router::new();
        for path in EnginePath::ALL {
            let route = format!("/relay/{{secret}}/{}", path.path());
            let handler =
                move |state: State<Arc<RelayCtx>>,
                      secret: Path<String>,
                      headers: HeaderMap,
                      body: Body| relay(state, secret, path, headers, body);
            router = match path.method() {
                Method::POST => router.route(&route, post(handler)),
                _ => router.route(&route, get(handler)),
            };
        }
        let router = router
            .layer(axum::middleware::from_fn(refuse_origin))
            .with_state(ctx);
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await?;
        let local_addr = listener.local_addr()?;
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                tracing::error!(%error, "inference relay listener failed");
            }
        });
        Ok(Self {
            base_url: format!("http://{local_addr}/relay/{secret}"),
            peer,
            local_addr,
            task,
        })
    }

    /// `http://127.0.0.1:<port>/relay/<capability>` — an OpenAI-compatible base URL (the
    /// provider appends `/v1/chat/completions`). It carries the capability: never log it whole.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn peer(&self) -> &str {
        &self.peer
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn shutdown(self) {
        self.task.abort();
    }
}

impl Drop for InferenceRelay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Browsers attach `Origin` to every cross-origin request; the provider never sends one.
async fn refuse_origin(request: Request, next: Next) -> Result<Response, StatusCode> {
    if request.headers().contains_key(header::ORIGIN) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(next.run(request).await)
}

async fn relay(
    State(ctx): State<Arc<RelayCtx>>,
    Path(secret): Path<String>,
    path: EnginePath,
    headers: HeaderMap,
    body: Body,
) -> Response {
    if !bool::from(secret.as_bytes().ct_eq(ctx.secret.as_bytes())) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let failed = |why: String| {
        (
            StatusCode::BAD_GATEWAY,
            format!(
                "{RELAY_FAILED}: cannot reach Link peer '{}': {why}",
                ctx.peer
            ),
        )
            .into_response()
    };
    let call = match ctx.resolver.peer_call(&ctx.peer).await {
        Ok(call) => call,
        Err(why) => return failed(why),
    };
    let client = match ctx.client_for(&call) {
        Ok(client) => client,
        Err(why) => return failed(why),
    };
    let url = format!("{}{}", call.base_url, path.control_route());
    match forward_request(&client, url, path, &headers, body)
        .bearer_auth(&call.token)
        .send()
        .await
    {
        Ok(response) => passthrough(response),
        Err(err) => failed(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_path_maps_to_one_control_route_and_method() {
        assert_eq!(
            EnginePath::ChatCompletions.control_route(),
            "/v1/swarm/inference/v1/chat/completions"
        );
        assert_eq!(EnginePath::ChatCompletions.method(), Method::POST);
        assert_eq!(
            EnginePath::Models.control_route(),
            "/v1/swarm/inference/v1/models"
        );
        assert_eq!(EnginePath::Status.method(), Method::GET);
    }

    #[test]
    fn the_refusal_names_the_switch_and_the_node() {
        let text = chat_serving_disabled("WorksMacStudio.lan");
        assert!(text.starts_with("chatServingDisabled: "), "{text}");
        assert!(text.contains("Let my other Macs use this Mac › Answer chat"));
        assert!(text.ends_with("off on WorksMacStudio.lan"));
    }
}
