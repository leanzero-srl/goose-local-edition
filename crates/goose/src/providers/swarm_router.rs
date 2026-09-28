//! The swarm's idle guard: routes ONE chat completion to an idle node of the configured pool.
//!
//! The `swarm` provider's `swarm` model is a chat model backed by whichever configured node has a
//! free slot. Sessions in one goosed share this process-wide router, so several chats draw on the
//! same slot accounting: a node's slots are a `tokio::sync::Semaphore` sized to its capacity, a
//! turn holds one permit for the life of its stream, and a turn that finds no free slot anywhere
//! QUEUES on every servable node's semaphore and takes the first permit that frees — no clock, no
//! cap (gate 5): the wait ends when a slot frees or the caller drops the stream.
//!
//! The pool is re-read from the `swarm` config key on every turn so edits take effect without a
//! restart. Nodes come in three kinds — LM Studio (via the `lmstudio` declarative provider at the
//! configured endpoint), the MLX sidecar (via `omlx`, servable only while the process-wide
//! [`goose_sidecar::engine::MlxEngineManager`] reports it running and serving the device's id) and
//! cloud devices (the registry provider for the device's family). A node that cannot serve is not
//! a candidate and its reason is carried into the error when nothing can serve.
//!
//! The model name picks the route (design DESIGN-NODES-AND-STRATEGIES.md §7.1): `swarm` is "Any
//! node (Auto)" over the pool above; `node:<id>` and `strategy:<id>[@<role>]` route along a chain
//! built from the `nodes` definitions, decided by the role's when-rule (`nodes::resolve`), with a
//! not-loaded MLX entry handed to the node loader through `nodes::seam`. Every lease — MLX, LM
//! Studio or cloud — leaves a served-turn record for its session (`nodes::served`).

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex, OnceLock};
use std::time::Instant;

use async_trait::async_trait;
use rmcp::model::{Role, Tool};
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::base::{MessageStream, Provider};
use super::mlx_distributed_owner;
use super::mlx_remote::{self, PublishedRoute, RouteRecord};
use super::mlx_serving_intent::{self, IntentRecord, ServingIntent};
use crate::config::{Config, ConfigError};
use crate::conversation::message::Message;
use crate::nodes::residency::ServingFacts;
use crate::nodes::resolve::{resolve, Decision, EntryFact, PassedOver, ShareState, Tried};
use crate::nodes::seam::{Demand, DemandFrom};
use crate::nodes::{
    NodeChainEntry, NodeDefKind, NodeIfNotLoaded, NodePlacement, NodeRole, NodeRoleEntry, NodeWhen,
    NodesReadResponse, ResolvedNodeDef, RouteModel,
};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use goose_sdk_types::custom_requests::{
    MlxPlacementKeyDto, MlxPlacementKindDto, NodeEnsureServing, NodeLoadRefusalCode,
    NodeServedTurnDto, NodeTriedDto, NodesServingKind,
};
use goose_sidecar::engine::{served_model_id, EngineSettings, EngineStatus};

const SWARM_CONFIG_KEY: &str = "swarm";
const LMSTUDIO_HOST_ENV: &str = "LMSTUDIO_HOST";
const LMSTUDIO_TOKEN_KEY: &str = "LMSTUDIO_API_KEY";
const OMLX_HOST_ENV: &str = "OMLX_HOST";
const MLX_ENGINE_CONFIG_KEY: &str = "mlx_engine";

/// The `swarm` config block, as much of it as routing needs. Mirrors `SwarmDevice` in
/// goose-cli's swarm.rs (id, model_id, weight, enabled, instances, provider, engine) plus the
/// block's `endpoint`; unknown fields are ignored so the engine's config stays the one source.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct PoolConfig {
    pub endpoint: String,
    pub devices: Vec<PoolDevice>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            // The engine's `default_endpoint` (goose-cli swarm.rs) and lmstudio.json's default.
            endpoint: "http://localhost:1234".to_string(),
            devices: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct PoolDevice {
    pub id: String,
    pub model_id: String,
    pub weight: u32,
    pub enabled: bool,
    pub instances: u32,
    /// Cloud family name (`bedrock`, `zai`, `google`, `deepseek`); `None`/`lmstudio` = local.
    pub provider: Option<String>,
    /// Local engine: `None`/`lmstudio` = LM Studio, `mlx-sidecar` = the supervised MLX engine.
    pub engine: Option<String>,
    /// The pool's Share — what the Share stepper writes and the build scheduler routes by. Chat's
    /// Auto reads it as its tie-break after free slots (D1). `None` = not set.
    pub speed_weight: Option<u32>,
}

impl Default for PoolDevice {
    fn default() -> Self {
        Self {
            id: String::new(),
            model_id: String::new(),
            weight: 0,
            enabled: false,
            instances: 1,
            provider: None,
            engine: None,
            speed_weight: None,
        }
    }
}

/// Swarm cloud family → goose provider-registry key: the `CLOUD_DEFS` rows
/// (crates/goose-cli/src/commands/swarm/cloud.rs) whose `name` and `registry` differ; every other
/// row is an identity. The engine's table is the source, and a parity test in its test module
/// (D7) runs every row through [`cloud_registry_name`], so a new row that differs fails the build
/// instead of misrouting.
const CLOUD_REGISTRY: &[(&str, &str)] = &[
    ("bedrock", "aws_bedrock"),
    ("zai", "zai"),
    ("google", "google"),
    ("deepseek", "custom_deepseek"),
];

pub fn cloud_registry_name(family: &str) -> &str {
    let lower = family.to_lowercase();
    CLOUD_REGISTRY
        .iter()
        .find(|(name, _)| *name == lower)
        .map(|(_, registry)| *registry)
        .unwrap_or(family)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeKind {
    LmStudio {
        endpoint: String,
    },
    MlxSidecar,
    /// The single engine on a LeanZero Link peer, reached through this Mac's relay to the
    /// peer's chat proxy (`mlx_remote`). Never from config: one node per live remote route.
    MlxRemote(RemoteTarget),
    Cloud {
        registry: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteTarget {
    /// The peer's Link node id (the route's `peer`) — its placement key is `link:<peer>`.
    pub peer: String,
    /// The peer by its one name (its owner's name for it, else its hostname) — what every reason
    /// says. The node id keeps the hostname.
    pub peer_name: String,
    /// The relay's base URL — it carries the relay's capability, so it never enters a reason.
    pub base_url: String,
    /// The peer profile's thinking choices for the served model.
    pub template_kwargs: Option<Map<String, Value>>,
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub id: String,
    pub model_id: String,
    pub weight: u32,
    /// The pool's Share (D1); `None` = not set.
    pub share: Option<u32>,
    pub capacity: u32,
    pub kind: NodeKind,
}

impl Node {
    /// Auto's tie-break after free slots. An unset Share reads as 1 — the build scheduler's own
    /// reading of an unset `speed_weight` — so a pool with equal or unset Shares keeps today's
    /// order (the heavier `weight`, as before).
    fn tie_share(&self) -> u32 {
        self.share.map_or(1, |s| s.max(1))
    }

    /// The goose provider that dispatches to this node, and the cache key that distinguishes two
    /// instances of the same provider aimed at different hosts (the declarative providers read
    /// their host env at creation, so one instance per host is the correct unit).
    fn provider_name(&self) -> &str {
        match &self.kind {
            NodeKind::LmStudio { .. } => "lmstudio",
            NodeKind::MlxSidecar | NodeKind::MlxRemote(_) => "omlx",
            NodeKind::Cloud { registry } => registry,
        }
    }

    fn provider_cache_key(&self) -> String {
        match &self.kind {
            NodeKind::LmStudio { endpoint } => format!("lmstudio@{endpoint}"),
            NodeKind::MlxSidecar => format!(
                "omlx@{}",
                std::env::var(OMLX_HOST_ENV).unwrap_or_else(|_| "unset".to_string())
            ),
            NodeKind::MlxRemote(target) => format!("omlx-remote@{}", target.base_url),
            NodeKind::Cloud { registry } => registry.clone(),
        }
    }
}

/// Turn the config into nodes. Only enabled devices are nodes.
pub(crate) fn nodes_from_config(cfg: &PoolConfig) -> Vec<Node> {
    cfg.devices
        .iter()
        .filter(|d| d.enabled)
        .map(|d| node_from_device(&cfg.endpoint, d))
        .collect()
}

/// One device as a node. Its kind is decided the way the engine decides it (a cloud `provider`
/// wins, then `engine`, then LM Studio).
fn node_from_device(endpoint: &str, d: &PoolDevice) -> Node {
    let kind = match (
        d.provider
            .as_deref()
            .filter(|p| !p.eq_ignore_ascii_case("lmstudio")),
        d.engine.as_deref(),
    ) {
        (Some(family), _) => NodeKind::Cloud {
            registry: cloud_registry_name(family).to_string(),
        },
        (None, Some("mlx-sidecar")) => NodeKind::MlxSidecar,
        (None, _) => NodeKind::LmStudio {
            endpoint: endpoint.to_string(),
        },
    };
    let capacity = match kind {
        // The sidecar's admission cap is the engine's, not the device's instance count.
        NodeKind::MlxSidecar => goose_sidecar::engine::MAX_CONCURRENT_REQUESTS,
        _ => d.instances.max(1),
    };
    Node {
        id: d.id.clone(),
        model_id: d.model_id.clone(),
        weight: d.weight,
        share: d.speed_weight,
        capacity,
        kind,
    }
}

/// The pool plus the live remote route's node. The route's node takes the heaviest configured
/// Share and weight, so a tie on free slots goes to the placement the user chose; its capacity is
/// the peer's own admission cap.
pub(crate) fn with_remote_route(mut nodes: Vec<Node>, route: Option<&PublishedRoute>) -> Vec<Node> {
    if let Some(route) = route {
        let weight = nodes.iter().map(|n| n.weight).max().unwrap_or(1);
        let share = nodes.iter().filter_map(|n| n.share).max();
        nodes.push(Node {
            id: route.node_id(),
            model_id: route.served_model_id.clone(),
            weight,
            share,
            capacity: route.capacity,
            kind: NodeKind::MlxRemote(RemoteTarget {
                peer: route.peer.clone(),
                peer_name: route.peer_name().to_string(),
                base_url: route.base_url.clone(),
                template_kwargs: route.template_kwargs.clone(),
            }),
        });
    }
    nodes
}

/// Why this Mac's own sidecar node is not a candidate while chat is routed to a peer, or
/// `None` when no route is up. An unreadable route record refuses to guess which engine serves.
fn sidecar_routed_away(record: &RouteRecord) -> Option<String> {
    match record {
        RouteRecord::Mine(route) | RouteRecord::Other(route) => Some(format!(
            "this Mac's MLX chat is served from {} (remote single, node {}) — stop it to use this Mac's own engine",
            route.peer_name(),
            route.node_id()
        )),
        RouteRecord::Unreadable { path, error } => Some(format!(
            "the remote-single route record {} is unreadable ({error}); which engine serves this Mac's chat is unknown",
            path.display()
        )),
        RouteRecord::Absent | RouteRecord::Stale(_) => None,
    }
}

/// A missing or unreadable `swarm` block is a named error, never an empty pool.
pub(crate) fn load_pool() -> Result<PoolConfig, ProviderError> {
    match Config::global().get_param::<PoolConfig>(SWARM_CONFIG_KEY) {
        Ok(cfg) => Ok(cfg),
        Err(ConfigError::NotFound(_)) => Err(ProviderError::ExecutionError(
            "swarm chat: no `swarm` block in config.yaml — add your nodes under `swarm.devices` \
             (Swarm settings) before selecting the swarm model"
                .to_string(),
        )),
        Err(e) => Err(ProviderError::ExecutionError(format!(
            "swarm chat: the `swarm` config block could not be read ({e}); nothing was routed"
        ))),
    }
}

/// What a probe learns about a SERVABLE node at pick time: the engine's own in-flight count when
/// it reports one (the MLX sidecar's `/v1/status`, cross-process truth) and the loaded context
/// window when the catalog carries it. Both `None` when the source did not say — never a default.
/// An MLX sidecar node also carries the id its engine serves (`serves`, what the routed call must
/// name), and `follows` = the model the node is set to when it serves the owner's own start of a
/// different model instead (see [`mlx_node_verdict`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Servable {
    pub live_in_flight: Option<u32>,
    pub context_window: Option<u64>,
    pub serves: Option<String>,
    pub follows: Option<String>,
}

/// `Ok(facts)` = servable; `Err(reason)` = not a candidate, and the reason is what the operator reads.
#[async_trait]
pub(crate) trait NodeProbe: Send + Sync {
    async fn probe(&self, node: &Node) -> Result<Servable, String>;
}

/// Where a node's provider instance comes from. Real: `crate::providers::create`, cached per
/// provider+host; tests inject fakes.
#[async_trait]
pub(crate) trait ProviderSource: Send + Sync {
    async fn provider_for(&self, node: &Node) -> Result<Arc<dyn Provider>, String>;
}

/// True when the operator exported the variable before goosed started — their value wins forever.
/// Otherwise the router owns it and keeps it aligned to the pool (the same rule
/// `acp::server::mlx_engine::align_omlx_host_env` applies to OMLX_HOST).
static LMSTUDIO_HOST_USER_OWNED: OnceLock<bool> = OnceLock::new();
static OMLX_HOST_USER_OWNED: OnceLock<bool> = OnceLock::new();

fn align_host_env(var: &'static str, owned: &OnceLock<bool>, value: &str) {
    let user_owned = *owned.get_or_init(|| std::env::var_os(var).is_some());
    if user_owned {
        if let Ok(current) = std::env::var(var) {
            if current != value {
                tracing::warn!(
                    target: "swarm_router",
                    var,
                    current = %current,
                    pool = %value,
                    "host env was exported before goosed started and differs from the pool's; the exported value wins"
                );
            }
        }
        return;
    }
    std::env::set_var(var, value);
}

pub(crate) struct LiveProviders {
    cache: tokio::sync::Mutex<HashMap<String, Arc<dyn Provider>>>,
}

impl LiveProviders {
    fn new() -> Self {
        Self {
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ProviderSource for LiveProviders {
    async fn provider_for(&self, node: &Node) -> Result<Arc<dyn Provider>, String> {
        if let NodeKind::LmStudio { endpoint } = &node.kind {
            align_host_env(LMSTUDIO_HOST_ENV, &LMSTUDIO_HOST_USER_OWNED, endpoint);
        }
        let key = node.provider_cache_key();
        let mut cache = self.cache.lock().await;
        if let Some(p) = cache.get(&key) {
            return Ok(p.clone());
        }
        let p = match &node.kind {
            NodeKind::MlxRemote(target) => remote_provider(target)?,
            _ => {
                let name = node.provider_name();
                crate::providers::create(name, vec![])
                    .await
                    .map_err(|e| format!("creating the '{name}' provider: {e}"))?
            }
        };
        cache.insert(key, p.clone());
        Ok(p)
    }
}

/// The `omlx` provider aimed at ONE relay rather than at `OMLX_HOST` (one per process): the same
/// declarative definition, its base URL replaced before it is built, so the remote node has its
/// own instance and host while the local engine keeps `OMLX_HOST`.
fn remote_provider(target: &RemoteTarget) -> Result<Arc<dyn Provider>, String> {
    let mut config = crate::config::declarative_providers::load_provider("omlx")
        .map_err(|e| format!("loading the 'omlx' provider definition: {e}"))?
        .config;
    config.base_url = format!("{}/v1/chat/completions", target.base_url);
    config.env_vars = None;
    let provider = super::openai_def::from_custom_config(config, None).map_err(|e| {
        format!(
            "creating the provider for {}'s engine: {e}",
            target.peer_name
        )
    })?;
    Ok(Arc::new(provider))
}

/// The real servability probe: LM Studio's `/v1/models` must list the device's model id, the MLX
/// manager must report `running` with the device's served id, a cloud node must create.
pub(crate) struct LiveProbe {
    http: reqwest::Client,
    providers: Arc<LiveProviders>,
}

impl LiveProbe {
    fn lm_api_token() -> Option<String> {
        match Config::global().get_secret::<String>(LMSTUDIO_TOKEN_KEY) {
            Ok(k) if !k.trim().is_empty() => Some(k),
            Ok(_) | Err(ConfigError::NotFound(_)) => None,
            Err(e) => {
                tracing::warn!(
                    target: "swarm_router",
                    error = %e,
                    "{LMSTUDIO_TOKEN_KEY} could not be read from the secret store; probing LM Studio without a bearer"
                );
                None
            }
        }
    }

    async fn get_json(
        &self,
        url: &str,
        token: Option<String>,
    ) -> Result<serde_json::Value, String> {
        let mut req = self.http.get(url);
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("{url} unreachable ({e})"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("GET {url} answered {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("GET {url} returned an unparseable body ({e})"))
    }

    /// Servability is `/v1/models` listing the id (what the endpoint will actually serve). The
    /// context window comes from the same entry when it carries one (`context_window`, the
    /// rapid-mlx spelling; `max_context_length`), else from LM Studio's native `/api/v0/models`
    /// `loaded_context_length` — the key the swarm engine's residency probe reads. A catalog that
    /// says nothing leaves the window unknown; it is never guessed.
    async fn probe_lmstudio(&self, endpoint: &str, model_id: &str) -> Result<Servable, String> {
        let host = endpoint.trim_end_matches('/');
        let url = format!("{host}/v1/models");
        let body = self.get_json(&url, Self::lm_api_token()).await?;
        let entry = body
            .get("data")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| format!("GET {url} carried no `data` model list"))?
            .iter()
            .find(|m| m.get("id").and_then(serde_json::Value::as_str) == Some(model_id))
            .ok_or_else(|| format!("model '{model_id}' is not listed by {url}"))?;
        let mut context_window = ["context_window", "max_context_length"]
            .iter()
            .find_map(|key| entry.get(key).and_then(serde_json::Value::as_u64));
        if context_window.is_none() {
            let native = format!("{host}/api/v0/models");
            match self.get_json(&native, Self::lm_api_token()).await {
                Ok(catalog) => {
                    context_window = catalog
                        .get("data")
                        .and_then(serde_json::Value::as_array)
                        .and_then(|a| {
                            a.iter().find(|m| {
                                m.get("id").and_then(serde_json::Value::as_str) == Some(model_id)
                            })
                        })
                        .and_then(|m| m.get("loaded_context_length"))
                        .and_then(serde_json::Value::as_u64);
                }
                Err(e) => tracing::warn!(
                    target: "swarm_router",
                    model = model_id,
                    error = %e,
                    "LM Studio's native catalog did not answer; the node's context window stays unknown"
                ),
            }
        }
        Ok(Servable {
            context_window,
            ..Servable::default()
        })
    }

    /// THE NODE IS THE TRUTH, NOT THIS PROCESS'S MANAGER. The engine is mounted by the desktop's
    /// goosed; a second goosed (each window holds its own goose-serve) or the CLI has a manager that
    /// knows nothing — measured 2026-09-05 13:20: with the engine idle on :8090 the CLI was told
    /// "MLX engine is stopped". So the decision comes from probing the engine's own HTTP surface at
    /// the base URL the local manager reports when it is the one running it, else the configured
    /// port; the local manager only enriches the reason when nothing listens.
    async fn probe_mlx(&self, node: &Node) -> Result<Servable, String> {
        if let Some(reason) = sidecar_routed_away(&mlx_remote::read()) {
            return Err(reason);
        }
        let stale = match distributed_target(
            mlx_distributed_owner::own_active_base_url(),
            mlx_distributed_owner::read(),
        )? {
            DistributedTarget::At { base, diagnostic } => {
                return judge_mlx_node(node, self.probe_mlx_at(&base, &diagnostic).await?);
            }
            DistributedTarget::None { stale } => stale,
        };
        let local = goose_sidecar::engine::global_manager().status().await;
        let base = mlx_base_url(
            &local,
            Config::global().get_param::<EngineSettings>(MLX_ENGINE_CONFIG_KEY),
        )?;
        let mut diagnostic = match (&local.state, &local.last_error) {
            (state, Some(err)) => format!("this process's manager: {state}, {err}"),
            (state, None) => format!("this process's manager: {state}"),
        };
        if let Some(stale) = stale {
            diagnostic.push_str("; ");
            diagnostic.push_str(&stale);
        }
        judge_mlx_node(node, self.probe_mlx_at(&base, &diagnostic).await?)
    }

    /// The engine at `base` as it answers: servable facts with `serves` = the id `/v1/models`
    /// lists. Whether a node may take it is [`judge_mlx_node`]'s.
    async fn probe_mlx_at(&self, base: &str, local_diagnostic: &str) -> Result<Servable, String> {
        let models_url = format!("{base}/v1/models");
        let resp = self.http.get(&models_url).send().await.map_err(|e| {
            format!(
                "MLX engine is not listening on {base} — mount it in the MLX window ({local_diagnostic}; {e})"
            )
        })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("GET {models_url} answered {status}"));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| format!("GET {models_url} body unreadable ({e})"))?;
        let (served, context_window, _parser) =
            goose_sidecar::engine::parse_model_info(&body).map_err(|e| format!("{e:#}"))?;
        let served = served.ok_or_else(|| {
            format!("MLX engine on {base} lists a model without an id in {models_url}")
        })?;
        let status_url = format!("{base}/v1/status");
        let live_in_flight = match self.http.get(&status_url).send().await {
            Ok(resp) => match resp.text().await {
                Ok(body) => goose_sidecar::engine::parse_active_requests(&body)
                    .map_err(|e| format!("{e:#}")),
                Err(e) => Err(format!("GET {status_url} body unreadable ({e})")),
            },
            Err(e) => Err(format!("GET {status_url} failed ({e})")),
        };
        let live_in_flight = match live_in_flight {
            Ok(n) => Some(n),
            Err(err) => {
                tracing::warn!(
                    target: "swarm_router",
                    error = %err,
                    "MLX engine reported no in-flight count; routing on in-process leases alone"
                );
                None
            }
        };
        align_host_env(OMLX_HOST_ENV, &OMLX_HOST_USER_OWNED, base);
        Ok(Servable {
            live_in_flight,
            context_window,
            serves: Some(served),
            follows: None,
        })
    }

    /// A peer's engine through the relay. Servable = the peer's `/v1/models` lists the route's
    /// served id; in-flight = the peer engine's own `/v1/status`. A refusal carries the peer's
    /// named reason (its `chatServingDisabled` 403, `engineUnreachable` 502, the relay's
    /// `linkRelayFailed`) and names the peer — never the relay URL, which holds its capability.
    async fn probe_remote(
        &self,
        target: &RemoteTarget,
        model_id: &str,
    ) -> Result<Servable, String> {
        let peer = &target.peer_name;
        let get = |path: &'static str| {
            let url = format!("{}/{path}", target.base_url);
            let http = self.http.clone();
            async move {
                let resp = http
                    .get(url)
                    .send()
                    .await
                    .map_err(|e| format!("this Mac's Link relay did not answer ({e})"))?;
                let status = resp.status();
                let body = resp
                    .text()
                    .await
                    .map_err(|e| format!("{path} body unreadable ({e})"))?;
                if status.is_success() {
                    Ok(body)
                } else {
                    Err(format!("{path} answered {status}: {}", body.trim()))
                }
            }
        };
        let models = get("v1/models")
            .await
            .map_err(|e| format!("{peer}'s MLX engine is not serving through Link — {e}"))?;
        let (served, context_window, _parser) =
            goose_sidecar::engine::parse_model_info(&models).map_err(|e| format!("{e:#}"))?;
        match served.as_deref() {
            Some(served) if served == model_id => {}
            Some(served) => {
                return Err(format!(
                    "{peer}'s MLX engine serves '{served}', the route wants '{model_id}'"
                ))
            }
            None => return Err(format!("{peer}'s MLX engine lists a model without an id")),
        }
        let live_in_flight = match get("v1/status").await.and_then(|body| {
            goose_sidecar::engine::parse_active_requests(&body).map_err(|e| format!("{e:#}"))
        }) {
            Ok(n) => Some(n),
            Err(err) => {
                tracing::warn!(
                    target: "swarm_router",
                    peer = %peer,
                    error = %err,
                    "the peer's MLX engine reported no in-flight count; routing on in-process leases alone"
                );
                None
            }
        };
        Ok(Servable {
            live_in_flight,
            context_window,
            ..Servable::default()
        })
    }
}

/// The node against what its engine serves, read against this Mac's live `mlx_engine` block and
/// serving intent (see [`mlx_node_verdict`]).
fn judge_mlx_node(node: &Node, mut facts: Servable) -> Result<Servable, String> {
    if let Some(served) = &facts.serves {
        let repo = goose_sidecar::model_identity::served_repo(&mlx_settings()?, served);
        facts.follows = mlx_node_verdict(
            &node.id,
            &node.model_id,
            served,
            &repo,
            &mlx_serving_intent::read(),
        )?;
    }
    Ok(facts)
}

/// The `mlx_engine` block the served id is read against: its alias names ONE model. No block =
/// no alias; an unreadable block is a named reason — which model the alias names is unknown.
fn mlx_settings() -> Result<EngineSettings, String> {
    match Config::global().get_param::<EngineSettings>(MLX_ENGINE_CONFIG_KEY) {
        Ok(settings) => Ok(settings),
        Err(ConfigError::NotFound(_)) => Ok(EngineSettings::default()),
        Err(e) => Err(format!(
            "the `{MLX_ENGINE_CONFIG_KEY}` config block is unreadable ({e}); which model the engine's served id names is unknown"
        )),
    }
}

/// How an MLX sidecar node stands to the model its engine serves as `served` (loaded from `repo`):
/// `Ok(None)` = the node names that model, in any of its forms (`model_identity`); `Ok(Some(set))`
/// = it does not, but this Mac's owner STARTED that model through goose (the single engine's
/// Mount, the split's start — every way, the tray and the relaunch restore included, records the
/// serving intent), so chat follows the owner's start and `set` — the node's `model_id`, the swarm
/// build pool's and the benchmark's pin — is left exactly as the user wrote it; `Err` = a model
/// nobody started here (a Link peer's Mount on this Mac, a build's mount), which the node was not
/// set to: a real mismatch, said in the words the desktop's notice parses.
///
/// Why the router follows rather than the start rewriting `swarm.devices` (Q-128): the device
/// list is the build pool a `goose swarm` run and a benchmark export read, and a chat model switch
/// must not silently change what the next benchmark measures. Following is never silent either —
/// the routed call and its usage name the served model, which is what the chat chip shows.
fn mlx_node_verdict(
    node_id: &str,
    node_model: &str,
    served: &str,
    repo: &str,
    intent: &IntentRecord,
) -> Result<Option<String>, String> {
    if goose_sidecar::model_identity::node_names_model(node_id, node_model, served, repo) {
        return Ok(None);
    }
    let mismatch = format!("MLX engine serves '{served}', the device wants '{node_model}'");
    match intent {
        IntentRecord::Present(
            ServingIntent::Single { model_id } | ServingIntent::Split { model_id },
        ) if model_id == repo || model_id == served => Ok(Some(node_model.to_string())),
        IntentRecord::Unreadable { path, error } => Err(format!(
            "{mismatch} — whether this Mac's owner started it is unknown: the serving record {} is unreadable ({error})",
            path.display()
        )),
        _ => Err(format!(
            "{mismatch} — it was not started from this Mac's goose, so chat does not follow it"
        )),
    }
}

#[derive(Debug, PartialEq)]
enum DistributedTarget {
    /// The distributed engine owns this Mac: the sidecar node is served by it (its wrapper answers
    /// /v1/models with the served id and /v1/status with the in-flight count — the surface
    /// `probe_mlx_at` reads).
    At { base: String, diagnostic: String },
    /// No distributed engine owns this Mac; `stale` names a record whose goosed is gone.
    None { stale: Option<String> },
}

/// THIS process's supervised run first; else the run another window's goosed published (each
/// desktop window runs its own goosed, and only one of them supervises the ranks). A stale record
/// is named and ignored; an unreadable one refuses to guess which engine owns the Mac.
fn distributed_target(
    own_active_base: Option<String>,
    record: mlx_distributed_owner::OwnerRecord,
) -> Result<DistributedTarget, String> {
    use mlx_distributed_owner::OwnerRecord;
    if let Some(base) = own_active_base {
        return Ok(DistributedTarget::At {
            base,
            diagnostic: "the distributed MLX engine owns this Mac".to_string(),
        });
    }
    match record {
        OwnerRecord::Other(engine) => Ok(DistributedTarget::At {
            diagnostic: format!(
                "the distributed MLX engine of another window (goosed pid {}) owns this Mac",
                engine.pid
            ),
            base: engine.base_url,
        }),
        OwnerRecord::Stale(engine) => {
            let stale = format!(
                "a stale distributed-engine record names goosed pid {} ({}), which is gone — ignored",
                engine.pid, engine.base_url
            );
            tracing::warn!(target: "swarm_router", "{stale}");
            Ok(DistributedTarget::None { stale: Some(stale) })
        }
        OwnerRecord::Unreadable { path, error } => Err(format!(
            "the distributed-engine owner record {} is unreadable ({error}); which MLX engine owns this Mac is unknown",
            path.display()
        )),
        OwnerRecord::Mine(_) | OwnerRecord::Absent => Ok(DistributedTarget::None { stale: None }),
    }
}

/// Where the sidecar node listens: the local manager's base URL while THIS process runs the
/// engine, else the configured `mlx_engine.port` on loopback (the default port when no block was
/// written). An unreadable block is a named reason — pointing the probe at the default port would
/// impersonate a configuration the operator wrote and we could not read.
fn mlx_base_url(
    local: &EngineStatus,
    settings: Result<EngineSettings, ConfigError>,
) -> Result<String, String> {
    if local.state == "running" {
        if let Some(base) = &local.base_url {
            return Ok(base.clone());
        }
    }
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

#[async_trait]
impl NodeProbe for LiveProbe {
    async fn probe(&self, node: &Node) -> Result<Servable, String> {
        match &node.kind {
            NodeKind::LmStudio { endpoint } => self.probe_lmstudio(endpoint, &node.model_id).await,
            NodeKind::MlxSidecar => self.probe_mlx(node).await,
            NodeKind::MlxRemote(target) => self.probe_remote(target, &node.model_id).await,
            NodeKind::Cloud { .. } => self
                .providers
                .provider_for(node)
                .await
                .map(|_| Servable::default()),
        }
    }
}

/// A probed, servable node: the node, its slots, its free slots and its context window.
type Slot = (Node, Arc<Semaphore>, u32, Option<u64>);

/// A slot on a node, held for the life of the stream it serves.
pub(crate) struct Lease {
    pub node: Node,
    _permit: OwnedSemaphorePermit,
    /// The node is the local MLX engine: this turn is listed as in flight on it for exactly the
    /// lease's life (see `mlx_serving`).
    _serving: Option<super::mlx_serving::ServingGuard>,
    /// The node's own context window as its probe read it at this pick; `None` = it did not say.
    pub context_window: Option<u64>,
}

pub(crate) struct Router {
    /// Node → its slots. Keyed by id AND capacity so a capacity edit mints a fresh semaphore and
    /// the old one drains on its own.
    slots: StdMutex<HashMap<String, Arc<Semaphore>>>,
    /// Conversation key → the node that last served it.
    sticky: StdMutex<HashMap<u64, String>>,
    /// A strategy role's smooth weighted round-robin state (`strategy:<id>@<role>` → weights),
    /// committed only when its pick is leased.
    shares: StdMutex<HashMap<String, ShareState>>,
    queued: AtomicUsize,
    /// The smallest context window among the servable nodes at the last pick; 0 = no node said.
    /// What `get_context_limit` hands goose so its own compaction fires before the node's wall.
    last_pool_context_limit: AtomicU32,
    /// Whether a pick or a measurement has read a servable pool at all — so "no node said" is
    /// told apart from "nobody has looked yet".
    pool_measured: std::sync::atomic::AtomicBool,
}

impl Router {
    pub(crate) fn new() -> Self {
        Self {
            slots: StdMutex::new(HashMap::new()),
            sticky: StdMutex::new(HashMap::new()),
            shares: StdMutex::new(HashMap::new()),
            queued: AtomicUsize::new(0),
            last_pool_context_limit: AtomicU32::new(0),
            pool_measured: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The pool's context limit as of the last pick, `None` until a servable node has reported one.
    pub(crate) fn pool_context_limit(&self) -> Option<usize> {
        match self.last_pool_context_limit.load(Ordering::SeqCst) {
            0 => None,
            n => Some(n as usize),
        }
    }

    fn record_pool_window(&self, smallest_window: Option<u64>) {
        let limit = smallest_window.map_or(0, |w| u32::try_from(w).unwrap_or(u32::MAX));
        self.last_pool_context_limit.store(limit, Ordering::SeqCst);
        self.pool_measured.store(true, Ordering::SeqCst);
    }

    /// The pool's window as `get_context_limit` answers it: the last pick's, else measured NOW by
    /// the probes a pick runs (and its one-node-per-engine rule), without taking a slot. Before
    /// this, a fresh goosed answered the default for the unknown model name "swarm" — 128,000 —
    /// until its first pick, and that number was saved on every session (Q-18). `Err` names why
    /// no window is known; it is never a number standing in for one.
    pub(crate) async fn pool_context_window(
        &self,
        nodes: &[Node],
        probe: &dyn NodeProbe,
    ) -> Result<usize, String> {
        if let Some(limit) = self.pool_context_limit() {
            return Ok(limit);
        }
        if self.pool_measured.load(Ordering::SeqCst) {
            return Err(
                "no servable node reported its context window at the last pick".to_string(),
            );
        }
        let probes = futures::future::join_all(nodes.iter().map(|n| probe.probe(n))).await;
        let mut candidates: Vec<(&Node, Servable)> = Vec::new();
        let mut reasons: Vec<String> = Vec::new();
        for (node, outcome) in nodes.iter().zip(probes) {
            match outcome {
                Ok(facts) => candidates.push((node, facts)),
                Err(reason) => reasons.push(format!("{}: {reason}", node.id)),
            }
        }
        one_node_per_engine(&mut candidates, &mut reasons);
        if candidates.is_empty() {
            return Err(if reasons.is_empty() {
                "no enabled device is configured under `swarm.devices`".to_string()
            } else {
                format!("no node can serve — {}", reasons.join("; "))
            });
        }
        let smallest = candidates
            .iter()
            .filter_map(|(_, facts)| facts.context_window)
            .min();
        self.record_pool_window(smallest);
        match smallest {
            Some(window) => Ok(usize::try_from(window).unwrap_or(usize::MAX)),
            None => {
                let ids: Vec<&str> = candidates.iter().map(|(n, _)| n.id.as_str()).collect();
                Err(format!(
                    "no servable node reports its context window ({})",
                    ids.join(", ")
                ))
            }
        }
    }

    fn semaphore(&self, node: &Node) -> Arc<Semaphore> {
        let key = format!("{}@{}", node.id, node.capacity);
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key)
            .or_insert_with(|| Arc::new(Semaphore::new(node.capacity as usize)))
            .clone()
    }

    /// `stream` receives no session id, so the conversation is keyed by what is stable across its
    /// turns: the system prompt and the first user message.
    pub(crate) fn conversation_key(system: &str, messages: &[Message]) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        system.hash(&mut h);
        if let Some(first) = messages.iter().find(|m| m.role == Role::User) {
            first.as_concat_text().hash(&mut h);
        }
        h.finish()
    }

    /// Choose a node and take one of its slots. Sticky first; else the servable node with the most
    /// free slots (ties → the larger Share, then the higher weight); else queue on every servable
    /// node until a permit frees.
    /// `saturated` names nodes this turn already saw refuse admission.
    pub(crate) async fn pick(
        &self,
        nodes: &[Node],
        probe: &dyn NodeProbe,
        key: u64,
        saturated: &HashSet<String>,
    ) -> Result<Lease, ProviderError> {
        let probes = futures::future::join_all(nodes.iter().map(|n| probe.probe(n))).await;
        let mut candidates: Vec<(&Node, Servable)> = Vec::new();
        let mut reasons: Vec<String> = Vec::new();
        for (node, outcome) in nodes.iter().zip(probes) {
            match outcome {
                Ok(_) if saturated.contains(&node.id) => {
                    reasons.push(format!("{}: refused admission this turn", node.id));
                }
                Ok(facts) => candidates.push((node, facts)),
                Err(reason) => reasons.push(format!("{}: {reason}", node.id)),
            }
        }
        one_node_per_engine(&mut candidates, &mut reasons);
        let mut servable: Vec<Slot> = Vec::new();
        let mut smallest_window: Option<u64> = None;
        for (node, facts) in candidates {
            let slot = self.slot(node, facts);
            if let Some(window) = slot.3 {
                smallest_window = Some(smallest_window.map_or(window, |w| w.min(window)));
            }
            servable.push(slot);
        }
        if !servable.is_empty() {
            self.record_pool_window(smallest_window);
        }
        if servable.is_empty() {
            return Err(ProviderError::ExecutionError(format!(
                "swarm chat: no node can serve this turn — {}",
                if reasons.is_empty() {
                    "no enabled device is configured under `swarm.devices`".to_string()
                } else {
                    reasons.join("; ")
                }
            )));
        }

        let sticky_id = self
            .sticky
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned();
        let preferred = sticky_id
            .as_deref()
            .and_then(|id| {
                servable
                    .iter()
                    .find(|(n, _, free, _)| n.id == id && *free > 0)
            })
            .or_else(|| {
                servable
                    .iter()
                    .filter(|(_, _, free, _)| *free > 0)
                    .max_by_key(|(n, _, free, _)| (*free, n.tie_share(), n.weight))
            });
        if let Some((node, sem, free, window)) = preferred {
            if let Ok(permit) = sem.clone().try_acquire_owned() {
                return Ok(self.leased(node, *window, permit, *free, 0, key));
            }
        }
        self.queue_on(&servable, key).await
    }

    /// A probed node with its slots: the node (naming what its engine serves), its semaphore, its
    /// free slots (the larger of this process's leases and the engine's own in-flight count) and
    /// its context window.
    fn slot(&self, node: &Node, facts: Servable) -> Slot {
        let sem = self.semaphore(node);
        let leased = node.capacity.saturating_sub(sem.available_permits() as u32);
        let used = facts.live_in_flight.map_or(leased, |l| l.max(leased));
        let free = node.capacity.saturating_sub(used);
        let mut node = node.clone();
        if let Some(served) = facts.serves {
            if let Some(set) = &facts.follows {
                tracing::info!(
                    target: "swarm_router",
                    node = %node.id,
                    set_to = %set,
                    serves = %served,
                    "chat follows the model this Mac's owner started; the node's model_id is left as written"
                );
            }
            node.model_id = served;
        }
        (node, sem, free, facts.context_window)
    }

    /// Queue on every slot given and take the first permit that frees — no clock, no cap (gate 5).
    async fn queue_on(&self, servable: &[Slot], key: u64) -> Result<Lease, ProviderError> {
        let start = Instant::now();
        let waiting = Waiting::enter(&self.queued);
        tracing::info!(
            target: "swarm_router",
            nodes = %servable.iter().map(|(n, _, _, _)| n.id.as_str()).collect::<Vec<_>>().join(","),
            queue_depth = waiting.depth,
            "queued"
        );
        let waits = servable
            .iter()
            .map(|(_, sem, _, _)| Box::pin(sem.clone().acquire_owned()))
            .collect::<Vec<_>>();
        let (first, index, _rest) = futures::future::select_all(waits).await;
        drop(waiting);
        let permit = first.map_err(|e| {
            ProviderError::ExecutionError(format!("swarm chat: a node's slot pool closed ({e})"))
        })?;
        let (node, _, _, window) = &servable[index];
        Ok(self.leased(node, *window, permit, 0, start.elapsed().as_millis(), key))
    }

    fn leased(
        &self,
        node: &Node,
        context_window: Option<u64>,
        permit: OwnedSemaphorePermit,
        free_slots: u32,
        queued_ms: u128,
        key: u64,
    ) -> Lease {
        self.sticky
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, node.id.clone());
        tracing::info!(
            target: "swarm_router",
            node = %node.id,
            model = %node.model_id,
            free_slots,
            queued_ms = queued_ms as u64,
            queue_depth = self.queued.load(Ordering::SeqCst),
            "pick"
        );
        let peer = match &node.kind {
            NodeKind::MlxSidecar => Some(None),
            NodeKind::MlxRemote(target) => Some(Some(target.peer_name.as_str())),
            NodeKind::LmStudio { .. } | NodeKind::Cloud { .. } => None,
        };
        let serving = peer.map(|peer| {
            super::mlx_serving::register(
                super::mlx_serving::ServingVia::SwarmRouter,
                crate::session_context::current_session_id(),
                crate::background_work::current_kind(),
                node.provider_name(),
                &node.model_id,
                Some(&node.id),
                peer,
            )
        });
        Lease {
            node: node.clone(),
            _permit: permit,
            _serving: serving,
            context_window,
        }
    }
}

/// One caller waiting in `queue_on`, counted in `queue_depth` for exactly as long as it waits.
/// Q-400: the count was taken back only after the wait returned, so a caller dropped mid-wait — the
/// end-of-turn reviewer yielding to a user turn drops its call — was counted forever: #3r's
/// `queue_depth` climbed 0 → 77 over one chat while picks saw 8 free slots.
struct Waiting<'a> {
    queued: &'a AtomicUsize,
    depth: usize,
}

impl<'a> Waiting<'a> {
    fn enter(queued: &'a AtomicUsize) -> Self {
        let depth = queued.fetch_add(1, Ordering::SeqCst) + 1;
        Self { queued, depth }
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.queued.fetch_sub(1, Ordering::SeqCst);
    }
}

/// This Mac's MLX engine serves ONE model, so at most one sidecar node follows it: none when a node
/// names the served model (that node is the engine's), else the heaviest follower (the first on a
/// tie). Every follower set aside says which node chat goes to instead.
fn one_node_per_engine(candidates: &mut Vec<(&Node, Servable)>, reasons: &mut Vec<String>) {
    let named = candidates
        .iter()
        .find(|(n, f)| n.kind == NodeKind::MlxSidecar && f.follows.is_none())
        .map(|(n, _)| n.id.clone());
    let kept = match &named {
        Some(id) => id.clone(),
        None => match candidates
            .iter()
            .filter(|(_, f)| f.follows.is_some())
            .rev()
            .max_by_key(|(n, _)| n.weight)
        {
            Some((n, _)) => n.id.clone(),
            None => return,
        },
    };
    let instead = if named.is_some() {
        format!("chat goes to {kept}, which names it")
    } else {
        format!("chat follows it on {kept}")
    };
    candidates.retain(|(node, facts)| match (&facts.follows, &facts.serves) {
        (Some(set), Some(served)) if named.is_some() || node.id != kept => {
            reasons.push(format!(
                "{}: MLX engine serves '{served}', the device wants '{set}' — {instead}",
                node.id
            ));
            false
        }
        _ => true,
    });
}

static ROUTER: LazyLock<Router> = LazyLock::new(Router::new);

/// The shared router's pool window — what the provider reports to goose for `swarm` chat: the
/// last pick's, else measured now against the configured pool (see [`Router::pool_context_window`]).
pub(crate) async fn pool_context_window() -> Result<usize, String> {
    if let Some(limit) = ROUTER.pool_context_limit() {
        return Ok(limit);
    }
    let cfg = load_pool().map_err(|err| match err {
        ProviderError::ExecutionError(reason) => reason
            .strip_prefix("swarm chat: ")
            .unwrap_or(&reason)
            .to_string(),
        other => other.to_string(),
    })?;
    let nodes = with_remote_route(nodes_from_config(&cfg), mlx_remote::read().live().as_ref());
    ROUTER.pool_context_window(&nodes, &*PROBE).await
}
static PROVIDERS: LazyLock<Arc<LiveProviders>> = LazyLock::new(|| Arc::new(LiveProviders::new()));
static PROBE: LazyLock<LiveProbe> = LazyLock::new(|| LiveProbe {
    http: reqwest::Client::new(),
    providers: PROVIDERS.clone(),
});

/// The sidecar's admission refusal as it reaches this layer: Rapid-MLX answers `503 "Server is
/// busy (max concurrent requests reached)…"` past its cap (spelling from goose-cli's
/// `provider_failures::sidecar_admission_cap_refusal`); LM Studio's queue-full answer is a 503 too.
pub(crate) fn is_admission_refusal(err: &ProviderError) -> bool {
    let text = err.to_string().to_lowercase();
    (text.contains("server is busy") && text.contains("max concurrent")) || text.contains("503")
}

/// Route one chat turn: pick a node, delegate to its provider, and hold the slot until the
/// returned stream ends or is dropped. A node that refuses admission is set aside for this turn
/// and the next free node is tried; when none is left the refusal is returned unchanged so the
/// agent's own provider retry backs off. Content is never retried.
/// One chat turn as the provider received it.
pub(crate) struct Turn<'a> {
    pub model_config: &'a ModelConfig,
    pub system: &'a str,
    pub messages: &'a [Message],
    pub tools: &'a [Tool],
    /// The session's captured MLX thinking choices (see [`SessionTemplateKwargs`]).
    pub session: &'a SessionTemplateKwargs,
}

/// Where an MLX sidecar node's per-model thinking choices come from: the model profile of the HF
/// directory the node serves. `Ok(None)` = nothing to send.
pub(crate) trait TemplateKwargsSource: Send + Sync {
    fn template_kwargs(&self, served_model_id: &str) -> Result<Option<Map<String, Value>>, String>;
}

/// The live source: the persisted `mlx_engine` block the MLX window writes.
struct ConfiguredTemplateKwargs;

impl TemplateKwargsSource for ConfiguredTemplateKwargs {
    fn template_kwargs(&self, served_model_id: &str) -> Result<Option<Map<String, Value>>, String> {
        match Config::global().get_param::<EngineSettings>(MLX_ENGINE_CONFIG_KEY) {
            Ok(settings) => profile_template_kwargs(&settings, served_model_id),
            // No block: no profile exists, so there is no choice to send.
            Err(ConfigError::NotFound(_)) => Ok(None),
            Err(e) => Err(format!(
                "the `mlx_engine` config block could not be read ({e}), so the thinking choices for '{served_model_id}' are unknown; nothing was routed"
            )),
        }
    }
}

/// The served id names its HF directory the way `engine::served_model_id` made it: the alias
/// names the configured model, and every other served id IS its HF directory id (the alias applies
/// to its own model only). An alias with no configured model behind it cannot be tied to a
/// profile and sends nothing — unless some profile DOES carry thinking choices, which would then
/// be silently dropped: that is an error.
fn profile_template_kwargs(
    settings: &EngineSettings,
    served: &str,
) -> Result<Option<Map<String, Value>>, String> {
    let hf_id = match settings.served_model_name.as_deref() {
        Some(alias) if alias == served => settings
            .model_id
            .as_deref()
            .filter(|id| served_model_id(settings, id) == served),
        _ => Some(served),
    };
    if let Some(hf_id) = hf_id {
        return Ok(settings
            .model_profiles
            .get(hf_id)
            .and_then(goose_sidecar::thinking::chat_template_kwargs));
    }
    let configured: Vec<&str> = settings
        .model_profiles
        .iter()
        .filter(|(_, profile)| goose_sidecar::thinking::chat_template_kwargs(profile).is_some())
        .map(|(id, _)| id.as_str())
        .collect();
    if configured.is_empty() {
        return Ok(None);
    }
    Err(format!(
        "the MLX engine serves '{served}', which no model profile can be tied to (configured model {:?}, served name {:?}), while {} carry thinking choices that would be dropped",
        settings.model_id,
        settings.served_model_name,
        configured.join(", ")
    ))
}

/// One session's MLX thinking choices, captured the first time the session routes a turn to a
/// given served model and reused for every later turn. Effort rewrites the system prompt and the
/// switch changes both it and the generation prompt, so a profile edited mid-session must not
/// reach a running conversation — it would void the engine's prefix cache. A new session reads
/// the profile afresh.
#[derive(Default)]
pub(crate) struct SessionTemplateKwargs {
    captured: StdMutex<HashMap<String, Option<Map<String, Value>>>>,
}

impl SessionTemplateKwargs {
    fn for_model(
        &self,
        served_model_id: &str,
        source: &dyn TemplateKwargsSource,
    ) -> Result<Option<Map<String, Value>>, ProviderError> {
        let mut captured = self
            .captured
            .lock()
            .expect("session template kwargs poisoned");
        if let Some(kwargs) = captured.get(served_model_id) {
            return Ok(kwargs.clone());
        }
        let kwargs = source
            .template_kwargs(served_model_id)
            .map_err(|e| ProviderError::ExecutionError(format!("swarm chat: {e}")))?;
        captured.insert(served_model_id.to_string(), kwargs.clone());
        Ok(kwargs)
    }
}

/// A remote node's choices: the PEER's profile for the model it serves, read at route start.
struct PeerProfileKwargs(Option<Map<String, Value>>);

impl TemplateKwargsSource for PeerProfileKwargs {
    fn template_kwargs(&self, _: &str) -> Result<Option<Map<String, Value>>, String> {
        Ok(self.0.clone())
    }
}

/// Adds the kwargs under `request_params.chat_template_kwargs`, which the OpenAI format copies into
/// the body verbatim. A key the session's own request params already set wins.
fn add_template_kwargs(
    cfg: &mut ModelConfig,
    kwargs: Map<String, Value>,
) -> Result<(), ProviderError> {
    let params = cfg.request_params.get_or_insert_with(HashMap::new);
    match params
        .entry("chat_template_kwargs".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
    {
        Value::Object(existing) => {
            for (key, value) in kwargs {
                existing.entry(key).or_insert(value);
            }
            Ok(())
        }
        other => Err(ProviderError::ExecutionError(format!(
            "swarm chat: request_params.chat_template_kwargs is {other}, not an object, so the MLX model's thinking choices cannot be added"
        ))),
    }
}

/// A remote-single route whose engine is LOADING on its peer (`mounting`: a Run, or the route's
/// own restore after the peer relaunched). A turn that finds no node while it loads waits for the
/// load to end instead of failing — the promise the desktop's Loading bar makes ("a message waits
/// until it is ready"). The wait follows the route's own state, never a clock: `ready` → the turn
/// is routed; `failed`, `reconnecting` or a withdrawn route → the turn ends at once with the
/// route's words. A route that is not loading (`reconnecting` included: nothing says the peer
/// comes back) is never waited on. Installed by the ACP server, which owns the route's status.
#[async_trait]
pub(crate) trait RouteLoad: Send + Sync {
    /// `None` when the live route is not loading; else resolves when the load ends: `Ok(())` =
    /// the route serves, pick again; `Err(words)` = the turn's named error.
    async fn settle(&self) -> Option<Result<(), String>>;
}

/// No route status in this process (the CLI): nothing is waited on.
pub(crate) struct NoRouteLoad;

#[async_trait]
impl RouteLoad for NoRouteLoad {
    async fn settle(&self) -> Option<Result<(), String>> {
        None
    }
}

static ROUTE_LOAD: std::sync::OnceLock<Arc<dyn RouteLoad>> = std::sync::OnceLock::new();

pub(crate) fn install_route_load(load: Arc<dyn RouteLoad>) {
    let _ = ROUTE_LOAD.set(load);
}

/// Route one Auto turn over the pool: pick, delegate, and record the lease (see `route_chain` for
/// the node and strategy routes).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn route_stream(
    router: &Router,
    nodes: &[Node],
    probe: &dyn NodeProbe,
    providers: &dyn ProviderSource,
    kwargs_source: &dyn TemplateKwargsSource,
    route_load: &dyn RouteLoad,
    seam: &dyn NodesSeam,
    turn: Turn<'_>,
) -> Result<MessageStream, ProviderError> {
    let key = Router::conversation_key(turn.system, turn.messages);
    let mut saturated = HashSet::new();
    let mut last_refusal: Option<ProviderError> = None;
    loop {
        let lease = match router.pick(nodes, probe, key, &saturated).await {
            Ok(lease) => lease,
            Err(no_node) => {
                let routed_remote = nodes
                    .iter()
                    .any(|n| matches!(n.kind, NodeKind::MlxRemote(_)));
                if last_refusal.is_none() && routed_remote {
                    match route_load.settle().await {
                        Some(Ok(())) => continue,
                        Some(Err(words)) => return Err(ProviderError::ExecutionError(words)),
                        None => {}
                    }
                }
                return Err(last_refusal.unwrap_or(no_node));
            }
        };
        let Some(lease) = clear_of_queued_switches(seam, lease).await else {
            continue;
        };
        match stream_on(lease, providers, kwargs_source, &turn).await? {
            Streamed::Served(stream, node) => {
                note_served(seam, pool_lease_way(seam, &node), || NodeServedTurnDto {
                    node: node.id.clone(),
                    role: None,
                    rank: 1,
                    reason: None,
                    tried: Vec::new(),
                    loaded_ms: None,
                    at_ms: now_ms(),
                });
                return Ok(stream);
            }
            Streamed::Refused(node, e) => {
                saturated.insert(node);
                last_refusal = Some(e);
            }
        }
    }
}

/// The loader's step 1 for a lease (design §6.4): a switch queued before this turn's reply opened
/// is honoured by EVERY MLX lease, not only by a demand for a way that is not serving — otherwise
/// steady replies on the running way starve it. When one is queued, the lease is given back (its
/// slot and in-flight mark are not held while waiting), the reply waits behind the switch holding
/// nothing, and `None` sends the turn to route again on what serves after it. The set of switches
/// queued before a reply opened only shrinks, so the re-route ends. With nothing queued — and
/// always for LM Studio, cloud, no installed loader or a call outside any session — this is a
/// scan of the loader's queue and the lease goes on unchanged.
async fn clear_of_queued_switches(seam: &dyn NodesSeam, lease: Lease) -> Option<Lease> {
    if !matches!(
        lease.node.kind,
        NodeKind::MlxSidecar | NodeKind::MlxRemote(_)
    ) || !seam.loader_installed()
    {
        return Some(lease);
    }
    let Some(session) = crate::session_context::current_session_id() else {
        return Some(lease);
    };
    let node = lease.node.id.clone();
    if seam.queued_switch_ahead(&session, &node).is_none() {
        return Some(lease);
    }
    drop(lease);
    seam.wait_behind_queued_switches(&session, &node).await;
    None
}

/// What one lease's call came to.
enum Streamed {
    /// The node's stream, holding the lease for its life, and the node that serves it.
    Served(MessageStream, Node),
    /// The node refused admission (its id): set it aside for this turn and try the next.
    Refused(String, ProviderError),
}

/// Delegate the turn to the leased node's provider, naming the node's model and window and adding
/// its thinking choices. Content is never retried; only an admission refusal comes back to try the
/// next node.
async fn stream_on(
    lease: Lease,
    providers: &dyn ProviderSource,
    kwargs_source: &dyn TemplateKwargsSource,
    turn: &Turn<'_>,
) -> Result<Streamed, ProviderError> {
    let provider = providers
        .provider_for(&lease.node)
        .await
        .map_err(ProviderError::ExecutionError)?;
    let mut node_cfg = turn.model_config.clone();
    node_cfg.model_name = lease.node.model_id.clone();
    // The session's config is the `swarm` model's (an unknown name → the 128,000 default); the
    // routed call is the node's model, so it carries the node's own window or states none.
    node_cfg.context_limit = lease
        .context_window
        .map(|w| usize::try_from(w).unwrap_or(usize::MAX));
    let kwargs = match &lease.node.kind {
        NodeKind::MlxSidecar => turn
            .session
            .for_model(&lease.node.model_id, kwargs_source)?,
        NodeKind::MlxRemote(target) => turn.session.for_model(
            &lease.node.model_id,
            &PeerProfileKwargs(target.template_kwargs.clone()),
        )?,
        _ => None,
    };
    if let Some(kwargs) = kwargs {
        add_template_kwargs(&mut node_cfg, kwargs)?;
    }
    match provider
        .stream(&node_cfg, turn.system, turn.messages, turn.tools)
        .await
    {
        Ok(inner) => {
            let inner = if matches!(
                lease.node.kind,
                NodeKind::MlxSidecar | NodeKind::MlxRemote(_)
            ) {
                super::mlx_speed::observe(inner)
            } else {
                inner
            };
            let node = lease.node.clone();
            Ok(Streamed::Served(leased_stream(inner, lease), node))
        }
        Err(e) if is_admission_refusal(&e) => {
            tracing::warn!(
                target: "swarm_router",
                node = %lease.node.id,
                error = %e,
                "node refused admission; trying the next free node"
            );
            Ok(Streamed::Refused(lease.node.id.clone(), e))
        }
        Err(e) => Err(e),
    }
}

/// Holds the lease for exactly the life of the stream. A struct rather than a `move` closure on
/// purpose: an edition-2021 closure that names `lease.node.model_id` captures only that field path,
/// and the permit would be released before the first chunk (the failover test caught exactly that).
/// Usage names the node's model so the UI can show which node served.
struct LeasedStream {
    inner: MessageStream,
    lease: Lease,
}

impl futures::Stream for LeasedStream {
    type Item = <MessageStream as futures::Stream>::Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.inner.as_mut().poll_next(cx).map(|next| {
            next.map(|item| {
                item.map(|(message, usage)| {
                    let usage = usage.map(|mut u| {
                        u.model = this.lease.node.model_id.clone();
                        u
                    });
                    (message, usage)
                })
            })
        })
    }
}

fn leased_stream(inner: MessageStream, lease: Lease) -> MessageStream {
    Box::pin(LeasedStream { inner, lease })
}

/// The provider's entry point: the pool (Auto) or the node or strategy the model names, the live
/// probe, the shared router. `records_served` is false for the session-title call, which is not a
/// turn the chip reads.
pub(crate) async fn route_chat(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    session: &SessionTemplateKwargs,
    records_served: bool,
) -> Result<MessageStream, ProviderError> {
    let route = nodes_route(&model_config.model_name).map_err(ProviderError::ExecutionError)?;
    let seam = LiveNodesSeam { records_served };
    let turn = Turn {
        model_config,
        system,
        messages,
        tools,
        session,
    };
    let providers: &LiveProviders = &PROVIDERS;
    let Some(route) = route else {
        let cfg = load_pool()?;
        let nodes = with_remote_route(nodes_from_config(&cfg), mlx_remote::read().live().as_ref());
        if nodes.is_empty() {
            let disabled: Vec<String> = cfg
                .devices
                .iter()
                .map(|d| format!("{} (disabled)", d.id))
                .collect();
            return Err(ProviderError::ExecutionError(format!(
                "swarm chat: no enabled device under `swarm.devices` — {}",
                if disabled.is_empty() {
                    "the list is empty".to_string()
                } else {
                    disabled.join(", ")
                }
            )));
        }
        return route_stream(
            &ROUTER,
            &nodes,
            &*PROBE,
            providers,
            &ConfiguredTemplateKwargs,
            live_route_load(),
            &seam,
            turn,
        )
        .await;
    };
    let plan = live_plan(&route)
        .await
        .map_err(|e| ProviderError::ExecutionError(format!("swarm chat: {e}")))?;
    let members = LiveMembers::read(seam.loader_installed()).await;
    route_chain(
        &ROUTER,
        &plan,
        &members,
        &*PROBE,
        providers,
        &ConfiguredTemplateKwargs,
        live_route_load(),
        &seam,
        turn,
    )
    .await
}

fn live_route_load() -> &'static dyn RouteLoad {
    static NONE: NoRouteLoad = NoRouteLoad;
    match ROUTE_LOAD.get() {
        Some(load) => load.as_ref(),
        None => &NONE,
    }
}

/// The context window goose compacts against for `model`: the pool's for Auto (unchanged), the
/// node's own for `node:<id>`, the smallest in the chain for `strategy:<id>[@role]` — so goose
/// compacts before the smallest node's wall. `Err` names why no window is known; never a number
/// standing in for one.
pub(crate) async fn route_context_window(model: &str) -> Result<usize, String> {
    let Some(route) = nodes_route(model)? else {
        return pool_context_window().await;
    };
    let plan = live_plan(&route).await?;
    let members = LiveMembers::read(crate::nodes::seam::loader_installed()).await;
    chain_window(&plan, &members, &*PROBE).await
}

// ---------------------------------------------------------------------------------------------
// Routes to a node or a strategy (design §6.3, §7.1): `node:<id>`, `strategy:<id>` (its Chat
// chain) and `strategy:<id>@<role>` (a delegate's Build). The chain is built from the node
// definitions, never from `swarm.devices`; the same probes, one-node-per-engine rule, slots,
// stickiness and queueing apply; the role's when-rule decides through `nodes::resolve` (the rule
// the desktop's mirror runs on the same fixture); a not-loaded MLX entry under `load` goes to the
// node loader through the seam. A chain with nothing servable ends the turn naming every entry and
// its reason — it never falls to "any node" (gate 1).
// ---------------------------------------------------------------------------------------------

/// The route a model name selects on the `swarm` provider: `Ok(None)` = the pool (Auto — `swarm`,
/// the build ids and any other name, today's routing); `Ok(Some)` = a node or a strategy; `Err` =
/// a name that uses the nodes grammar and does not parse, refused by name rather than read as Auto
/// (a silent substitution).
fn nodes_route(name: &str) -> Result<Option<RouteModel>, String> {
    match crate::nodes::parse_route_model(name) {
        Some(route @ (RouteModel::Node { .. } | RouteModel::Strategy { .. })) => Ok(Some(route)),
        Some(_) => Ok(None),
        // The grammar's own prefixes (`nodes::parse_route_model`).
        None if ["node:", "strategy:", "swarm-build:"]
            .iter()
            .any(|prefix| name.starts_with(prefix)) =>
        {
            Err(format!(
                "swarm chat: '{name}' names no node or strategy (an id is non-empty and carries no ':' or '@'; a role is chat, planning, build, testing, frontend or backend); nothing was routed"
            ))
        }
        None => Ok(None),
    }
}

/// What routing reaches besides the probes: the node loader (through S0's seam), the loader's note
/// of the way a lease holds, and the served-turn record. Live = `crate::nodes`; tests use fakes.
#[async_trait]
pub(crate) trait NodesSeam: Send + Sync {
    async fn ensure_serving(&self, demand: Demand) -> NodeEnsureServing;
    fn loader_installed(&self) -> bool;
    fn note_lease(&self, session: &str, way: &MlxPlacementKeyDto);
    fn served(&self, session: &str, turn: NodeServedTurnDto);
    /// See `nodes::seam::queued_switch_ahead` (asked before every MLX lease).
    fn queued_switch_ahead(&self, session: &str, node: &str) -> Option<String>;
    async fn wait_behind_queued_switches(&self, session: &str, node: &str);
}

pub(crate) struct LiveNodesSeam {
    records_served: bool,
}

#[async_trait]
impl NodesSeam for LiveNodesSeam {
    async fn ensure_serving(&self, demand: Demand) -> NodeEnsureServing {
        crate::nodes::seam::ensure_serving(demand).await
    }

    fn loader_installed(&self) -> bool {
        crate::nodes::seam::loader_installed()
    }

    fn note_lease(&self, session: &str, way: &MlxPlacementKeyDto) {
        crate::nodes::seam::note_lease(session, way);
    }

    fn queued_switch_ahead(&self, session: &str, node: &str) -> Option<String> {
        crate::nodes::seam::queued_switch_ahead(session, node)
    }

    async fn wait_behind_queued_switches(&self, session: &str, node: &str) {
        crate::nodes::seam::wait_behind_queued_switches(session, node).await;
    }

    /// This process's record at once (the chip reads it at the turn's end), then the session's,
    /// off the turn's path so a slow write never delays the reply; a failed save is logged by name.
    fn served(&self, session: &str, turn: NodeServedTurnDto) {
        if !self.records_served {
            return;
        }
        crate::nodes::served::remember(session, turn.clone());
        let session = session.to_string();
        tokio::spawn(async move {
            let sessions = crate::session::SessionManager::instance();
            if let Err(e) = crate::nodes::served::record(&sessions, &session, turn).await {
                tracing::warn!(
                    target: "swarm_router",
                    session = %session,
                    error = %e,
                    "the served-turn record could not be saved in the session; the chip reads this process's record until a reload"
                );
            }
        });
    }
}

fn now_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(u64::MAX)
}

/// A lease's notes, for the session the turn belongs to: the loader's note of the way it holds
/// (when there is a way to name), and the served-turn record. A call outside any session (the CLI,
/// a probe) has no record to keep.
fn note_served(
    seam: &dyn NodesSeam,
    way: Option<MlxPlacementKeyDto>,
    record: impl FnOnce() -> NodeServedTurnDto,
) {
    let Some(session) = crate::session_context::current_session_id() else {
        return;
    };
    if let Some(way) = way {
        seam.note_lease(&session, &way);
    }
    seam.served(&session, record());
}

fn single_way(mac: String) -> MlxPlacementKeyDto {
    MlxPlacementKeyDto {
        kind: MlxPlacementKindDto::Single,
        nodes: vec![mac],
        link: None,
    }
}

/// Whether the split serves in this Mac's engine's place, and the way its owner published.
enum SplitNow {
    None,
    Serves(Result<Option<MlxPlacementKeyDto>, String>),
}

fn split_now() -> SplitNow {
    use mlx_distributed_owner::OwnerRecord;
    let split = mlx_distributed_owner::own_active_base_url().is_some()
        || matches!(
            mlx_distributed_owner::read(),
            OwnerRecord::Mine(_) | OwnerRecord::Other(_)
        );
    if split {
        SplitNow::Serves(mlx_distributed_owner::read_way())
    } else {
        SplitNow::None
    }
}

/// The way an Auto or follower lease on an MLX node holds, for the loader's note — read only while
/// a loader is installed (with none, a note does nothing). This Mac's single is `local`, a remote
/// single `link:<peer>`, the split the way its owner published (`read_way`), so a reply on the
/// split holds it and a switch waits for that reply like any other.
fn pool_lease_way(seam: &dyn NodesSeam, node: &Node) -> Option<MlxPlacementKeyDto> {
    if !seam.loader_installed() {
        return None;
    }
    let split = match node.kind {
        NodeKind::MlxSidecar => split_now(),
        _ => SplitNow::None,
    };
    lease_way(node, split)
}

/// A split whose record names no way (an older goose published it) or cannot be read leaves the
/// lease unnamed — said in the log, never guessed.
fn lease_way(node: &Node, split: SplitNow) -> Option<MlxPlacementKeyDto> {
    match &node.kind {
        NodeKind::MlxRemote(target) => {
            Some(single_way(crate::nodes::residency::peer_key(&target.peer)))
        }
        NodeKind::MlxSidecar => match split {
            SplitNow::None => Some(single_way(crate::nodes::THIS_MAC.to_string())),
            SplitNow::Serves(Ok(Some(way))) => Some(way),
            SplitNow::Serves(Ok(None)) => {
                tracing::warn!(
                    target: "swarm_router",
                    node = %node.id,
                    "this lease is on the split, whose owner record (written by a goose before the split's way was published) names no way; the loader is not told which way it holds"
                );
                None
            }
            SplitNow::Serves(Err(e)) => {
                tracing::warn!(
                    target: "swarm_router",
                    node = %node.id,
                    error = %e,
                    "this lease is on the split, whose owner record could not be read for its way; the loader is not told which way it holds"
                );
                None
            }
        },
        NodeKind::LmStudio { .. } | NodeKind::Cloud { .. } => None,
    }
}

/// A route's chain, read against the `nodes` config for this turn.
pub(crate) struct ChainPlan {
    /// How errors name the route: `"Flash · this Mac"` or `the strategy "Everyday" (chat)`.
    label: String,
    /// The role the chain serves; `None` for a `node:` route.
    role: Option<NodeRole>,
    /// Where the role's round-robin state lives (`strategy:<id>@<role>`).
    share_key: String,
    entry: NodeRoleEntry,
    defs: HashMap<String, ResolvedNodeDef>,
}

/// The chain `route` names. A `node:` route is a chain of that one node, and a node that cannot
/// serve ends the turn (the user chose exactly it). A removed node or strategy is named, never
/// replaced.
fn chain_plan(route: &RouteModel, read: &NodesReadResponse) -> Result<ChainPlan, String> {
    let defs: HashMap<String, ResolvedNodeDef> = read
        .nodes
        .iter()
        .map(|n| (n.def.id.clone(), n.clone()))
        .collect();
    match route {
        RouteModel::Node { id } => {
            let def = defs.get(id).ok_or_else(|| {
                format!("the node '{id}' was removed. Pick another node from the chip.")
            })?;
            Ok(ChainPlan {
                label: format!("\"{}\"", def.def.name),
                role: None,
                share_key: crate::nodes::format_route_model(route),
                entry: NodeRoleEntry {
                    // The weight is read only by `share`; a one-node chain fails over.
                    chain: vec![NodeChainEntry {
                        node: id.clone(),
                        weight: 1,
                    }],
                    when: NodeWhen::Failover,
                    if_not_loaded: NodeIfNotLoaded::Load,
                },
                defs,
            })
        }
        RouteModel::Strategy { id, role } => {
            let strategy = read
                .config
                .strategies
                .iter()
                .find(|s| &s.id == id)
                .ok_or_else(|| {
                    format!("the strategy '{id}' was removed. Pick another from the chip.")
                })?;
            let role = role.unwrap_or(NodeRole::Chat);
            let entry = crate::nodes::effective_entry(strategy, role).ok_or_else(|| {
                format!(
                    "the strategy \"{}\" sets neither Chat nor Build, so {} has no node",
                    strategy.name,
                    crate::nodes::role_str(role)
                )
            })?;
            // A chat's own node set (Q-359) is named for what it is to the person — never by the
            // strategy name the set carries only for uniqueness. Only the words differ.
            let label = match &strategy.chat {
                Some(_) => format!("this chat's nodes ({})", crate::nodes::role_str(role)),
                None => format!(
                    "the strategy \"{}\" ({})",
                    strategy.name,
                    crate::nodes::role_str(role)
                ),
            };
            Ok(ChainPlan {
                label,
                role: Some(role),
                share_key: crate::nodes::format_route_model(&RouteModel::Strategy {
                    id: id.clone(),
                    role: Some(role),
                }),
                entry: entry.clone(),
                defs,
            })
        }
        other => Err(format!(
            "'{}' is not a node or strategy route",
            crate::nodes::format_route_model(other)
        )),
    }
}

async fn live_plan(route: &RouteModel) -> Result<ChainPlan, String> {
    let read = crate::nodes::read(Config::global(), this_mac_name().await)
        .map_err(|e| format!("{e:#}; nothing was routed"))?;
    chain_plan(route, &read)
}

/// This Mac's name, read once per process (adoption names this Mac's engine node with it).
async fn this_mac_name() -> Result<String, String> {
    static NAME: tokio::sync::OnceCell<Result<String, String>> = tokio::sync::OnceCell::const_new();
    NAME.get_or_init(crate::nodes::acp::this_mac_name)
        .await
        .clone()
}

/// A chain node as routing reaches it now.
#[derive(Clone)]
pub(crate) struct Member {
    node: Node,
    /// The node names one way (not `follows`): an engine serving another model means it is not
    /// loaded — never followed.
    pinned: bool,
    /// The way a lease on it holds (the loader's note); `None` for cloud and for a follower.
    way: Option<MlxPlacementKeyDto>,
}

/// How a chain node is reached now: the routable node to probe, or the fact that says why there
/// is none. Live = the node definitions against the records of what serves; tests use fakes.
#[async_trait]
pub(crate) trait ChainMembers: Send + Sync {
    async fn member(&self, def: &ResolvedNodeDef) -> Result<Member, EntryFact>;
}

struct LiveMembers {
    pool: Result<Option<PoolConfig>, String>,
    mac_name: String,
    loader_installed: bool,
}

impl LiveMembers {
    async fn read(loader_installed: bool) -> Self {
        let pool = match Config::global().get_param::<PoolConfig>(SWARM_CONFIG_KEY) {
            Ok(cfg) => Ok(Some(cfg)),
            Err(ConfigError::NotFound(_)) => Ok(None),
            Err(e) => Err(format!("the `swarm` config block could not be read ({e})")),
        };
        // Only the words describing a serving way use the name (the residency read does the same).
        let mac_name = this_mac_name()
            .await
            .unwrap_or_else(|_| "This Mac".to_string());
        Self {
            pool,
            mac_name,
            loader_installed,
        }
    }
}

#[async_trait]
impl ChainMembers for LiveMembers {
    async fn member(&self, def: &ResolvedNodeDef) -> Result<Member, EntryFact> {
        if is_pinned(def) {
            let serving = crate::nodes::residency::serving_now(&self.mac_name).await;
            return pinned_member(
                def,
                &serving,
                mlx_remote::read().live().as_ref(),
                self.loader_installed,
            );
        }
        match &def.def.pool_device {
            Some(device) => pool_member(def, &self.pool, device),
            None => cloud_member(def),
        }
    }
}

fn cant_run(reason: impl Into<String>) -> EntryFact {
    EntryFact::CantRun {
        reason: reason.into(),
    }
}

fn is_pinned(def: &ResolvedNodeDef) -> bool {
    def.def.kind == NodeDefKind::Mlx
        && def
            .def
            .placement
            .as_ref()
            .is_some_and(|p| *p != NodePlacement::Follows)
}

/// An MLX node that names one way: routable only while THAT way serves this Mac's goose with its
/// model (one MLX way serves at a time, so any other way means it is not loaded).
fn pinned_member(
    def: &ResolvedNodeDef,
    serving: &ServingFacts,
    route: Option<&PublishedRoute>,
    loader_installed: bool,
) -> Result<Member, EntryFact> {
    let way = match serving {
        ServingFacts::Unknown(reason) => return Err(cant_run(reason.clone())),
        ServingFacts::Nothing => return Err(EntryFact::NotLoaded),
        ServingFacts::Way(way) if crate::nodes::residency::names_way(def, way) => way,
        ServingFacts::Way(_) => return Err(EntryFact::NotLoaded),
    };
    if let Some(phase) = &way.load_phase {
        // Its own way is mid-load (Run it, or the loader). The loader owns waiting on a load;
        // with none installed there is nothing to wait on, and its absence's words ("start it in
        // Run it") would be wrong for a way that is already starting.
        if loader_installed {
            return Err(EntryFact::NotLoaded);
        }
        return Err(cant_run(format!(
            "{} is still loading on this Mac ({phase}); it serves once the load ends",
            def.def.name
        )));
    }
    let model = def
        .model
        .clone()
        .ok_or_else(|| cant_run("it names no model"))?;
    let node = match way.kind {
        NodesServingKind::Single | NodesServingKind::Split => Node {
            id: def.def.id.clone(),
            model_id: model,
            weight: goose_sidecar::engine::MAX_CONCURRENT_REQUESTS,
            share: None,
            capacity: goose_sidecar::engine::MAX_CONCURRENT_REQUESTS,
            kind: NodeKind::MlxSidecar,
        },
        NodesServingKind::RemoteSingle => {
            let route = route.ok_or_else(|| {
                cant_run("the remote-single route ended while this turn was routed")
            })?;
            Node {
                id: def.def.id.clone(),
                model_id: route.served_model_id.clone(),
                weight: route.capacity,
                share: None,
                capacity: route.capacity,
                kind: NodeKind::MlxRemote(RemoteTarget {
                    peer: route.peer.clone(),
                    peer_name: route.peer_name().to_string(),
                    base_url: route.base_url.clone(),
                    template_kwargs: route.template_kwargs.clone(),
                }),
            }
        }
    };
    Ok(Member {
        node,
        pinned: true,
        way: crate::nodes::acp::node_key(def),
    })
}

/// A node adopted from the pool: its device, routed exactly as Auto routes it (an MLX device
/// follows this Mac's engine, Q-128). A device switched off in the pool is said, not used.
fn pool_member(
    def: &ResolvedNodeDef,
    pool: &Result<Option<PoolConfig>, String>,
    device_id: &str,
) -> Result<Member, EntryFact> {
    let cfg = match pool {
        Ok(Some(cfg)) => cfg,
        Ok(None) => {
            return Err(cant_run(
                "it reads from your swarm pool, and config.yaml has no `swarm` block",
            ))
        }
        Err(e) => return Err(cant_run(e.clone())),
    };
    let device = cfg
        .devices
        .iter()
        .find(|d| d.id == device_id)
        .ok_or_else(|| cant_run("it is no longer in your swarm pool"))?;
    if !device.enabled {
        return Err(cant_run("it is turned off in your swarm pool"));
    }
    let mut node = node_from_device(&cfg.endpoint, device);
    node.id = def.def.id.clone();
    Ok(Member {
        node,
        pinned: false,
        way: None,
    })
}

/// A cloud or endpoint node the user made: its provider and model. Its capacity is what a pool
/// device written without `instances` gets.
fn cloud_member(def: &ResolvedNodeDef) -> Result<Member, EntryFact> {
    if def.def.kind == NodeDefKind::Mlx {
        return Err(cant_run("it is an MLX node with no way to run"));
    }
    let model = def
        .model
        .clone()
        .ok_or_else(|| cant_run("it names no model"))?;
    let provider = def
        .provider
        .clone()
        .ok_or_else(|| cant_run("it names no provider"))?;
    let capacity = PoolDevice::default().instances;
    Ok(Member {
        node: Node {
            id: def.def.id.clone(),
            model_id: model,
            weight: capacity,
            share: None,
            capacity,
            kind: NodeKind::Cloud {
                registry: cloud_registry_name(&provider).to_string(),
            },
        },
        pinned: false,
        way: None,
    })
}

/// Every chain node's fact right now, and the slots of the ones that can take work.
struct ChainFacts {
    facts: HashMap<String, EntryFact>,
    slots: HashMap<String, (Slot, Option<MlxPlacementKeyDto>)>,
    /// A chain node is on a remote-single route (a turn may wait for its load, as Auto's does).
    routed_remote: bool,
}

type Looked<'a> = (&'a str, bool, Result<(Member, Servable), EntryFact>);

async fn chain_facts(
    router: &Router,
    plan: &ChainPlan,
    members: &dyn ChainMembers,
    probe: &dyn NodeProbe,
    overrides: &HashMap<String, EntryFact>,
    saturated: &HashSet<String>,
) -> ChainFacts {
    let mut ids: Vec<&str> = Vec::new();
    for link in &plan.entry.chain {
        if !ids.contains(&link.node.as_str()) {
            ids.push(&link.node);
        }
    }
    let looked: Vec<Looked> = futures::future::join_all(ids.iter().map(|id| async move {
        let member = match (overrides.get(*id), plan.defs.get(*id)) {
            (Some(fact), _) => Err(fact.clone()),
            (None, None) => Err(cant_run(format!("there is no node '{id}'"))),
            (None, Some(_)) if saturated.contains(*id) => {
                Err(cant_run("refused admission this turn"))
            }
            (None, Some(def)) => members.member(def).await,
        };
        let remote = matches!(&member, Ok(m) if matches!(m.node.kind, NodeKind::MlxRemote(_)));
        let probed = match member {
            Err(fact) => Err(fact),
            Ok(member) => match probe.probe(&member.node).await {
                Err(reason) => Err(EntryFact::CantRun { reason }),
                Ok(facts) if member.pinned && facts.follows.is_some() => Err(EntryFact::NotLoaded),
                Ok(facts) => Ok((member, facts)),
            },
        };
        (*id, remote, probed)
    }))
    .await;

    let mut out = ChainFacts {
        facts: HashMap::new(),
        slots: HashMap::new(),
        routed_remote: looked.iter().any(|(_, remote, _)| *remote),
    };
    let mut candidates: Vec<(&Node, Servable)> = Vec::new();
    let mut ways: HashMap<&str, Option<MlxPlacementKeyDto>> = HashMap::new();
    for (id, _, probed) in &looked {
        match probed {
            Err(fact) => {
                out.facts.insert(id.to_string(), fact.clone());
            }
            Ok((member, facts)) => {
                candidates.push((&member.node, facts.clone()));
                ways.insert(id, member.way.clone());
            }
        }
    }
    let before: Vec<String> = candidates.iter().map(|(n, _)| n.id.clone()).collect();
    let mut reasons = Vec::new();
    one_node_per_engine(&mut candidates, &mut reasons);
    for id in before {
        if candidates.iter().any(|(n, _)| n.id == id) {
            continue;
        }
        // `one_node_per_engine` names each follower it sets aside as "<id>: <reason>".
        let prefix = format!("{id}: ");
        let reason = reasons
            .iter()
            .find_map(|r| r.strip_prefix(&prefix))
            .map(str::to_string)
            .unwrap_or_else(|| format!("{id} was set aside for another node of its engine"));
        out.facts.insert(id, cant_run(reason));
    }
    for (node, facts) in candidates {
        let slot = router.slot(node, facts);
        let fact = if slot.2 > 0 {
            EntryFact::Servable
        } else {
            EntryFact::Busy
        };
        out.facts.insert(node.id.clone(), fact);
        let way = ways.remove(node.id.as_str()).flatten();
        out.slots.insert(node.id.clone(), (slot, way));
    }
    out
}

fn passed_words(why: &PassedOver) -> String {
    match why {
        PassedOver::Busy => "busy".to_string(),
        PassedOver::NotLoaded => "not loaded".to_string(),
        PassedOver::CantRun { reason } => reason.clone(),
        PassedOver::LoadFailed { words } => words.clone(),
        PassedOver::Unknown => "nothing is known about it".to_string(),
    }
}

/// The words a load refusal leaves on its entry: the loader's own, with a failed load said so.
fn refusal_words(code: NodeLoadRefusalCode, reason: &str) -> String {
    match code {
        NodeLoadRefusalCode::LoadFailed => format!("failed to load: {reason}"),
        _ => reason.to_string(),
    }
}

fn rank_of(plan: &ChainPlan, node: &str) -> u32 {
    plan.entry
        .chain
        .iter()
        .position(|l| l.node == node)
        .map_or(0, |i| i as u32 + 1)
}

/// The served-turn record of a chain lease: its rank, why the 1st did not take the turn when it
/// was passed over, every entry passed over, and the load time when this turn loaded it.
fn chain_record(
    plan: &ChainPlan,
    node: &str,
    tried: &[Tried],
    loaded_ms: Option<u64>,
) -> NodeServedTurnDto {
    let first = plan.entry.chain.first().map(|l| l.node.as_str());
    let reason = first
        .filter(|first| *first != node)
        .and_then(|first| tried.iter().find(|t| t.node == first))
        .map(|t| passed_words(&t.why));
    NodeServedTurnDto {
        node: node.to_string(),
        role: plan.role,
        rank: rank_of(plan, node),
        reason,
        tried: tried
            .iter()
            .map(|t| NodeTriedDto {
                node: t.node.clone(),
                reason: passed_words(&t.why),
            })
            .collect(),
        loaded_ms,
        at_ms: now_ms(),
    }
}

/// Route one turn along a chain. Each pass reads every entry's fact, lets the role's when-rule
/// decide (`nodes::resolve`), and acts: serve, queue on the busy ones, or ask the loader for a
/// not-loaded one and read again. Every pass either ends the turn, settles one entry for good
/// (a load answered or refused, an admission refused) or waits out one of the switches queued
/// before the turn's reply opened (a set that only shrinks), so the walk ends without a count or
/// a clock.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn route_chain(
    router: &Router,
    plan: &ChainPlan,
    members: &dyn ChainMembers,
    probe: &dyn NodeProbe,
    providers: &dyn ProviderSource,
    kwargs_source: &dyn TemplateKwargsSource,
    route_load: &dyn RouteLoad,
    seam: &dyn NodesSeam,
    turn: Turn<'_>,
) -> Result<MessageStream, ProviderError> {
    let key = Router::conversation_key(turn.system, turn.messages);
    let mut overrides: HashMap<String, EntryFact> = HashMap::new();
    let mut saturated = HashSet::new();
    let mut last_refusal: Option<ProviderError> = None;
    // Node → how long the loader took to answer Ready for this turn.
    let mut loaded: HashMap<String, u64> = HashMap::new();
    loop {
        let current = chain_facts(router, plan, members, probe, &overrides, &saturated).await;
        let sticky = router
            .sticky
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned();
        // A role no pick has shared yet has no state: every weight starts at 0.
        let mut share = router
            .shares
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&plan.share_key)
            .cloned()
            .unwrap_or_default();
        let decision = resolve(&plan.entry, &current.facts, sticky.as_deref(), &mut share);
        let (lease, tried) = match decision {
            Decision::Serve { node, tried, .. } => {
                let (slot, _) = &current.slots[&node];
                let lease = match slot.1.clone().try_acquire_owned() {
                    Ok(permit) => router.leased(&slot.0, slot.3, permit, slot.2, 0, key),
                    Err(_) => router.queue_on(std::slice::from_ref(slot), key).await?,
                };
                (lease, tried)
            }
            Decision::Queue { nodes, tried } => {
                let slots: Vec<Slot> = nodes
                    .iter()
                    .filter_map(|n| current.slots.get(n).map(|(slot, _)| slot.clone()))
                    .collect();
                (router.queue_on(&slots, key).await?, tried)
            }
            Decision::Load { node, .. } => {
                let session = crate::session_context::current_session_id();
                let fact = if loaded.contains_key(&node) {
                    Some(EntryFact::LoadFailed {
                        words: "the node loader said it serves, but its way is still not the one serving this Mac's chat".to_string(),
                    })
                } else if let Some(session) = session {
                    let demanded = Instant::now();
                    let answer = seam
                        .ensure_serving(Demand {
                            node: plan.defs[&node].def.clone(),
                            from: DemandFrom::Turn(session),
                            role: plan.role,
                        })
                        .await;
                    match answer {
                        NodeEnsureServing::Ready => {
                            loaded.insert(
                                node.clone(),
                                u64::try_from(demanded.elapsed().as_millis()).unwrap_or(u64::MAX),
                            );
                            None
                        }
                        NodeEnsureServing::Refused { code, reason } => {
                            Some(EntryFact::LoadFailed {
                                words: refusal_words(code, &reason),
                            })
                        }
                        // A turn's demand is answered once it is settled (Ready or Refused); a
                        // Wait leaves the turn nothing to wait on, which is said, not spun on.
                        NodeEnsureServing::Wait { reason } => Some(EntryFact::LoadFailed {
                            words: format!(
                                "the node loader answered that this turn waits ({reason}), with nothing to wake it"
                            ),
                        }),
                    }
                } else {
                    // A load waits for the replies it would stop and holds its way for a reply; a
                    // call outside any session has neither, so it never demands one — a demand
                    // with no session is a card's Start, answered at once with a wait.
                    Some(EntryFact::LoadFailed {
                        words: format!(
                            "this model call belongs to no session, so it cannot wait for a load; start {} in Run it",
                            plan.defs[&node].def.name
                        ),
                    })
                };
                if let Some(fact) = fact {
                    overrides.insert(node, fact);
                }
                continue;
            }
            Decision::Exhausted { tried } => {
                if last_refusal.is_none() && current.routed_remote {
                    match route_load.settle().await {
                        Some(Ok(())) => continue,
                        Some(Err(words)) => return Err(ProviderError::ExecutionError(words)),
                        None => {}
                    }
                }
                return Err(last_refusal.unwrap_or_else(|| {
                    ProviderError::ExecutionError(format!(
                        "swarm chat: {}: no node can serve this turn — {}",
                        plan.label,
                        tried
                            .iter()
                            .map(|t| format!("{}: {}", t.node, passed_words(&t.why)))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ))
                }));
            }
        };
        // Before the round-robin commits: a lease that waited behind a queued switch was not a
        // pick, and the turn routes again on what serves after the switch.
        let Some(lease) = clear_of_queued_switches(seam, lease).await else {
            continue;
        };
        router
            .shares
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(plan.share_key.clone(), share);
        let way = current
            .slots
            .get(&lease.node.id)
            .and_then(|(_, way)| way.clone());
        match stream_on(lease, providers, kwargs_source, &turn).await? {
            Streamed::Served(stream, node) => {
                let way = way.or_else(|| pool_lease_way(seam, &node));
                note_served(seam, way, || {
                    chain_record(plan, &node.id, &tried, loaded.get(&node.id).copied())
                });
                return Ok(stream);
            }
            Streamed::Refused(node, e) => {
                saturated.insert(node);
                last_refusal = Some(e);
            }
        }
    }
}

/// The window goose compacts a chain route against: the smallest any chain node reports. A node
/// that cannot say (not loaded, not reachable) is named in the log; when none can, the answer is
/// the named reasons.
async fn chain_window(
    plan: &ChainPlan,
    members: &dyn ChainMembers,
    probe: &dyn NodeProbe,
) -> Result<usize, String> {
    let mut known: Vec<u64> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for link in &plan.entry.chain {
        let window = match plan.defs.get(&link.node) {
            None => Err(format!("there is no node '{}'", link.node)),
            Some(def) => match members.member(def).await {
                Err(EntryFact::CantRun { reason }) => Err(reason),
                Err(EntryFact::LoadFailed { words }) => Err(words),
                Err(_) => Err("not loaded".to_string()),
                Ok(member) => match probe.probe(&member.node).await {
                    Err(reason) => Err(reason),
                    Ok(facts) if member.pinned && facts.follows.is_some() => {
                        Err("not loaded".to_string())
                    }
                    Ok(facts) => facts
                        .context_window
                        .ok_or_else(|| "it does not report its context window".to_string()),
                },
            },
        };
        match window {
            Ok(window) => known.push(window),
            Err(reason) => unknown.push(format!("{}: {reason}", link.node)),
        }
    }
    let Some(smallest) = known.iter().min().copied() else {
        return Err(format!(
            "no node of {} reports its context window — {}",
            plan.label,
            unknown.join("; ")
        ));
    };
    if !unknown.is_empty() {
        tracing::warn!(
            target: "swarm_router",
            route = %plan.label,
            window = smallest,
            unknown = %unknown.join("; "),
            "the context window is the smallest the chain's reporting nodes say; these nodes could not say theirs"
        );
    }
    Ok(usize::try_from(smallest).unwrap_or(usize::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use goose_providers::base::stream_from_single_message;
    use goose_providers::conversation::token_usage::{ProviderUsage, Usage};

    fn node(id: &str, capacity: u32, weight: u32) -> Node {
        Node {
            id: id.to_string(),
            model_id: format!("{id}-model"),
            weight,
            share: None,
            capacity,
            kind: NodeKind::LmStudio {
                endpoint: "http://test".to_string(),
            },
        }
    }

    /// Per-node outcome: `Ok(facts)` servable, `Err(reason)` not.
    struct FakeProbe(HashMap<String, Result<Servable, String>>);

    impl FakeProbe {
        fn all_idle(nodes: &[Node]) -> Self {
            Self(
                nodes
                    .iter()
                    .map(|n| (n.id.clone(), Ok(Servable::default())))
                    .collect(),
            )
        }
    }

    fn busy(live: u32) -> Result<Servable, String> {
        Ok(Servable {
            live_in_flight: Some(live),
            ..Servable::default()
        })
    }

    fn window(context_window: u64) -> Result<Servable, String> {
        Ok(Servable {
            context_window: Some(context_window),
            ..Servable::default()
        })
    }

    #[async_trait]
    impl NodeProbe for FakeProbe {
        async fn probe(&self, node: &Node) -> Result<Servable, String> {
            self.0
                .get(&node.id)
                .cloned()
                .unwrap_or_else(|| Err("not in the fake".to_string()))
        }
    }

    #[tokio::test]
    async fn pick_takes_the_node_with_the_most_free_slots() {
        let router = Router::new();
        let nodes = vec![node("a", 2, 9), node("b", 4, 1)];
        let probe = FakeProbe::all_idle(&nodes);
        let lease = router
            .pick(&nodes, &probe, 1, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(lease.node.id, "b");
        // A hold on b's slots: b has 3 free, a has 2 → still b; two more → a wins at 2 vs 1.
        let l2 = router
            .pick(&nodes, &probe, 2, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(l2.node.id, "b");
        let l3 = router
            .pick(&nodes, &probe, 3, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(l3.node.id, "a");
        drop((lease, l2, l3));
    }

    #[tokio::test]
    async fn a_lease_on_the_mlx_engine_is_listed_as_serving_its_session_for_its_life() {
        let router = Router::new();
        let mlx = Node {
            kind: NodeKind::MlxSidecar,
            ..node("serving-test-mlx", 2, 1)
        };
        let lm = node("serving-test-lm", 2, 9);
        let probe = FakeProbe::all_idle(&[mlx.clone(), lm.clone()]);
        let mine = |id: &str| {
            super::super::mlx_serving::snapshot()
                .into_iter()
                .filter(|e| e.node_id.as_deref() == Some(id))
                .collect::<Vec<_>>()
        };

        let lease = crate::session_context::with_session_id(Some("20260923_42".to_string()), {
            router.pick(std::slice::from_ref(&mlx), &probe, 11, &HashSet::new())
        })
        .await
        .unwrap();
        let listed = mine("serving-test-mlx");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id.as_deref(), Some("20260923_42"));
        assert_eq!(listed[0].provider, "omlx");
        assert_eq!(listed[0].model, "serving-test-mlx-model");
        assert_eq!(listed[0].peer, None);
        assert_eq!(listed[0].work, None);

        // Q-185: goose's own call for a session (the end-of-turn fact check) is listed under the
        // session AND its kind, so the engine card never calls it the chat's answer.
        let check = crate::background_work::run(
            crate::background_work::BackgroundWorkKind::FactCheck,
            "20260923_42",
            router.pick(std::slice::from_ref(&mlx), &probe, 14, &HashSet::new()),
        )
        .await
        .unwrap();
        let listed = mine("serving-test-mlx");
        assert_eq!(listed.len(), 2);
        assert_eq!(
            listed[1].work,
            Some(crate::background_work::BackgroundWorkKind::FactCheck)
        );
        assert_eq!(listed[1].session_id.as_deref(), Some("20260923_42"));
        drop(check);

        // A lease on a linked Mac's engine is listed too, naming that Mac, so the desktop counts
        // this app's turn against the engine that runs it instead of as someone else's request.
        let remote = Node {
            kind: NodeKind::MlxRemote(RemoteTarget {
                peer: "studio".to_string(),
                peer_name: "Work's Mac Studio".to_string(),
                base_url: "http://127.0.0.1:61001/relay/cafe".to_string(),
                template_kwargs: None,
            }),
            ..node("serving-test-remote", 2, 1)
        };
        let probe_remote = FakeProbe::all_idle(std::slice::from_ref(&remote));
        let routed = crate::session_context::with_session_id(Some("20260923_43".to_string()), {
            router.pick(
                std::slice::from_ref(&remote),
                &probe_remote,
                13,
                &HashSet::new(),
            )
        })
        .await
        .unwrap();
        let listed_remote = mine("serving-test-remote");
        assert_eq!(listed_remote.len(), 1);
        assert_eq!(listed_remote[0].session_id.as_deref(), Some("20260923_43"));
        assert_eq!(listed_remote[0].peer.as_deref(), Some("Work's Mac Studio"));
        drop(routed);
        assert!(mine("serving-test-remote").is_empty());

        // An LM Studio lease is not the MLX engine's work and is never listed.
        let other = router
            .pick(std::slice::from_ref(&lm), &probe, 12, &HashSet::new())
            .await
            .unwrap();
        assert!(mine("serving-test-lm").is_empty());

        drop(lease);
        assert!(mine("serving-test-mlx").is_empty());
        drop(other);
    }

    #[tokio::test]
    async fn the_engines_live_in_flight_count_reduces_free_slots() {
        let router = Router::new();
        let nodes = vec![node("mlx", 8, 5), node("lm", 1, 1)];
        let mut probe = FakeProbe::all_idle(&nodes);
        // The sidecar reports 8 in flight from another process: zero free, so the 1-slot LM Studio
        // node wins even though the sidecar's cap is eight times larger.
        probe.0.insert("mlx".to_string(), busy(8));
        let lease = router
            .pick(&nodes, &probe, 1, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(lease.node.id, "lm");
    }

    #[tokio::test]
    async fn the_pool_context_limit_is_the_smallest_servable_window_and_unknown_stays_unknown() {
        let router = Router::new();
        let nodes = vec![node("big", 2, 1), node("small", 2, 1), node("mute", 2, 1)];
        assert_eq!(router.pool_context_limit(), None, "nothing picked yet");
        let unknown = FakeProbe::all_idle(&nodes);
        let lease = router
            .pick(&nodes, &unknown, 1, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(
            router.pool_context_limit(),
            None,
            "no node reported a window"
        );
        drop(lease);
        let probe = FakeProbe(HashMap::from([
            ("big".to_string(), window(262_144)),
            ("small".to_string(), window(32_768)),
            ("mute".to_string(), Ok(Servable::default())),
        ]));
        let lease = router
            .pick(&nodes, &probe, 1, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(router.pool_context_limit(), Some(32_768));
        drop(lease);
        // The small node goes away: the limit follows the pool that can actually serve.
        let mut only_big = FakeProbe(HashMap::from([("big".to_string(), window(262_144))]));
        only_big
            .0
            .insert("small".to_string(), Err("engine stopped".to_string()));
        only_big
            .0
            .insert("mute".to_string(), Err("engine stopped".to_string()));
        let lease = router
            .pick(&nodes, &only_big, 1, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(router.pool_context_limit(), Some(262_144));
        drop(lease);
    }

    /// Q-18: before any pick, the window is MEASURED by the pick's own probes — no slot taken —
    /// and when nothing can say it the answer names why, never a number.
    #[tokio::test]
    async fn before_any_pick_the_pool_window_is_measured_and_unknown_is_said() {
        let nodes = vec![node("big", 2, 1), node("small", 2, 1), node("down", 2, 1)];
        let probe = FakeProbe(HashMap::from([
            ("big".to_string(), window(262_144)),
            ("small".to_string(), window(32_768)),
            ("down".to_string(), Err("engine stopped".to_string())),
        ]));
        let router = Router::new();
        assert_eq!(router.pool_context_window(&nodes, &probe).await, Ok(32_768));
        assert_eq!(
            router.pool_context_limit(),
            Some(32_768),
            "kept like a pick's"
        );
        let free: usize = nodes
            .iter()
            .map(|n| router.semaphore(n).available_permits())
            .sum();
        assert_eq!(free, 6, "a measurement holds no slot");

        let mute = Router::new();
        let err = mute
            .pool_context_window(&nodes, &FakeProbe::all_idle(&nodes))
            .await
            .unwrap_err();
        assert!(err.contains("big, small, down"), "{err}");
        assert!(mute
            .pool_context_window(&nodes, &FakeProbe::all_idle(&nodes))
            .await
            .is_err());

        let dead = Router::new();
        let all_down = FakeProbe(HashMap::from([(
            "big".to_string(),
            Err("connection refused".to_string()),
        )]));
        let err = dead
            .pool_context_window(&nodes[..1], &all_down)
            .await
            .unwrap_err();
        assert!(err.contains("big: connection refused"), "{err}");
        assert_eq!(
            Router::new().pool_context_window(&[], &all_down).await,
            Err("no enabled device is configured under `swarm.devices`".to_string())
        );
    }

    #[tokio::test]
    async fn ties_go_to_the_heavier_node() {
        let router = Router::new();
        let nodes = vec![node("light", 2, 1), node("heavy", 2, 3)];
        let probe = FakeProbe::all_idle(&nodes);
        let lease = router
            .pick(&nodes, &probe, 1, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(lease.node.id, "heavy");
    }

    #[tokio::test]
    async fn sticky_wins_while_it_has_a_free_slot() {
        let router = Router::new();
        let nodes = vec![node("a", 2, 1), node("b", 4, 1)];
        let probe = FakeProbe::all_idle(&nodes);
        router.sticky.lock().unwrap().insert(7, "a".to_string());
        let lease = router
            .pick(&nodes, &probe, 7, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(
            lease.node.id, "a",
            "sticky beats most-free while a has a slot"
        );
        let second = router
            .pick(&nodes, &probe, 7, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(second.node.id, "a");
        let third = router
            .pick(&nodes, &probe, 7, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(
            third.node.id, "b",
            "a is full → most-free, and stickiness moves with it"
        );
        assert_eq!(router.sticky.lock().unwrap().get(&7).unwrap(), "b");
        drop((lease, second, third));
    }

    /// Q-400: a caller dropped while it waits in the queue leaves the depth — the reviewer that
    /// yields to a user turn drops its call mid-wait. #3r counted every one of them forever.
    #[tokio::test]
    async fn a_pick_dropped_while_it_waits_leaves_the_queue() {
        let router = Arc::new(Router::new());
        let nodes = Arc::new(vec![node("only", 1, 1)]);
        let probe = Arc::new(FakeProbe::all_idle(&nodes));
        let held = router
            .pick(&nodes, &*probe, 1, &HashSet::new())
            .await
            .unwrap();
        let (r, n, p) = (router.clone(), nodes.clone(), probe.clone());
        let waiter = tokio::spawn(async move { r.pick(&n, &*p, 2, &HashSet::new()).await.is_ok() });
        while router.queued.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert_eq!(router.queued.load(Ordering::SeqCst), 0);
        drop(held);
    }

    /// Q-400, #3r (2026-09-28, the split chat 20260928_21, 8 slots): one end-of-turn reviewer per
    /// turn, each outlived by the next turn. They piled up — 18 asked at 13:42:43, 8 leased and
    /// 10 queued, all dropped 4 s later by the next turn — and `queue_depth` never came down
    /// (0 → 55 → 77). Now, across 19 turns: the chat's own call never finds anyone queued ahead of
    /// it or waits, only the newest turn's reviewer runs once the chat is idle, and the depth is 0.
    #[tokio::test]
    async fn a_chats_reviewers_do_not_pile_up_queue_ahead_of_its_turn_or_leak_the_depth() {
        use crate::turn_priority::TurnPriority;
        const CHAT: &str = "20260928_21";
        let router = Arc::new(Router::new());
        let nodes = Arc::new(vec![node("split", 8, 1)]);
        let probe = Arc::new(FakeProbe::all_idle(&nodes));
        let priority = Arc::new(TurnPriority::new());
        let slots = router.semaphore(&nodes[0]);
        let settle = || async {
            for _ in 0..200 {
                tokio::task::yield_now().await;
            }
        };
        let mut reviewers = Vec::new();
        for turn in 1..=19u64 {
            let user = priority.user_turn_in(CHAT);
            settle().await;
            assert_eq!(
                router.queued.load(Ordering::SeqCst),
                0,
                "turn {turn}: nobody is queued ahead of the chat's own call"
            );
            let agent = router
                .pick(&nodes, &*probe, 1, &HashSet::new())
                .await
                .unwrap();
            assert_eq!(
                slots.available_permits(),
                7,
                "turn {turn}: the agent's call runs alone"
            );
            drop(agent);
            drop(user);
            let mark = priority.chat_mark(CHAT);
            let (r, n, p, pr) = (
                router.clone(),
                nodes.clone(),
                probe.clone(),
                priority.clone(),
            );
            reviewers.push(tokio::spawn(async move {
                pr.after_user_turns("end-of-turn reviewer", &mark, || {
                    let (r, n, p) = (r.clone(), n.clone(), p.clone());
                    async move {
                        let _lease = r.pick(&n, &*p, 2, &HashSet::new()).await.unwrap();
                        std::future::pending::<()>().await
                    }
                })
                .await
            }));
            settle().await;
            assert_eq!(
                slots.available_permits(),
                7,
                "turn {turn}: once the chat is idle, only the newest reviewer holds a slot"
            );
            assert_eq!(router.queued.load(Ordering::SeqCst), 0, "turn {turn}");
        }
        let newest = reviewers.pop().unwrap();
        for superseded in reviewers {
            assert_eq!(superseded.await.unwrap(), None);
        }
        newest.abort();
        let _ = newest.await;
        assert_eq!(slots.available_permits(), 8);
        assert_eq!(router.queued.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_full_pool_queues_until_a_permit_frees() {
        let router = Arc::new(Router::new());
        let nodes = Arc::new(vec![node("only", 1, 1)]);
        let probe = Arc::new(FakeProbe::all_idle(&nodes));
        let first = router
            .pick(&nodes, &*probe, 1, &HashSet::new())
            .await
            .unwrap();
        let (r, n, p) = (router.clone(), nodes.clone(), probe.clone());
        let waiter = tokio::spawn(async move {
            r.pick(&n, &*p, 2, &HashSet::new())
                .await
                .map(|l| l.node.id.clone())
        });
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        assert!(!waiter.is_finished(), "second pick must wait for the slot");
        assert_eq!(router.queued.load(Ordering::SeqCst), 1);
        drop(first);
        let id = waiter.await.unwrap().unwrap();
        assert_eq!(id, "only");
        assert_eq!(router.queued.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn zero_servable_nodes_is_a_named_error_listing_every_device_and_reason() {
        let router = Router::new();
        let nodes = vec![node("mlx", 8, 1), node("lm", 1, 1)];
        let probe = FakeProbe(HashMap::from([
            (
                "mlx".to_string(),
                Err("MLX engine is stopped — mount it in the MLX window".to_string()),
            ),
            (
                "lm".to_string(),
                Err("model 'lm-model' is not listed by http://test/v1/models".to_string()),
            ),
        ]));
        let err = router
            .pick(&nodes, &probe, 1, &HashSet::new())
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("no node can serve this turn"), "{err}");
        assert!(err.contains("mlx: MLX engine is stopped"), "{err}");
        assert!(err.contains("lm: model 'lm-model' is not listed"), "{err}");
    }

    const PINNED_27B: &str = "mihai-qwen3.8-27b-atlassian-q8-mlx";
    const FLASH: &str = "rapid-mlx/Qwen3.8-Flash-Next-4bit";
    const FLASH_ALIAS: &str = "mihai-flash-qwen3.8-flash-next-4bit-mlx";

    fn sidecar(id: &str, model_id: &str, weight: u32) -> Node {
        Node {
            id: id.to_string(),
            model_id: model_id.to_string(),
            weight,
            share: None,
            capacity: 2,
            kind: NodeKind::MlxSidecar,
        }
    }

    fn engine_serves(served: &str, follows: Option<&str>) -> Result<Servable, String> {
        Ok(Servable {
            serves: Some(served.to_string()),
            follows: follows.map(str::to_string),
            ..Servable::default()
        })
    }

    /// Q-128, 11:50: the device is pinned to the 27B, the owner ran Flash across both Macs. The
    /// verdict against the live words: no form of the 27B names Flash, and only the owner's own
    /// start is followed — never a Link peer's Mount, never an unreadable record.
    #[test]
    fn a_pinned_node_follows_only_the_owners_own_start() {
        let pinned =
            |intent: &IntentRecord| mlx_node_verdict("mihai-mlx", PINNED_27B, FLASH, FLASH, intent);
        let started = |intent| IntentRecord::Present(intent);
        assert_eq!(
            pinned(&started(ServingIntent::Split {
                model_id: FLASH.to_string()
            })),
            Ok(Some(PINNED_27B.to_string()))
        );
        assert_eq!(
            pinned(&started(ServingIntent::Single {
                model_id: FLASH.to_string()
            })),
            Ok(Some(PINNED_27B.to_string()))
        );
        let words = format!("MLX engine serves '{FLASH}', the device wants '{PINNED_27B}'");
        for (intent, why) in [
            (IntentRecord::Absent, "not started from this Mac's goose"),
            (
                started(ServingIntent::Split {
                    model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
                }),
                "not started from this Mac's goose",
            ),
            (
                started(ServingIntent::RemoteSingle {
                    peer: "studio".to_string(),
                    peer_name: "Studio".to_string(),
                    model_id: FLASH.to_string(),
                }),
                "not started from this Mac's goose",
            ),
            (
                IntentRecord::Unreadable {
                    path: "/state/mlx-serving-intent.json".into(),
                    error: "expected value".to_string(),
                },
                "/state/mlx-serving-intent.json is unreadable (expected value)",
            ),
        ] {
            let err = pinned(&intent).unwrap_err();
            assert!(err.starts_with(&words), "{err}");
            assert!(err.contains(why), "{err}");
            assert!(
                !err.contains("; "),
                "the notice splits reasons on '; ': {err}"
            );
        }
        assert_eq!(
            mlx_node_verdict(
                "mihai-flash-mlx",
                FLASH_ALIAS,
                FLASH,
                FLASH,
                &IntentRecord::Absent
            ),
            Ok(None),
            "11:57: the Add-node alias names the pipeline's HF id"
        );
    }

    /// 11:50 through the router: the pinned node takes the turn, the routed call and its usage
    /// name the served model (the chip's name), and the pool's node keeps its pin.
    #[tokio::test]
    async fn the_followed_start_is_what_the_call_names_and_the_pin_is_kept() {
        let nodes = vec![sidecar("mihai-mlx", PINNED_27B, 1)];
        let probe = FakeProbe(HashMap::from([(
            "mihai-mlx".to_string(),
            engine_serves(FLASH, Some(PINNED_27B)),
        )]));
        let lease = Router::new()
            .pick(&nodes, &probe, 1, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(lease.node.id, "mihai-mlx");
        assert_eq!(lease.node.model_id, FLASH);
        assert_eq!(nodes[0].model_id, PINNED_27B);
    }

    /// 11:57: both nodes are in the pool and the split serves Flash. The one engine is ONE node:
    /// the node that names Flash takes it even when the pinned 27B node is heavier, and the 27B
    /// node says where chat went.
    #[tokio::test]
    async fn a_node_that_names_the_served_model_is_the_engines_node() {
        let nodes = vec![
            sidecar("mihai-mlx", PINNED_27B, 5),
            sidecar("mihai-flash-mlx", FLASH_ALIAS, 1),
        ];
        let probe = FakeProbe(HashMap::from([
            (
                "mihai-mlx".to_string(),
                engine_serves(FLASH, Some(PINNED_27B)),
            ),
            ("mihai-flash-mlx".to_string(), engine_serves(FLASH, None)),
        ]));
        let lease = Router::new()
            .pick(&nodes, &probe, 1, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(lease.node.id, "mihai-flash-mlx");
        assert_eq!(lease.node.model_id, FLASH);

        let mut candidates = nodes
            .iter()
            .zip([
                engine_serves(FLASH, Some(PINNED_27B)).unwrap(),
                engine_serves(FLASH, None).unwrap(),
            ])
            .collect::<Vec<_>>();
        let mut reasons = Vec::new();
        one_node_per_engine(&mut candidates, &mut reasons);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0.id, "mihai-flash-mlx");
        assert_eq!(
            reasons,
            [format!(
                "mihai-mlx: MLX engine serves '{FLASH}', the device wants '{PINNED_27B}' — chat goes to mihai-flash-mlx, which names it"
            )]
        );
    }

    #[test]
    fn two_followers_of_one_engine_leave_it_to_the_heavier() {
        let nodes = [
            sidecar("a-mlx", "a-model", 1),
            sidecar("b-mlx", "b-model", 3),
            sidecar("c-mlx", "c-model", 3),
        ];
        let mut candidates = nodes
            .iter()
            .map(|n| (n, engine_serves(FLASH, Some(&n.model_id)).unwrap()))
            .collect::<Vec<_>>();
        let mut reasons = Vec::new();
        one_node_per_engine(&mut candidates, &mut reasons);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0.id, "b-mlx", "the first of the heaviest");
        assert_eq!(reasons.len(), 2);
        assert!(
            reasons
                .iter()
                .all(|r| r.ends_with("chat follows it on b-mlx")),
            "{reasons:?}"
        );
    }

    #[test]
    fn only_enabled_devices_become_nodes_and_kinds_follow_the_engine() {
        let cfg: PoolConfig = serde_yaml::from_str(
            r#"
endpoint: http://lm.local:1234
planner_model: x
devices:
  - id: workhorse-mlx
    model_id: workhorse-qwen3.5-9b-4bit-mlx
    weight: 2
    enabled: true
    instances: 1
    engine: mlx-sidecar
  - id: mihai-lm
    model_id: mihai-qwen
    weight: 1
    enabled: true
    instances: 2
  - id: off
    model_id: off-model
    weight: 1
    enabled: false
  - id: cloud
    model_id: anthropic.claude
    weight: 1
    enabled: true
    provider: bedrock
"#,
        )
        .unwrap();
        let nodes = nodes_from_config(&cfg);
        let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["workhorse-mlx", "mihai-lm", "cloud"]);
        assert_eq!(nodes[0].kind, NodeKind::MlxSidecar);
        assert_eq!(
            nodes[0].capacity,
            goose_sidecar::engine::MAX_CONCURRENT_REQUESTS
        );
        assert_eq!(
            nodes[1].kind,
            NodeKind::LmStudio {
                endpoint: "http://lm.local:1234".to_string()
            }
        );
        assert_eq!(nodes[1].capacity, 2);
        assert_eq!(
            nodes[2].kind,
            NodeKind::Cloud {
                registry: "aws_bedrock".to_string()
            }
        );
        assert_eq!(nodes[2].provider_name(), "aws_bedrock");
    }

    #[test]
    fn admission_refusal_is_recognised_by_the_engines_own_words() {
        let capped = ProviderError::ServerError(
            "Server error (503 Service Unavailable) at http://127.0.0.1:8090/v1/chat/completions: \
             HTTP 503: {\"error\":{\"message\":\"Server is busy (max concurrent requests reached). \
             Please try again later. (currently 8 in-flight)\"}}"
                .to_string(),
        );
        assert!(is_admission_refusal(&capped));
        assert!(!is_admission_refusal(&ProviderError::ServerError(
            "Internal error while decoding".to_string()
        )));
        assert!(!is_admission_refusal(
            &ProviderError::ContextLengthExceeded("too long".to_string())
        ));
    }

    /// No MLX profile carries a choice — what every node saw before thinking choices existed.
    struct NoKwargs;

    impl TemplateKwargsSource for NoKwargs {
        fn template_kwargs(&self, _: &str) -> Result<Option<Map<String, Value>>, String> {
            Ok(None)
        }
    }

    /// Resolves through the real profile lookup against in-memory settings.
    struct SettingsKwargs(StdMutex<EngineSettings>);

    impl TemplateKwargsSource for SettingsKwargs {
        fn template_kwargs(&self, served: &str) -> Result<Option<Map<String, Value>>, String> {
            profile_template_kwargs(&self.0.lock().unwrap(), served)
        }
    }

    /// Records the exact config each turn reached the node provider with.
    #[derive(Default)]
    struct RecordingProviders(StdMutex<Vec<ModelConfig>>);

    struct RecordingProvider(Arc<RecordingProviders>);

    #[async_trait]
    impl Provider for RecordingProvider {
        fn get_name(&self) -> &str {
            "recording"
        }
        async fn stream(
            &self,
            model_config: &ModelConfig,
            _: &str,
            _: &[Message],
            _: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.0 .0.lock().unwrap().push(model_config.clone());
            Ok(stream_from_single_message(
                Message::assistant().with_text("ok"),
                ProviderUsage::new(model_config.model_name.clone(), Usage::default()),
            ))
        }
    }

    struct RecordingSource(Arc<RecordingProviders>);

    #[async_trait]
    impl ProviderSource for RecordingSource {
        async fn provider_for(&self, _: &Node) -> Result<Arc<dyn Provider>, String> {
            Ok(Arc::new(RecordingProvider(self.0.clone())))
        }
    }

    const SERVED: &str = "workhorse-qwen3.8-27b";
    const HF_ID: &str = "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx";

    fn mlx_node() -> Node {
        Node {
            id: "mlx".to_string(),
            model_id: SERVED.to_string(),
            weight: 1,
            share: None,
            capacity: 1,
            kind: NodeKind::MlxSidecar,
        }
    }

    fn engine_settings(profile: goose_sidecar::engine::ModelProfile) -> EngineSettings {
        EngineSettings {
            model_id: Some(HF_ID.to_string()),
            served_model_name: Some(SERVED.to_string()),
            model_profiles: std::collections::BTreeMap::from([(HF_ID.to_string(), profile)]),
            ..Default::default()
        }
    }

    fn agent_tool() -> Tool {
        Tool::new(
            "developer__shell",
            "run a command",
            serde_json::json!({"type":"object","properties":{"command":{"type":"string"}}})
                .as_object()
                .unwrap()
                .clone(),
        )
    }

    /// Routes one turn to `node` and returns the config the node's provider received.
    async fn routed_config(
        node: Node,
        source: &dyn TemplateKwargsSource,
        session: &SessionTemplateKwargs,
        model_config: &ModelConfig,
    ) -> ModelConfig {
        let recorded = Arc::new(RecordingProviders::default());
        let nodes = vec![node];
        let probe = FakeProbe::all_idle(&nodes);
        let messages = vec![Message::user().with_text("hi")];
        let tools = vec![agent_tool()];
        let stream = route_stream(
            &Router::new(),
            &nodes,
            &probe,
            &RecordingSource(recorded.clone()),
            source,
            &NoRouteLoad,
            &RecordingSeam::default(),
            Turn {
                model_config,
                system: "sys",
                messages: &messages,
                tools: &tools,
                session,
            },
        )
        .await
        .unwrap();
        drop(stream);
        let configs = recorded.0.lock().unwrap();
        assert_eq!(configs.len(), 1);
        configs[0].clone()
    }

    /// The OpenAI chat body the `omlx` provider would send for `cfg`, as bytes.
    fn request_bytes(cfg: &ModelConfig) -> String {
        let body = goose_providers::formats::openai::create_request(
            cfg,
            "sys",
            &[Message::user().with_text("hi")],
            &[agent_tool()],
            &goose_providers::images::ImageFormat::OpenAi,
            true,
        )
        .unwrap();
        serde_json::to_string(&body).unwrap()
    }

    /// THE ISOLATION PROOF. With the thinking choices at their defaults (auto, template default) —
    /// including a profile that carries sampling values — the config a node receives and the
    /// request bytes built from it equal what route_stream produced before the choices existed
    /// (`model_config.clone()` with the node's model name and the node's own window — here none,
    /// the fake probe reports none — nothing else).
    #[tokio::test]
    async fn default_choices_leave_the_mlx_request_byte_identical() {
        let mut session_params = ModelConfig::new("swarm");
        session_params.request_params = Some(HashMap::from([(
            "top_k".to_string(),
            serde_json::json!(20),
        )]));
        for model_config in [ModelConfig::new("swarm"), session_params] {
            let mut before = model_config.clone();
            before.model_name = SERVED.to_string();
            before.context_limit = None;
            for profile in [
                goose_sidecar::engine::ModelProfile::default(),
                goose_sidecar::engine::ModelProfile {
                    temperature: Some(0.6),
                    top_k: Some(20),
                    ..Default::default()
                },
            ] {
                let source = SettingsKwargs(StdMutex::new(engine_settings(profile)));
                let after = routed_config(
                    mlx_node(),
                    &source,
                    &SessionTemplateKwargs::default(),
                    &model_config,
                )
                .await;
                assert_eq!(
                    serde_json::to_string(&after).unwrap(),
                    serde_json::to_string(&before).unwrap()
                );
                assert_eq!(request_bytes(&after), request_bytes(&before));
                assert!(!request_bytes(&after).contains("chat_template_kwargs"));
            }
        }
    }

    /// Q-65's log (2026-09-25): the split's turns carried `context_limit: 128000` — the `swarm`
    /// session's default for an unknown model name — while the node served 262,144. The routed call
    /// carries the window the node's probe read at this pick.
    #[tokio::test]
    async fn the_routed_call_carries_the_nodes_own_window_not_the_sessions_default() {
        let recorded = Arc::new(RecordingProviders::default());
        let nodes = vec![mlx_node()];
        let probe = FakeProbe(HashMap::from([("mlx".to_string(), window(262_144))]));
        let messages = vec![Message::user().with_text("hi")];
        let session_config = ModelConfig::new("swarm");
        let stream = route_stream(
            &Router::new(),
            &nodes,
            &probe,
            &RecordingSource(recorded.clone()),
            &SettingsKwargs(StdMutex::new(EngineSettings::default())),
            &NoRouteLoad,
            &RecordingSeam::default(),
            Turn {
                model_config: &session_config,
                system: "sys",
                messages: &messages,
                tools: &[],
                session: &SessionTemplateKwargs::default(),
            },
        )
        .await
        .unwrap();
        drop(stream);
        let configs = recorded.0.lock().unwrap();
        assert_eq!(configs[0].context_limit, Some(262_144));
        assert_eq!(configs[0].context_limit(), 262_144);
        assert_eq!(
            configs[0].max_tokens, None,
            "goose still sends no max_tokens: the server's own budget applies"
        );
    }

    #[tokio::test]
    async fn thinking_on_puts_the_kwargs_on_mlx_requests_and_nowhere_else() {
        let profile = goose_sidecar::engine::ModelProfile {
            thinking: Some(goose_sidecar::engine::ThinkingMode::On),
            reasoning_effort: Some("low".to_string()),
            ..Default::default()
        };
        let source = SettingsKwargs(StdMutex::new(engine_settings(profile)));
        let mlx = routed_config(
            mlx_node(),
            &source,
            &SessionTemplateKwargs::default(),
            &ModelConfig::new("swarm"),
        )
        .await;
        let body: Value = serde_json::from_str(&request_bytes(&mlx)).unwrap();
        assert_eq!(
            body["chat_template_kwargs"],
            serde_json::json!({"enable_thinking": true, "reasoning_effort": "low"})
        );
        assert!(body.get("reasoning_effort").is_none());

        let mut lm = node("lm", 1, 1);
        lm.model_id = SERVED.to_string();
        let lm = routed_config(
            lm,
            &source,
            &SessionTemplateKwargs::default(),
            &ModelConfig::new("swarm"),
        )
        .await;
        assert!(!request_bytes(&lm).contains("chat_template_kwargs"));
    }

    #[tokio::test]
    async fn a_session_keeps_the_choices_it_started_with() {
        let on = goose_sidecar::engine::ModelProfile {
            thinking: Some(goose_sidecar::engine::ThinkingMode::On),
            reasoning_effort: Some("xhigh".to_string()),
            ..Default::default()
        };
        let source = SettingsKwargs(StdMutex::new(engine_settings(on)));
        let session = SessionTemplateKwargs::default();
        let cfg = ModelConfig::new("swarm");
        let kwargs = |c: &ModelConfig| {
            serde_json::from_str::<Value>(&request_bytes(c)).unwrap()["chat_template_kwargs"]
                .clone()
        };
        let first = routed_config(mlx_node(), &source, &session, &cfg).await;
        *source.0.lock().unwrap() = engine_settings(goose_sidecar::engine::ModelProfile {
            thinking: Some(goose_sidecar::engine::ThinkingMode::Off),
            ..Default::default()
        });
        let later = routed_config(mlx_node(), &source, &session, &cfg).await;
        assert_eq!(
            kwargs(&first),
            kwargs(&later),
            "the running session is locked"
        );
        assert_eq!(
            kwargs(&first),
            serde_json::json!({"enable_thinking": true, "reasoning_effort": "xhigh"})
        );
        let fresh =
            routed_config(mlx_node(), &source, &SessionTemplateKwargs::default(), &cfg).await;
        assert_eq!(
            kwargs(&fresh),
            serde_json::json!({"enable_thinking": false}),
            "a new session reads the edited profile"
        );
    }

    #[test]
    fn a_served_id_resolves_to_its_profile_or_names_why_it_cannot() {
        let on = goose_sidecar::engine::ModelProfile {
            thinking: Some(goose_sidecar::engine::ThinkingMode::On),
            ..Default::default()
        };
        let aliased = engine_settings(on.clone());
        assert!(profile_template_kwargs(&aliased, SERVED).unwrap().is_some());
        // Another model is served under its own HF id (the alias is the configured model's), so
        // it reads its own profile — none here.
        assert_eq!(
            profile_template_kwargs(&aliased, "something-else").unwrap(),
            None
        );
        let with_other = EngineSettings {
            model_profiles: std::collections::BTreeMap::from([
                (HF_ID.to_string(), on.clone()),
                ("pub/other".to_string(), on.clone()),
            ]),
            ..aliased.clone()
        };
        assert!(profile_template_kwargs(&with_other, "pub/other")
            .unwrap()
            .is_some());
        let orphan_alias = EngineSettings {
            model_id: None,
            ..aliased.clone()
        };
        let err = profile_template_kwargs(&orphan_alias, SERVED).unwrap_err();
        assert!(err.contains(HF_ID) && err.contains(SERVED), "{err}");

        let unaliased = EngineSettings {
            served_model_name: None,
            model_id: None,
            ..aliased.clone()
        };
        assert!(profile_template_kwargs(&unaliased, HF_ID)
            .unwrap()
            .is_some());
        assert_eq!(
            profile_template_kwargs(&unaliased, "pub/other").unwrap(),
            None
        );

        let no_choices = engine_settings(goose_sidecar::engine::ModelProfile::default());
        assert_eq!(
            profile_template_kwargs(&no_choices, "something-else").unwrap(),
            None
        );
    }

    #[test]
    fn session_request_params_win_and_a_non_object_is_refused() {
        let on = || {
            let mut map = Map::new();
            map.insert("enable_thinking".to_string(), Value::Bool(true));
            map.insert("reasoning_effort".to_string(), Value::String("low".into()));
            map
        };
        let mut cfg = ModelConfig::new("m");
        cfg.request_params = Some(HashMap::from([(
            "chat_template_kwargs".to_string(),
            serde_json::json!({"enable_thinking": false}),
        )]));
        add_template_kwargs(&mut cfg, on()).unwrap();
        assert_eq!(
            cfg.request_params.unwrap()["chat_template_kwargs"],
            serde_json::json!({"enable_thinking": false, "reasoning_effort": "low"})
        );
        let mut bad = ModelConfig::new("m");
        bad.request_params = Some(HashMap::from([(
            "chat_template_kwargs".to_string(),
            serde_json::json!("x"),
        )]));
        assert!(add_template_kwargs(&mut bad, on()).is_err());
    }

    /// A fake node provider: `a` refuses admission the way the sidecar does, `b` answers.
    struct FakeProviders;

    struct RefusingProvider;
    struct AnsweringProvider;

    #[async_trait]
    impl Provider for RefusingProvider {
        fn get_name(&self) -> &str {
            "refusing"
        }
        async fn stream(
            &self,
            _: &ModelConfig,
            _: &str,
            _: &[Message],
            _: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            Err(ProviderError::ServerError(
                "HTTP 503: Server is busy (max concurrent requests reached)".to_string(),
            ))
        }
    }

    #[async_trait]
    impl Provider for AnsweringProvider {
        fn get_name(&self) -> &str {
            "answering"
        }
        async fn stream(
            &self,
            model_config: &ModelConfig,
            _: &str,
            _: &[Message],
            _: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            Ok(stream_from_single_message(
                Message::assistant().with_text("hello from b"),
                ProviderUsage::new(model_config.model_name.clone(), Usage::default()),
            ))
        }
    }

    #[async_trait]
    impl ProviderSource for FakeProviders {
        async fn provider_for(&self, node: &Node) -> Result<Arc<dyn Provider>, String> {
            Ok(match node.id.as_str() {
                "a" => Arc::new(RefusingProvider),
                _ => Arc::new(AnsweringProvider),
            })
        }
    }

    #[tokio::test]
    async fn an_admission_refusal_fails_over_to_the_next_node_and_holds_its_slot() {
        let router = Router::new();
        // a is preferred (more free slots), refuses; b answers and its model id rides the usage.
        let nodes = vec![node("a", 4, 1), node("b", 1, 1)];
        let probe = FakeProbe::all_idle(&nodes);
        let messages = vec![Message::user().with_text("hi")];
        let mut stream = route_stream(
            &router,
            &nodes,
            &probe,
            &FakeProviders,
            &NoKwargs,
            &NoRouteLoad,
            &RecordingSeam::default(),
            Turn {
                model_config: &ModelConfig::new("swarm"),
                system: "sys",
                messages: &messages,
                tools: &[],
                session: &SessionTemplateKwargs::default(),
            },
        )
        .await
        .unwrap();
        let b_sem = router.semaphore(&nodes[1]);
        assert_eq!(
            b_sem.available_permits(),
            0,
            "b's slot is held while the stream lives"
        );
        let a_sem = router.semaphore(&nodes[0]);
        assert_eq!(
            a_sem.available_permits(),
            4,
            "a's refused lease was released"
        );
        let (message, usage) = stream.next().await.unwrap().unwrap();
        assert_eq!(message.unwrap().as_concat_text(), "hello from b");
        assert_eq!(usage.unwrap().model, "b-model");
        assert!(stream.next().await.is_none());
        assert_eq!(
            b_sem.available_permits(),
            0,
            "held until the stream is dropped"
        );
        drop(stream);
        assert_eq!(
            b_sem.available_permits(),
            1,
            "the slot frees with the stream"
        );
    }

    #[tokio::test]
    async fn when_every_node_refuses_the_refusal_returns_unchanged() {
        struct AllRefuse;
        #[async_trait]
        impl ProviderSource for AllRefuse {
            async fn provider_for(&self, _: &Node) -> Result<Arc<dyn Provider>, String> {
                Ok(Arc::new(RefusingProvider))
            }
        }
        let router = Router::new();
        let nodes = vec![node("a", 1, 1)];
        let probe = FakeProbe::all_idle(&nodes);
        let err = route_stream(
            &router,
            &nodes,
            &probe,
            &AllRefuse,
            &NoKwargs,
            &NoRouteLoad,
            &RecordingSeam::default(),
            Turn {
                model_config: &ModelConfig::new("swarm"),
                system: "sys",
                messages: &[],
                tools: &[],
                session: &SessionTemplateKwargs::default(),
            },
        )
        .await
        .err()
        .unwrap();
        assert!(matches!(err, ProviderError::ServerError(_)));
        assert!(err.to_string().contains("max concurrent"));
        assert_eq!(router.semaphore(&nodes[0]).available_permits(), 1);
    }

    // -----------------------------------------------------------------------------------------
    // Q-53: a turn that arrives while the route's engine loads on its peer waits for it.
    // -----------------------------------------------------------------------------------------

    /// The Studio's engine through the relay: the proxy's `502 engineUnreachable` until the load
    /// lands, then servable.
    struct LoadingPeer(std::sync::atomic::AtomicBool);

    #[async_trait]
    impl NodeProbe for LoadingPeer {
        async fn probe(&self, _: &Node) -> Result<Servable, String> {
            if self.0.load(Ordering::SeqCst) {
                Ok(Servable::default())
            } else {
                Err("Work's Mac Studio's MLX engine is not serving through Link — v1/models answered 502 Bad Gateway: engineUnreachable: no MLX engine answers at http://127.0.0.1:8090".to_string())
            }
        }
    }

    /// The route's load as the ACP server would settle it: `ends` is what the load came to, and
    /// a load that ends serving flips the peer's probe.
    struct Load<'a> {
        peer: &'a LoadingPeer,
        ends: Option<Result<(), String>>,
        settles: AtomicUsize,
    }

    #[async_trait]
    impl RouteLoad for Load<'_> {
        async fn settle(&self) -> Option<Result<(), String>> {
            self.settles.fetch_add(1, Ordering::SeqCst);
            if matches!(self.ends, Some(Ok(()))) {
                self.peer.0.store(true, Ordering::SeqCst);
            }
            self.ends.clone()
        }
    }

    fn studio_route() -> Node {
        Node {
            kind: NodeKind::MlxRemote(RemoteTarget {
                peer: "studio".to_string(),
                peer_name: "Work's Mac Studio".to_string(),
                base_url: "http://127.0.0.1:61001/relay/cafe".to_string(),
                template_kwargs: None,
            }),
            ..node("remote-studio", 8, 1)
        }
    }

    async fn turn_on(
        nodes: &[Node],
        probe: &dyn NodeProbe,
        load: &dyn RouteLoad,
    ) -> Result<MessageStream, ProviderError> {
        let messages = vec![Message::user().with_text("hi")];
        route_stream(
            &Router::new(),
            nodes,
            probe,
            &FakeProviders,
            &NoKwargs,
            load,
            &RecordingSeam::default(),
            Turn {
                model_config: &ModelConfig::new("swarm"),
                system: "sys",
                messages: &messages,
                tools: &[],
                session: &SessionTemplateKwargs::default(),
            },
        )
        .await
    }

    #[tokio::test]
    async fn a_turn_sent_while_the_route_loads_waits_and_is_answered_when_it_serves() {
        let peer = LoadingPeer(std::sync::atomic::AtomicBool::new(false));
        let load = Load {
            peer: &peer,
            ends: Some(Ok(())),
            settles: AtomicUsize::new(0),
        };
        let mut stream = turn_on(&[studio_route()], &peer, &load).await.unwrap();
        let (message, _) = stream.next().await.unwrap().unwrap();
        assert_eq!(message.unwrap().as_concat_text(), "hello from b");
        assert_eq!(load.settles.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_load_that_fails_ends_the_turn_with_the_routes_words_and_nothing_else_waits() {
        let peer = LoadingPeer(std::sync::atomic::AtomicBool::new(false));
        let words = "swarm chat: Work's Mac Studio's engine did not come up while this message waited — memory gate BLOCK";
        let failed = Load {
            peer: &peer,
            ends: Some(Err(words.to_string())),
            settles: AtomicUsize::new(0),
        };
        let err = turn_on(&[studio_route()], &peer, &failed)
            .await
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            ProviderError::ExecutionError(words.to_string()).to_string()
        );

        // Not loading (reconnecting, failed, off): the pick's own named error, at once.
        let idle = Load {
            peer: &peer,
            ends: None,
            settles: AtomicUsize::new(0),
        };
        let err = turn_on(&[studio_route()], &peer, &idle)
            .await
            .err()
            .unwrap();
        assert!(
            err.to_string().contains("no node can serve this turn"),
            "{err}"
        );
        assert!(err.to_string().contains("engineUnreachable"), "{err}");

        // A pool with no route never asks.
        let lm = node("lm", 1, 1);
        let never = Load {
            peer: &peer,
            ends: Some(Ok(())),
            settles: AtomicUsize::new(0),
        };
        let down = FakeProbe(HashMap::from([(
            "lm".to_string(),
            Err("LM Studio is down".to_string()),
        )]));
        assert!(turn_on(&[lm], &down, &never).await.is_err());
        assert_eq!(never.settles.load(Ordering::SeqCst), 0);
    }

    /// The base URL rule: a stopped local manager defers to the configured port; no block → the
    /// engine's default port; an unreadable block is a named reason, never a default.
    #[tokio::test]
    async fn mlx_base_url_comes_from_config_when_this_process_runs_no_engine() {
        let stopped = goose_sidecar::engine::MlxEngineManager::new()
            .status()
            .await;
        assert_eq!(stopped.state, "stopped");
        let settings = EngineSettings {
            port: 8090,
            ..EngineSettings::default()
        };
        assert_eq!(
            mlx_base_url(&stopped, Ok(settings)).unwrap(),
            "http://127.0.0.1:8090"
        );
        assert_eq!(
            mlx_base_url(&stopped, Err(ConfigError::NotFound("mlx_engine".into()))).unwrap(),
            format!("http://127.0.0.1:{}", EngineSettings::default().port)
        );
        let err = mlx_base_url(
            &stopped,
            Err(ConfigError::DeserializeError("port: not a number".into())),
        )
        .unwrap_err();
        assert!(err.contains("unreadable"), "{err}");
        assert!(err.contains("port: not a number"), "{err}");
    }

    /// The engine's own HTTP surface decides: a fake engine answering /v1/models + /v1/status is
    /// servable with its in-flight count and context window; a served-id mismatch and a dead port
    /// are the named reasons.
    #[tokio::test]
    async fn probe_mlx_reads_the_engine_over_http_not_this_processs_manager() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let engine = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"object":"list","data":[{"id":"workhorse-qwen3.5-9b-4bit-mlx","object":"model","context_window":262144,"tool_call_parser":"qwen3_coder"}]}"#,
            ))
            .mount(&engine)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/status"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"status":"generating","num_running":1,"num_waiting":2}"#),
            )
            .mount(&engine)
            .await;
        let probe = LiveProbe {
            http: reqwest::Client::new(),
            providers: Arc::new(LiveProviders::new()),
        };
        let facts = probe.probe_mlx_at(&engine.uri(), "stopped").await.unwrap();
        assert_eq!(facts.live_in_flight, Some(3));
        assert_eq!(facts.context_window, Some(262_144));
        assert_eq!(
            facts.serves.as_deref(),
            Some("workhorse-qwen3.5-9b-4bit-mlx")
        );
        assert_eq!(facts.follows, None);

        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://127.0.0.1:{}", l.local_addr().unwrap().port())
        };
        let down = probe
            .probe_mlx_at(&dead, "this process's manager: stopped")
            .await
            .unwrap_err();
        assert!(down.contains("MLX engine is not listening on"), "{down}");
        assert!(down.contains("mount it in the MLX window"), "{down}");
        assert!(down.contains("this process's manager: stopped"), "{down}");
    }

    fn remote_route(capacity: u32) -> PublishedRoute {
        PublishedRoute {
            pid: 4242,
            base_url: "http://127.0.0.1:61001/relay/cafe".to_string(),
            peer: "worksmacstudio-lan-9c1e2a".to_string(),
            peer_hostname: "WorksMacStudio.lan".to_string(),
            peer_computer_name: None,
            model_id: HF_ID.to_string(),
            served_model_id: SERVED.to_string(),
            capacity,
            template_kwargs: Some(
                serde_json::json!({"enable_thinking": false})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        }
    }

    #[test]
    fn a_live_remote_route_is_one_node_with_the_peers_cap_and_the_heaviest_weight() {
        let pool = vec![node("lm", 2, 3), mlx_node()];
        assert_eq!(
            with_remote_route(pool.clone(), None).len(),
            2,
            "no route, no node"
        );

        let nodes = with_remote_route(pool, Some(&remote_route(6)));
        let remote = nodes.last().unwrap();
        assert_eq!(remote.id, "remote-WorksMacStudio.lan");
        assert_eq!(remote.model_id, SERVED, "the served id the peer derived");
        assert_eq!(remote.capacity, 6, "the peer's own admission cap");
        assert_eq!(
            remote.weight, 3,
            "a tie on free slots goes to the placement chosen"
        );
        assert_eq!(remote.provider_name(), "omlx");
        assert_eq!(
            remote.provider_cache_key(),
            "omlx-remote@http://127.0.0.1:61001/relay/cafe",
            "its own provider instance, never the OMLX_HOST one"
        );
        assert_ne!(remote.provider_cache_key(), nodes[1].provider_cache_key());
    }

    #[test]
    fn while_chat_is_routed_to_a_peer_this_macs_sidecar_is_not_a_candidate() {
        let mine = sidecar_routed_away(&RouteRecord::Mine(remote_route(8))).unwrap();
        assert!(mine.contains("served from WorksMacStudio.lan"), "{mine}");
        assert!(
            !mine.contains("/relay/"),
            "the capability never enters a reason: {mine}"
        );
        assert!(sidecar_routed_away(&RouteRecord::Other(remote_route(8))).is_some());
        // The reason names the Mac by its owner's name; the node id keeps the hostname.
        let named = PublishedRoute {
            peer_computer_name: Some("Work's Mac Studio".to_string()),
            ..remote_route(8)
        };
        let said = sidecar_routed_away(&RouteRecord::Mine(named.clone())).unwrap();
        assert!(
            said.starts_with("this Mac's MLX chat is served from Work's Mac Studio (remote single, node remote-WorksMacStudio.lan)"),
            "{said}"
        );
        let nodes = with_remote_route(vec![], Some(&named));
        match &nodes[0].kind {
            NodeKind::MlxRemote(target) => assert_eq!(target.peer_name, "Work's Mac Studio"),
            other => panic!("not the remote node: {other:?}"),
        }
        let torn = sidecar_routed_away(&RouteRecord::Unreadable {
            path: "/state/mlx-remote-route.json".into(),
            error: "EOF".into(),
        })
        .unwrap();
        assert!(torn.contains("unreadable"), "{torn}");
        assert!(sidecar_routed_away(&RouteRecord::Absent).is_none());
        assert!(
            sidecar_routed_away(&RouteRecord::Stale(remote_route(8))).is_none(),
            "a dead owner's route is ignored"
        );
    }

    #[tokio::test]
    async fn the_remote_node_is_probed_through_the_relay_and_names_the_peer_not_the_url() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let relay = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/relay/cafe/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                r#"{{"object":"list","data":[{{"id":"{SERVED}","object":"model","context_window":262144}}]}}"#
            )))
            .mount(&relay)
            .await;
        Mock::given(method("GET"))
            .and(path("/relay/cafe/v1/status"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"num_running":2,"num_waiting":1}"#),
            )
            .mount(&relay)
            .await;
        let probe = LiveProbe {
            http: reqwest::Client::new(),
            providers: Arc::new(LiveProviders::new()),
        };
        let target = RemoteTarget {
            peer: "studio".to_string(),
            peer_name: "WorksMacStudio.lan".to_string(),
            base_url: format!("{}/relay/cafe", relay.uri()),
            template_kwargs: None,
        };
        let facts = probe.probe_remote(&target, SERVED).await.unwrap();
        assert_eq!(facts.live_in_flight, Some(3), "the peer engine's own count");
        assert_eq!(facts.context_window, Some(262_144));

        let wrong = probe.probe_remote(&target, "other").await.unwrap_err();
        assert!(
            wrong.contains("WorksMacStudio.lan's MLX engine serves"),
            "{wrong}"
        );

        let refusing = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string(
                "chatServingDisabled: \"Let my other Macs use this Mac › Answer chat\" is off on WorksMacStudio.lan",
            ))
            .mount(&refusing)
            .await;
        let off = RemoteTarget {
            base_url: format!("{}/relay/cafe", refusing.uri()),
            ..target
        };
        let reason = probe.probe_remote(&off, SERVED).await.unwrap_err();
        assert!(reason.contains("chatServingDisabled"), "{reason}");
        assert!(
            reason.starts_with("WorksMacStudio.lan's MLX engine is not serving"),
            "{reason}"
        );
        assert!(
            !reason.contains("cafe"),
            "the capability never enters a reason: {reason}"
        );
    }

    /// The remote node's provider is the `omlx` definition aimed at the relay's capability path:
    /// the chat request lands under `/relay/<cap>/v1/chat/completions`, the peer profile's
    /// kwargs ride it, and a stream that ends without its marker is still the named cut
    /// (50ca4247d) — the proxy passes it through unchanged and the parser names it here.
    #[tokio::test]
    async fn the_remote_provider_posts_under_the_relay_path_and_a_cut_stream_stays_an_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let relay = MockServer::start().await;
        let cut = "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Par\"}}]}\n\n";
        Mock::given(method("POST"))
            .and(path("/relay/cafe/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(cut),
            )
            .mount(&relay)
            .await;
        let target = RemoteTarget {
            peer: "studio".to_string(),
            peer_name: "WorksMacStudio.lan".to_string(),
            base_url: format!("{}/relay/cafe", relay.uri()),
            template_kwargs: None,
        };
        let provider = remote_provider(&target).expect("the omlx definition builds");
        let messages = vec![Message::user().with_text("Capital of France?")];
        let mut cfg = ModelConfig::new(SERVED);
        add_template_kwargs(
            &mut cfg,
            serde_json::json!({"enable_thinking": false})
                .as_object()
                .unwrap()
                .clone(),
        )
        .unwrap();
        let mut stream = provider.stream(&cfg, "sys", &messages, &[]).await.unwrap();
        let mut outcome = Ok(());
        while let Some(item) = stream.next().await {
            if let Err(e) = item {
                outcome = Err(e);
                break;
            }
        }
        let err = outcome.expect_err("a stream without finish_reason/[DONE] is not an answer");
        assert!(
            err.to_string()
                .contains(goose_providers::errors::STREAM_TRUNCATED),
            "{err}"
        );
        let requests = relay.received_requests().await.unwrap();
        let posts: Vec<_> = requests
            .iter()
            .filter(|r| r.method == wiremock::http::Method::POST)
            .collect();
        assert_eq!(posts.len(), 1, "one chat request, not retried");
        assert!(
            requests
                .iter()
                .all(|r| r.url.path().starts_with("/relay/cafe/v1/")),
            "every call stays under the relay's capability path: {:?}",
            requests
                .iter()
                .map(|r| r.url.path().to_string())
                .collect::<Vec<_>>()
        );
        let body: Value = serde_json::from_slice(&posts[0].body).unwrap();
        assert_eq!(body["model"], SERVED);
        assert_eq!(
            body["chat_template_kwargs"],
            serde_json::json!({"enable_thinking": false})
        );
    }

    /// Q-260 over Link: the remote single's request is built by the same `omlx` provider, so the
    /// peer engine's own `/v1/models` declaration — read through the relay, under its capability
    /// path — decides; a text-only peer is sent the image's placeholder and the chat is told.
    #[tokio::test]
    async fn a_text_only_peer_over_link_is_sent_a_placeholder_never_the_image() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let relay = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/relay/cafe/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": [{"id": SERVED, "capabilities": ["text", "tools"]}],
            })))
            .mount(&relay)
            .await;
        let answer = "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"No image here.\"}}]}\n\n\
                      data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
                      data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/relay/cafe/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(answer),
            )
            .mount(&relay)
            .await;
        let target = RemoteTarget {
            peer: "studio".to_string(),
            peer_name: "WorksMacStudio.lan".to_string(),
            base_url: format!("{}/relay/cafe", relay.uri()),
            template_kwargs: None,
        };
        let provider = remote_provider(&target).expect("the omlx definition builds");
        let messages = vec![Message::user()
            .with_text("What does this screenshot say?")
            .with_image("iVBORw0KGgo=", "image/png")];
        let stream = provider
            .stream(&ModelConfig::new(SERVED), "sys", &messages, &[])
            .await
            .unwrap();
        let items: Vec<_> = stream.collect().await;
        let first = items[0].as_ref().unwrap().0.as_ref().unwrap();
        assert!(
            matches!(
                &first.content[..],
                [crate::conversation::message::MessageContent::SystemNotification(n)]
                    if n.msg.ends_with("reads text only — the image attachment (image/png) was not sent")
            ),
            "{first:?}"
        );
        assert!(items.iter().all(|item| item.is_ok()));

        let requests = relay.received_requests().await.unwrap();
        let post = requests
            .iter()
            .find(|r| r.method == wiremock::http::Method::POST)
            .unwrap();
        let body = String::from_utf8(post.body.clone()).unwrap();
        assert!(!body.contains("image_url"), "{body}");
        assert!(
            body.contains("[image attachment (image/png) not sent: this model reads text only]"),
            "{body}"
        );
    }

    /// Live 2026-09-24 (3.0.19): the distributed engine served the HF id while the `mihai-mlx` node
    /// names the single engine's alias, so the router refused the node. The rank specs now carry
    /// the id `engine::served_model_id` derives — the single engine's `--served-model-name` — and
    /// the wrapper's /v1/models answer (its exact shape, `owned_by: goose-distributed`) passes the
    /// same probe the single engine passes.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_distributed_engine_serves_the_nodes_id_and_the_router_accepts_it() {
        use goose_sidecar::distributed::{
            launch::{rank_specs, TensorLaunch},
            plan::TensorPrefill,
            DistributedConfig,
        };
        let launch = TensorLaunch {
            planned_bytes: 1,
            prompt_cache_limit_bytes: 1,
            prompt_cache_entries: 1,
            mlx_cache_limit_bytes: 1,
            prefill: TensorPrefill {
                step: 1,
                workspace_bytes: 1,
                pair_bytes: 1,
                kv_bytes_per_token: 1,
                sequence_state_bytes: 1,
                batch_transient_ratio: 1.0,
            },
        };
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        const HF: &str = "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx";
        const NODE_MODEL: &str = "mihai-qwen3.8-27b-atlassian-q8-mlx";
        let settings = EngineSettings {
            model_id: Some(HF.to_string()),
            served_model_name: Some(NODE_MODEL.to_string()),
            ..EngineSettings::default()
        };
        let config: DistributedConfig = serde_json::from_value(serde_json::json!({
            "model_id": HF, "backend": "jaccl", "port": 8091, "coordinator_port": 8092,
            "nodes": [
                {"name": "a", "tb_ip": "192.168.0.1", "tb_netmask": "255.255.255.252",
                 "tb_interface": "en3", "tb_service": "TB", "rdma_device": "rdma_en3",
                 "python": "/p", "model_dir": "/m"},
                {"name": "b", "ssh": "peer", "tb_ip": "192.168.0.2", "tb_netmask": "255.255.255.252",
                 "tb_interface": "en3", "tb_service": "TB", "rdma_device": "rdma_en3",
                 "python": "/p", "model_dir": "/m"}
            ]
        }))
        .unwrap();
        let served =
            goose_sidecar::model_identity::ServedNames::of(&settings, &config.model_id, &[]);
        let specs = rank_specs(&config, &served, &[launch, launch], 65_536, 2.0);
        assert!(specs.iter().all(|s| s.served_id == NODE_MODEL), "{specs:?}");
        assert_eq!(
            specs[0].served_aliases,
            [HF],
            "the split answers to its HF id too (Q-131)"
        );

        let listing = |ids: &[&str]| {
            let data: Vec<String> = ids
                .iter()
                .map(|id| {
                    format!(
                        r#"{{"id":"{id}","object":"model","owned_by":"goose-distributed","context_window":65536}}"#
                    )
                })
                .collect();
            format!(r#"{{"object":"list","data":[{}]}}"#, data.join(","))
        };
        let wrapper = |id: &str| listing(&[id]);
        let probe = LiveProbe {
            http: reqwest::Client::new(),
            providers: Arc::new(LiveProviders::new()),
        };
        let engine = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(listing(&[
                specs[0].served_id.as_str(),
                specs[0].served_aliases[0].as_str(),
            ])))
            .mount(&engine)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/status"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"num_running":0,"num_waiting":0,"status":"ok"}"#),
            )
            .mount(&engine)
            .await;
        const OWNER: &str = "the distributed MLX engine owns this Mac";
        let verdict = |served: &str, intent: &IntentRecord| {
            let repo = goose_sidecar::model_identity::served_repo(&settings, served);
            mlx_node_verdict("mihai-mlx", NODE_MODEL, served, &repo, intent)
        };
        let facts = probe.probe_mlx_at(&engine.uri(), OWNER).await.unwrap();
        assert_eq!(facts.context_window, Some(65_536));
        assert_eq!(facts.live_in_flight, Some(0));
        let served = facts.serves.unwrap();
        assert_eq!(served, NODE_MODEL);
        assert_eq!(verdict(&served, &IntentRecord::Absent), Ok(None));

        // An older wrapper that served the 27B's HF id: the same model on disk, so the node named
        // by its Add-node alias takes it (Q-128's one identity) instead of being refused.
        let before = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(wrapper(HF)))
            .mount(&before)
            .await;
        let served = probe
            .probe_mlx_at(&before.uri(), OWNER)
            .await
            .unwrap()
            .serves
            .unwrap();
        assert_eq!(served, HF);
        assert_eq!(verdict(&served, &IntentRecord::Absent), Ok(None));

        // Live 2026-09-24 (3.0.26): a Flash split served under the 27B's alias. The alias is the
        // 27B's own, so a split of ANOTHER model serves that model's HF id, and the node named for
        // the 27B is never mistaken for it: it follows only the owner's own start of the split
        // (Q-128), and is refused by name for a split nobody started here.
        const FLASH: &str = "rapid-mlx/Qwen3.8-Flash-Next-4bit";
        let flash_config = DistributedConfig {
            model_id: FLASH.to_string(),
            ..config.clone()
        };
        let flash_served =
            goose_sidecar::model_identity::ServedNames::of(&settings, &flash_config.model_id, &[]);
        assert_eq!(
            flash_served,
            goose_sidecar::model_identity::ServedNames::only(FLASH)
        );
        let flash_specs = rank_specs(&flash_config, &flash_served, &[launch, launch], 65_536, 2.0);
        assert!(
            flash_specs.iter().all(|s| s.served_id == FLASH),
            "{flash_specs:?}"
        );
        let flash = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(wrapper(FLASH)))
            .mount(&flash)
            .await;
        let served = probe
            .probe_mlx_at(&flash.uri(), OWNER)
            .await
            .unwrap()
            .serves
            .unwrap();
        assert_eq!(served, FLASH);
        let refused = verdict(&served, &IntentRecord::Absent).unwrap_err();
        assert!(
            refused.contains(&format!(
                "serves '{FLASH}', the device wants '{NODE_MODEL}'"
            )),
            "{refused}"
        );
        let started = IntentRecord::Present(ServingIntent::Split {
            model_id: FLASH.to_string(),
        });
        assert_eq!(verdict(&served, &started), Ok(Some(NODE_MODEL.to_string())));
    }

    /// A second desktop window runs its own goosed, whose distributed manager supervises nothing:
    /// the record the owning window published is what points its router at the engine. A stale
    /// record is named and ignored; an unreadable one refuses to guess.
    #[test]
    fn another_windows_distributed_engine_is_found_through_its_published_record() {
        use mlx_distributed_owner::{OwnerRecord, PublishedEngine};
        let engine = PublishedEngine {
            pid: 4242,
            base_url: "http://127.0.0.1:8191".to_string(),
            served_model_id: "mihai-qwen3.8-27b-atlassian-q8-mlx".to_string(),
            model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            backend: "jaccl".to_string(),
            node_names: vec!["a".to_string(), "b".to_string()],
        };
        match distributed_target(None, OwnerRecord::Other(engine.clone())).unwrap() {
            DistributedTarget::At { base, diagnostic } => {
                assert_eq!(base, "http://127.0.0.1:8191");
                assert!(
                    diagnostic.contains("another window (goosed pid 4242)"),
                    "{diagnostic}"
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            distributed_target(
                Some("http://127.0.0.1:9000".to_string()),
                OwnerRecord::Other(engine.clone())
            )
            .unwrap(),
            DistributedTarget::At {
                base: "http://127.0.0.1:9000".to_string(),
                diagnostic: "the distributed MLX engine owns this Mac".to_string()
            },
            "this process's own run wins over any record"
        );
        match distributed_target(None, OwnerRecord::Stale(engine.clone())).unwrap() {
            DistributedTarget::None { stale: Some(stale) } => {
                assert!(
                    stale.contains("goosed pid 4242") && stale.contains("gone"),
                    "{stale}"
                )
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            distributed_target(None, OwnerRecord::Mine(engine)).unwrap(),
            DistributedTarget::None { stale: None },
            "our own record with our manager idle is a run that ended, not an engine"
        );
        let unreadable = distributed_target(
            None,
            OwnerRecord::Unreadable {
                path: "/x/mlx-distributed-owner.json".into(),
                error: "EOF".to_string(),
            },
        )
        .unwrap_err();
        assert!(unreadable.contains("unreadable (EOF)"), "{unreadable}");
    }

    #[test]
    fn conversation_key_is_stable_across_turns_of_one_conversation() {
        let first = vec![Message::user().with_text("build me a ledger")];
        let later = vec![
            Message::user().with_text("build me a ledger"),
            Message::assistant().with_text("sure"),
            Message::user().with_text("now add tests"),
        ];
        assert_eq!(
            Router::conversation_key("sys", &first),
            Router::conversation_key("sys", &later)
        );
        assert_ne!(
            Router::conversation_key("sys", &first),
            Router::conversation_key("sys", &[Message::user().with_text("other")])
        );
    }

    // -----------------------------------------------------------------------------------------
    // D1: Auto's tie-break reads the pool's Share; with equal or unset Shares it is today's.
    // -----------------------------------------------------------------------------------------

    fn shared(id: &str, capacity: u32, weight: u32, share: Option<u32>) -> Node {
        Node {
            share,
            ..node(id, capacity, weight)
        }
    }

    #[tokio::test]
    async fn ties_go_to_the_larger_share_before_the_heavier_weight() {
        let router = Router::new();
        let nodes = vec![
            shared("heavy", 2, 3, None),
            shared("light-fast", 2, 1, Some(3)),
        ];
        let probe = FakeProbe::all_idle(&nodes);
        let lease = router
            .pick(&nodes, &probe, 1, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(
            lease.node.id, "light-fast",
            "the Share outranks concurrency"
        );
        drop(lease);

        // Equal Shares: the heavier weight, as before.
        let nodes = vec![
            shared("light", 2, 1, Some(2)),
            shared("heavy", 2, 3, Some(2)),
        ];
        let lease = router
            .pick(&nodes, &FakeProbe::all_idle(&nodes), 2, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(lease.node.id, "heavy");
        drop(lease);

        // More free slots still wins over any Share.
        let nodes = vec![shared("roomy", 4, 1, None), shared("fast", 2, 1, Some(9))];
        let lease = router
            .pick(&nodes, &FakeProbe::all_idle(&nodes), 3, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(lease.node.id, "roomy");
    }

    /// The byte-identity proof for Auto: over every pool of up to four nodes, every weight and
    /// every free-slot count, with the Shares unset or all equal, the new key picks exactly the
    /// node today's key `(free, weight)` picks (`max_by_key` keeps the LAST of equals, both times).
    #[test]
    fn with_unset_or_equal_shares_auto_picks_exactly_what_it_picked_before() {
        let values = [0u32, 1, 2, 3];
        let mut pools = 0;
        for len in 1..=4usize {
            let combos = values.len().pow(2 * len as u32);
            for mut code in 0..combos {
                let mut slots: Vec<(Node, u32)> = Vec::new();
                for i in 0..len {
                    let weight = values[code % values.len()];
                    code /= values.len();
                    let free = values[code % values.len()];
                    code /= values.len();
                    slots.push((node(&format!("n{i}"), 4, weight), free));
                }
                for share in [None, Some(1), Some(5)] {
                    let with_share: Vec<(Node, u32)> = slots
                        .iter()
                        .map(|(n, free)| (Node { share, ..n.clone() }, *free))
                        .collect();
                    let today = with_share
                        .iter()
                        .filter(|(_, free)| *free > 0)
                        .max_by_key(|(n, free)| (*free, n.weight))
                        .map(|(n, _)| n.id.clone());
                    let now = with_share
                        .iter()
                        .filter(|(_, free)| *free > 0)
                        .max_by_key(|(n, free)| (*free, n.tie_share(), n.weight))
                        .map(|(n, _)| n.id.clone());
                    assert_eq!(now, today, "{slots:?} share {share:?}");
                    pools += 1;
                }
            }
        }
        assert!(pools > 100_000, "{pools}");
    }

    #[test]
    fn the_pools_share_is_the_devices_speed_weight_and_the_route_takes_the_largest() {
        let cfg = PoolConfig {
            endpoint: "http://lm".to_string(),
            devices: vec![
                PoolDevice {
                    id: "fast".to_string(),
                    model_id: "m".to_string(),
                    weight: 1,
                    enabled: true,
                    speed_weight: Some(4),
                    ..PoolDevice::default()
                },
                PoolDevice {
                    id: "unset".to_string(),
                    model_id: "m".to_string(),
                    weight: 1,
                    enabled: true,
                    ..PoolDevice::default()
                },
            ],
        };
        let nodes = nodes_from_config(&cfg);
        assert_eq!(nodes[0].share, Some(4));
        assert_eq!(nodes[1].share, None);
        let with_route = with_remote_route(nodes, Some(&remote_route(2)));
        assert_eq!(with_route.last().unwrap().share, Some(4));
        // The block as the desktop writes it parses the Share.
        let parsed: PoolConfig = serde_json::from_value(serde_json::json!({
            "devices": [{"id": "a", "model_id": "m", "weight": 1, "enabled": true, "speed_weight": 3}]
        }))
        .unwrap();
        assert_eq!(parsed.devices[0].speed_weight, Some(3));
    }

    // -----------------------------------------------------------------------------------------
    // Routes to a node or a strategy (S3).
    // -----------------------------------------------------------------------------------------

    use crate::nodes::{NodeDef, NodeOrigin};
    use std::collections::VecDeque;

    type MemberMap = Arc<StdMutex<HashMap<String, Result<Member, EntryFact>>>>;

    struct FakeMembers(MemberMap);

    #[async_trait]
    impl ChainMembers for FakeMembers {
        async fn member(&self, def: &ResolvedNodeDef) -> Result<Member, EntryFact> {
            self.0
                .lock()
                .unwrap()
                .get(&def.def.id)
                .cloned()
                .unwrap_or_else(|| Err(cant_run("not in the fake")))
        }
    }

    fn member(id: &str) -> Member {
        Member {
            node: node(id, 1, 1),
            pinned: false,
            way: None,
        }
    }

    fn members(entries: Vec<(&str, Result<Member, EntryFact>)>) -> MemberMap {
        Arc::new(StdMutex::new(
            entries
                .into_iter()
                .map(|(id, m)| (id.to_string(), m))
                .collect(),
        ))
    }

    #[derive(Default)]
    struct RecordingSeam {
        /// What each demand is answered, in order; none left = the loader's named absence.
        answers: StdMutex<VecDeque<NodeEnsureServing>>,
        /// On `Ready`: the member the node becomes.
        becomes: StdMutex<Option<(MemberMap, String, Member)>>,
        loader: bool,
        demands: StdMutex<Vec<String>>,
        served: StdMutex<Vec<(String, NodeServedTurnDto)>>,
        notes: StdMutex<Vec<(String, MlxPlacementKeyDto)>>,
        /// What each "is a switch queued ahead?" is answered, in order; none left = nothing is.
        ahead: StdMutex<VecDeque<Option<String>>>,
        /// Every (session, node) the router asked about before a lease.
        asked: StdMutex<Vec<(String, String)>>,
        /// Every (session, node) that waited behind a queued switch.
        waited: StdMutex<Vec<(String, String)>>,
        /// A node's slots, read while a lease waits behind a queued switch.
        watch: StdMutex<Option<Arc<Semaphore>>>,
        free_while_waiting: StdMutex<Vec<usize>>,
    }

    impl RecordingSeam {
        fn answering(answers: Vec<NodeEnsureServing>) -> Self {
            Self {
                answers: StdMutex::new(answers.into()),
                ..Self::default()
            }
        }

        fn last(&self) -> NodeServedTurnDto {
            self.served.lock().unwrap().last().unwrap().1.clone()
        }
    }

    #[async_trait]
    impl NodesSeam for RecordingSeam {
        async fn ensure_serving(&self, demand: Demand) -> NodeEnsureServing {
            self.demands.lock().unwrap().push(demand.node.id.clone());
            let answer = self.answers.lock().unwrap().pop_front().unwrap_or_else(|| {
                NodeEnsureServing::Refused {
                    code: NodeLoadRefusalCode::LoaderAbsent,
                    reason: crate::nodes::seam::loader_absent_reason(&demand.node.name),
                }
            });
            if answer == NodeEnsureServing::Ready {
                if let Some((map, id, member)) = self.becomes.lock().unwrap().take() {
                    map.lock().unwrap().insert(id, Ok(member));
                }
            }
            answer
        }

        fn loader_installed(&self) -> bool {
            self.loader
        }

        fn note_lease(&self, session: &str, way: &MlxPlacementKeyDto) {
            self.notes
                .lock()
                .unwrap()
                .push((session.to_string(), way.clone()));
        }

        fn served(&self, session: &str, turn: NodeServedTurnDto) {
            self.served
                .lock()
                .unwrap()
                .push((session.to_string(), turn));
        }

        fn queued_switch_ahead(&self, session: &str, node: &str) -> Option<String> {
            self.asked
                .lock()
                .unwrap()
                .push((session.to_string(), node.to_string()));
            self.ahead.lock().unwrap().pop_front().flatten()
        }

        async fn wait_behind_queued_switches(&self, session: &str, node: &str) {
            self.waited
                .lock()
                .unwrap()
                .push((session.to_string(), node.to_string()));
            if let Some(slots) = self.watch.lock().unwrap().as_ref() {
                self.free_while_waiting
                    .lock()
                    .unwrap()
                    .push(slots.available_permits());
            }
        }
    }

    struct AllAnswer;

    #[async_trait]
    impl ProviderSource for AllAnswer {
        async fn provider_for(&self, _: &Node) -> Result<Arc<dyn Provider>, String> {
            Ok(Arc::new(AnsweringProvider))
        }
    }

    fn cloud_def(id: &str) -> NodeDef {
        NodeDef {
            id: id.to_string(),
            name: format!("Node {id}"),
            kind: NodeDefKind::Cloud,
            model: Some(format!("{id}-model")),
            placement: None,
            goal: None,
            provider: Some("openrouter".to_string()),
            keep_loaded: false,
            pool_device: None,
            origin: NodeOrigin::User,
        }
    }

    fn chain(entry: NodeRoleEntry) -> ChainPlan {
        let defs = entry
            .chain
            .iter()
            .map(|l| {
                (
                    l.node.clone(),
                    crate::nodes::resolve_def(&cloud_def(&l.node), &Ok(None), false),
                )
            })
            .collect();
        ChainPlan {
            label: "the strategy \"Test\" (chat)".to_string(),
            role: Some(NodeRole::Chat),
            share_key: "strategy:test@chat".to_string(),
            entry,
            defs,
        }
    }

    fn role(
        nodes: &[(&str, u32)],
        when: NodeWhen,
        if_not_loaded: NodeIfNotLoaded,
    ) -> NodeRoleEntry {
        NodeRoleEntry {
            chain: nodes
                .iter()
                .map(|(n, w)| NodeChainEntry {
                    node: n.to_string(),
                    weight: *w,
                })
                .collect(),
            when,
            if_not_loaded,
        }
    }

    const SESSION: &str = "20260927_7";

    #[allow(clippy::too_many_arguments)]
    async fn chain_turn(
        router: &Router,
        plan: &ChainPlan,
        members: &dyn ChainMembers,
        probe: &dyn NodeProbe,
        providers: &dyn ProviderSource,
        seam: &dyn NodesSeam,
        first_message: &str,
    ) -> Result<MessageStream, ProviderError> {
        let messages = vec![Message::user().with_text(first_message)];
        let session = SessionTemplateKwargs::default();
        crate::session_context::with_session_id(
            Some(SESSION.to_string()),
            route_chain(
                router,
                plan,
                members,
                probe,
                providers,
                &NoKwargs,
                &NoRouteLoad,
                seam,
                Turn {
                    model_config: &ModelConfig::new("strategy:test"),
                    system: "sys",
                    messages: &messages,
                    tools: &[],
                    session: &session,
                },
            ),
        )
        .await
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ResolveCase {
        name: String,
        entry: NodeRoleEntry,
        facts: HashMap<String, EntryFact>,
        #[serde(default)]
        sticky: Option<String>,
        share: ShareState,
        expect: Decision,
        share_after: ShareState,
    }

    #[derive(serde::Deserialize)]
    struct Fixture {
        resolve: Vec<ResolveCase>,
    }

    fn tried_dtos(tried: &[Tried]) -> Vec<NodeTriedDto> {
        tried
            .iter()
            .map(|t| NodeTriedDto {
                node: t.node.clone(),
                reason: passed_words(&t.why),
            })
            .collect()
    }

    /// The shared fixture (the one `nodes::resolve` and the desktop's mirror run), driven through
    /// the ROUTER: each case's facts become live members and probes, and what the router does —
    /// the node it leases, the rank and entries it records, the load it demands, the refusal it
    /// ends the turn with, the round-robin state it keeps — is the case's decision.
    #[tokio::test]
    async fn the_router_routes_every_fixture_case_as_the_rule_decides() {
        let fixture: Fixture =
            serde_json::from_str(include_str!("../nodes/nodes.fixture.json")).unwrap();
        let mut driven = 0;
        let mut unknown = Vec::new();
        for case in fixture.resolve {
            if case
                .entry
                .chain
                .iter()
                .any(|l| !case.facts.contains_key(&l.node))
            {
                // The router always knows a fact for every entry; "no fact" is resolve's own guard.
                unknown.push(case.name);
                continue;
            }
            let mut entries = Vec::new();
            let mut probes = HashMap::new();
            for (id, fact) in &case.facts {
                match fact {
                    EntryFact::Servable => {
                        entries.push((id.as_str(), Ok(member(id))));
                        probes.insert(id.clone(), Ok(Servable::default()));
                    }
                    EntryFact::Busy => {
                        entries.push((id.as_str(), Ok(member(id))));
                        probes.insert(id.clone(), busy(1));
                    }
                    EntryFact::CantRun { reason } => {
                        entries.push((id.as_str(), Ok(member(id))));
                        probes.insert(id.clone(), Err(reason.clone()));
                    }
                    other => entries.push((id.as_str(), Err(other.clone()))),
                }
            }
            let plan = chain(case.entry.clone());
            let router = Router::new();
            router
                .shares
                .lock()
                .unwrap()
                .insert(plan.share_key.clone(), case.share.clone());
            let messages = vec![Message::user().with_text(&case.name)];
            let key = Router::conversation_key("sys", &messages);
            if let Some(sticky) = &case.sticky {
                router.sticky.lock().unwrap().insert(key, sticky.clone());
            }
            let seam = RecordingSeam::default();
            let outcome = chain_turn(
                &router,
                &plan,
                &FakeMembers(members(entries)),
                &FakeProbe(probes),
                &AllAnswer,
                &seam,
                &case.name,
            )
            .await;
            let name = &case.name;
            match &case.expect {
                Decision::Serve { node, rank, tried } => {
                    assert!(outcome.is_ok(), "{name}: {:?}", outcome.err());
                    let record = seam.last();
                    assert_eq!(&record.node, node, "{name}");
                    assert_eq!(record.rank, *rank, "{name}");
                    assert_eq!(record.tried, tried_dtos(tried), "{name}");
                    assert_eq!(record.role, Some(NodeRole::Chat), "{name}");
                    assert_eq!(
                        router.shares.lock().unwrap().get(&plan.share_key).cloned(),
                        Some(case.share_after.clone()),
                        "{name} (share state)"
                    );
                }
                Decision::Queue { nodes, tried } => {
                    assert!(outcome.is_ok(), "{name}: {:?}", outcome.err());
                    let record = seam.last();
                    assert!(nodes.contains(&record.node), "{name}: {}", record.node);
                    assert_eq!(record.tried, tried_dtos(tried), "{name}");
                }
                Decision::Load { node, .. } => {
                    assert_eq!(
                        seam.demands.lock().unwrap().first(),
                        Some(node),
                        "{name}: the loader is asked for the entry the rule loads"
                    );
                }
                Decision::Exhausted { tried } => {
                    let err = outcome.err().unwrap().to_string();
                    assert!(
                        err.contains("no node can serve this turn — "),
                        "{name}: {err}"
                    );
                    for t in tried {
                        let named = format!("{}: {}", t.node, passed_words(&t.why));
                        assert!(err.contains(&named), "{name}: {named} missing from {err}");
                    }
                    assert!(seam.served.lock().unwrap().is_empty(), "{name}");
                }
            }
            driven += 1;
        }
        assert_eq!(unknown.len(), 1, "{unknown:?}");
        assert!(driven >= 69, "{driven}");
    }

    #[tokio::test]
    async fn a_chain_with_nothing_servable_ends_the_turn_naming_every_entry_never_any_node() {
        let plan = chain(role(
            &[("a", 1), ("b", 1), ("c", 1)],
            NodeWhen::Failover,
            NodeIfNotLoaded::UseNext,
        ));
        let map = members(vec![
            ("a", Err(EntryFact::NotLoaded)),
            ("b", Ok(member("b"))),
            (
                "c",
                Err(cant_run(
                    "Work's Mac Studio is not connected to LeanZero Link",
                )),
            ),
        ]);
        let probe = FakeProbe(HashMap::from([(
            "b".to_string(),
            Err("OpenRouter answered 401".to_string()),
        )]));
        let seam = RecordingSeam::default();
        let err = chain_turn(
            &Router::new(),
            &plan,
            &FakeMembers(map),
            &probe,
            &AllAnswer,
            &seam,
            "x",
        )
        .await
        .err()
        .unwrap()
        .to_string();
        assert!(
            err.contains(
                "swarm chat: the strategy \"Test\" (chat): no node can serve this turn — a: not loaded; b: OpenRouter answered 401; c: Work's Mac Studio is not connected to LeanZero Link"
            ),
            "{err}"
        );
        assert!(
            seam.demands.lock().unwrap().is_empty(),
            "useNext never loads"
        );
    }

    #[tokio::test]
    async fn a_failed_load_under_failover_goes_to_the_next_entry_with_the_failure_named() {
        let plan = chain(role(
            &[("a", 1), ("b", 1)],
            NodeWhen::Failover,
            NodeIfNotLoaded::Load,
        ));
        let map = members(vec![
            ("a", Err(EntryFact::NotLoaded)),
            ("b", Ok(member("b"))),
        ]);
        let seam = RecordingSeam::answering(vec![NodeEnsureServing::Refused {
            code: NodeLoadRefusalCode::LoadFailed,
            reason: "memory gate BLOCK".to_string(),
        }]);
        let stream = chain_turn(
            &Router::new(),
            &plan,
            &FakeMembers(map),
            &FakeProbe::all_idle(&[node("b", 1, 1)]),
            &AllAnswer,
            &seam,
            "x",
        )
        .await
        .unwrap();
        drop(stream);
        assert_eq!(*seam.demands.lock().unwrap(), vec!["a".to_string()]);
        let record = seam.last();
        assert_eq!(record.node, "b");
        assert_eq!(record.rank, 2);
        assert_eq!(
            record.reason.as_deref(),
            Some("failed to load: memory gate BLOCK")
        );
        assert_eq!(
            record.tried,
            vec![NodeTriedDto {
                node: "a".to_string(),
                reason: "failed to load: memory gate BLOCK".to_string()
            }]
        );
        assert_eq!(record.loaded_ms, None);
        assert_eq!(seam.served.lock().unwrap()[0].0, SESSION);
    }

    #[tokio::test]
    async fn a_not_loaded_node_with_no_loader_installed_is_the_seams_named_refusal() {
        assert!(
            !crate::nodes::seam::loader_installed(),
            "no unit test installs a loader"
        );
        let plan = ChainPlan {
            label: "\"Node a\"".to_string(),
            role: None,
            ..chain(role(&[("a", 1)], NodeWhen::Failover, NodeIfNotLoaded::Load))
        };
        let map = members(vec![("a", Err(EntryFact::NotLoaded))]);
        let err = chain_turn(
            &Router::new(),
            &plan,
            &FakeMembers(map),
            &FakeProbe(HashMap::new()),
            &AllAnswer,
            &LiveNodesSeam {
                records_served: false,
            },
            "x",
        )
        .await
        .err()
        .unwrap()
        .to_string();
        assert!(
            err.contains(&format!(
                "swarm chat: \"Node a\": no node can serve this turn — a: {}",
                crate::nodes::seam::loader_absent_reason("Node a")
            )),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_node_the_loader_readies_serves_the_turn_with_its_load_time_recorded() {
        let map = members(vec![("a", Err(EntryFact::NotLoaded))]);
        let seam = RecordingSeam::answering(vec![NodeEnsureServing::Ready]);
        *seam.becomes.lock().unwrap() = Some((map.clone(), "a".to_string(), member("a")));
        let plan = chain(role(&[("a", 1)], NodeWhen::Failover, NodeIfNotLoaded::Load));
        let stream = chain_turn(
            &Router::new(),
            &plan,
            &FakeMembers(map),
            &FakeProbe::all_idle(&[node("a", 1, 1)]),
            &AllAnswer,
            &seam,
            "x",
        )
        .await
        .unwrap();
        drop(stream);
        let record = seam.last();
        assert_eq!((record.node.as_str(), record.rank), ("a", 1));
        assert!(record.loaded_ms.is_some(), "{record:?}");
        assert!(record.tried.is_empty());
    }

    #[tokio::test]
    async fn a_ready_that_never_serves_and_a_wait_are_named_never_spun_on() {
        let plan = chain(role(&[("a", 1)], NodeWhen::Failover, NodeIfNotLoaded::Load));
        for (answer, words) in [
            (
                NodeEnsureServing::Ready,
                "a: the node loader said it serves, but its way is still not the one serving this Mac's chat",
            ),
            (
                NodeEnsureServing::Wait {
                    reason: "27B is answering 1".to_string(),
                },
                "a: the node loader answered that this turn waits (27B is answering 1), with nothing to wake it",
            ),
        ] {
            let seam = RecordingSeam::answering(vec![answer]);
            let map = members(vec![("a", Err(EntryFact::NotLoaded))]);
            let err = chain_turn(
                &Router::new(),
                &plan,
                &FakeMembers(map),
                &FakeProbe(HashMap::new()),
                &AllAnswer,
                &seam,
                "x",
            )
            .await
            .err()
            .unwrap()
            .to_string();
            assert!(err.contains(words), "{err}");
            assert_eq!(seam.demands.lock().unwrap().len(), 1, "one demand per turn");
        }
    }

    #[tokio::test]
    async fn an_admission_refusal_in_a_chain_goes_to_the_next_entry_and_is_recorded() {
        let plan = chain(role(
            &[("a", 1), ("b", 1)],
            NodeWhen::Failover,
            NodeIfNotLoaded::Load,
        ));
        let map = members(vec![("a", Ok(member("a"))), ("b", Ok(member("b")))]);
        let seam = RecordingSeam::default();
        let mut stream = chain_turn(
            &Router::new(),
            &plan,
            &FakeMembers(map),
            &FakeProbe::all_idle(&[node("a", 1, 1), node("b", 1, 1)]),
            &FakeProviders,
            &seam,
            "x",
        )
        .await
        .unwrap();
        let (message, _) = stream.next().await.unwrap().unwrap();
        assert_eq!(message.unwrap().as_concat_text(), "hello from b");
        let record = seam.last();
        assert_eq!((record.node.as_str(), record.rank), ("b", 2));
        assert_eq!(
            record.reason.as_deref(),
            Some("refused admission this turn")
        );
    }

    #[tokio::test]
    async fn share_keeps_a_conversation_on_its_node_and_round_robins_new_ones() {
        let router = Router::new();
        let plan = chain(role(
            &[("a", 1), ("b", 1)],
            NodeWhen::Share,
            NodeIfNotLoaded::Load,
        ));
        let map = members(vec![("a", Ok(member("a"))), ("b", Ok(member("b")))]);
        let probe = FakeProbe::all_idle(&[node("a", 1, 1), node("b", 1, 1)]);
        let seam = RecordingSeam::default();
        let mut served = Vec::new();
        for conversation in ["first", "first", "second"] {
            let stream = chain_turn(
                &router,
                &plan,
                &FakeMembers(map.clone()),
                &probe,
                &AllAnswer,
                &seam,
                conversation,
            )
            .await
            .unwrap();
            drop(stream);
            served.push(seam.last().node);
        }
        assert_eq!(served, vec!["a", "a", "b"], "sticky per conversation");
        assert_eq!(
            router.shares.lock().unwrap().get("strategy:test@chat"),
            Some(&ShareState::from([
                ("a".to_string(), 0),
                ("b".to_string(), 0)
            ])),
            "the sticky turn never advanced the round-robin"
        );
    }

    #[tokio::test]
    async fn overflow_with_every_entry_busy_queues_on_all_and_takes_the_first_that_frees() {
        let router = Arc::new(Router::new());
        let held_a = router
            .semaphore(&node("a", 1, 1))
            .try_acquire_owned()
            .unwrap();
        let held_b = router
            .semaphore(&node("b", 1, 1))
            .try_acquire_owned()
            .unwrap();
        let seam = Arc::new(RecordingSeam::default());
        let (r, s) = (router.clone(), seam.clone());
        let turn = tokio::spawn(async move {
            let plan = chain(role(
                &[("a", 1), ("b", 1)],
                NodeWhen::Overflow,
                NodeIfNotLoaded::Load,
            ));
            let map = members(vec![("a", Ok(member("a"))), ("b", Ok(member("b")))]);
            chain_turn(
                &r,
                &plan,
                &FakeMembers(map),
                &FakeProbe::all_idle(&[node("a", 1, 1), node("b", 1, 1)]),
                &AllAnswer,
                &*s,
                "x",
            )
            .await
            .map(drop)
        });
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        assert!(!turn.is_finished(), "every entry is busy: the turn queues");
        assert_eq!(router.queued.load(Ordering::SeqCst), 1);
        drop(held_b);
        turn.await.unwrap().unwrap();
        assert_eq!(seam.last().node, "b");
        drop(held_a);
    }

    #[tokio::test]
    async fn a_served_record_is_written_for_an_mlx_an_lm_studio_and_a_cloud_lease() {
        let lm = node("lm", 1, 1);
        let mlx = Node {
            kind: NodeKind::MlxSidecar,
            ..node("mlx", 1, 1)
        };
        let cloud = Node {
            kind: NodeKind::Cloud {
                registry: "openrouter".to_string(),
            },
            ..node("cloud", 1, 1)
        };
        let seam = RecordingSeam::default();
        for n in [lm, mlx, cloud] {
            let nodes = vec![n];
            let messages = vec![Message::user().with_text("hi")];
            let session = SessionTemplateKwargs::default();
            let stream = crate::session_context::with_session_id(
                Some(SESSION.to_string()),
                route_stream(
                    &Router::new(),
                    &nodes,
                    &FakeProbe::all_idle(&nodes),
                    &AllAnswer,
                    &NoKwargs,
                    &NoRouteLoad,
                    &seam,
                    Turn {
                        model_config: &ModelConfig::new("swarm"),
                        system: "sys",
                        messages: &messages,
                        tools: &[],
                        session: &session,
                    },
                ),
            )
            .await
            .unwrap();
            drop(stream);
        }
        let served = seam.served.lock().unwrap();
        let nodes: Vec<&str> = served.iter().map(|(_, r)| r.node.as_str()).collect();
        assert_eq!(nodes, vec!["lm", "mlx", "cloud"]);
        for (session, record) in served.iter() {
            assert_eq!(session, SESSION);
            assert_eq!((record.role, record.rank), (None, 1));
            assert!(record.tried.is_empty() && record.reason.is_none());
        }
        assert!(
            seam.notes.lock().unwrap().is_empty(),
            "no loader installed: no way is read for Auto's note"
        );
    }

    #[tokio::test]
    async fn a_lease_outside_any_session_keeps_no_record() {
        let nodes = vec![node("lm", 1, 1)];
        let messages = vec![Message::user().with_text("hi")];
        let seam = RecordingSeam::default();
        let stream = route_stream(
            &Router::new(),
            &nodes,
            &FakeProbe::all_idle(&nodes),
            &AllAnswer,
            &NoKwargs,
            &NoRouteLoad,
            &seam,
            Turn {
                model_config: &ModelConfig::new("swarm"),
                system: "sys",
                messages: &messages,
                tools: &[],
                session: &SessionTemplateKwargs::default(),
            },
        )
        .await
        .unwrap();
        drop(stream);
        assert!(seam.served.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_pinned_leases_way_is_noted_for_the_loader() {
        let way = single_way(crate::nodes::THIS_MAC.to_string());
        let pinned = Member {
            node: Node {
                kind: NodeKind::MlxSidecar,
                ..node("flash", 1, 1)
            },
            pinned: true,
            way: Some(way.clone()),
        };
        let plan = chain(role(
            &[("flash", 1)],
            NodeWhen::Failover,
            NodeIfNotLoaded::Load,
        ));
        let seam = RecordingSeam::default();
        let stream = chain_turn(
            &Router::new(),
            &plan,
            &FakeMembers(members(vec![("flash", Ok(pinned))])),
            &FakeProbe::all_idle(&[node("flash", 1, 1)]),
            &AllAnswer,
            &seam,
            "x",
        )
        .await
        .unwrap();
        drop(stream);
        assert_eq!(
            *seam.notes.lock().unwrap(),
            vec![(SESSION.to_string(), way)]
        );
        // A pinned node whose engine serves another model is not loaded — never followed.
        let other = Member {
            node: Node {
                kind: NodeKind::MlxSidecar,
                ..node("flash", 1, 1)
            },
            pinned: true,
            way: None,
        };
        let seam = RecordingSeam::default();
        let follows = FakeProbe(HashMap::from([(
            "flash".to_string(),
            engine_serves(PINNED_27B, Some(FLASH)),
        )]));
        let err = chain_turn(
            &Router::new(),
            &plan,
            &FakeMembers(members(vec![("flash", Ok(other))])),
            &follows,
            &AllAnswer,
            &seam,
            "y",
        )
        .await
        .err()
        .unwrap()
        .to_string();
        assert_eq!(*seam.demands.lock().unwrap(), vec!["flash".to_string()]);
        assert!(
            err.contains("flash: loading nodes is not available"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_chains_window_is_the_nodes_own_or_the_smallest_its_nodes_report() {
        let one = chain(role(&[("a", 1)], NodeWhen::Failover, NodeIfNotLoaded::Load));
        let three = chain(role(
            &[("a", 1), ("b", 1), ("c", 1)],
            NodeWhen::Failover,
            NodeIfNotLoaded::Load,
        ));
        let map = members(vec![
            ("a", Ok(member("a"))),
            ("b", Ok(member("b"))),
            ("c", Err(EntryFact::NotLoaded)),
        ]);
        let probe = FakeProbe(HashMap::from([
            ("a".to_string(), window(262_144)),
            ("b".to_string(), window(32_768)),
        ]));
        let members = FakeMembers(map);
        assert_eq!(chain_window(&one, &members, &probe).await, Ok(262_144));
        assert_eq!(chain_window(&three, &members, &probe).await, Ok(32_768));
        let only_c = chain(role(&[("c", 1)], NodeWhen::Failover, NodeIfNotLoaded::Load));
        let err = chain_window(&only_c, &members, &probe).await.unwrap_err();
        assert!(err.contains("c: not loaded"), "{err}");
        assert!(err.starts_with("no node of the strategy"), "{err}");
    }

    #[test]
    fn the_nodes_grammar_routes_to_chains_and_a_malformed_id_is_refused_by_name() {
        assert_eq!(nodes_route("swarm"), Ok(None));
        assert_eq!(nodes_route("swarm-build"), Ok(None));
        assert_eq!(nodes_route("swarm-build:strategy:daily"), Ok(None));
        assert_eq!(nodes_route("swarm-anything-else"), Ok(None));
        assert_eq!(
            nodes_route("node:flash"),
            Ok(Some(RouteModel::Node {
                id: "flash".to_string()
            }))
        );
        assert_eq!(
            nodes_route("strategy:daily@build"),
            Ok(Some(RouteModel::Strategy {
                id: "daily".to_string(),
                role: Some(NodeRole::Build)
            }))
        );
        for bad in [
            "node:",
            "strategy:daily@nope",
            "swarm-build:oops",
            "node:a:b",
        ] {
            let err = nodes_route(bad).unwrap_err();
            assert!(
                err.contains(&format!("'{bad}' names no node or strategy")),
                "{err}"
            );
        }
    }

    fn read_of(config: crate::nodes::NodesConfig) -> NodesReadResponse {
        NodesReadResponse {
            nodes: config
                .defs
                .iter()
                .map(|d| crate::nodes::resolve_def(d, &Ok(None), false))
                .collect(),
            config,
            stored: true,
            lm_studio_hidden: 0,
            swarm_error: None,
            notes: Vec::new(),
        }
    }

    #[test]
    fn a_strategy_route_reads_its_roles_chain_and_a_removed_one_is_named() {
        let mut config = crate::nodes::empty_config();
        config.defs = vec![cloud_def("a"), cloud_def("b")];
        config.strategies = vec![crate::nodes::NodeStrategy {
            id: "daily".to_string(),
            name: "Daily".to_string(),
            note: None,
            roles: crate::nodes::NodeStrategyRoles {
                chat: Some(role(&[("a", 1)], NodeWhen::Failover, NodeIfNotLoaded::Load)),
                build: Some(role(
                    &[("b", 2), ("a", 1)],
                    NodeWhen::Share,
                    NodeIfNotLoaded::Load,
                )),
                ..Default::default()
            },
            chat: None,
        }];
        let read = read_of(config);
        let chat = chain_plan(&nodes_route("strategy:daily").unwrap().unwrap(), &read).unwrap();
        assert_eq!(chat.role, Some(NodeRole::Chat));
        assert_eq!(chat.entry.chain[0].node, "a");
        assert_eq!(chat.label, "the strategy \"Daily\" (chat)");
        let build = chain_plan(
            &nodes_route("strategy:daily@build").unwrap().unwrap(),
            &read,
        )
        .unwrap();
        assert_eq!(build.entry.when, NodeWhen::Share);
        assert_eq!(build.share_key, "strategy:daily@build");
        // Testing inherits Build.
        let testing = chain_plan(
            &nodes_route("strategy:daily@testing").unwrap().unwrap(),
            &read,
        )
        .unwrap();
        assert_eq!(testing.entry, build.entry);
        let node = chain_plan(&nodes_route("node:b").unwrap().unwrap(), &read).unwrap();
        assert_eq!(node.role, None);
        assert_eq!(node.entry.chain.len(), 1);
        assert_eq!(node.label, "\"Node b\"");
        let gone = chain_plan(&nodes_route("node:ghost").unwrap().unwrap(), &read)
            .err()
            .unwrap();
        assert_eq!(
            gone,
            "the node 'ghost' was removed. Pick another node from the chip."
        );
        let gone = chain_plan(&nodes_route("strategy:ghost").unwrap().unwrap(), &read)
            .err()
            .unwrap();
        assert!(gone.contains("the strategy 'ghost' was removed"), "{gone}");
    }

    #[test]
    fn a_chats_own_node_set_is_named_so_and_routes_as_any_strategy() {
        let mut config = crate::nodes::empty_config();
        config.defs = vec![cloud_def("a"), cloud_def("b")];
        let nodes = vec!["a".to_string(), "b".to_string()];
        let set = crate::nodes::NodeStrategy {
            id: "chat-7".to_string(),
            name: "This chat's nodes (7)".to_string(),
            note: None,
            roles: crate::nodes::chat_set_roles(&nodes, false),
            chat: Some("7".to_string()),
        };
        let named = crate::nodes::NodeStrategy {
            id: "named".to_string(),
            name: "Named".to_string(),
            chat: None,
            ..set.clone()
        };
        config.strategies = vec![set, named];
        let read = read_of(config);
        for (role, suffix) in [("", "chat"), ("@build", "build")] {
            let own = chain_plan(
                &nodes_route(&format!("strategy:chat-7{role}"))
                    .unwrap()
                    .unwrap(),
                &read,
            )
            .unwrap();
            let same = chain_plan(
                &nodes_route(&format!("strategy:named{role}"))
                    .unwrap()
                    .unwrap(),
                &read,
            )
            .unwrap();
            assert_eq!(own.label, format!("this chat's nodes ({suffix})"));
            assert_eq!(same.label, format!("the strategy \"Named\" ({suffix})"));
            assert_eq!(own.entry, same.entry, "the label is the only difference");
            assert_eq!(own.role, same.role);
        }
    }

    fn pinned_def(id: &str, model: &str, placement: NodePlacement) -> ResolvedNodeDef {
        crate::nodes::resolve_def(
            &NodeDef {
                id: id.to_string(),
                name: format!("Node {id}"),
                kind: NodeDefKind::Mlx,
                model: Some(model.to_string()),
                placement: Some(placement),
                goal: None,
                provider: None,
                keep_loaded: false,
                pool_device: None,
                origin: NodeOrigin::User,
            },
            &Ok(None),
            false,
        )
    }

    fn serving(
        kind: NodesServingKind,
        macs: &[&str],
        model: &str,
        served: &str,
        load_phase: Option<&str>,
    ) -> ServingFacts {
        ServingFacts::Way(goose_sdk_types::custom_requests::NodesServingWayDto {
            kind,
            macs: macs.iter().map(|m| m.to_string()).collect(),
            link: None,
            model_id: model.to_string(),
            served_model_id: served.to_string(),
            mac_names: vec!["Mihai Macbook".to_string()],
            load_phase: load_phase.map(str::to_string),
        })
    }

    #[test]
    fn a_pinned_node_is_routable_only_while_its_own_way_serves() {
        let here = pinned_def(
            "flash",
            FLASH,
            NodePlacement::Single {
                macs: vec![crate::nodes::THIS_MAC.to_string()],
                link: None,
            },
        );
        let serves_it = serving(
            NodesServingKind::Single,
            &[crate::nodes::THIS_MAC],
            FLASH,
            FLASH_ALIAS,
            None,
        );
        let m = pinned_member(&here, &serves_it, None, false).unwrap();
        assert!(m.pinned);
        assert_eq!(m.node.kind, NodeKind::MlxSidecar);
        assert_eq!(m.node.model_id, FLASH);
        assert_eq!(m.way, Some(single_way(crate::nodes::THIS_MAC.to_string())));

        // Another way serves (a split of the 27B): not loaded, never "servable elsewhere".
        let split = serving(NodesServingKind::Split, &[], HF_ID, PINNED_27B, None);
        assert_eq!(
            pinned_member(&here, &split, None, false).err(),
            Some(EntryFact::NotLoaded)
        );
        assert_eq!(
            pinned_member(&here, &ServingFacts::Nothing, None, false).err(),
            Some(EntryFact::NotLoaded)
        );
        // Unknown which way serves: named, never guessed.
        assert_eq!(
            pinned_member(
                &here,
                &ServingFacts::Unknown("the route record is unreadable".to_string()),
                None,
                false
            )
            .err(),
            Some(cant_run("the route record is unreadable"))
        );
        // Its own way mid-load: the loader waits on it; with none, said with the phase.
        let loading = serving(
            NodesServingKind::Single,
            &[crate::nodes::THIS_MAC],
            FLASH,
            FLASH_ALIAS,
            Some("loading"),
        );
        assert_eq!(
            pinned_member(&here, &loading, None, true).err(),
            Some(EntryFact::NotLoaded)
        );
        assert_eq!(
            pinned_member(&here, &loading, None, false).err(),
            Some(cant_run(
                "Node flash is still loading on this Mac (loading); it serves once the load ends"
            ))
        );

        // A remote single is reached through the live route, named by its peer.
        let route = remote_route(6);
        let there = pinned_def(
            "studio-27b",
            HF_ID,
            NodePlacement::Single {
                macs: vec![crate::nodes::residency::peer_key(&route.peer)],
                link: None,
            },
        );
        let remote_serves = serving(
            NodesServingKind::RemoteSingle,
            &[&crate::nodes::residency::peer_key(&route.peer)],
            HF_ID,
            SERVED,
            None,
        );
        let m = pinned_member(&there, &remote_serves, Some(&route), false).unwrap();
        assert_eq!(m.node.capacity, 6);
        assert_eq!(m.node.model_id, SERVED);
        match &m.node.kind {
            NodeKind::MlxRemote(target) => assert_eq!(target.peer, route.peer),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            pinned_member(&there, &remote_serves, None, false).err(),
            Some(cant_run(
                "the remote-single route ended while this turn was routed"
            ))
        );
    }

    #[test]
    fn a_pool_node_routes_as_auto_routes_its_device_and_a_switched_off_one_is_said() {
        let cfg = PoolConfig {
            endpoint: "http://lm".to_string(),
            devices: vec![
                PoolDevice {
                    id: "mihai-mlx".to_string(),
                    model_id: PINNED_27B.to_string(),
                    weight: 2,
                    enabled: true,
                    engine: Some("mlx-sidecar".to_string()),
                    ..PoolDevice::default()
                },
                PoolDevice {
                    id: "off".to_string(),
                    model_id: "m".to_string(),
                    enabled: false,
                    provider: Some("bedrock".to_string()),
                    ..PoolDevice::default()
                },
            ],
        };
        let def = |id: &str, device: &str| {
            crate::nodes::resolve_def(
                &NodeDef {
                    pool_device: Some(device.to_string()),
                    kind: NodeDefKind::Mlx,
                    model: None,
                    provider: None,
                    placement: Some(NodePlacement::Follows),
                    origin: NodeOrigin::Pool,
                    ..cloud_def(id)
                },
                &Ok(None),
                false,
            )
        };
        let pool = Ok(Some(cfg));
        let m = pool_member(&def("mihai-mlx-pool", "mihai-mlx"), &pool, "mihai-mlx").unwrap();
        assert!(
            !m.pinned,
            "a pool MLX node follows this Mac's engine (Q-128)"
        );
        assert_eq!(m.node.kind, NodeKind::MlxSidecar);
        assert_eq!(m.node.id, "mihai-mlx-pool", "the node's own id");
        assert_eq!(m.node.model_id, PINNED_27B);
        assert_eq!(
            pool_member(&def("off", "off"), &pool, "off").err(),
            Some(cant_run("it is turned off in your swarm pool"))
        );
        assert_eq!(
            pool_member(&def("gone", "gone"), &pool, "gone").err(),
            Some(cant_run("it is no longer in your swarm pool"))
        );
        assert_eq!(
            pool_member(&def("x", "x"), &Err("unreadable".to_string()), "x").err(),
            Some(cant_run("unreadable"))
        );
        // A cloud node the user made: its family maps through the registry.
        let bedrock = crate::nodes::resolve_def(
            &NodeDef {
                provider: Some("bedrock".to_string()),
                ..cloud_def("claude")
            },
            &Ok(None),
            false,
        );
        let m = cloud_member(&bedrock).unwrap();
        assert_eq!(
            m.node.kind,
            NodeKind::Cloud {
                registry: "aws_bedrock".to_string()
            }
        );
        assert_eq!(m.node.capacity, PoolDevice::default().instances);
    }

    // -----------------------------------------------------------------------------------------
    // S3b (Q-196): every MLX lease asks the loader for a switch queued ahead of its reply; a
    // lease on the split names the way its owner published; a call with no session never demands.
    // -----------------------------------------------------------------------------------------

    fn mlx(id: &str, capacity: u32, weight: u32) -> Node {
        Node {
            kind: NodeKind::MlxSidecar,
            ..node(id, capacity, weight)
        }
    }

    fn with_loader() -> RecordingSeam {
        RecordingSeam {
            loader: true,
            ..RecordingSeam::default()
        }
    }

    async fn auto_turn(
        router: &Router,
        nodes: &[Node],
        seam: &dyn NodesSeam,
        first_message: &str,
    ) -> MessageStream {
        let messages = vec![Message::user().with_text(first_message)];
        let session = SessionTemplateKwargs::default();
        crate::session_context::with_session_id(
            Some(SESSION.to_string()),
            route_stream(
                router,
                nodes,
                &FakeProbe::all_idle(nodes),
                &AllAnswer,
                &NoKwargs,
                &NoRouteLoad,
                seam,
                Turn {
                    model_config: &ModelConfig::new("swarm"),
                    system: "sys",
                    messages: &messages,
                    tools: &[],
                    session: &session,
                },
            ),
        )
        .await
        .unwrap()
    }

    /// Gap 2: a reply on the RUNNING way never reached the loader, so steady replies starved a
    /// queued switch. Now its lease asks; with a switch queued ahead it gives its slot back, waits
    /// behind the switch and routes again on what serves after it.
    #[tokio::test]
    async fn a_lease_behind_a_queued_switch_gives_its_slot_back_waits_and_routes_again() {
        let router = Router::new();
        let nodes = vec![mlx("mlx", 1, 1)];
        let seam = with_loader();
        *seam.ahead.lock().unwrap() = VecDeque::from([Some("27B · both Macs".to_string())]);
        *seam.watch.lock().unwrap() = Some(router.semaphore(&nodes[0]));
        let stream = auto_turn(&router, &nodes, &seam, "hi").await;
        let asked = (SESSION.to_string(), "mlx".to_string());
        assert_eq!(
            *seam.asked.lock().unwrap(),
            vec![asked.clone(), asked.clone()],
            "asked before the first lease, and again before the lease taken after the wait"
        );
        assert_eq!(*seam.waited.lock().unwrap(), vec![asked]);
        assert_eq!(
            *seam.free_while_waiting.lock().unwrap(),
            vec![1],
            "the node's one slot was free while the turn waited"
        );
        assert_eq!(router.semaphore(&nodes[0]).available_permits(), 0);
        drop(stream);
        assert_eq!(router.semaphore(&nodes[0]).available_permits(), 1);
        assert_eq!(seam.served.lock().unwrap().len(), 1, "one served turn");

        // The same on a chain's pinned node: the round-robin commits only the lease that serves.
        let way = single_way(crate::nodes::THIS_MAC.to_string());
        let pinned = Member {
            node: mlx("flash", 1, 1),
            pinned: true,
            way: Some(way.clone()),
        };
        let plan = chain(role(
            &[("flash", 1)],
            NodeWhen::Share,
            NodeIfNotLoaded::Load,
        ));
        let seam = with_loader();
        *seam.ahead.lock().unwrap() = VecDeque::from([Some("27B · both Macs".to_string())]);
        let stream = chain_turn(
            &router,
            &plan,
            &FakeMembers(members(vec![("flash", Ok(pinned))])),
            &FakeProbe::all_idle(&[mlx("flash", 1, 1)]),
            &AllAnswer,
            &seam,
            "x",
        )
        .await
        .unwrap();
        drop(stream);
        assert_eq!(seam.asked.lock().unwrap().len(), 2);
        assert_eq!(seam.waited.lock().unwrap().len(), 1);
        assert_eq!(
            *seam.notes.lock().unwrap(),
            vec![(SESSION.to_string(), way)]
        );
    }

    /// With nothing queued, Auto routes exactly as it did before the ask existed: the same node
    /// for every turn, the same served records, no wait — and only MLX leases ask at all.
    #[tokio::test]
    async fn with_nothing_queued_auto_routes_exactly_as_before_and_only_mlx_leases_ask() {
        let nodes = vec![
            node("lm", 2, 1),
            mlx("mlx", 1, 3),
            Node {
                kind: NodeKind::Cloud {
                    registry: "openrouter".to_string(),
                },
                ..node("cloud", 1, 2)
            },
        ];
        let mut runs: Vec<Vec<String>> = Vec::new();
        for seam in [RecordingSeam::default(), with_loader()] {
            let router = Router::new();
            let mut held = Vec::new();
            for turn in 0..4 {
                held.push(auto_turn(&router, &nodes, &seam, &format!("turn {turn}")).await);
            }
            let served: Vec<String> = seam
                .served
                .lock()
                .unwrap()
                .iter()
                .map(|(_, r)| r.node.clone())
                .collect();
            let mlx_leases = served.iter().filter(|n| *n == "mlx").count();
            let asked = seam.asked.lock().unwrap().len();
            if seam.loader {
                assert_eq!(
                    asked, mlx_leases,
                    "one ask per MLX lease, none for LM Studio or cloud"
                );
            } else {
                assert_eq!(asked, 0, "no loader installed: nothing is asked");
            }
            assert!(seam.waited.lock().unwrap().is_empty());
            runs.push(served);
        }
        assert_eq!(runs[0], runs[1], "the ask changes no pick");
        assert_eq!(runs[0].len(), 4);
        assert!(runs[0].contains(&"mlx".to_string()), "{:?}", runs[0]);
    }

    /// Gap 3: a lease on the split names the way its owner published, so the reply holds it.
    #[test]
    fn a_lease_on_the_split_names_the_way_its_owner_published() {
        let way = MlxPlacementKeyDto {
            kind: MlxPlacementKindDto::Pipeline,
            nodes: vec!["local".to_string(), "link:wh".to_string()],
            link: Some("jaccl".to_string()),
        };
        let sidecar = mlx("mlx", 1, 1);
        assert_eq!(
            lease_way(&sidecar, SplitNow::Serves(Ok(Some(way.clone())))),
            Some(way.clone())
        );
        assert_eq!(
            lease_way(&sidecar, SplitNow::None),
            Some(single_way(crate::nodes::THIS_MAC.to_string()))
        );
        assert_eq!(
            lease_way(&sidecar, SplitNow::Serves(Ok(None))),
            None,
            "a record from a goose before the way names none: not guessed"
        );
        assert_eq!(
            lease_way(&sidecar, SplitNow::Serves(Err("EOF".to_string()))),
            None
        );
        assert_eq!(
            lease_way(&node("lm", 1, 1), SplitNow::Serves(Ok(Some(way)))),
            None
        );
    }

    /// The S5 open risk: a demand with no session is a card's Start, answered `Wait` at once. A
    /// model call outside any session never builds one; the entry is refused by name instead.
    #[tokio::test]
    async fn a_call_with_no_session_never_demands_a_load_and_is_refused_by_name() {
        let plan = chain(role(&[("a", 1)], NodeWhen::Failover, NodeIfNotLoaded::Load));
        let seam = RecordingSeam {
            loader: true,
            ..RecordingSeam::answering(vec![NodeEnsureServing::Wait {
                reason: "a card's wait".to_string(),
            }])
        };
        let messages = vec![Message::user().with_text("x")];
        let session = SessionTemplateKwargs::default();
        let err = route_chain(
            &Router::new(),
            &plan,
            &FakeMembers(members(vec![("a", Err(EntryFact::NotLoaded))])),
            &FakeProbe(HashMap::new()),
            &AllAnswer,
            &NoKwargs,
            &NoRouteLoad,
            &seam,
            Turn {
                model_config: &ModelConfig::new("strategy:test"),
                system: "sys",
                messages: &messages,
                tools: &[],
                session: &session,
            },
        )
        .await
        .err()
        .unwrap()
        .to_string();
        assert!(
            seam.demands.lock().unwrap().is_empty(),
            "no demand is made without a session"
        );
        assert!(
            err.contains(
                "a: this model call belongs to no session, so it cannot wait for a load; start Node a in Run it"
            ),
            "{err}"
        );
    }
}
