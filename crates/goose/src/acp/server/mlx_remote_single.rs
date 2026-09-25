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
use leanzero_link::inference::{InferenceRelay, PeerCallResolver, ENGINE_UNREACHABLE};
use leanzero_link::manager::{AuthState, LinkError, LinkManager};
use leanzero_link::state::{ChatServing, MlxOp};
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

/// The engine ops a route makes on its peer over LeanZero Link (the mesh's `mlx_proxy`) — a seam
/// so the restore runs against a stand-in peer in tests.
#[async_trait::async_trait]
pub(super) trait PeerControl: Send + Sync {
    async fn mlx_op(
        &self,
        peer: &str,
        op: MlxOp,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, LinkError>;
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
}

async fn peer_op<Req: Serialize, Resp: DeserializeOwned>(
    control: &dyn PeerControl,
    peer: &str,
    op: MlxOp,
    req: &Req,
) -> Result<Resp, MlxRemoteSingleRefusalDto> {
    let body = serde_json::to_value(req).map_err(|e| refusal("peerUnreachable", e.to_string()))?;
    let value = control
        .mlx_op(peer, op, body)
        .await
        .map_err(|e| link_refusal(&e))?;
    serde_json::from_value(value).map_err(|e| {
        refusal(
            "peerTooOld",
            format!("the peer's {} answer did not decode: {e}", op.path()),
        )
    })
}

/// The peer engine's `/v1/models` through the relay: its context window when it serves the route's
/// model, else why not in words.
async fn route_models(route: &PublishedRoute) -> Result<Option<u64>, String> {
    match relay_get(&route.base_url, "v1/models").await {
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

/// The route's state, re-probed through the relay, and the peer's own engine status when the
/// relay says it is not serving yet (mounting vs failed).
async fn observe_route(control: Option<&dyn PeerControl>, route: &PublishedRoute) -> RouteObservation {
    let mut status = MlxRemoteSingleStatusDto {
        state: "mounting".to_string(),
        peer: Some(route.peer.clone()),
        peer_hostname: Some(route.peer_hostname.clone()),
        peer_computer_name: route.peer_computer_name.clone(),
        base_url: Some(route.base_url.clone()),
        model_id: Some(route.model_id.clone()),
        served_model_id: Some(route.served_model_id.clone()),
        capacity: Some(route.capacity),
        ..Default::default()
    };
    let models_error = match route_models(route).await {
        Ok(window) => {
            status.state = "ready".to_string();
            status.context_window = window;
            None
        }
        Err(why) => Some(why),
    };
    match relay_get(&route.base_url, "v1/status").await {
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
            peer_op::<_, MlxEngineStatusResponse>(
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
        None => Err(refusal(
            "linkNotConnected",
            "LeanZero Link has not started in this goose",
        )),
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
            status.state = "failed".to_string();
            status.last_error = Some(format!("{models_error}; {}", refused.message));
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
    Arc::new(|route: &PublishedRoute| {
        matches!(mlx_remote::read(), RouteRecord::Mine(now) if now.base_url == route.base_url)
    })
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
/// `failed` with the peer's words until the route serves or the owner runs it again.
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
                        tokio::spawn(restore_route(
                            book,
                            control,
                            route.clone(),
                            still_routed,
                        ));
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
                // A restore under way reads `mounting` here: the same placement asked again joins it.
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
