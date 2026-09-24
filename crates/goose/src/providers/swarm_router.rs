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
use crate::config::{Config, ConfigError};
use crate::conversation::message::Message;
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
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
        }
    }
}

/// Swarm cloud family → goose provider-registry key. Copied from `CLOUD_DEFS` in
/// crates/goose-cli/src/commands/swarm.rs (the `name` → `registry` pairs); the two differ for
/// bedrock and deepseek, which is why the mapping exists at all.
const CLOUD_REGISTRY: &[(&str, &str)] = &[
    ("bedrock", "aws_bedrock"),
    ("zai", "zai"),
    ("google", "google"),
    ("deepseek", "custom_deepseek"),
];

fn cloud_registry_name(family: &str) -> &str {
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
    pub peer_hostname: String,
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
    pub capacity: u32,
    pub kind: NodeKind,
}

impl Node {
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

/// Turn the config into nodes. Only enabled devices are nodes; a device's kind is decided the way
/// the engine decides it (a cloud `provider` wins, then `engine`, then LM Studio).
pub(crate) fn nodes_from_config(cfg: &PoolConfig) -> Vec<Node> {
    cfg.devices
        .iter()
        .filter(|d| d.enabled)
        .map(|d| {
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
                    endpoint: cfg.endpoint.clone(),
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
                capacity,
                kind,
            }
        })
        .collect()
}

/// The pool plus the live remote route's node. The route's node takes the heaviest configured
/// weight, so a tie on free slots goes to the placement the user chose; its capacity is the
/// peer's own admission cap.
pub(crate) fn with_remote_route(mut nodes: Vec<Node>, route: Option<&PublishedRoute>) -> Vec<Node> {
    if let Some(route) = route {
        let weight = nodes.iter().map(|n| n.weight).max().unwrap_or(1);
        nodes.push(Node {
            id: route.node_id(),
            model_id: route.served_model_id.clone(),
            weight,
            capacity: route.capacity,
            kind: NodeKind::MlxRemote(RemoteTarget {
                peer_hostname: route.peer_hostname.clone(),
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
            route.peer_hostname,
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Servable {
    pub live_in_flight: Option<u32>,
    pub context_window: Option<u64>,
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
            target.peer_hostname
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
            live_in_flight: None,
            context_window,
        })
    }

    /// THE NODE IS THE TRUTH, NOT THIS PROCESS'S MANAGER. The engine is mounted by the desktop's
    /// goosed; a second goosed (each window holds its own goose-serve) or the CLI has a manager that
    /// knows nothing — measured 2026-09-05 13:20: with the engine idle on :8090 the CLI was told
    /// "MLX engine is stopped". So the decision comes from probing the engine's own HTTP surface at
    /// the base URL the local manager reports when it is the one running it, else the configured
    /// port; the local manager only enriches the reason when nothing listens.
    async fn probe_mlx(&self, model_id: &str) -> Result<Servable, String> {
        if let Some(reason) = sidecar_routed_away(&mlx_remote::read()) {
            return Err(reason);
        }
        let stale = match distributed_target(
            mlx_distributed_owner::own_active_base_url(),
            mlx_distributed_owner::read(),
        )? {
            DistributedTarget::At { base, diagnostic } => {
                return self.probe_mlx_at(&base, model_id, &diagnostic).await;
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
        self.probe_mlx_at(&base, model_id, &diagnostic).await
    }

    async fn probe_mlx_at(
        &self,
        base: &str,
        model_id: &str,
        local_diagnostic: &str,
    ) -> Result<Servable, String> {
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
        match served.as_deref() {
            Some(served) if served == model_id => {}
            Some(served) => {
                return Err(format!(
                    "MLX engine serves '{served}', the device wants '{model_id}'"
                ))
            }
            None => {
                return Err(format!(
                    "MLX engine on {base} lists a model without an id in {models_url}"
                ))
            }
        }
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
        let peer = &target.peer_hostname;
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
        })
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
            NodeKind::MlxSidecar => self.probe_mlx(&node.model_id).await,
            NodeKind::MlxRemote(target) => self.probe_remote(target, &node.model_id).await,
            NodeKind::Cloud { .. } => self
                .providers
                .provider_for(node)
                .await
                .map(|_| Servable::default()),
        }
    }
}

/// A slot on a node, held for the life of the stream it serves.
pub(crate) struct Lease {
    pub node: Node,
    _permit: OwnedSemaphorePermit,
    /// The node is the local MLX engine: this turn is listed as in flight on it for exactly the
    /// lease's life (see `mlx_serving`).
    _serving: Option<super::mlx_serving::ServingGuard>,
}

pub(crate) struct Router {
    /// Node → its slots. Keyed by id AND capacity so a capacity edit mints a fresh semaphore and
    /// the old one drains on its own.
    slots: StdMutex<HashMap<String, Arc<Semaphore>>>,
    /// Conversation key → the node that last served it.
    sticky: StdMutex<HashMap<u64, String>>,
    queued: AtomicUsize,
    /// The smallest context window among the servable nodes at the last pick; 0 = no node said.
    /// What `get_context_limit` hands goose so its own compaction fires before the node's wall.
    last_pool_context_limit: AtomicU32,
}

impl Router {
    pub(crate) fn new() -> Self {
        Self {
            slots: StdMutex::new(HashMap::new()),
            sticky: StdMutex::new(HashMap::new()),
            queued: AtomicUsize::new(0),
            last_pool_context_limit: AtomicU32::new(0),
        }
    }

    /// The pool's context limit as of the last pick, `None` until a servable node has reported one.
    pub(crate) fn pool_context_limit(&self) -> Option<usize> {
        match self.last_pool_context_limit.load(Ordering::SeqCst) {
            0 => None,
            n => Some(n as usize),
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
    /// free slots (ties → higher weight); else queue on every servable node until a permit frees.
    /// `saturated` names nodes this turn already saw refuse admission.
    pub(crate) async fn pick(
        &self,
        nodes: &[Node],
        probe: &dyn NodeProbe,
        key: u64,
        saturated: &HashSet<String>,
    ) -> Result<Lease, ProviderError> {
        let probes = futures::future::join_all(nodes.iter().map(|n| probe.probe(n))).await;
        let mut servable: Vec<(&Node, Arc<Semaphore>, u32)> = Vec::new();
        let mut reasons: Vec<String> = Vec::new();
        let mut smallest_window: Option<u64> = None;
        for (node, outcome) in nodes.iter().zip(probes) {
            match outcome {
                Ok(_) if saturated.contains(&node.id) => {
                    reasons.push(format!("{}: refused admission this turn", node.id));
                }
                Ok(facts) => {
                    let sem = self.semaphore(node);
                    let leased = node.capacity.saturating_sub(sem.available_permits() as u32);
                    let used = facts.live_in_flight.map_or(leased, |l| l.max(leased));
                    let free = node.capacity.saturating_sub(used);
                    servable.push((node, sem, free));
                    if let Some(window) = facts.context_window {
                        smallest_window = Some(smallest_window.map_or(window, |w| w.min(window)));
                    }
                }
                Err(reason) => reasons.push(format!("{}: {reason}", node.id)),
            }
        }
        if !servable.is_empty() {
            let limit = smallest_window.map_or(0, |w| u32::try_from(w).unwrap_or(u32::MAX));
            self.last_pool_context_limit.store(limit, Ordering::SeqCst);
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
            .and_then(|id| servable.iter().find(|(n, _, free)| n.id == id && *free > 0))
            .or_else(|| {
                servable
                    .iter()
                    .filter(|(_, _, free)| *free > 0)
                    .max_by_key(|(n, _, free)| (*free, n.weight))
            });
        if let Some((node, sem, free)) = preferred {
            if let Ok(permit) = sem.clone().try_acquire_owned() {
                return Ok(self.leased(node, permit, *free, 0, key));
            }
        }

        let start = Instant::now();
        let depth = self.queued.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::info!(
            target: "swarm_router",
            nodes = %servable.iter().map(|(n, _, _)| n.id.as_str()).collect::<Vec<_>>().join(","),
            queue_depth = depth,
            "queued"
        );
        let waits = servable
            .iter()
            .map(|(_, sem, _)| Box::pin(sem.clone().acquire_owned()))
            .collect::<Vec<_>>();
        let (first, index, _rest) = futures::future::select_all(waits).await;
        self.queued.fetch_sub(1, Ordering::SeqCst);
        let permit = first.map_err(|e| {
            ProviderError::ExecutionError(format!("swarm chat: a node's slot pool closed ({e})"))
        })?;
        let node = servable[index].0;
        Ok(self.leased(node, permit, 0, start.elapsed().as_millis(), key))
    }

    fn leased(
        &self,
        node: &Node,
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
        let serving = matches!(node.kind, NodeKind::MlxSidecar).then(|| {
            super::mlx_serving::register(
                super::mlx_serving::ServingVia::SwarmRouter,
                crate::session_context::current_session_id(),
                node.provider_name(),
                &node.model_id,
                Some(&node.id),
            )
        });
        Lease {
            node: node.clone(),
            _permit: permit,
            _serving: serving,
        }
    }
}

static ROUTER: LazyLock<Router> = LazyLock::new(Router::new);

/// The shared router's pool context limit — what the provider reports to goose for `swarm` chat.
pub(crate) fn pool_context_limit() -> Option<usize> {
    ROUTER.pool_context_limit()
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

/// The served id names its HF directory through the settings: the configured model when its
/// served id matches (an alias via `served_model_name` included), else the served id itself when
/// no alias is configured. A served id no profile can be tied to sends nothing — unless some
/// profile DOES carry thinking choices, which would then be silently dropped: that is an error.
fn profile_template_kwargs(
    settings: &EngineSettings,
    served: &str,
) -> Result<Option<Map<String, Value>>, String> {
    let hf_id = settings
        .model_id
        .as_deref()
        .filter(|id| served_model_id(settings, id) == served)
        .or_else(|| settings.served_model_name.is_none().then_some(served));
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

pub(crate) async fn route_stream(
    router: &Router,
    nodes: &[Node],
    probe: &dyn NodeProbe,
    providers: &dyn ProviderSource,
    kwargs_source: &dyn TemplateKwargsSource,
    turn: Turn<'_>,
) -> Result<MessageStream, ProviderError> {
    let Turn {
        model_config,
        system,
        messages,
        tools,
        session,
    } = turn;
    let key = Router::conversation_key(system, messages);
    let mut saturated = HashSet::new();
    let mut last_refusal: Option<ProviderError> = None;
    loop {
        let lease = match router.pick(nodes, probe, key, &saturated).await {
            Ok(lease) => lease,
            Err(no_node) => return Err(last_refusal.unwrap_or(no_node)),
        };
        let provider = providers
            .provider_for(&lease.node)
            .await
            .map_err(ProviderError::ExecutionError)?;
        let mut node_cfg = model_config.clone();
        node_cfg.model_name = lease.node.model_id.clone();
        let kwargs = match &lease.node.kind {
            NodeKind::MlxSidecar => session.for_model(&lease.node.model_id, kwargs_source)?,
            NodeKind::MlxRemote(target) => session.for_model(
                &lease.node.model_id,
                &PeerProfileKwargs(target.template_kwargs.clone()),
            )?,
            _ => None,
        };
        if let Some(kwargs) = kwargs {
            add_template_kwargs(&mut node_cfg, kwargs)?;
        }
        match provider.stream(&node_cfg, system, messages, tools).await {
            Ok(inner) => {
                let inner = if matches!(
                    lease.node.kind,
                    NodeKind::MlxSidecar | NodeKind::MlxRemote(_)
                ) {
                    super::mlx_speed::observe(inner)
                } else {
                    inner
                };
                return Ok(leased_stream(inner, lease));
            }
            Err(e) if is_admission_refusal(&e) => {
                tracing::warn!(
                    target: "swarm_router",
                    node = %lease.node.id,
                    error = %e,
                    "node refused admission; trying the next free node"
                );
                saturated.insert(lease.node.id.clone());
                last_refusal = Some(e);
            }
            Err(e) => return Err(e),
        }
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

/// The provider's entry point: the live pool, the live probe, the shared router.
pub(crate) async fn route_chat(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    session: &SessionTemplateKwargs,
) -> Result<MessageStream, ProviderError> {
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
    let providers: &LiveProviders = &PROVIDERS;
    route_stream(
        &ROUTER,
        &nodes,
        &*PROBE,
        providers,
        &ConfiguredTemplateKwargs,
        Turn {
            model_config,
            system,
            messages,
            tools,
            session,
        },
    )
    .await
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
            context_window: None,
        })
    }

    fn window(context_window: u64) -> Result<Servable, String> {
        Ok(Servable {
            live_in_flight: None,
            context_window: Some(context_window),
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
    /// (`model_config.clone()` with the node's model name, nothing else).
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
        let err = profile_template_kwargs(&aliased, "something-else").unwrap_err();
        assert!(
            err.contains(HF_ID) && err.contains("something-else"),
            "{err}"
        );

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
        let facts = probe
            .probe_mlx_at(&engine.uri(), "workhorse-qwen3.5-9b-4bit-mlx", "stopped")
            .await
            .unwrap();
        assert_eq!(facts.live_in_flight, Some(3));
        assert_eq!(facts.context_window, Some(262_144));

        let mismatch = probe
            .probe_mlx_at(&engine.uri(), "some-other-model", "stopped")
            .await
            .unwrap_err();
        assert!(
            mismatch.contains(
                "serves 'workhorse-qwen3.5-9b-4bit-mlx', the device wants 'some-other-model'"
            ),
            "{mismatch}"
        );

        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://127.0.0.1:{}", l.local_addr().unwrap().port())
        };
        let down = probe
            .probe_mlx_at(
                &dead,
                "workhorse-qwen3.5-9b-4bit-mlx",
                "this process's manager: stopped",
            )
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
            peer_hostname: "WorksMacStudio.lan".to_string(),
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
                "chatServingDisabled: \"Allow this Mac to serve chat to linked devices\" is off on WorksMacStudio.lan",
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
            peer_hostname: "WorksMacStudio.lan".to_string(),
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

    /// Live 2026-09-24 (3.0.19): the distributed engine served the HF id while the `mihai-mlx` node
    /// names the single engine's alias, so the router refused the node. The rank specs now carry
    /// the id `engine::served_model_id` derives — the single engine's `--served-model-name` — and
    /// the wrapper's /v1/models answer (its exact shape, `owned_by: goose-distributed`) passes the
    /// same probe the single engine passes.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_distributed_engine_serves_the_nodes_id_and_the_router_accepts_it() {
        use goose_sidecar::distributed::{launch::rank_specs, DistributedConfig};
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
        let served = served_model_id(&settings, &config.model_id);
        let specs = rank_specs(&config, &served, &[(1, 1), (1, 1)], 65_536, 2.0);
        assert!(specs.iter().all(|s| s.served_id == NODE_MODEL), "{specs:?}");

        let wrapper = |id: &str| {
            format!(
                r#"{{"object":"list","data":[{{"id":"{id}","object":"model","owned_by":"goose-distributed","context_window":65536}}]}}"#
            )
        };
        let probe = LiveProbe {
            http: reqwest::Client::new(),
            providers: Arc::new(LiveProviders::new()),
        };
        let engine = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(wrapper(&specs[0].served_id)))
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
        let facts = probe
            .probe_mlx_at(
                &engine.uri(),
                NODE_MODEL,
                "the distributed MLX engine owns this Mac",
            )
            .await
            .unwrap();
        assert_eq!(facts.context_window, Some(65_536));
        assert_eq!(facts.live_in_flight, Some(0));

        let before = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(wrapper(HF)))
            .mount(&before)
            .await;
        let refused = probe
            .probe_mlx_at(
                &before.uri(),
                NODE_MODEL,
                "the distributed MLX engine owns this Mac",
            )
            .await
            .unwrap_err();
        assert!(
            refused.contains(&format!("serves '{HF}', the device wants '{NODE_MODEL}'")),
            "{refused}"
        );
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
}
