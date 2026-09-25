//! REMOTE SINGLE over LeanZero Link — both sides of "run the single MLX engine on another Mac":
//!
//! - SERVING ([`GoosedChatServing`]): THIS Mac's single engine answers a same-account device's
//!   chat through the control service's inference proxy (`/v1/swarm/inference/v1/*`), behind the
//!   owner's switch [`ALLOW_CHAT_SERVING_KEY`] — off by default, read on every request; off →
//!   `403 chatServingDisabled`. The proxy never mounts: nothing listening is its `502`.
//! - REQUESTING (`mlxEngine/remoteSingle{Start,Stop,Status}`): mount a model on a peer through the
//!   existing `/v1/swarm/mlx/mount` op (the peer's memory gate decides), start this goosed's
//!   loopback relay to the peer's proxy, and publish the route ([`mlx_remote`]) so every window's
//!   swarm router serves MLX chat from the peer.
//! - RESTORING (Q-34): while this goosed owns a route and the peer answers over Link with its
//!   engine `stopped` and `stoppedBy: notStarted` (its goose relaunched — quitting the app stops
//!   its sidecar), this goosed re-mounts the route's model there through the same start path
//!   Run takes ([`mount_on_peer`]), once per outage, reported as `restore` on the status. The
//!   trigger is the peer's reported state on a status read, never a clock.

use super::*;
use crate::config::ConfigError;
use crate::providers::mlx_remote::{self, PublishedRoute, RouteRecord};
use crate::providers::mlx_serving_intent::{IntentKind, ServingIntent};
use goose_sidecar::engine::{served_model_id, EngineSettings};
use leanzero_link::inference::{
    InferenceRelay, PeerCallResolver, ENGINE_UNREACHABLE, RELAY_FAILED,
};
use leanzero_link::manager::{AuthState, LinkError, LinkManager};
use leanzero_link::state::{ChatServing, MlxOp};
use leanzero_link::wire::NodeStatus;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Mutex as StdMutex;
use tokio::sync::Mutex as TokioMutex;

/// The owner's switch ("Let my other Macs use this Mac › Answer chat"): a goose config key, or
/// the same name as an env override (`Config::get_param` upper-cases the key). Absent = OFF.
pub const ALLOW_CHAT_SERVING_KEY: &str = "LEANZERO_LINK_ALLOW_CHAT_SERVING";

const MLX_ENGINE_CONFIG_KEY: &str = "mlx_engine";

/// Read the switch. `NotFound` is the documented default (OFF); an unreadable value is logged and
/// read as OFF — fail closed, never silently open.
pub(super) fn chat_serving_allowed() -> bool {
    match Config::global().get_param::<bool>(ALLOW_CHAT_SERVING_KEY) {
        Ok(value) => value,
        Err(ConfigError::NotFound(_)) => false,
        Err(error) => {
            warn!(
                %error,
                key = ALLOW_CHAT_SERVING_KEY,
                "leanzeroLink: the chat-serving setting is unreadable; treating it as OFF"
            );
            false
        }
    }
}

/// Where THIS Mac's single engine listens: the configured `mlx_engine.port` on loopback (the
/// manager binds exactly there), the default port when no block was written. An unreadable block
/// is a named reason — pointing the proxy at the default port would impersonate a configuration
/// the owner wrote and we could not read.
fn engine_base_url_from(settings: Result<EngineSettings, ConfigError>) -> Result<String, String> {
    let port = match settings {
        Ok(settings) => settings.port,
        Err(ConfigError::NotFound(_)) => EngineSettings::default().port,
        Err(e) => {
            return Err(format!(
                "the `{MLX_ENGINE_CONFIG_KEY}` config block is unreadable ({e}); the engine's port is unknown"
            ))
        }
    };
    Ok(format!("http://127.0.0.1:{port}"))
}

/// THIS Mac's single engine, served to linked devices (the control route's seam).
pub struct GoosedChatServing;

impl ChatServing for GoosedChatServing {
    fn serving_allowed(&self) -> bool {
        chat_serving_allowed()
    }

    fn engine_base_url(&self) -> Result<String, String> {
        engine_base_url_from(Config::global().get_param::<EngineSettings>(MLX_ENGINE_CONFIG_KEY))
    }
}

// ---------------------------------------------------------------------------------------------
// The requester side.
// ---------------------------------------------------------------------------------------------

fn refusal(code: &str, message: impl Into<String>) -> MlxRemoteSingleRefusalDto {
    MlxRemoteSingleRefusalDto {
        code: code.to_string(),
        message: message.into(),
    }
}

fn off_status() -> MlxRemoteSingleStatusDto {
    MlxRemoteSingleStatusDto {
        state: "off".to_string(),
        ..Default::default()
    }
}

/// What a GET through the relay answered.
enum RelayAnswer {
    Ok(String),
    Status { code: u16, body: String },
    NoAnswer(String),
}

async fn relay_get(base_url: &str, path: &str) -> RelayAnswer {
    let client = match reqwest::Client::builder().no_proxy().build() {
        Ok(client) => client,
        Err(e) => return RelayAnswer::NoAnswer(format!("building the relay client: {e}")),
    };
    let response = match client.get(format!("{base_url}/{path}")).send().await {
        Ok(response) => response,
        Err(e) => {
            return RelayAnswer::NoAnswer(format!("this Mac's Link relay did not answer ({e})"))
        }
    };
    let code = response.status().as_u16();
    match response.text().await {
        Ok(body) if (200..300).contains(&code) => RelayAnswer::Ok(body),
        Ok(body) => RelayAnswer::Status {
            code,
            body: body.trim().to_string(),
        },
        Err(e) => RelayAnswer::NoAnswer(format!("{path} body unreadable ({e})")),
    }
}

/// The Metal memory a linked Mac's engine holds, from its own `/v1/status` through the relay:
/// `metal.active_memory_gb + metal.cache_memory_gb` — what that Mac gets back when the engine
/// exits (both are the process's buffers). Rapid-MLX reports them in decimal GB
/// (`mx.get_active_memory() / 1e9`, scheduler.rs `metal_active_memory_gb`).
pub(super) async fn peer_engine_held_bytes(base_url: &str) -> Result<u64, String> {
    match relay_get(base_url, "v1/status").await {
        RelayAnswer::Ok(body) => metal_held_bytes(&body),
        RelayAnswer::Status { code, body } => Err(format!("{code}: {body}")),
        RelayAnswer::NoAnswer(why) => Err(why),
    }
}

fn metal_held_bytes(body: &str) -> Result<u64, String> {
    let status: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("the engine's status is not JSON ({e})"))?;
    let metal = status
        .get("metal")
        .ok_or("the engine's status carries no `metal` block")?;
    let gb = |key: &str| {
        metal
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| format!("the engine's status carries no metal.{key}"))
    };
    Ok(((gb("active_memory_gb")? + gb("cache_memory_gb")?) * 1e9) as u64)
}

/// The peer's answer to "may I route chat to you?" read BEFORE a model is loaded there — so a
/// peer whose owner has not opted in costs no mount. `None` = go ahead (its engine may simply not
/// be mounted yet: the proxy's `502 engineUnreachable`).
fn serving_refusal(answer: &RelayAnswer) -> Option<MlxRemoteSingleRefusalDto> {
    match answer {
        RelayAnswer::Ok(_) => None,
        RelayAnswer::Status { code: 502, body } if body.starts_with(ENGINE_UNREACHABLE) => None,
        RelayAnswer::Status { code: 403, body } => Some(refusal("chatServingDisabled", body.clone())),
        RelayAnswer::Status {
            code: 404 | 501,
            body,
        } => Some(refusal(
            "peerTooOld",
            format!(
                "its goose does not serve chat to linked devices (it answered: {}); update goose there",
                if body.is_empty() { "not found" } else { body }
            ),
        )),
        RelayAnswer::Status { code, body } => Some(refusal(
            "peerUnreachable",
            format!("the peer's chat proxy answered {code}: {body}"),
        )),
        RelayAnswer::NoAnswer(why) => Some(refusal("peerUnreachable", why.clone())),
    }
}

fn link_refusal(error: &LinkError) -> MlxRemoteSingleRefusalDto {
    match error {
        LinkError::NotConnected => refusal(
            "linkNotConnected",
            "this Mac is not connected to the LeanZero Link mesh",
        ),
        LinkError::UnknownPeer(_) => refusal("unknownPeer", error.to_string()),
        LinkError::MlxProxy(text) if text.starts_with("peer returned 403") => {
            refusal("remoteManagementDisabled", text.clone())
        }
        LinkError::MlxProxy(text) if text.starts_with("peer returned 501") => refusal(
            "peerTooOld",
            format!("its goose does not manage models for linked devices ({text})"),
        ),
        LinkError::MlxControl(inner) => refusal("peerMountFailed", inner.to_string()),
        other => refusal("peerUnreachable", other.to_string()),
    }
}

/// Whether a Link op's error means the peer never answered — this Mac is off the mesh, the mesh
/// does not list the peer, or the dial or the send through the mesh failed — rather than the
/// peer answering with a refusal (`peer returned <code>`, an undecodable body, a classed
/// `MlxControl` answer).
fn peer_did_not_answer(error: &LinkError) -> bool {
    match error {
        LinkError::NotConnected | LinkError::UnknownPeer(_) | LinkError::PeerDial(_) => true,
        LinkError::MlxProxy(text) => {
            !text.starts_with("peer returned") && !text.starts_with("peer responded")
        }
        _ => false,
    }
}

/// The engine ops a route makes on its peer over LeanZero Link (the mesh's `mlx_proxy`), and the
/// mesh fabric's own view of that peer — a seam so the restore runs against a stand-in peer in
/// tests.
#[async_trait::async_trait]
pub(super) trait PeerControl: Send + Sync {
    async fn mlx_op(
        &self,
        peer: &str,
        op: MlxOp,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, LinkError>;

    /// The fabric's verdict on `peer` from its own polls — no dial: the words while it cannot
    /// reach the peer (or this Mac is off the mesh), `None` while it reaches it.
    async fn unreachable(&self, peer: &str) -> Option<String>;

    /// Resolves with [`Self::unreachable`]'s words the first time the fabric cannot reach `peer`.
    async fn lost(&self, peer: &str) -> String;
}

#[async_trait::async_trait]
impl PeerControl for LinkManager {
    async fn mlx_op(
        &self,
        peer: &str,
        op: MlxOp,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, LinkError> {
        self.mlx_proxy(peer, op, body).await
    }

    async fn unreachable(&self, peer: &str) -> Option<String> {
        let Some(registry) = self.active_registry().await else {
            return Some(link_refusal(&LinkError::NotConnected).message);
        };
        let node = registry
            .peer_nodes()
            .into_iter()
            .find(|node| node.node_id == peer || node.hostname == peer);
        match node {
            None => Some(link_refusal(&LinkError::UnknownPeer(peer.to_string())).message),
            Some(node) if node.status == NodeStatus::Offline => Some(match node.last_poll_error {
                Some(error) => format!("the LeanZero Link mesh cannot reach it ({error})"),
                None => "the LeanZero Link mesh has not reached it since it listed it".to_string(),
            }),
            Some(_) => None,
        }
    }

    /// The fabric's view changes only when one of its polls lands, so it is looked at on the
    /// fabric's own poll cadence (`liveness_interval`, the cadence the relay's in-flight watch
    /// looks on) — never a clock of this file's.
    async fn lost(&self, peer: &str) -> String {
        loop {
            if let Some(words) = self.unreachable(peer).await {
                return words;
            }
            match self.peer_call(peer).await {
                Ok(call) => tokio::time::sleep(call.liveness_interval).await,
                Err(error) => return link_refusal(&error).message,
            }
        }
    }
}

/// A Link op's refusal, and whether the peer answered at all ([`peer_did_not_answer`]).
struct PeerRefusal {
    refusal: MlxRemoteSingleRefusalDto,
    no_answer: bool,
}

async fn peer_op_answer<Req: Serialize, Resp: DeserializeOwned>(
    control: &dyn PeerControl,
    peer: &str,
    op: MlxOp,
    req: &Req,
) -> Result<Resp, PeerRefusal> {
    let answered = |refusal| PeerRefusal {
        refusal,
        no_answer: false,
    };
    let body = serde_json::to_value(req)
        .map_err(|e| answered(refusal("peerUnreachable", e.to_string())))?;
    let value = control
        .mlx_op(peer, op, body)
        .await
        .map_err(|e| PeerRefusal {
            refusal: link_refusal(&e),
            no_answer: peer_did_not_answer(&e),
        })?;
    serde_json::from_value(value).map_err(|e| {
        answered(refusal(
            "peerTooOld",
            format!("the peer's {} answer did not decode: {e}", op.path()),
        ))
    })
}

async fn peer_op<Req: Serialize, Resp: DeserializeOwned>(
    control: &dyn PeerControl,
    peer: &str,
    op: MlxOp,
    req: &Req,
) -> Result<Resp, MlxRemoteSingleRefusalDto> {
    peer_op_answer(control, peer, op, req)
        .await
        .map_err(|refused| refused.refusal)
}

/// The peer engine's `/v1/models` through the relay: its context window when it serves the route's
/// model, else why not in words.
async fn route_models(route: &PublishedRoute) -> Result<Option<u64>, String> {
    served_window(route, relay_get(&route.base_url, "v1/models").await)
}

fn served_window(route: &PublishedRoute, answer: RelayAnswer) -> Result<Option<u64>, String> {
    match answer {
        RelayAnswer::Ok(body) => match goose_sidecar::engine::parse_model_info(&body) {
            Ok((Some(served), window, _)) if served == route.served_model_id => Ok(window),
            Ok((served, _, _)) => Err(format!(
                "{} now serves {:?}, the route wants '{}'",
                route.peer_name(),
                served,
                route.served_model_id
            )),
            Err(e) => Err(format!("{e:#}")),
        },
        RelayAnswer::Status { code, body } => Err(format!("{code}: {body}")),
        RelayAnswer::NoAnswer(why) => Err(why),
    }
}

/// The relay's own `502` for a peer it could not reach at all — as opposed to the peer's proxy
/// answering that its engine does not (`engineUnreachable`).
fn relay_lost_the_peer(answer: &RelayAnswer) -> bool {
    matches!(answer, RelayAnswer::Status { code: 502, body } if body.starts_with(RELAY_FAILED))
}

/// A status read: the route's status and, when the proxy did not serve, the peer's own engine
/// status as it answered over Link (`None` when it did not answer, or was not asked).
struct RouteObservation {
    status: MlxRemoteSingleStatusDto,
    peer_engine: Option<MlxEngineStatusDto>,
}

async fn route_status(
    control: Option<&dyn PeerControl>,
    route: &PublishedRoute,
) -> MlxRemoteSingleStatusDto {
    observe_route(control, route).await.status
}

fn route_facts(route: &PublishedRoute) -> MlxRemoteSingleStatusDto {
    MlxRemoteSingleStatusDto {
        state: "mounting".to_string(),
        peer: Some(route.peer.clone()),
        peer_hostname: Some(route.peer_hostname.clone()),
        peer_computer_name: route.peer_computer_name.clone(),
        base_url: Some(route.base_url.clone()),
        model_id: Some(route.model_id.clone()),
        served_model_id: Some(route.served_model_id.clone()),
        capacity: Some(route.capacity),
        ..Default::default()
    }
}

/// `reconnecting`: the route is published and its Mac does not answer right now — LeanZero Link
/// cannot reach it, so nothing about its engine is known and nothing is asked of it.
fn reconnecting(route: &PublishedRoute, why: String) -> RouteObservation {
    let words = format!(
        "{} does not answer over LeanZero Link right now: {why}",
        route.peer_name()
    );
    RouteObservation {
        status: MlxRemoteSingleStatusDto {
            state: "reconnecting".to_string(),
            active_requests_error: Some(words.clone()),
            last_error: Some(words),
            ..route_facts(route)
        },
        peer_engine: None,
    }
}

/// The route's state. The mesh fabric's own view of the peer is read FIRST — it costs no dial —
/// and a peer it cannot reach is `reconnecting` at once: every probe through the mesh to a dead
/// peer waits on the transport (a fresh dial is held 5.0 s by tailscaled's SOCKS listener, and
/// the relay's kept-alive connection to it stays silent until the relay's in-flight watch gives
/// up). Otherwise the route is re-probed through the relay, and the probes race the fabric
/// losing the peer, so a read that started just before the peer went away answers when the
/// fabric notices instead of when the transport does.
async fn observe_route(
    control: Option<&dyn PeerControl>,
    route: &PublishedRoute,
) -> RouteObservation {
    let Some(control) = control else {
        return probe_route(None, route).await;
    };
    if let Some(why) = control.unreachable(&route.peer).await {
        return reconnecting(route, why);
    }
    tokio::select! {
        biased;
        observed = probe_route(Some(control), route) => observed,
        why = control.lost(&route.peer) => reconnecting(route, why),
    }
}

/// The route re-probed through the relay, and the peer's own engine status when the relay says
/// it is not serving yet (mounting vs failed vs not answering at all).
async fn probe_route(
    control: Option<&dyn PeerControl>,
    route: &PublishedRoute,
) -> RouteObservation {
    let mut status = route_facts(route);
    let (models, engine_status) = tokio::join!(
        relay_get(&route.base_url, "v1/models"),
        relay_get(&route.base_url, "v1/status")
    );
    let relay_lost_peer = relay_lost_the_peer(&models);
    let models_error = match served_window(route, models) {
        Ok(window) => {
            status.state = "ready".to_string();
            status.context_window = window;
            None
        }
        Err(why) => Some(why),
    };
    match engine_status {
        RelayAnswer::Ok(body) => {
            match goose_sidecar::engine::parse_active_requests(&body) {
                Ok(n) => status.active_requests = Some(n),
                Err(e) => status.active_requests_error = Some(format!("{e:#}")),
            }
            status.generation_tps = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("generation_tps").and_then(serde_json::Value::as_f64));
        }
        RelayAnswer::Status { code, body } => {
            status.active_requests_error = Some(format!("{code}: {body}"))
        }
        RelayAnswer::NoAnswer(why) => status.active_requests_error = Some(why),
    }
    let Some(models_error) = models_error else {
        return RouteObservation {
            status,
            peer_engine: None,
        };
    };
    // Not serving through the proxy: the peer's own engine state says loading or failed.
    let peer_state = match control {
        Some(control) => {
            peer_op_answer::<_, MlxEngineStatusResponse>(
                control,
                &route.peer,
                MlxOp::Status,
                &MlxEngineStatusRequest {
                    node_id: None,
                    fit_model_id: None,
                },
            )
            .await
        }
        None => Err(PeerRefusal {
            refusal: refusal(
                "linkNotConnected",
                "LeanZero Link has not started in this goose",
            ),
            // This goose cannot ask the peer; the relay's own 502 says whether it reached it.
            no_answer: relay_lost_peer,
        }),
    };
    let mut peer_engine = None;
    match peer_state {
        Ok(peer) if peer.status.state == "mounting" => peer_engine = Some(peer.status),
        Ok(peer) => {
            // The proxy was asked BEFORE the peer answered. An engine that became ready between
            // the two reads says `running` here and failed there — measured on the 3.0.31
            // restore: the Studio's engine was ready at 06:22:53.638, between the proxy's 502 and
            // the status op at 06:22:53.655, and the restore gave up on an engine that served.
            // The proxy is asked again now that the peer says running; only its answer after
            // that decides.
            let proxy_answer = if peer.status.state == "running" {
                route_models(route).await
            } else {
                Err(models_error)
            };
            match proxy_answer {
                Ok(window) => {
                    status.state = "ready".to_string();
                    status.context_window = window;
                }
                Err(proxy_answer) => {
                    status.state = "failed".to_string();
                    status.last_error = Some(peer_engine_failure(
                        route.peer_name(),
                        &peer.status,
                        &proxy_answer,
                    ));
                }
            }
            peer_engine = Some(peer.status);
        }
        Err(refused) => {
            status.state = if refused.no_answer {
                "reconnecting"
            } else {
                "failed"
            }
            .to_string();
            status.last_error = Some(format!("{models_error}; {}", refused.refusal.message));
        }
    }
    RouteObservation {
        status,
        peer_engine,
    }
}

/// Why the peer's engine does not serve through the proxy, in the peer's own words first (a dead
/// engine is its `failed` with the exit and last log lines), then the proxy's answer — a peer
/// goose older than the liveness fix still says `running` for a dead engine.
fn peer_engine_failure(peer: &str, engine: &MlxEngineStatusDto, proxy_answer: &str) -> String {
    let own_words = engine
        .last_error
        .as_deref()
        .or(engine.probe_error.as_deref())
        .map(|err| format!(" ({err})"))
        .unwrap_or_default();
    format!(
        "{peer}'s goose reports its engine {}{own_words}, and it does not serve through LeanZero Link: {proxy_answer}",
        engine.state
    )
}

/// The route as every surface reads it. Only the goosed that owns the route restores it; another
/// window's goosed reports what it sees.
async fn current_status() -> MlxRemoteSingleStatusDto {
    let manager = super::link::existing_link_manager();
    match mlx_remote::read() {
        RouteRecord::Mine(route) => {
            owned_route_status(
                &ROUTE_RESTORE,
                manager.map(|m| m as Arc<dyn PeerControl>),
                &route,
                routed_here(),
            )
            .await
        }
        RouteRecord::Other(route) => {
            route_status(manager.as_deref().map(|m| m as &dyn PeerControl), &route).await
        }
        RouteRecord::Unreadable { path, error } => MlxRemoteSingleStatusDto {
            state: "failed".to_string(),
            last_error: Some(format!(
                "the remote-single route record {} is unreadable ({error})",
                path.display()
            )),
            ..Default::default()
        },
        RouteRecord::Absent | RouteRecord::Stale(_) => off_status(),
    }
}

// ---------------------------------------------------------------------------------------------
// The start path, shared by Run and the route's own restore.
// ---------------------------------------------------------------------------------------------

/// What a start (or a restore) does once it has read the peer's engine.
enum PeerMountStep {
    Mount,
    /// The engine already serves, or is loading, the route's model: nothing to mount.
    Keep,
    Refuse(MlxRemoteSingleRefusalDto),
}

/// What the peer said on the way: what a route publishes.
struct PeerMount {
    capacity: u32,
    served: String,
    template_kwargs: Option<serde_json::Map<String, serde_json::Value>>,
    mounted: bool,
}

/// The serving precheck through the relay (a peer whose owner has not opted in costs no mount),
/// the peer's engine status and saved settings, `decide` on those facts (the engine's status, the
/// served id, whether the proxy answered), then the peer's Mount — its memory gate decides.
async fn mount_on_peer(
    control: &dyn PeerControl,
    relay_base: &str,
    peer: &str,
    peer_name: &str,
    model_id: &str,
    decide: impl FnOnce(&MlxEngineStatusDto, &str, bool) -> PeerMountStep,
) -> Result<PeerMount, MlxRemoteSingleRefusalDto> {
    let precheck = relay_get(relay_base, "v1/status").await;
    if let Some(refused) = serving_refusal(&precheck) {
        return Err(refused);
    }
    let engine_answers = matches!(precheck, RelayAnswer::Ok(_));

    let peer_status: MlxEngineStatusResponse = peer_op(
        control,
        peer,
        MlxOp::Status,
        &MlxEngineStatusRequest {
            node_id: None,
            fit_model_id: None,
        },
    )
    .await?;
    let capacity = peer_status.status.max_concurrent_requests.ok_or_else(|| {
        refusal(
            "peerTooOld",
            format!("{peer_name}'s goose does not report its admission cap (maxConcurrentRequests); update goose there"),
        )
    })?;
    let peer_settings: MlxEngineSettingsResponse = peer_op(
        control,
        peer,
        MlxOp::SettingsRead,
        &MlxEngineSettingsReadRequest { node_id: None },
    )
    .await?;
    let peer_settings = super::mlx_engine::settings_from_dto(peer_settings.settings);
    let served = served_model_id(&peer_settings, model_id);
    let template_kwargs = peer_settings
        .model_profiles
        .get(model_id)
        .and_then(goose_sidecar::thinking::chat_template_kwargs);

    let mounted = match decide(&peer_status.status, &served, engine_answers) {
        PeerMountStep::Keep => false,
        PeerMountStep::Refuse(refused) => return Err(refused),
        PeerMountStep::Mount => {
            let mounted: MlxEngineMountResponse = peer_op(
                control,
                peer,
                MlxOp::Mount,
                &MlxEngineMountRequest {
                    model_id: model_id.to_string(),
                    node_id: None,
                },
            )
            .await?;
            if let Some(refused) = mounted.refusal {
                return Err(refusal(
                    "peerMountFailed",
                    format!(
                        "{peer_name}'s memory gate refused '{model_id}': {}",
                        refused.fit.message
                    ),
                ));
            }
            true
        }
    };
    Ok(PeerMount {
        capacity,
        served,
        template_kwargs,
        mounted,
    })
}

// ---------------------------------------------------------------------------------------------
// The route's own restore (Q-34).
// ---------------------------------------------------------------------------------------------

/// What the peer's reported engine says a restore may do. Decided from the peer's own facts only:
/// `stoppedBy` (who stopped the engine since its goose started), `servingIntent` (what its owner
/// serves there at its launch), `hosting`, and the model it runs.
#[derive(Debug, PartialEq)]
enum RestoreStep {
    /// Stopped and nobody stopped it — its goose relaunched: mount the route's model again.
    Mount,
    /// The route's own model is loading or running there.
    Keep,
    /// Not this goosed's to start, in words; `None` = not a stopped engine at all (it failed with
    /// its own reason, which the status already carries).
    Decline(Option<String>),
}

fn restore_step(route: &PublishedRoute, engine: &MlxEngineStatusDto) -> RestoreStep {
    let peer = route.peer_name();
    let model = &route.model_id;
    if engine.hosting.is_some() {
        return RestoreStep::Decline(Some(format!(
            "{peer} serves a rank of a split across Macs now, so this Mac does not mount '{model}' there"
        )));
    }
    match engine.state.as_str() {
        "mounting" | "running" if engine.model_id.as_deref() == Some(model.as_str()) => {
            RestoreStep::Keep
        }
        "mounting" | "running" => RestoreStep::Decline(Some(format!(
            "{peer}'s engine runs '{}' now, started there after this route's '{model}'; this Mac does not mount over it — Run it again to take {peer} back",
            engine.model_id.as_deref().unwrap_or("a model it does not name")
        ))),
        "stopped" => match engine.stopped_by.as_deref() {
            Some("notStarted") => match &engine.serving_intent {
                Some(intent) if intent.kind == "single" && intent.model_id != *model => {
                    RestoreStep::Decline(Some(format!(
                        "{peer}'s owner serves '{}' there and its own launch brings it back; this Mac does not mount '{model}' over it — Run it again to take {peer} back",
                        intent.model_id
                    )))
                }
                Some(intent) if intent.kind == "split" => RestoreStep::Decline(Some(format!(
                    "{peer}'s owner runs a split ('{}') from there and its own launch brings it back; this Mac does not mount '{model}' over it",
                    intent.model_id
                ))),
                _ => RestoreStep::Mount,
            },
            Some("owner") => RestoreStep::Decline(Some(format!(
                "{peer}'s owner stopped its engine there, so this Mac does not start it again — Run it again to bring '{model}' back"
            ))),
            Some("linkedMac") => RestoreStep::Decline(Some(format!(
                "a linked Mac stopped {peer}'s engine over LeanZero Link, so this Mac does not start it again — Run it again to bring '{model}' back"
            ))),
            Some(other) => RestoreStep::Decline(Some(format!(
                "{peer}'s goose says its engine was stopped by '{other}', which this goose does not know; not restoring '{model}'"
            ))),
            None => RestoreStep::Decline(Some(format!(
                "{peer}'s goose does not say what stopped its engine (it predates stoppedBy), so this Mac cannot tell its relaunch from its owner's Stop and does not mount over it — update goose there, or Run it again"
            ))),
        },
        _ => RestoreStep::Decline(None),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum RestorePhase {
    /// The restore holds the route's op lock and is on its way through [`mount_on_peer`].
    Mounting,
    /// The peer accepted the Mount; the engine loads there.
    Mounted,
    /// The words that ended it. Not tried again until the route serves or the owner runs it.
    Failed(String),
}

/// Where the route's restore stands, keyed by the route's relay base URL (a Run is a fresh relay,
/// so a fresh key), and the lock Start, Stop and a restore's mount take so a Stop never lands
/// between a restore's "the route is still up" and its Mount.
pub(super) struct RouteRestore {
    phase: StdMutex<Option<(String, RestorePhase)>>,
    ops: TokioMutex<()>,
}

impl RouteRestore {
    const fn new() -> Self {
        Self {
            phase: StdMutex::new(None),
            ops: TokioMutex::const_new(()),
        }
    }

    fn entry(&self) -> std::sync::MutexGuard<'_, Option<(String, RestorePhase)>> {
        self.phase.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn phase(&self, route: &PublishedRoute) -> Option<RestorePhase> {
        match &*self.entry() {
            Some((key, phase)) if *key == route.base_url => Some(phase.clone()),
            _ => None,
        }
    }

    /// Arm-and-take in one step: true for exactly one caller per outage.
    fn claim(&self, route: &PublishedRoute) -> bool {
        let mut entry = self.entry();
        if matches!(&*entry, Some((key, _)) if *key == route.base_url) {
            return false;
        }
        *entry = Some((route.base_url.clone(), RestorePhase::Mounting));
        true
    }

    /// Moves this route's restore on; a route served (re-armed) or replaced meanwhile is left be.
    fn advance(&self, route: &PublishedRoute, phase: RestorePhase) {
        let mut entry = self.entry();
        if let Some((key, current)) = &mut *entry {
            if *key == route.base_url {
                *current = phase;
            }
        }
    }

    fn rearm(&self, route: &PublishedRoute) {
        let mut entry = self.entry();
        if matches!(&*entry, Some((key, _)) if *key == route.base_url) {
            *entry = None;
        }
    }

    fn forget(&self) {
        *self.entry() = None;
    }
}

static ROUTE_RESTORE: RouteRestore = RouteRestore::new();

/// Is `route` still this goosed's route — read at the moment a restore would mount.
type RouteCheck = Arc<dyn Fn(&PublishedRoute) -> bool + Send + Sync>;

fn routed_here() -> RouteCheck {
    Arc::new(
        |route: &PublishedRoute| matches!(mlx_remote::read(), RouteRecord::Mine(now) if now.base_url == route.base_url),
    )
}

fn model_short_name(model_id: &str) -> &str {
    model_id.rsplit('/').next().unwrap_or(model_id)
}

fn restoring_line(route: &PublishedRoute) -> MlxRemoteSingleRestoreDto {
    MlxRemoteSingleRestoreDto {
        phase: "restoring".to_string(),
        message: format!(
            "Restoring {} on {}…",
            model_short_name(&route.model_id),
            route.peer_name()
        ),
    }
}

fn restore_failure(route: &PublishedRoute, words: &str) -> String {
    format!(
        "Restoring {} on {} failed: {words}",
        model_short_name(&route.model_id),
        route.peer_name()
    )
}

/// The owner's status read, and the restore it may start. The trigger is the peer's answer on
/// this read — engine `stopped`, nothing stopped it ([`restore_step`]) — never a clock; one
/// restore per outage (the route serving again re-arms it), and a failed one stays a named
/// `failed` with the peer's words until the route serves or the owner runs it again. A peer that
/// does not answer at all is `reconnecting`: its engine is unknown, so it starts nothing.
async fn owned_route_status(
    book: &'static RouteRestore,
    control: Option<Arc<dyn PeerControl>>,
    route: &PublishedRoute,
    still_routed: RouteCheck,
) -> MlxRemoteSingleStatusDto {
    let observed = observe_route(control.as_deref(), route).await;
    let mut status = observed.status;
    if status.state == "ready" {
        book.rearm(route);
        return status;
    }
    if status.state == "reconnecting" {
        // Nothing is known of the peer's engine and nothing can be asked of it: no restore
        // starts, and one under way keeps its phase until the peer answers again.
        return status;
    }
    match book.phase(route) {
        None => {
            let (Some(control), Some(engine)) = (control, observed.peer_engine.as_ref()) else {
                return status;
            };
            if status.state != "failed" {
                return status;
            }
            match restore_step(route, engine) {
                RestoreStep::Mount => {
                    if book.claim(route) {
                        tokio::spawn(restore_route(book, control, route.clone(), still_routed));
                    }
                    status.state = "mounting".to_string();
                    status.last_error = None;
                    status.restore = Some(restoring_line(route));
                }
                RestoreStep::Decline(Some(why)) => {
                    status.last_error = Some(match status.last_error.take() {
                        Some(observed) => format!("{why}. {observed}"),
                        None => why,
                    });
                }
                RestoreStep::Keep | RestoreStep::Decline(None) => {}
            }
        }
        Some(RestorePhase::Mounting) => {
            status.state = "mounting".to_string();
            status.last_error = None;
            status.restore = Some(restoring_line(route));
        }
        Some(RestorePhase::Mounted) if status.state == "failed" => {
            let words = status
                .last_error
                .take()
                .unwrap_or_else(|| format!("{}'s engine did not come up", route.peer_name()));
            let message = restore_failure(route, &words);
            book.advance(route, RestorePhase::Failed(message.clone()));
            status.last_error = Some(message.clone());
            status.restore = Some(MlxRemoteSingleRestoreDto {
                phase: "failed".to_string(),
                message,
            });
        }
        Some(RestorePhase::Mounted) => status.restore = Some(restoring_line(route)),
        Some(RestorePhase::Failed(message)) => {
            if status.state == "failed" {
                status.last_error = Some(message.clone());
                status.restore = Some(MlxRemoteSingleRestoreDto {
                    phase: "failed".to_string(),
                    message,
                });
            }
        }
    }
    status
}

/// One re-mount, through the same path Run takes. Under the route's op lock: a Stop that got
/// there first has withdrawn the route (nothing is mounted), and the peer's engine is read again
/// here, so an engine its owner started meanwhile is kept or declined, never mounted over.
async fn restore_route(
    book: &'static RouteRestore,
    control: Arc<dyn PeerControl>,
    route: PublishedRoute,
    still_routed: RouteCheck,
) {
    let _ops = book.ops.lock().await;
    if !still_routed(&route) {
        book.rearm(&route);
        return;
    }
    let outcome = mount_on_peer(
        control.as_ref(),
        &route.base_url,
        &route.peer,
        route.peer_name(),
        &route.model_id,
        |engine, _, _| match restore_step(&route, engine) {
            RestoreStep::Mount => PeerMountStep::Mount,
            RestoreStep::Keep => PeerMountStep::Keep,
            RestoreStep::Decline(why) => PeerMountStep::Refuse(refusal(
                "restoreDeclined",
                why.unwrap_or_else(|| {
                    format!(
                        "{}'s goose reports its engine {}",
                        route.peer_name(),
                        engine.state
                    )
                }),
            )),
        },
    )
    .await;
    match outcome {
        Ok(mount) => {
            tracing::info!(
                peer = %route.peer_name(),
                model = %route.model_id,
                mounted = mount.mounted,
                "mlx remote single: the peer came back without its engine; the route's model was mounted there again"
            );
            book.advance(&route, RestorePhase::Mounted);
        }
        Err(refused) => {
            let message = restore_failure(&route, &refused.message);
            warn!(code = %refused.code, "{message}");
            book.advance(&route, RestorePhase::Failed(message));
        }
    }
}

static LAST_REMOTE_STATE: StdMutex<String> = StdMutex::new(String::new());

impl GooseAcpAgent {
    async fn connected_link_manager(&self) -> Result<Arc<LinkManager>, MlxRemoteSingleRefusalDto> {
        let manager = self
            .link_manager()
            .await
            .map_err(|e| refusal("linkNotConnected", format!("{e:?}")))?;
        if !matches!(manager.status().await.auth, AuthState::Connected { .. }) {
            return Err(link_refusal(&LinkError::NotConnected));
        }
        Ok(manager)
    }

    async fn remote_single_start(
        &self,
        req: &MlxEngineRemoteSingleStartRequest,
    ) -> Result<MlxRemoteSingleStatusDto, MlxRemoteSingleRefusalDto> {
        let _ops = ROUTE_RESTORE.ops.lock().await;
        match mlx_remote::read() {
            RouteRecord::Mine(route) if route.peer == req.peer && route.model_id == req.model_id => {
                let manager = super::link::existing_link_manager();
                // A restore under way reads `mounting` here: the same placement asked again joins
                // it. A peer that does not answer (`reconnecting`) is kept too: a fresh start
                // would withdraw the route and then fail to reach the same peer.
                let status = owned_route_status(
                    &ROUTE_RESTORE,
                    manager.map(|m| m as Arc<dyn PeerControl>),
                    &route,
                    routed_here(),
                )
                .await;
                if status.state != "failed" {
                    return Ok(status);
                }
                // The same placement asked again after the peer's engine failed: start it over
                // (a fresh relay, a fresh mount) instead of answering "failed" forever.
                mlx_remote::uninstall().map_err(|e| {
                    refusal(
                        "remoteSingleActive",
                        format!("withdrawing the failed route before restarting it: {e:#}"),
                    )
                })?;
            }
            RouteRecord::Other(route) if route.peer == req.peer && route.model_id == req.model_id => {
                let manager = super::link::existing_link_manager();
                return Ok(route_status(
                    manager.as_deref().map(|m| m as &dyn PeerControl),
                    &route,
                )
                .await);
            }
            RouteRecord::Mine(route) => {
                return Err(refusal(
                    "remoteSingleActive",
                    format!(
                        "chat is already served from {} ({}); stop it first",
                        route.peer_name(),
                        route.model_id
                    ),
                ))
            }
            RouteRecord::Other(route) => {
                return Err(refusal(
                    "remoteSingleActive",
                    format!(
                        "another goose window on this Mac (goosed pid {}) routes chat to {} ({}); stop it from that window",
                        route.pid,
                        route.peer_name(),
                        route.model_id
                    ),
                ))
            }
            RouteRecord::Unreadable { path, error } => {
                return Err(refusal(
                    "remoteSingleActive",
                    format!(
                        "the remote-single route record {} is unreadable ({error}); refusing to start a second route",
                        path.display()
                    ),
                ))
            }
            RouteRecord::Absent | RouteRecord::Stale(_) => {}
        }
        if let Some(owner) = distributed_owner() {
            return Err(refusal("distributedOwnsThisMac", owner));
        }

        let manager = self.connected_link_manager().await?;
        let (peer_hostname, peer_computer_name) = peer_names(&manager, &req.peer).await?;
        let peer_name = match peer_computer_name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => peer_hostname.clone(),
        };

        let relay = InferenceRelay::start(
            req.peer.clone(),
            manager.clone() as Arc<dyn PeerCallResolver>,
        )
        .await
        .map_err(|e| {
            refusal(
                "peerUnreachable",
                format!("starting this Mac's Link relay: {e}"),
            )
        })?;
        // A peer engine that died reports `failed` (goose-sidecar observes its process on every
        // poll) and one that hangs does not answer the proxy: either way the Mount runs, and for
        // the same model the peer's supervisor restarts it behind its crash breaker.
        let mount = mount_on_peer(
            manager.as_ref(),
            relay.base_url(),
            &req.peer,
            &peer_name,
            &req.model_id,
            |engine, served, engine_answers| {
                let already_serving = engine_answers
                    && engine.state == "running"
                    && engine.served_model_id.as_deref() == Some(served);
                if already_serving {
                    PeerMountStep::Keep
                } else {
                    PeerMountStep::Mount
                }
            },
        )
        .await?;

        let route = PublishedRoute {
            pid: std::process::id(),
            base_url: relay.base_url().to_string(),
            peer: req.peer.clone(),
            peer_hostname,
            peer_computer_name,
            model_id: req.model_id.clone(),
            served_model_id: mount.served,
            capacity: mount.capacity,
            template_kwargs: mount.template_kwargs,
        };
        mlx_remote::install(relay, route.clone()).map_err(|e| {
            refusal(
                "peerUnreachable",
                format!("publishing the route for this Mac's other windows failed: {e:#}"),
            )
        })?;
        ROUTE_RESTORE.forget();
        super::mlx_engine::align_omlx_host_env();
        tracing::info!(
            peer = %route.peer_name(),
            model = %route.model_id,
            served = %route.served_model_id,
            capacity = route.capacity,
            mounted = mount.mounted,
            "mlx remote single: chat routed to the peer's engine through LeanZero Link"
        );
        Ok(route_status(Some(manager.as_ref()), &route).await)
    }

    pub(super) async fn on_mlx_engine_remote_single_start(
        &self,
        req: MlxEngineRemoteSingleStartRequest,
    ) -> Result<MlxEngineRemoteSingleStartResponse, agent_client_protocol::Error> {
        Ok(match self.remote_single_start(&req).await {
            Ok(status) => {
                super::mlx_engine::remember_serving(ServingIntent::RemoteSingle {
                    peer: req.peer.clone(),
                    peer_name: status
                        .peer_computer_name
                        .clone()
                        .filter(|name| !name.trim().is_empty())
                        .or_else(|| status.peer_hostname.clone())
                        .unwrap_or_else(|| req.peer.clone()),
                    model_id: req.model_id.clone(),
                });
                MlxEngineRemoteSingleStartResponse {
                    started: true,
                    refusal: None,
                    status,
                }
            }
            Err(refused) => MlxEngineRemoteSingleStartResponse {
                started: false,
                refusal: Some(refused),
                status: current_status().await,
            },
        })
    }

    pub(super) async fn on_mlx_engine_remote_single_stop(
        &self,
        req: MlxEngineRemoteSingleStopRequest,
    ) -> Result<MlxEngineRemoteSingleStopResponse, agent_client_protocol::Error> {
        if let RouteRecord::Other(route) = mlx_remote::read() {
            return Err(agent_client_protocol::Error::invalid_params().data(format!(
                "remoteSingleActive: another goose window on this Mac (goosed pid {}) owns the route to {}; stop it from that window",
                route.pid,
                route.peer_name()
            )));
        }
        // Withdrawn under the route's op lock: a restore waiting for it finds no route and mounts
        // nothing; one that got there first finishes its Mount before the Unmount below.
        let _ops = ROUTE_RESTORE.ops.lock().await;
        let route = mlx_remote::uninstall()
            .internal_err_ctx("withdrawing the remote-single route record")?;
        ROUTE_RESTORE.forget();
        super::mlx_engine::forget_serving(IntentKind::RemoteSingle);
        super::mlx_engine::align_omlx_host_env();
        let (mut unmounted, mut unmount_error) = (false, None);
        if let (Some(route), false) = (&route, req.keep_mounted) {
            let outcome = match self.connected_link_manager().await {
                Ok(manager) => {
                    peer_op::<_, EmptyResponse>(
                        manager.as_ref(),
                        &route.peer,
                        MlxOp::Unmount,
                        &MlxEngineUnmountRequest { node_id: None },
                    )
                    .await
                }
                Err(refused) => Err(refused),
            };
            match outcome {
                Ok(_) => unmounted = true,
                Err(refused) => {
                    unmount_error = Some(format!(
                        "{}'s engine was left mounted: {}",
                        route.peer_name(),
                        refused.message
                    ))
                }
            }
        }
        Ok(MlxEngineRemoteSingleStopResponse {
            unmounted,
            unmount_error,
            status: current_status().await,
        })
    }

    pub(super) async fn on_mlx_engine_remote_single_status(
        &self,
        _req: MlxEngineRemoteSingleStatusRequest,
    ) -> Result<MlxEngineRemoteSingleStatusResponse, agent_client_protocol::Error> {
        super::mlx_engine::align_omlx_host_env();
        let status = current_status().await;
        let entered_ready = {
            let mut last = LAST_REMOTE_STATE.lock().unwrap_or_else(|e| e.into_inner());
            let entered = status.state == "ready" && *last != "ready";
            *last = status.state.clone();
            entered
        };
        if entered_ready {
            // The omlx provider now reaches the peer's engine (OMLX_HOST follows the relay); its
            // inventory must list the served id before the next model switch.
            if let Err(e) = self
                .start_provider_inventory_refresh(&["omlx".to_string()])
                .await
            {
                warn!(error = ?e, "omlx inventory refresh after the remote engine came up failed");
            }
        }
        Ok(MlxEngineRemoteSingleStatusResponse { status })
    }
}

/// The mesh hostname of `peer` (a node id or a hostname) and the name its owner gave it, from the
/// live peer view.
async fn peer_names(
    manager: &LinkManager,
    peer: &str,
) -> Result<(String, Option<String>), MlxRemoteSingleRefusalDto> {
    let registry = manager
        .active_registry()
        .await
        .ok_or_else(|| link_refusal(&LinkError::NotConnected))?;
    registry
        .peer_nodes()
        .into_iter()
        .find(|node| node.node_id == peer || node.hostname == peer)
        .map(|node| (node.hostname, node.computer_name))
        .ok_or_else(|| link_refusal(&LinkError::UnknownPeer(peer.to_string())))
}

/// This Mac's distributed engine, when one owns it (this window's or another's).
fn distributed_owner() -> Option<String> {
    use crate::providers::mlx_distributed_owner::{self as owner_record, OwnerRecord};
    if owner_record::own_active_base_url().is_some() {
        return Some(
            "this window's distributed MLX engine owns this Mac; stop it first".to_string(),
        );
    }
    match owner_record::read() {
        OwnerRecord::Other(engine) => Some(format!(
            "the distributed MLX engine of another window (goosed pid {}) owns this Mac; stop it first",
            engine.pid
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route_to(base_url: String) -> PublishedRoute {
        PublishedRoute {
            pid: 1,
            base_url,
            peer: "wh".to_string(),
            peer_hostname: "WorksMacStudio.lan".to_string(),
            peer_computer_name: Some("Work's Mac Studio".to_string()),
            model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            served_model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            capacity: 8,
            template_kwargs: None,
        }
    }

    /// A relay stand-in: `/v1/models` answers with `status` and `body`.
    async fn relay(status: u16, body: &'static str) -> String {
        let app = axum::Router::new().route(
            "/relay/cap/v1/models",
            axum::routing::get(move || async move {
                (axum::http::StatusCode::from_u16(status).unwrap(), body)
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/relay/cap")
    }

    // -----------------------------------------------------------------------------------------
    // Q-34: the route's own restore, against a stand-in peer and relay.
    // -----------------------------------------------------------------------------------------

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    const QWEN: &str = "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx";

    /// Work's Mac Studio as it answers over Link: its engine status (what `Status` returns), its
    /// Mount (counted; refused with `mount_refusal` when set) and what its chat proxy serves.
    struct StandInPeer {
        engine: StdMutex<MlxEngineStatusDto>,
        mount_refusal: Option<String>,
        mounts: AtomicUsize,
        /// The model the peer's engine lists on `/v1/models` through the proxy; `None` = nothing
        /// listens there (the proxy's `502 engineUnreachable`).
        proxy_serves: Arc<StdMutex<Option<String>>>,
        /// The Studio's Link is gone and the fabric has not noticed yet: every op fails in the
        /// transport with these words, and the relay answers its own `502 linkRelayFailed`.
        link_down: Arc<StdMutex<Option<String>>>,
        /// The fabric's verdict (`unreachable`): `Some` once its polls cannot reach the Studio.
        fabric_lost: StdMutex<Option<String>>,
        /// Every op the route made on the peer over Link.
        ops: AtomicUsize,
    }

    impl StandInPeer {
        fn new(engine: MlxEngineStatusDto) -> Arc<Self> {
            Arc::new(Self {
                engine: StdMutex::new(engine),
                mount_refusal: None,
                mounts: AtomicUsize::new(0),
                proxy_serves: Arc::new(StdMutex::new(None)),
                link_down: Arc::new(StdMutex::new(None)),
                fabric_lost: StdMutex::new(None),
                ops: AtomicUsize::new(0),
            })
        }

        fn ops(&self) -> usize {
            self.ops.load(Ordering::SeqCst)
        }

        fn link_dies(&self, words: &str) {
            *self.link_down.lock().unwrap() = Some(words.to_string());
        }

        fn link_returns(&self) {
            *self.link_down.lock().unwrap() = None;
            *self.fabric_lost.lock().unwrap() = None;
        }

        fn fabric_notices(&self, words: &str) {
            *self.fabric_lost.lock().unwrap() = Some(words.to_string());
        }

        fn mounts(&self) -> usize {
            self.mounts.load(Ordering::SeqCst)
        }

        fn set_engine(&self, engine: MlxEngineStatusDto) {
            *self.engine.lock().unwrap() = engine;
        }

        /// The load the Mount started has finished: the engine runs and the proxy serves it.
        fn load_finishes(&self) {
            let mut engine = self.engine.lock().unwrap();
            engine.state = "running".to_string();
            engine.served_model_id = engine.model_id.clone();
            *self.proxy_serves.lock().unwrap() = engine.model_id.clone();
        }
    }

    #[async_trait::async_trait]
    impl PeerControl for StandInPeer {
        async fn mlx_op(
            &self,
            _peer: &str,
            op: MlxOp,
            body: serde_json::Value,
        ) -> Result<serde_json::Value, LinkError> {
            self.ops.fetch_add(1, Ordering::SeqCst);
            if let Some(words) = self.link_down.lock().unwrap().clone() {
                return Err(LinkError::MlxProxy(words));
            }
            match op {
                MlxOp::Status => {
                    let mut status = self.engine.lock().unwrap().clone();
                    status.max_concurrent_requests = Some(8);
                    Ok(serde_json::to_value(MlxEngineStatusResponse { status }).unwrap())
                }
                MlxOp::SettingsRead => {
                    Ok(serde_json::to_value(MlxEngineSettingsResponse::default()).unwrap())
                }
                MlxOp::Mount => {
                    self.mounts.fetch_add(1, Ordering::SeqCst);
                    if let Some(text) = &self.mount_refusal {
                        return Err(LinkError::MlxControl(
                            leanzero_link::state::MlxControlError::BadRequest(text.clone()),
                        ));
                    }
                    let mut engine = self.engine.lock().unwrap();
                    engine.state = "mounting".to_string();
                    engine.model_id = body["modelId"].as_str().map(str::to_string);
                    engine.stopped_by = None;
                    Ok(serde_json::json!({}))
                }
                other => panic!("the restore made an op it has no business making: {other:?}"),
            }
        }

        async fn unreachable(&self, _peer: &str) -> Option<String> {
            self.fabric_lost.lock().unwrap().clone()
        }

        /// The fabric's polls, as a test join: looked at until a test says it noticed.
        async fn lost(&self, peer: &str) -> String {
            loop {
                if let Some(words) = self.unreachable(peer).await {
                    return words;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    }

    /// This Mac's relay to the stand-in's chat proxy: `/v1/models` and `/v1/status` answer as the
    /// proxy does — the engine's own answer, or r3-2's `502 engineUnreachable` while nothing
    /// listens on the peer.
    async fn stand_in_relay(peer: &StandInPeer) -> String {
        const UNREACHABLE: &str = "engineUnreachable: no MLX engine answers at http://127.0.0.1:8090 on WorksMacStudio.lan — mount a model there first (error sending request for url (http://127.0.0.1:8090/v1/models))";
        let serves = peer.proxy_serves.clone();
        let (models, models_down, status_down) = (
            serves.clone(),
            peer.link_down.clone(),
            peer.link_down.clone(),
        );
        let relay_failed =
            |why: String| format!("linkRelayFailed: cannot reach Link peer 'wh': {why}");
        let app = axum::Router::new()
            .route(
                "/relay/cap/v1/models",
                axum::routing::get(move || {
                    let serves = models.lock().unwrap().clone();
                    let down = models_down.lock().unwrap().clone();
                    async move {
                        if let Some(why) = down {
                            return (axum::http::StatusCode::BAD_GATEWAY, relay_failed(why));
                        }
                        match serves {
                            Some(id) => (
                                axum::http::StatusCode::OK,
                                format!(
                                    r#"{{"object":"list","data":[{{"id":"{id}","object":"model","context_window":262144}}]}}"#
                                ),
                            ),
                            None => (axum::http::StatusCode::BAD_GATEWAY, UNREACHABLE.to_string()),
                        }
                    }
                }),
            )
            .route(
                "/relay/cap/v1/status",
                axum::routing::get(move || {
                    let up = serves.lock().unwrap().is_some();
                    let down = status_down.lock().unwrap().clone();
                    async move {
                        if let Some(why) = down {
                            return (axum::http::StatusCode::BAD_GATEWAY, relay_failed(why));
                        }
                        if up {
                            (
                                axum::http::StatusCode::OK,
                                r#"{"status":"idle","num_running":0,"num_waiting":0}"#.to_string(),
                            )
                        } else {
                            (axum::http::StatusCode::BAD_GATEWAY, UNREACHABLE.to_string())
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/relay/cap")
    }

    /// The Studio's goose right after its app relaunched: engine stopped, nothing stopped it.
    fn relaunched_engine() -> MlxEngineStatusDto {
        MlxEngineStatusDto {
            state: "stopped".to_string(),
            stopped_by: Some("notStarted".to_string()),
            ..Default::default()
        }
    }

    fn fresh_book() -> &'static RouteRestore {
        Box::leak(Box::new(RouteRestore::new()))
    }

    fn still_routed(flag: Arc<AtomicBool>) -> RouteCheck {
        Arc::new(move |_: &PublishedRoute| flag.load(Ordering::SeqCst))
    }

    /// The spawned restore is on its way; wait for it to leave `Mounting` (a test join, not a
    /// bound on anything).
    async fn restore_settles(book: &RouteRestore, route: &PublishedRoute) -> Option<RestorePhase> {
        loop {
            let phase = book.phase(route);
            if phase != Some(RestorePhase::Mounting) {
                return phase;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    async fn read(
        book: &'static RouteRestore,
        peer: &Arc<StandInPeer>,
        route: &PublishedRoute,
        routed: &Arc<AtomicBool>,
    ) -> MlxRemoteSingleStatusDto {
        owned_route_status(
            book,
            Some(peer.clone() as Arc<dyn PeerControl>),
            route,
            still_routed(routed.clone()),
        )
        .await
    }

    #[tokio::test]
    async fn r3_2_a_peer_back_without_its_engine_gets_the_routes_model_once_and_serves() {
        let peer = StandInPeer::new(relaunched_engine());
        let route = route_to(stand_in_relay(&peer).await);
        let (book, routed) = (fresh_book(), Arc::new(AtomicBool::new(true)));

        // 12:18:04 in r3-2: Link is back, the proxy says engineUnreachable, the Studio says stopped.
        let first = read(book, &peer, &route, &routed).await;
        assert_eq!(first.state, "mounting", "{:?}", first.last_error);
        assert_eq!(first.last_error, None);
        assert_eq!(
            first.restore,
            Some(MlxRemoteSingleRestoreDto {
                phase: "restoring".to_string(),
                message: "Restoring Qwen3.8-27B-Atlassian-Q8-mlx on Work's Mac Studio…".to_string(),
            })
        );
        assert_eq!(
            restore_settles(book, &route).await,
            Some(RestorePhase::Mounted)
        );
        assert_eq!(peer.mounts(), 1);

        // Every poll while the Studio loads: still restoring, never a second Mount.
        for _ in 0..3 {
            let loading = read(book, &peer, &route, &routed).await;
            assert_eq!(loading.state, "mounting");
            assert_eq!(loading.restore.as_ref().unwrap().phase, "restoring");
        }
        assert_eq!(peer.mounts(), 1);

        peer.load_finishes();
        let served = read(book, &peer, &route, &routed).await;
        assert_eq!(served.state, "ready", "{:?}", served.last_error);
        assert_eq!(served.restore, None);
        assert_eq!(served.context_window, Some(262144));
        assert_eq!(book.phase(&route), None, "serving re-arms the restore");
        assert_eq!(peer.mounts(), 1);

        // The next relaunch is a new outage: one more re-mount, not zero.
        peer.set_engine(relaunched_engine());
        *peer.proxy_serves.lock().unwrap() = None;
        assert_eq!(read(book, &peer, &route, &routed).await.state, "mounting");
        assert_eq!(
            restore_settles(book, &route).await,
            Some(RestorePhase::Mounted)
        );
        assert_eq!(peer.mounts(), 2);
    }

    #[tokio::test]
    async fn a_stop_from_this_mac_that_gets_the_lock_first_leaves_nothing_mounted() {
        let peer = StandInPeer::new(relaunched_engine());
        let route = route_to(stand_in_relay(&peer).await);
        let (book, routed) = (fresh_book(), Arc::new(AtomicBool::new(true)));

        // Stop holds the route's op lock while it withdraws the route.
        let stop = book.ops.lock().await;
        assert_eq!(read(book, &peer, &route, &routed).await.state, "mounting");
        routed.store(false, Ordering::SeqCst);
        drop(stop);

        assert_eq!(restore_settles(book, &route).await, None);
        assert_eq!(peer.mounts(), 0, "a withdrawn route is never mounted again");
    }

    #[tokio::test]
    async fn an_engine_switched_to_another_model_there_is_not_mounted_over() {
        let flash = "Mihai-LeanZero/Qwen3.8-Flash-mlx";
        let peer = StandInPeer::new(MlxEngineStatusDto {
            state: "running".to_string(),
            model_id: Some(flash.to_string()),
            served_model_id: Some(flash.to_string()),
            ..Default::default()
        });
        *peer.proxy_serves.lock().unwrap() = Some(flash.to_string());
        let route = route_to(stand_in_relay(&peer).await);
        let (book, routed) = (fresh_book(), Arc::new(AtomicBool::new(true)));

        let status = read(book, &peer, &route, &routed).await;
        assert_eq!(status.state, "failed");
        assert_eq!(status.restore, None);
        let why = status.last_error.unwrap();
        assert!(
            why.starts_with("Work's Mac Studio's engine runs 'Mihai-LeanZero/Qwen3.8-Flash-mlx' now, started there after this route's 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx'; this Mac does not mount over it"),
            "{why}"
        );
        assert_eq!(peer.mounts(), 0);
        assert_eq!(book.phase(&route), None);
    }

    #[tokio::test]
    async fn an_engine_its_owner_stopped_there_is_not_started_again() {
        let peer = StandInPeer::new(MlxEngineStatusDto {
            state: "stopped".to_string(),
            stopped_by: Some("owner".to_string()),
            ..Default::default()
        });
        let route = route_to(stand_in_relay(&peer).await);
        let (book, routed) = (fresh_book(), Arc::new(AtomicBool::new(true)));

        let status = read(book, &peer, &route, &routed).await;
        assert_eq!(status.state, "failed");
        let why = status.last_error.unwrap();
        assert!(
            why.starts_with("Work's Mac Studio's owner stopped its engine there, so this Mac does not start it again"),
            "{why}"
        );
        assert!(why.contains("reports its engine stopped"), "{why}");
        assert_eq!(peer.mounts(), 0);
    }

    #[tokio::test]
    async fn a_refused_remount_is_a_named_failure_in_the_peers_words_and_is_not_retried() {
        let peer = Arc::new(StandInPeer {
            mount_refusal: Some("memory gate BLOCK: model needs 40GB, 12GB free".to_string()),
            ..Arc::into_inner(StandInPeer::new(relaunched_engine())).unwrap()
        });
        let route = route_to(stand_in_relay(&peer).await);
        let (book, routed) = (fresh_book(), Arc::new(AtomicBool::new(true)));

        assert_eq!(read(book, &peer, &route, &routed).await.state, "mounting");
        let expected = "Restoring Qwen3.8-27B-Atlassian-Q8-mlx on Work's Mac Studio failed: memory gate BLOCK: model needs 40GB, 12GB free";
        assert_eq!(
            restore_settles(book, &route).await,
            Some(RestorePhase::Failed(expected.to_string()))
        );
        for _ in 0..3 {
            let failed = read(book, &peer, &route, &routed).await;
            assert_eq!(failed.state, "failed");
            assert_eq!(failed.last_error.as_deref(), Some(expected));
            assert_eq!(
                failed.restore,
                Some(MlxRemoteSingleRestoreDto {
                    phase: "failed".to_string(),
                    message: expected.to_string(),
                })
            );
        }
        assert_eq!(peer.mounts(), 1, "a failed restore is a state, not a loop");
    }

    #[tokio::test]
    async fn a_remount_whose_load_fails_there_ends_failed_with_the_peers_words() {
        let peer = StandInPeer::new(relaunched_engine());
        let route = route_to(stand_in_relay(&peer).await);
        let (book, routed) = (fresh_book(), Arc::new(AtomicBool::new(true)));

        read(book, &peer, &route, &routed).await;
        assert_eq!(
            restore_settles(book, &route).await,
            Some(RestorePhase::Mounted)
        );
        peer.set_engine(MlxEngineStatusDto {
            state: "failed".to_string(),
            model_id: Some(QWEN.to_string()),
            last_error: Some("engine exited with status 1".to_string()),
            ..Default::default()
        });
        let failed = read(book, &peer, &route, &routed).await;
        assert_eq!(failed.state, "failed");
        let message = failed.last_error.unwrap();
        assert!(
            message.starts_with("Restoring Qwen3.8-27B-Atlassian-Q8-mlx on Work's Mac Studio failed: Work's Mac Studio's goose reports its engine failed (engine exited with status 1)"),
            "{message}"
        );
        assert_eq!(failed.restore.unwrap().phase, "failed");
        read(book, &peer, &route, &routed).await;
        assert_eq!(peer.mounts(), 1);
    }

    // -----------------------------------------------------------------------------------------
    // Q-47: a published route whose Mac stops answering is `reconnecting`, never `failed`.
    // -----------------------------------------------------------------------------------------

    /// The words this Mac's goosed logged for the Studio during the 2026-09-25 relaunch
    /// (11:21:49.595Z, `leanzero_link::state`) — a send the mesh could not deliver.
    const STUDIO_GONE: &str =
        "error sending request for url (http://100.64.0.5:41226/v1/swarm/mlx/status)";
    const FABRIC_LOST: &str = "the LeanZero Link mesh cannot reach it (error sending request for url (http://100.64.0.5:41226/v1/swarm/nodes))";

    fn serving_engine() -> MlxEngineStatusDto {
        MlxEngineStatusDto {
            state: "running".to_string(),
            model_id: Some(QWEN.to_string()),
            served_model_id: Some(QWEN.to_string()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_studio_that_relaunches_reads_reconnecting_then_restoring_then_ready() {
        let peer = StandInPeer::new(serving_engine());
        *peer.proxy_serves.lock().unwrap() = Some(QWEN.to_string());
        let route = route_to(stand_in_relay(&peer).await);
        let (book, routed) = (fresh_book(), Arc::new(AtomicBool::new(true)));
        assert_eq!(read(book, &peer, &route, &routed).await.state, "ready");

        // t≈9 s: the Studio's goose quits and its Link with it; the fabric has not polled yet.
        peer.link_dies(STUDIO_GONE);
        let lost = read(book, &peer, &route, &routed).await;
        assert_eq!(lost.state, "reconnecting", "{:?}", lost.last_error);
        let why = lost.last_error.unwrap();
        assert!(
            why.starts_with(
                "502: linkRelayFailed: cannot reach Link peer 'wh': error sending request"
            ),
            "{why}"
        );
        assert!(
            why.ends_with(&format!(
                "; mlx proxy request to a peer failed: {STUDIO_GONE}"
            )),
            "{why}"
        );
        assert_eq!(lost.restore, None);
        assert_eq!(
            book.phase(&route),
            None,
            "an unreachable peer starts no restore"
        );

        // t≈17 s: the fabric's poll failed — every read answers from that, with no dial.
        peer.fabric_notices(FABRIC_LOST);
        let ops = peer.ops();
        for _ in 0..3 {
            let known = read(book, &peer, &route, &routed).await;
            assert_eq!(known.state, "reconnecting");
            assert_eq!(
                known.last_error.as_deref(),
                Some(
                    format!("Work's Mac Studio does not answer over LeanZero Link right now: {FABRIC_LOST}")
                        .as_str()
                )
            );
            assert_eq!(known.active_requests, None);
            assert_eq!(known.active_requests_error, known.last_error);
        }
        assert_eq!(
            peer.ops(),
            ops,
            "the fabric's verdict costs no op on the peer"
        );
        assert_eq!(peer.mounts(), 0);

        // t≈20 s: the Studio answers again, its relaunched goose without an engine: the restore.
        peer.set_engine(relaunched_engine());
        *peer.proxy_serves.lock().unwrap() = None;
        peer.link_returns();
        let back = read(book, &peer, &route, &routed).await;
        assert_eq!(back.state, "mounting", "{:?}", back.last_error);
        assert_eq!(back.restore.unwrap().phase, "restoring");
        assert_eq!(
            restore_settles(book, &route).await,
            Some(RestorePhase::Mounted)
        );
        assert_eq!(peer.mounts(), 1);

        peer.load_finishes();
        let served = read(book, &peer, &route, &routed).await;
        assert_eq!(served.state, "ready", "{:?}", served.last_error);
        assert_eq!(served.last_error, None);
    }

    #[tokio::test]
    async fn a_peer_that_answers_with_its_engine_failed_is_failed_not_reconnecting() {
        let peer = StandInPeer::new(MlxEngineStatusDto {
            state: "failed".to_string(),
            model_id: Some(QWEN.to_string()),
            last_error: Some("engine exited with status 1".to_string()),
            ..Default::default()
        });
        let route = route_to(stand_in_relay(&peer).await);
        let (book, routed) = (fresh_book(), Arc::new(AtomicBool::new(true)));

        let status = read(book, &peer, &route, &routed).await;
        assert_eq!(status.state, "failed");
        let why = status.last_error.unwrap();
        assert!(
            why.starts_with(
                "Work's Mac Studio's goose reports its engine failed (engine exited with status 1)"
            ),
            "{why}"
        );
        assert_eq!(status.restore, None);
        assert_eq!(peer.mounts(), 0);
    }

    #[tokio::test]
    async fn a_read_in_flight_when_the_studio_goes_answers_when_the_fabric_notices() {
        // A relay whose kept-alive connection to the gone Studio never answers; it says when a
        // probe has reached it.
        let asked = Arc::new(tokio::sync::Notify::new());
        let silent_get = |asked: Arc<tokio::sync::Notify>| {
            axum::routing::get(move || {
                asked.notify_one();
                std::future::pending::<&'static str>()
            })
        };
        let silent = axum::Router::new()
            .route("/relay/cap/v1/models", silent_get(asked.clone()))
            .route("/relay/cap/v1/status", silent_get(asked.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, silent).await.unwrap() });
        let route = route_to(format!("http://{addr}/relay/cap"));
        let peer = StandInPeer::new(serving_engine());
        let (book, routed) = (fresh_book(), Arc::new(AtomicBool::new(true)));

        let reading = {
            let (peer, route, routed) = (peer.clone(), route.clone(), routed.clone());
            tokio::spawn(async move { read(book, &peer, &route, &routed).await })
        };
        asked.notified().await;
        peer.fabric_notices(FABRIC_LOST);
        // A test bound, so a regression fails instead of hanging.
        let status = tokio::time::timeout(std::time::Duration::from_secs(10), reading)
            .await
            .expect("the read must answer when the fabric loses the peer")
            .unwrap();
        assert_eq!(status.state, "reconnecting");
        assert!(status.last_error.unwrap().ends_with(FABRIC_LOST));
        assert_eq!(peer.ops(), 0);
    }

    #[tokio::test]
    async fn without_link_in_this_goose_the_relays_own_502_is_reconnecting() {
        let peer = StandInPeer::new(relaunched_engine());
        let route = route_to(stand_in_relay(&peer).await);

        let loading = route_status(None, &route).await;
        assert_eq!(
            loading.state, "failed",
            "the peer's proxy answered: its engine does not"
        );
        assert!(loading
            .last_error
            .unwrap()
            .starts_with("502: engineUnreachable"));

        peer.link_dies(STUDIO_GONE);
        let lost = route_status(None, &route).await;
        assert_eq!(lost.state, "reconnecting");
        assert!(lost
            .last_error
            .unwrap()
            .starts_with("502: linkRelayFailed: cannot reach Link peer 'wh'"));
    }

    #[test]
    fn only_a_link_error_with_no_answer_from_the_peer_means_reconnecting() {
        for silent in [
            LinkError::NotConnected,
            LinkError::UnknownPeer("wh".into()),
            LinkError::MlxProxy(STUDIO_GONE.into()),
        ] {
            assert!(peer_did_not_answer(&silent), "{silent}");
        }
        for answered in [
            LinkError::MlxProxy("peer returned 403: remote model management is disabled".into()),
            LinkError::MlxProxy("peer returned 501: mlx control is not wired".into()),
            LinkError::MlxProxy("peer responded but its body did not parse: eof".into()),
            LinkError::MlxControl(leanzero_link::state::MlxControlError::Failed(
                "engine exited".into(),
            )),
        ] {
            assert!(!peer_did_not_answer(&answered), "{answered}");
        }
    }

    #[test]
    fn only_a_stopped_engine_nobody_stopped_is_restored() {
        let route = route_to("http://127.0.0.1:1/relay/cap".to_string());
        let stopped = |by: Option<&str>| MlxEngineStatusDto {
            state: "stopped".to_string(),
            stopped_by: by.map(str::to_string),
            ..Default::default()
        };
        assert_eq!(
            restore_step(&route, &stopped(Some("notStarted"))),
            RestoreStep::Mount
        );
        for by in [Some("owner"), Some("linkedMac"), Some("somebodyNew"), None] {
            assert!(
                matches!(
                    restore_step(&route, &stopped(by)),
                    RestoreStep::Decline(Some(_))
                ),
                "{by:?}"
            );
        }

        // The Studio's owner serves another model there, and its own launch brings it back.
        let intent = |kind: &str, model: &str| MlxEngineStatusDto {
            serving_intent: Some(MlxServingIntentDto {
                kind: kind.to_string(),
                model_id: model.to_string(),
                ..Default::default()
            }),
            ..stopped(Some("notStarted"))
        };
        let RestoreStep::Decline(Some(why)) =
            restore_step(&route, &intent("single", "other/Flash"))
        else {
            panic!("the owner's own model must not be mounted over");
        };
        assert!(
            why.starts_with("Work's Mac Studio's owner serves 'other/Flash' there"),
            "{why}"
        );
        assert!(matches!(
            restore_step(&route, &intent("split", QWEN)),
            RestoreStep::Decline(Some(_))
        ));
        assert_eq!(
            restore_step(&route, &intent("single", QWEN)),
            RestoreStep::Mount
        );
        assert_eq!(
            restore_step(&route, &intent("remoteSingle", "any")),
            RestoreStep::Mount
        );

        let hosting = MlxEngineStatusDto {
            hosting: Some(Default::default()),
            ..stopped(Some("notStarted"))
        };
        assert!(matches!(
            restore_step(&route, &hosting),
            RestoreStep::Decline(Some(_))
        ));
        let ours_loading = MlxEngineStatusDto {
            state: "mounting".to_string(),
            model_id: Some(QWEN.to_string()),
            ..Default::default()
        };
        assert_eq!(restore_step(&route, &ours_loading), RestoreStep::Keep);
        let failed = MlxEngineStatusDto {
            state: "failed".to_string(),
            last_error: Some("engine exited".to_string()),
            ..Default::default()
        };
        assert_eq!(restore_step(&route, &failed), RestoreStep::Decline(None));
    }

    #[tokio::test]
    async fn the_route_serves_when_the_peer_engine_lists_its_model_through_the_relay() {
        let base = relay(
            200,
            r#"{"object":"list","data":[{"id":"Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx","object":"model","context_length":262144}]}"#,
        )
        .await;
        assert!(route_models(&route_to(base)).await.is_ok());
    }

    #[tokio::test]
    async fn a_loading_peer_engine_is_the_proxys_502_in_words() {
        let base = relay(
            502,
            "engineUnreachable: no MLX engine answers at http://127.0.0.1:8090",
        )
        .await;
        let why = route_models(&route_to(base)).await.unwrap_err();
        assert!(why.starts_with("502: engineUnreachable"), "{why}");
    }

    #[test]
    fn a_peer_engine_holds_its_active_and_cached_metal_memory() {
        // Work's Mac Studio's 27B, idle after chat, 2026-09-25.
        let body = r#"{"status":"idle","metal":{"active_memory_gb":40.17,"peak_memory_gb":54.21,"cache_memory_gb":13.02}}"#;
        assert_eq!(metal_held_bytes(body), Ok(53_190_000_000));
        assert!(metal_held_bytes(r#"{"status":"idle"}"#)
            .unwrap_err()
            .contains("no `metal` block"));
        assert!(metal_held_bytes(r#"{"metal":{"active_memory_gb":1.0}}"#)
            .unwrap_err()
            .contains("metal.cache_memory_gb"));
    }

    #[test]
    fn the_engine_address_is_the_configured_port_on_loopback_or_a_named_absence() {
        let settings = EngineSettings {
            port: 8093,
            ..Default::default()
        };
        assert_eq!(
            engine_base_url_from(Ok(settings)).unwrap(),
            "http://127.0.0.1:8093"
        );
        assert_eq!(
            engine_base_url_from(Err(ConfigError::NotFound("mlx_engine".into()))).unwrap(),
            format!("http://127.0.0.1:{}", EngineSettings::default().port)
        );
        let unreadable =
            engine_base_url_from(Err(ConfigError::DeserializeError("bad yaml".into())))
                .unwrap_err();
        assert!(unreadable.contains("unreadable"), "{unreadable}");
    }

    #[test]
    fn a_peer_that_has_not_opted_in_is_refused_before_any_mount() {
        let off = serving_refusal(&RelayAnswer::Status {
            code: 403,
            body: "chatServingDisabled: \"Let my other Macs use this Mac › Answer chat\" is off on WorksMacStudio.lan".into(),
        })
        .unwrap();
        assert_eq!(off.code, "chatServingDisabled");
        assert!(off.message.ends_with("off on WorksMacStudio.lan"));

        // Nothing mounted there yet is the normal start — the mount follows.
        assert!(serving_refusal(&RelayAnswer::Status {
            code: 502,
            body: format!("{ENGINE_UNREACHABLE}: no MLX engine answers at http://127.0.0.1:8090"),
        })
        .is_none());
        assert!(serving_refusal(&RelayAnswer::Ok("{}".into())).is_none());

        // An older goose there has no route at all.
        let old = serving_refusal(&RelayAnswer::Status {
            code: 404,
            body: String::new(),
        })
        .unwrap();
        assert_eq!(old.code, "peerTooOld");
        let gone = serving_refusal(&RelayAnswer::Status {
            code: 502,
            body: "linkRelayFailed: cannot reach Link peer 'x'".into(),
        })
        .unwrap();
        assert_eq!(gone.code, "peerUnreachable");
    }

    #[test]
    fn a_peer_that_says_running_while_nothing_answers_is_named_with_both_facts() {
        let engine = MlxEngineStatusDto {
            state: "running".to_string(),
            probe_error: Some("GET http://127.0.0.1:8095/v1/models: Connection refused".into()),
            ..Default::default()
        };
        let text = peer_engine_failure("WorksMacStudio.lan", &engine, "502: engineUnreachable: …");
        assert!(
            text.starts_with("WorksMacStudio.lan's goose reports its engine running (GET "),
            "{text}"
        );
        assert!(text.ends_with("does not serve through LeanZero Link: 502: engineUnreachable: …"));
    }

    #[test]
    fn a_peers_own_refusals_keep_their_class() {
        assert_eq!(
            link_refusal(&LinkError::MlxProxy(
                "peer returned 403: remote model management is disabled on this node".into()
            ))
            .code,
            "remoteManagementDisabled"
        );
        let gate = link_refusal(&LinkError::MlxControl(
            leanzero_link::state::MlxControlError::BadRequest(
                "memory gate BLOCK: model needs 40GB, 12GB free".into(),
            ),
        ));
        assert_eq!(gate.code, "peerMountFailed");
        assert_eq!(
            gate.message,
            "memory gate BLOCK: model needs 40GB, 12GB free"
        );
        assert_eq!(
            link_refusal(&LinkError::NotConnected).code,
            "linkNotConnected"
        );
        assert_eq!(
            link_refusal(&LinkError::UnknownPeer("ghost".into())).code,
            "unknownPeer"
        );
    }
}
