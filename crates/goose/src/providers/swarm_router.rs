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
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::base::{MessageStream, Provider};
use crate::config::{Config, ConfigError};
use crate::conversation::message::Message;
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;

const SWARM_CONFIG_KEY: &str = "swarm";
const LMSTUDIO_HOST_ENV: &str = "LMSTUDIO_HOST";
const LMSTUDIO_TOKEN_KEY: &str = "LMSTUDIO_API_KEY";
const OMLX_HOST_ENV: &str = "OMLX_HOST";

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
    LmStudio { endpoint: String },
    MlxSidecar,
    Cloud { registry: String },
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
            NodeKind::MlxSidecar => "omlx",
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
        let name = node.provider_name();
        let p = crate::providers::create(name, vec![])
            .await
            .map_err(|e| format!("creating the '{name}' provider: {e}"))?;
        cache.insert(key, p.clone());
        Ok(p)
    }
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

    async fn get_json(&self, url: &str) -> Result<serde_json::Value, String> {
        let mut req = self.http.get(url);
        if let Some(token) = Self::lm_api_token() {
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
        let body = self.get_json(&url).await?;
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
            match self.get_json(&native).await {
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

    async fn probe_mlx(&self, model_id: &str) -> Result<Servable, String> {
        let status = goose_sidecar::engine::global_manager().status().await;
        if status.state != "running" {
            let detail = match (&status.model_id, &status.last_error) {
                (_, Some(err)) => format!(" ({err})"),
                (Some(m), None) => format!(" (model {m})"),
                (None, None) => String::new(),
            };
            return Err(format!(
                "MLX engine is {}{detail} — mount it in the MLX window",
                status.state
            ));
        }
        match status.served_model_id.as_deref() {
            Some(served) if served == model_id => {}
            Some(served) => {
                return Err(format!(
                    "MLX engine serves '{served}', the device wants '{model_id}'"
                ))
            }
            None => {
                return Err(format!(
                    "MLX engine is running but its served id is unknown ({})",
                    status
                        .probe_error
                        .as_deref()
                        .unwrap_or("no /v1/models answer")
                ))
            }
        }
        if let Some(base_url) = status.base_url.as_deref() {
            align_host_env(OMLX_HOST_ENV, &OMLX_HOST_USER_OWNED, base_url);
        }
        if let Some(err) = &status.active_requests_error {
            tracing::warn!(
                target: "swarm_router",
                error = %err,
                "MLX engine reported no in-flight count; routing on in-process leases alone"
            );
        }
        Ok(Servable {
            live_in_flight: status.active_requests,
            context_window: status.context_window,
        })
    }
}

#[async_trait]
impl NodeProbe for LiveProbe {
    async fn probe(&self, node: &Node) -> Result<Servable, String> {
        match &node.kind {
            NodeKind::LmStudio { endpoint } => self.probe_lmstudio(endpoint, &node.model_id).await,
            NodeKind::MlxSidecar => self.probe_mlx(&node.model_id).await,
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
        Lease {
            node: node.clone(),
            _permit: permit,
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
}

pub(crate) async fn route_stream(
    router: &Router,
    nodes: &[Node],
    probe: &dyn NodeProbe,
    providers: &dyn ProviderSource,
    turn: Turn<'_>,
) -> Result<MessageStream, ProviderError> {
    let Turn {
        model_config,
        system,
        messages,
        tools,
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
        match provider.stream(&node_cfg, system, messages, tools).await {
            Ok(inner) => return Ok(leased_stream(inner, lease)),
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
) -> Result<MessageStream, ProviderError> {
    let cfg = load_pool()?;
    let nodes = nodes_from_config(&cfg);
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
        Turn {
            model_config,
            system,
            messages,
            tools,
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
            Turn {
                model_config: &ModelConfig::new("swarm"),
                system: "sys",
                messages: &messages,
                tools: &[],
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
            Turn {
                model_config: &ModelConfig::new("swarm"),
                system: "sys",
                messages: &[],
                tools: &[],
            },
        )
        .await
        .err()
        .unwrap();
        assert!(matches!(err, ProviderError::ServerError(_)));
        assert!(err.to_string().contains("max concurrent"));
        assert_eq!(router.semaphore(&nodes[0]).available_permits(), 1);
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
