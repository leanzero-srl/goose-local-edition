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
//!
//! - IN FLIGHT — a request the relay has sent is WATCHED until it ends (Q-32). A peer whose
//!   tailscaled dies leaves the requester's connection open and silent — no FIN, no RST ever
//!   crosses the mesh (r3-1.log `12:14:59 0 120.00s TimeoutError`: the request in flight hung
//!   until the client's own timeout, while every later request got a named `502` at the
//!   relay's 5 s connect timeout). The relay tags each request with a random id
//!   ([`RELAY_STREAM_HEADER`]); the serving node keeps the id in its [`InflightStreams`] for
//!   exactly as long as the response body lives, and answers `GET` [`streams_route`] with
//!   [`StreamLiveness`]. Every `liveness_interval` (the fabric's own poll cadence) the relay
//!   LOOKS — one fresh dial, bounded by the transport's connect timeout
//!   ([`PeerTimeout::FreshTotal`]) — and ends the request with a named [`RELAY_FAILED`] only
//!   on transport evidence ([`InFlightWatch`]): the peer, reachable, no longer holds it; or
//!   [`MESH_POLL_FAILURE_LOOKS`] looks in a row could not reach the peer at all. It never
//!   times the model: a generation that is silent for minutes (a long prefill) is held by
//!   the peer and answers every look, so it is never cut. Before the response head, the end
//!   is a `502` with the reason; after it, an SSE body gets one error event
//!   (`{"error":{"message":…,"type":"linkRelayFailed"}}`, which the OpenAI stream parser
//!   raises verbatim) and every body is then aborted — never a clean end.

use std::collections::HashSet;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures::{Stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::manager::MESH_POLL_FAILURE_LOOKS;
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

/// The request header carrying the relay's id for one request, so the serving node can say
/// whether it still holds it ([`streams_route`]). Never forwarded to the engine.
pub const RELAY_STREAM_HEADER: &str = "x-leanzero-link-stream";

/// Where a serving node answers [`StreamLiveness`] for one relay request id (bearer-gated
/// like every route): `GET <prefix>/streams/<id>`.
pub fn streams_route() -> String {
    format!("{INFERENCE_ROUTE_PREFIX}/streams/{{id}}")
}

/// Consecutive looks that must find the request RELEASED by a peer that had shown it (held,
/// under an earlier epoch, or across a look that could not reach it) before the relay ends
/// it: one look sees the release, the next confirms it was not a response completing with
/// its final bytes still crossing the mesh (a completed response ends the watch first).
const RELEASE_CONFIRM_LOOKS: u32 = 2; // ratio: one observation plus one confirmation

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

/// The passed headers of `response`, on a builder with its status.
fn passed_head(response: &reqwest::Response) -> axum::http::response::Builder {
    let mut builder = Response::builder().status(response.status().as_u16());
    for name in PASSED_RESPONSE_HEADERS {
        if let Some(value) = response.headers().get(&name) {
            builder = builder.header(name, value.clone());
        }
    }
    builder
}

/// Copy the passed headers and stream the body through unchanged. `held` (the serving node's
/// hold on a relay request id) lives exactly as long as the body does.
fn passthrough(response: reqwest::Response, held: Option<StreamHold>) -> Response {
    let builder = passed_head(&response);
    let body = Body::from_stream(response.bytes_stream().map(move |item| {
        let _held = &held;
        item
    }));
    framed(builder, body)
}

fn framed(builder: axum::http::response::Builder, body: Body) -> Response {
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
/// the refusal path (it names the node in the `403`). A request the relay tagged
/// ([`RELAY_STREAM_HEADER`]) is held in `streams` for as long as its response body lives.
pub(crate) async fn serve<F>(
    serving: Option<&Arc<dyn ChatServing>>,
    streams: &Arc<InflightStreams>,
    engine_http: &reqwest::Client,
    hostname: F,
    path: EnginePath,
    headers: HeaderMap,
    body: Body,
) -> Response
where
    F: std::future::Future<Output = String>,
{
    let held = headers
        .get(RELAY_STREAM_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|id| streams.hold(id));
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
        Ok(response) => passthrough(response, held),
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

/// A serving node's answer to "do you still hold relay request `<id>`?" — the wire of
/// `GET` [`streams_route`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamLiveness {
    /// Random per control-service start: a peer whose Link restarted answers under a NEW
    /// epoch, and holds nothing the old one held.
    pub epoch: String,
    /// The response to this id is still being served (its body has not been dropped).
    pub live: bool,
}

/// The relay request ids a control service is serving right now (see [`serve`]).
pub struct InflightStreams {
    epoch: String,
    held: StdMutex<HashSet<String>>,
}

impl InflightStreams {
    pub fn new() -> Self {
        Self {
            epoch: crate::replica::hex(&rand::random::<[u8; 16]>()),
            held: StdMutex::new(HashSet::new()),
        }
    }

    /// Hold `id` until the returned guard drops. An id that is not a relay id (32 lowercase
    /// hex — what [`InferenceRelay`] sends) is not held: nothing is stored from a header a
    /// relay did not mint.
    fn hold(self: &Arc<Self>, id: &str) -> Option<StreamHold> {
        if !is_stream_id(id) {
            return None;
        }
        self.held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_string());
        Some(StreamHold {
            streams: self.clone(),
            id: id.to_string(),
        })
    }

    pub fn liveness(&self, id: &str) -> StreamLiveness {
        StreamLiveness {
            epoch: self.epoch.clone(),
            live: self
                .held
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(id),
        }
    }
}

impl Default for InflightStreams {
    fn default() -> Self {
        Self::new()
    }
}

/// A serving node's hold on one relay request id; released on drop.
struct StreamHold {
    streams: Arc<InflightStreams>,
    id: String,
}

impl Drop for StreamHold {
    fn drop(&mut self) {
        self.streams
            .held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

fn is_stream_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// How the relay reaches its peer right now: the peer's control base URL, the bearer, and the
/// mesh proxy to dial through.
#[derive(Debug, Clone)]
pub struct PeerCall {
    pub base_url: String,
    pub token: String,
    pub proxy: Option<MeshProxy>,
    pub connect_timeout: Duration,
    /// How often a request in flight is looked at ([`InFlightWatch`]) — the fabric's own
    /// poll cadence. It decides when the relay LOOKS, never whether a request is cut: only
    /// what a look finds does.
    pub liveness_interval: Duration,
}

/// Resolves a peer (node id or mesh hostname) to a [`PeerCall`] on every relayed request. The
/// `LinkManager` implements it over its live peer registry; `Err(text)` is the named reason.
#[async_trait::async_trait]
pub trait PeerCallResolver: Send + Sync + 'static {
    async fn peer_call(&self, peer: &str) -> Result<PeerCall, String>;
}

/// The relay's two clients over one mesh proxy: the request's own (CONNECT-bounded only — a
/// generation runs as long as it runs) and the in-flight look's (a fresh dial each time,
/// bounded in total).
#[derive(Clone)]
struct RelayClients {
    request: reqwest::Client,
    look: reqwest::Client,
}

/// What the cached [`RelayClients`] were built for: the mesh proxy and the connect timeout.
type ClientsKey = (Option<MeshProxy>, Duration);

struct RelayCtx {
    peer: String,
    secret: String,
    resolver: Arc<dyn PeerCallResolver>,
    /// One pair of clients per mesh proxy address and connect timeout (the daemon's SOCKS
    /// port changes only across reconnects); rebuilt when the resolved call differs.
    clients: StdMutex<Option<(ClientsKey, RelayClients)>>,
}

impl RelayCtx {
    fn clients_for(&self, call: &PeerCall) -> Result<RelayClients, String> {
        let key = (call.proxy, call.connect_timeout);
        let mut cached = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached_key, clients)) = cached.as_ref() {
            if *cached_key == key {
                return Ok(clients.clone());
            }
        }
        let build = |timeout| peer_http_client(call.proxy, timeout).map_err(|e| e.to_string());
        let clients = RelayClients {
            request: build(PeerTimeout::ConnectOnly(call.connect_timeout))?,
            look: build(PeerTimeout::FreshTotal(call.connect_timeout))?,
        };
        *cached = Some((key, clients.clone()));
        Ok(clients)
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
            clients: StdMutex::new(None),
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
    let call = match ctx.resolver.peer_call(&ctx.peer).await {
        Ok(call) => call,
        Err(why) => return unreachable_peer(&ctx.peer, &why),
    };
    let clients = match ctx.clients_for(&call) {
        Ok(clients) => clients,
        Err(why) => return unreachable_peer(&ctx.peer, &why),
    };
    let stream_id = crate::replica::hex(&rand::random::<[u8; 16]>());
    let url = format!("{}{}", call.base_url, path.control_route());
    let send = forward_request(&clients.request, url, path, &headers, body)
        .bearer_auth(&call.token)
        .header(RELAY_STREAM_HEADER, &stream_id)
        .send();
    let look_url = format!(
        "{}{}",
        call.base_url,
        streams_route().replace("{id}", &stream_id)
    );
    let mut lost: LostPeer = Box::pin(watch_in_flight(
        clients.look,
        look_url,
        call.token.clone(),
        call.liveness_interval,
    ));
    tokio::select! {
        sent = send => match sent {
            Ok(response) => watched_passthrough(response, lost, ctx.peer.clone()),
            Err(err) => unreachable_peer(&ctx.peer, &err.to_string()),
        },
        evidence = &mut lost => (
            StatusCode::BAD_GATEWAY,
            lost_in_flight_text(&ctx.peer, &evidence),
        )
            .into_response(),
    }
}

/// The `502` for a request the relay could not deliver at all.
fn unreachable_peer(peer: &str, why: &str) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        format!("{RELAY_FAILED}: cannot reach Link peer '{peer}': {why}"),
    )
        .into_response()
}

/// The text a request the peer dropped IN FLIGHT ends with, given the watch's evidence.
fn lost_in_flight_text(peer: &str, evidence: &str) -> String {
    format!("{RELAY_FAILED}: Link peer '{peer}' lost this request in flight: {evidence}")
}

/// Resolves — with the evidence — only when [`InFlightWatch`] ends the request; a request
/// that ends first simply drops it.
type LostPeer = Pin<Box<dyn Future<Output = String> + Send>>;

/// Stream the peer's response through, ending it the moment `lost` resolves: an SSE body
/// gets one OpenAI-shaped error event naming the loss, then every body is aborted (the
/// chunked response is never terminated), so the requester reads a named error and never
/// a clean end. A response that completes drops the watch with it.
fn watched_passthrough(response: reqwest::Response, lost: LostPeer, peer: String) -> Response {
    let builder = passed_head(&response);
    let sse = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"));
    let body = watched_body(response.bytes_stream(), lost, sse, peer);
    framed(builder, Body::from_stream(body))
}

enum Watched<S> {
    Streaming {
        upstream: S,
        lost: LostPeer,
    },
    /// The error event was sent; the abort follows.
    Lost(String),
    Done,
}

fn watched_body<S>(
    upstream: S,
    lost: LostPeer,
    sse: bool,
    peer: String,
) -> impl Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send
where
    S: Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send + Unpin + 'static,
{
    futures::stream::unfold(Watched::Streaming { upstream, lost }, move |state| {
        let peer = peer.clone();
        async move {
            match state {
                Watched::Streaming {
                    mut upstream,
                    mut lost,
                } => {
                    tokio::select! {
                        biased;
                        item = upstream.next() => match item {
                            Some(Ok(bytes)) => Some((Ok(bytes), Watched::Streaming { upstream, lost })),
                            Some(Err(err)) => Some((Err(std::io::Error::other(err)), Watched::Done)),
                            None => None,
                        },
                        evidence = &mut lost => {
                            let message = lost_in_flight_text(&peer, &evidence);
                            tracing::error!(%message, "leanzero-link relay: request lost in flight");
                            if sse {
                                let frame = serde_json::json!({
                                    "error": {"message": message, "type": RELAY_FAILED, "code": 502}
                                });
                                Some((
                                    Ok(axum::body::Bytes::from(format!("data: {frame}\n\n"))),
                                    Watched::Lost(message),
                                ))
                            } else {
                                Some((Err(std::io::Error::other(message)), Watched::Done))
                            }
                        }
                    }
                }
                Watched::Lost(message) => {
                    // One pending poll between the event and the abort: the connection
                    // flushes what it holds while the body is pending, and an abort drops
                    // whatever is still unflushed (measured: without it the event never
                    // reached the requester).
                    tokio::task::yield_now().await;
                    Some((Err(std::io::Error::other(message)), Watched::Done))
                }
                Watched::Done => None,
            }
        }
    })
}

/// What one look at a request in flight found.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Look {
    /// The peer answered and still serves the request.
    Held { epoch: String },
    /// The peer answered and does NOT serve it (never received, finished, or dropped).
    Released { epoch: String },
    /// The peer answered, but not with [`StreamLiveness`] (an older goose without the route
    /// answers `404`): it is reachable, and says nothing about the request.
    Reached,
    /// No answer: the fresh dial or the answer did not arrive within the look's bound.
    Unreached(String),
}

/// The in-flight verdict, fed one [`Look`] at a time. Every rule is transport evidence — an
/// answer about THIS request, or the absence of any answer from the peer — never how long
/// the model has been quiet:
///
/// - [`MESH_POLL_FAILURE_LOOKS`] consecutive `Unreached` → the peer is gone (its Link died
///   and stayed down — r3-1's Studio).
/// - `Released` after the peer had SHOWN the request — held it, answered under an earlier
///   epoch, or been unreachable since it was sent — for [`RELEASE_CONFIRM_LOOKS`] looks in
///   a row → it was dropped on the peer's side (its Link restarted under it: the new
///   control service holds nothing the old one did).
/// - `Released` from a peer that has never shown it, for [`MESH_POLL_FAILURE_LOOKS`] looks in
///   a row → the request never arrived (it went down a kept-alive connection that was
///   already dead).
///
/// A slow, healthy generation is `Held` on every look and is never ended.
#[derive(Debug, Default)]
struct InFlightWatch {
    /// The peer has shown it knew this request: it held it, answered under an epoch other
    /// than its first, or could not be reached at some look since it was sent.
    shown: bool,
    first_epoch: Option<String>,
    /// Consecutive looks that could not reach the peer.
    unreached: u32,
    /// Consecutive looks at which the reachable peer did not hold the request.
    released: u32,
}

impl InFlightWatch {
    fn observe(&mut self, look: Look) -> Option<String> {
        match look {
            Look::Unreached(why) => {
                self.unreached += 1;
                self.released = 0;
                self.shown = true;
                (self.unreached >= MESH_POLL_FAILURE_LOOKS).then(|| {
                    format!(
                        "{} looks in a row could not reach the peer through the mesh (last: \
                         {why}) — its LeanZero Link is down",
                        self.unreached
                    )
                })
            }
            Look::Reached => {
                self.unreached = 0;
                self.released = 0;
                None
            }
            Look::Held { epoch } => {
                self.unreached = 0;
                self.released = 0;
                self.shown = true;
                self.first_epoch.get_or_insert(epoch);
                None
            }
            Look::Released { epoch } => {
                self.unreached = 0;
                self.released += 1;
                let first = self.first_epoch.get_or_insert(epoch.clone());
                if *first != epoch {
                    self.shown = true;
                }
                let needed = if self.shown {
                    RELEASE_CONFIRM_LOOKS
                } else {
                    MESH_POLL_FAILURE_LOOKS
                };
                (self.released >= needed).then(|| {
                    if self.shown {
                        format!(
                            "the peer answers but no longer holds it ({} looks in a row) — it \
                             was dropped on the peer's side, as when its LeanZero Link restarts",
                            self.released
                        )
                    } else {
                        format!(
                            "the peer answered {} looks in a row without ever receiving it",
                            self.released
                        )
                    }
                })
            }
        }
    }
}

/// Look at the request in flight every `interval` until [`InFlightWatch`] ends it; resolves
/// with the evidence. Dropped (never resolved) when the request ends first.
async fn watch_in_flight(
    client: reqwest::Client,
    url: String,
    token: String,
    interval: Duration,
) -> String {
    let mut watch = InFlightWatch::default();
    loop {
        tokio::time::sleep(interval).await;
        let look = look_once(&client, &url, &token).await;
        if let Some(evidence) = watch.observe(look) {
            return evidence;
        }
    }
}

async fn look_once(client: &reqwest::Client, url: &str, token: &str) -> Look {
    let response = match client.get(url).bearer_auth(token).send().await {
        Ok(response) => response,
        Err(err) => return Look::Unreached(err.to_string()),
    };
    if !response.status().is_success() {
        return Look::Reached;
    }
    match response.json::<StreamLiveness>().await {
        Ok(StreamLiveness { epoch, live: true }) => Look::Held { epoch },
        Ok(StreamLiveness { epoch, live: false }) => Look::Released { epoch },
        Err(err) if err.is_timeout() => Look::Unreached(err.to_string()),
        Err(_) => Look::Reached,
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

    fn held(epoch: &str) -> Look {
        Look::Held {
            epoch: epoch.to_string(),
        }
    }
    fn released(epoch: &str) -> Look {
        Look::Released {
            epoch: epoch.to_string(),
        }
    }
    fn unreached() -> Look {
        Look::Unreached("operation timed out".to_string())
    }

    /// Feed looks in order; the index of the look that ended the request, with its evidence.
    fn verdict(looks: Vec<Look>) -> Option<(usize, String)> {
        let mut watch = InFlightWatch::default();
        looks
            .into_iter()
            .enumerate()
            .find_map(|(i, look)| watch.observe(look).map(|why| (i, why)))
    }

    /// r3-1.log with an unfixed peer: its tailscaled was killed and stayed dead; every fresh
    /// dial goes unanswered (the later requests' `502 … error sending request` at 5.00 s).
    #[test]
    fn a_peer_that_stays_unreachable_ends_the_request_at_the_look_count() {
        let n = MESH_POLL_FAILURE_LOOKS as usize;
        let mut looks = vec![held("e1")];
        looks.extend((0..n).map(|_| unreached()));
        let (at, why) = verdict(looks).expect("ended");
        assert_eq!(at, n, "the Nth unreached look in a row, not before");
        assert!(why.contains("could not reach the peer"), "{why}");
        assert!(why.contains("operation timed out"), "{why}");
    }

    /// r3-1.log with the Q-31 peer: killed, restarted by its supervisor under a new epoch.
    #[test]
    fn a_peer_restarted_under_the_request_ends_it_on_the_confirming_look() {
        let (at, why) = verdict(vec![
            held("e1"),
            unreached(),
            unreached(),
            released("e2"),
            released("e2"),
        ])
        .expect("ended");
        assert_eq!(at, 4);
        assert!(why.contains("no longer holds it"), "{why}");
        // Killed before the first look saw it held: the unreachable looks are the evidence.
        let (at, _) = verdict(vec![unreached(), released("e2"), released("e2")]).expect("ended");
        assert_eq!(at, 2);
    }

    /// The negative control: a slow generation is held at every look, forever.
    #[test]
    fn a_held_request_is_never_ended_however_long_it_is_silent() {
        assert!(verdict((0..10_000).map(|_| held("e1")).collect()).is_none());
        // An older peer (no look route) that stays reachable is never ended either.
        assert!(verdict((0..10_000).map(|_| Look::Reached).collect()).is_none());
    }

    /// One release is not enough: a response completing with its last bytes still on the
    /// mesh reads `live: false` for a moment, and the relay's own end drops the watch.
    #[test]
    fn a_single_release_after_holding_is_not_a_verdict() {
        assert!(verdict(vec![held("e1"), released("e1"), held("e1")]).is_none());
    }

    /// Unreachable looks broken by an answer start over (a transient dial failure).
    #[test]
    fn an_answer_resets_the_unreachable_count() {
        let n = MESH_POLL_FAILURE_LOOKS as usize;
        let mut looks: Vec<Look> = (0..n - 1).map(|_| unreached()).collect();
        looks.push(held("e1"));
        looks.extend((0..n - 1).map(|_| unreached()));
        looks.push(Look::Reached);
        looks.extend((0..n - 1).map(|_| unreached()));
        assert!(verdict(looks).is_none());
    }

    /// A request the reachable peer never received (sent down a kept-alive connection that
    /// was already dead) ends at the look count, not at the first `live: false` — before
    /// the first look the request may simply not have arrived yet.
    #[test]
    fn a_request_never_received_ends_at_the_look_count() {
        let n = MESH_POLL_FAILURE_LOOKS as usize;
        let (at, why) = verdict((0..n).map(|_| released("e1")).collect()).expect("ended");
        assert_eq!(at, n - 1);
        assert!(why.contains("without ever receiving it"), "{why}");
    }

    #[test]
    fn only_a_relay_minted_id_is_held_and_the_hold_ends_with_its_guard() {
        let streams = Arc::new(InflightStreams::new());
        let id = crate::replica::hex(&[7u8; 16]);
        assert!(!streams.liveness(&id).live);
        let hold = streams.hold(&id).expect("a relay id");
        assert!(streams.liveness(&id).live);
        drop(hold);
        assert!(!streams.liveness(&id).live);
        for foreign in ["", "../../etc", &"A".repeat(32), &"0".repeat(33)] {
            assert!(streams.hold(foreign).is_none(), "{foreign}");
        }
        assert_ne!(
            streams.liveness(&id).epoch,
            InflightStreams::new().liveness(&id).epoch,
            "each control-service start answers under its own epoch"
        );
    }

    #[test]
    fn the_refusal_names_the_switch_and_the_node() {
        let text = chat_serving_disabled("WorksMacStudio.lan");
        assert!(text.starts_with("chatServingDisabled: "), "{text}");
        assert!(text.contains("Let my other Macs use this Mac › Answer chat"));
        assert!(text.ends_with("off on WorksMacStudio.lan"));
    }
}
