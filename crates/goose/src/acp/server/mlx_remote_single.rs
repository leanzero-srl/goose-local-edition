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

use super::*;
use crate::config::ConfigError;
use crate::providers::mlx_remote::{self, PublishedRoute, RouteRecord};
use goose_sidecar::engine::{served_model_id, EngineSettings};
use leanzero_link::inference::{InferenceRelay, PeerCallResolver, ENGINE_UNREACHABLE};
use leanzero_link::manager::{AuthState, LinkError, LinkManager};
use leanzero_link::state::{ChatServing, MlxOp};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Mutex as StdMutex;

/// The owner's switch ("Allow this Mac to serve chat to linked devices"): a goose config key, or
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

async fn peer_op<Req: Serialize, Resp: DeserializeOwned>(
    manager: &LinkManager,
    peer: &str,
    op: MlxOp,
    req: &Req,
) -> Result<Resp, MlxRemoteSingleRefusalDto> {
    let body = serde_json::to_value(req).map_err(|e| refusal("peerUnreachable", e.to_string()))?;
    let value = manager
        .mlx_proxy(peer, op, body)
        .await
        .map_err(|e| link_refusal(&e))?;
    serde_json::from_value(value).map_err(|e| {
        refusal(
            "peerTooOld",
            format!("the peer's {} answer did not decode: {e}", op.path()),
        )
    })
}

/// The route's state, re-probed through the relay, and the peer's own engine status when the
/// relay says it is not serving yet (mounting vs failed).
async fn route_status(
    manager: Option<&LinkManager>,
    route: &PublishedRoute,
) -> MlxRemoteSingleStatusDto {
    let mut status = MlxRemoteSingleStatusDto {
        state: "mounting".to_string(),
        peer: Some(route.peer.clone()),
        peer_hostname: Some(route.peer_hostname.clone()),
        model_id: Some(route.model_id.clone()),
        served_model_id: Some(route.served_model_id.clone()),
        capacity: Some(route.capacity),
        ..Default::default()
    };
    let models_error = match relay_get(&route.base_url, "v1/models").await {
        RelayAnswer::Ok(body) => match goose_sidecar::engine::parse_model_info(&body) {
            Ok((Some(served), window, _)) if served == route.served_model_id => {
                status.state = "ready".to_string();
                status.context_window = window;
                None
            }
            Ok((served, _, _)) => Some(format!(
                "{} now serves {:?}, the route wants '{}'",
                route.peer_hostname, served, route.served_model_id
            )),
            Err(e) => Some(format!("{e:#}")),
        },
        RelayAnswer::Status { code, body } => Some(format!("{code}: {body}")),
        RelayAnswer::NoAnswer(why) => Some(why),
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
        return status;
    };
    // Not serving through the proxy: the peer's own engine state says loading or failed.
    let peer_state = match manager {
        Some(manager) => {
            peer_op::<_, MlxEngineStatusResponse>(
                manager,
                &route.peer,
                MlxOp::Status,
                &MlxEngineStatusRequest { node_id: None },
            )
            .await
        }
        None => Err(refusal(
            "linkNotConnected",
            "LeanZero Link has not started in this goose",
        )),
    };
    match peer_state {
        Ok(peer) if peer.status.state == "mounting" => {}
        Ok(peer) => {
            status.state = "failed".to_string();
            status.last_error = Some(match peer.status.last_error {
                Some(err) => format!(
                    "{}'s engine is {}: {err}",
                    route.peer_hostname, peer.status.state
                ),
                None => format!(
                    "{}'s engine is {} ({models_error})",
                    route.peer_hostname, peer.status.state
                ),
            });
        }
        Err(refused) => {
            status.state = "failed".to_string();
            status.last_error = Some(format!("{models_error}; {}", refused.message));
        }
    }
    status
}

async fn current_status(manager: Option<&LinkManager>) -> MlxRemoteSingleStatusDto {
    match mlx_remote::read() {
        RouteRecord::Mine(route) | RouteRecord::Other(route) => route_status(manager, &route).await,
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
        match mlx_remote::read() {
            RouteRecord::Mine(route) | RouteRecord::Other(route)
                if route.peer == req.peer && route.model_id == req.model_id =>
            {
                let manager = super::link::existing_link_manager();
                return Ok(route_status(manager.as_deref(), &route).await);
            }
            RouteRecord::Mine(route) => {
                return Err(refusal(
                    "remoteSingleActive",
                    format!(
                        "chat is already served from {} ({}); stop it first",
                        route.peer_hostname, route.model_id
                    ),
                ))
            }
            RouteRecord::Other(route) => {
                return Err(refusal(
                    "remoteSingleActive",
                    format!(
                        "another goose window on this Mac (goosed pid {}) routes chat to {} ({}); stop it from that window",
                        route.pid, route.peer_hostname, route.model_id
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
        let peer_hostname = peer_hostname(&manager, &req.peer).await?;

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
        if let Some(refused) = serving_refusal(&relay_get(relay.base_url(), "v1/status").await) {
            return Err(refused);
        }

        let peer_status: MlxEngineStatusResponse = peer_op(
            &manager,
            &req.peer,
            MlxOp::Status,
            &MlxEngineStatusRequest { node_id: None },
        )
        .await?;
        let capacity = peer_status.status.max_concurrent_requests.ok_or_else(|| {
            refusal(
                "peerTooOld",
                format!("{peer_hostname}'s goose does not report its admission cap (maxConcurrentRequests); update goose there"),
            )
        })?;
        let peer_settings: MlxEngineSettingsResponse = peer_op(
            &manager,
            &req.peer,
            MlxOp::SettingsRead,
            &MlxEngineSettingsReadRequest { node_id: None },
        )
        .await?;
        let peer_settings = super::mlx_engine::settings_from_dto(peer_settings.settings);
        let served = served_model_id(&peer_settings, &req.model_id);
        let template_kwargs = peer_settings
            .model_profiles
            .get(&req.model_id)
            .and_then(goose_sidecar::thinking::chat_template_kwargs);

        let already_serving = peer_status.status.state == "running"
            && peer_status.status.served_model_id.as_deref() == Some(served.as_str());
        if !already_serving {
            let _: EmptyResponse = peer_op(
                &manager,
                &req.peer,
                MlxOp::Mount,
                &MlxEngineMountRequest {
                    model_id: req.model_id.clone(),
                    node_id: None,
                },
            )
            .await?;
        }

        let route = PublishedRoute {
            pid: std::process::id(),
            base_url: relay.base_url().to_string(),
            peer: req.peer.clone(),
            peer_hostname,
            model_id: req.model_id.clone(),
            served_model_id: served,
            capacity,
            template_kwargs,
        };
        mlx_remote::install(relay, route.clone()).map_err(|e| {
            refusal(
                "peerUnreachable",
                format!("publishing the route for this Mac's other windows failed: {e:#}"),
            )
        })?;
        super::mlx_engine::align_omlx_host_env();
        tracing::info!(
            peer = %route.peer_hostname,
            model = %route.model_id,
            served = %route.served_model_id,
            capacity = route.capacity,
            mounted = !already_serving,
            "mlx remote single: chat routed to the peer's engine through LeanZero Link"
        );
        Ok(route_status(Some(&manager), &route).await)
    }

    pub(super) async fn on_mlx_engine_remote_single_start(
        &self,
        req: MlxEngineRemoteSingleStartRequest,
    ) -> Result<MlxEngineRemoteSingleStartResponse, agent_client_protocol::Error> {
        Ok(match self.remote_single_start(&req).await {
            Ok(status) => MlxEngineRemoteSingleStartResponse {
                started: true,
                refusal: None,
                status,
            },
            Err(refused) => MlxEngineRemoteSingleStartResponse {
                started: false,
                refusal: Some(refused),
                status: current_status(super::link::existing_link_manager().as_deref()).await,
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
                route.pid, route.peer_hostname
            )));
        }
        let route = mlx_remote::uninstall()
            .internal_err_ctx("withdrawing the remote-single route record")?;
        super::mlx_engine::align_omlx_host_env();
        let (mut unmounted, mut unmount_error) = (false, None);
        if let (Some(route), false) = (&route, req.keep_mounted) {
            let outcome = match self.connected_link_manager().await {
                Ok(manager) => {
                    peer_op::<_, EmptyResponse>(
                        &manager,
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
                        route.peer_hostname, refused.message
                    ))
                }
            }
        }
        Ok(MlxEngineRemoteSingleStopResponse {
            unmounted,
            unmount_error,
            status: current_status(super::link::existing_link_manager().as_deref()).await,
        })
    }

    pub(super) async fn on_mlx_engine_remote_single_status(
        &self,
        _req: MlxEngineRemoteSingleStatusRequest,
    ) -> Result<MlxEngineRemoteSingleStatusResponse, agent_client_protocol::Error> {
        super::mlx_engine::align_omlx_host_env();
        let status = current_status(super::link::existing_link_manager().as_deref()).await;
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

/// The mesh hostname of `peer` (a node id or a hostname), from the live peer view.
async fn peer_hostname(
    manager: &LinkManager,
    peer: &str,
) -> Result<String, MlxRemoteSingleRefusalDto> {
    let registry = manager
        .active_registry()
        .await
        .ok_or_else(|| link_refusal(&LinkError::NotConnected))?;
    registry
        .peer_nodes()
        .into_iter()
        .find(|node| node.node_id == peer || node.hostname == peer)
        .map(|node| node.hostname)
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
            body: "chatServingDisabled: \"Allow this Mac to serve chat to linked devices\" is off on WorksMacStudio.lan".into(),
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
