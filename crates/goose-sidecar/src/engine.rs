//! The in-house MLX engine manager: one supervised Rapid-MLX process, one mounted model.
//!
//! Mounting is asynchronous by design — `mount` validates the model and the memory gate,
//! flips to `Mounting`, and returns; a spawned task drives `Sidecar::start` to `Running`
//! or `Failed`. Callers poll `status()`, which also probes the live engine's `/v1/models`
//! for its context window and tool-call parser and never fabricates either.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::hf::{self, LocalModel};
use crate::{
    listening_pids, measure, port_has_listener, wait_port_clear, MemoryGate, Sidecar,
    SidecarConfig, Verdict, GIB,
};

pub const ENGINE_LAUNCHER: [&str; 4] = [
    "uvx",
    "--from",
    "git+https://github.com/leanzero-srl/Rapid-MLX@v0.13.1",
    "rapid-mlx",
];

/// Per-model sampling and context settings. Sampling is per MODEL, not per engine:
/// each mounted model pulls its own profile from `EngineSettings::model_profiles`.
/// `context_limit` is profile state for goose's own context bookkeeping — Rapid-MLX
/// 0.13.1 has no context-length serve flag (`--max-tokens` caps generation, a
/// different knob), so it emits no argv.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelProfile {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub min_p: Option<f64>,
    pub repetition_penalty: Option<f64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub context_limit: Option<u32>,
}

impl ModelProfile {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineSettings {
    pub model_id: Option<String>,
    pub models_dir: String,
    pub port: u16,
    /// LEGACY flat sampling/context fields. Older persisted configs and UI states still
    /// carry them; `migrate_legacy` folds them into `model_profiles` and clears them.
    /// They are NOT read by `build_serve_command` — profiles are the source of truth.
    pub context_limit: Option<u32>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub min_p: Option<f64>,
    pub repetition_penalty: Option<f64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    /// Swarm-facing model id advertised by the server (`--served-model-name`). The fleet's
    /// node-identity convention lives in this name (`workhorse-…`); when unset the HF
    /// directory id is served as-is.
    pub served_model_name: Option<String>,
    pub spawn_command: Vec<String>,
    /// Per-model sampling/context profiles, keyed by the HF model id.
    pub model_profiles: BTreeMap<String, ModelProfile>,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            model_id: None,
            models_dir: "~/.goose/models".to_string(),
            port: 8090,
            context_limit: None,
            temperature: None,
            top_p: None,
            top_k: None,
            min_p: None,
            repetition_penalty: None,
            presence_penalty: None,
            frequency_penalty: None,
            served_model_name: None,
            spawn_command: ENGINE_LAUNCHER.iter().map(|s| s.to_string()).collect(),
            model_profiles: BTreeMap::new(),
        }
    }
}

impl EngineSettings {
    /// One-time migration of the legacy flat sampling/context fields into
    /// `model_profiles[model_id]`. The flats predate profiles, so they only fill
    /// profile fields still unset, and are cleared either way. Without a `model_id`
    /// there is no honest profile key — the flats stay put and this returns `false`
    /// (migration needs a model).
    pub fn migrate_legacy(&mut self) -> bool {
        let flats = ModelProfile {
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            min_p: self.min_p,
            repetition_penalty: self.repetition_penalty,
            presence_penalty: self.presence_penalty,
            frequency_penalty: self.frequency_penalty,
            context_limit: self.context_limit,
        };
        if flats.is_empty() {
            return false;
        }
        let Some(model_id) = self.model_id.clone() else {
            return false;
        };
        self.temperature = None;
        self.top_p = None;
        self.top_k = None;
        self.min_p = None;
        self.repetition_penalty = None;
        self.presence_penalty = None;
        self.frequency_penalty = None;
        self.context_limit = None;
        let profile = self.model_profiles.entry(model_id).or_default();
        profile.temperature = profile.temperature.or(flats.temperature);
        profile.top_p = profile.top_p.or(flats.top_p);
        profile.top_k = profile.top_k.or(flats.top_k);
        profile.min_p = profile.min_p.or(flats.min_p);
        profile.repetition_penalty = profile.repetition_penalty.or(flats.repetition_penalty);
        profile.presence_penalty = profile.presence_penalty.or(flats.presence_penalty);
        profile.frequency_penalty = profile.frequency_penalty.or(flats.frequency_penalty);
        profile.context_limit = profile.context_limit.or(flats.context_limit);
        true
    }
}

/// `~`-prefixed paths expand against the home dir; without one the literal path is kept,
/// which then surfaces loudly downstream as model-not-found with the literal visible.
pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// The id the engine will advertise in `/v1/models` — `served_model_name` when set, else the
/// HF directory id. Both the serve argv and the readiness check derive from this one place.
pub fn served_model_id(settings: &EngineSettings, model_id: &str) -> String {
    settings
        .served_model_name
        .clone()
        .unwrap_or_else(|| model_id.to_string())
}

pub fn build_serve_command(settings: &EngineSettings, model_id: &str) -> Vec<String> {
    let model_path = expand_tilde(&settings.models_dir).join(model_id);
    let mut argv = settings.spawn_command.clone();
    argv.extend([
        "serve".to_string(),
        model_path.to_string_lossy().into_owned(),
        "--port".to_string(),
        settings.port.to_string(),
        "--served-model-name".to_string(),
        served_model_id(settings, model_id),
        "--enable-prefix-cache".to_string(),
        "--max-concurrent-requests".to_string(),
        "8".to_string(),
    ]);
    let default_profile = ModelProfile::default();
    let profile = settings
        .model_profiles
        .get(model_id)
        .unwrap_or(&default_profile);
    let float_flags = [
        ("--default-temperature", profile.temperature),
        ("--default-top-p", profile.top_p),
        ("--default-min-p", profile.min_p),
        ("--default-repetition-penalty", profile.repetition_penalty),
        ("--default-presence-penalty", profile.presence_penalty),
        ("--default-frequency-penalty", profile.frequency_penalty),
    ];
    for (flag, value) in float_flags {
        if let Some(value) = value {
            argv.push(flag.to_string());
            argv.push(value.to_string());
        }
    }
    if let Some(top_k) = profile.top_k {
        argv.push("--default-top-k".to_string());
        argv.push(top_k.to_string());
    }
    argv
}

/// `mount` refuses to start an engine on a port that something this manager does not
/// supervise already listens on — the mirror of `status().stray_listener_port`. Starting
/// anyway would probe THAT listener as our readiness and report `Running` for a child that
/// then dies on the bind. `unmount` reclaims the port; a mount after that proceeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupervisedListenerError {
    pub port: u16,
}

impl std::fmt::Display for UnsupervisedListenerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "port {} has an unsupervised listener — unmount/reclaim it first",
            self.port
        )
    }
}

impl std::error::Error for UnsupervisedListenerError {}

enum ManagerState {
    Stopped,
    Mounting {
        model_id: String,
    },
    Running {
        model_id: String,
        sidecar: Box<Sidecar>,
        argv: Vec<String>,
    },
    Failed {
        model_id: String,
        error: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStatus {
    pub state: String,
    pub model_id: Option<String>,
    pub base_url: Option<String>,
    pub pid: Option<u32>,
    pub context_window: Option<u64>,
    pub tool_call_parser: Option<String>,
    /// The id the live engine actually serves (from /v1/models) — differs from `model_id`
    /// (the HF directory) whenever `served_model_name` aliases it. Chat must use THIS id.
    pub served_model_id: Option<String>,
    pub probe_error: Option<String>,
    pub gate_message: Option<String>,
    pub gate_verdict: Option<String>,
    /// Set when the manager supervises nothing but SOMETHING already listens on the
    /// configured port — an engine orphaned by a previous goosed. `unmount` reclaims it.
    pub stray_listener_port: Option<u16>,
    pub available_memory_gb: f64,
    pub total_memory_gb: f64,
    pub restart_required: bool,
    pub last_error: Option<String>,
}

/// A fixed, standard spawn PATH for the engine process. goosed's own PATH is a grab-bag of
/// goose-internal tool shims — the MCP `mcp-hermit` bootstrap AND the desktop-bundled
/// `ui/desktop/src/bin/uvx` wrapper — and inheriting it resolved `uvx` to those shims twice
/// on 2026-08-31 (a cold hermit python install, then a nested bash chain that never served).
/// A controlled environment makes resolution deterministic; if `uvx` is absent from these
/// standard locations the spawn fails loudly with exactly that message.
fn sidecar_spawn_path() -> String {
    "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string()
}

/// Terminate whatever LISTENS on `port` — per-pid (never a group: the orphan's group is not
/// provably ours), SIGTERM then SIGKILL. Reaching for `lsof` is deliberate: the orphan is not
/// our child, so there is no handle; the port is OUR configured port, which is the authority
/// to reclaim it. Only LISTEN sockets are targeted — a client connection to the port (goosed's
/// own probe pool) is not an engine.
async fn reclaim_port(port: u16) {
    let pids = match listening_pids(port).await {
        Ok(pids) => pids,
        Err(e) => {
            tracing::warn!(port, error = %e, "reclaim: lsof unavailable; port left occupied");
            return;
        }
    };
    if pids.is_empty() {
        return;
    }
    tracing::warn!(
        port,
        ?pids,
        "reclaiming port from unsupervised engine (SIGTERM)"
    );
    #[cfg(unix)]
    {
        for pid in &pids {
            unsafe { libc::kill(*pid as libc::pid_t, libc::SIGTERM) };
        }
        if wait_port_clear(port).await {
            return;
        }
        tracing::warn!(port, ?pids, "reclaim: grace window expired (SIGKILL)");
        for pid in &pids {
            unsafe { libc::kill(*pid as libc::pid_t, libc::SIGKILL) };
        }
    }
}

pub struct MlxEngineManager {
    state: Arc<Mutex<ManagerState>>,
    settings: StdMutex<EngineSettings>,
    last_gate: StdMutex<Option<crate::GateResult>>,
    gate: MemoryGate,
    probe_client: reqwest::Client,
}

impl MlxEngineManager {
    pub fn new() -> Self {
        Self::with_gate(MemoryGate::default())
    }

    pub fn with_gate(gate: MemoryGate) -> Self {
        Self {
            state: Arc::new(Mutex::new(ManagerState::Stopped)),
            settings: StdMutex::new(EngineSettings::default()),
            last_gate: StdMutex::new(None),
            gate,
            probe_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client with static configuration"),
        }
    }

    /// Legacy flat sampling fields migrate into profiles here, in memory, so EVERY
    /// consumer (the ACP layer persists the migration; the swarm engine registry does
    /// not) spawns from profile truth even when handed an un-migrated config.
    pub fn set_settings(&self, mut settings: EngineSettings) {
        settings.migrate_legacy();
        *self.settings.lock().unwrap() = settings;
    }

    pub fn settings(&self) -> EngineSettings {
        self.settings.lock().unwrap().clone()
    }

    /// Validate the model and the memory gate, flip to `Mounting`, and return; the engine
    /// start continues in a spawned task. A gate `Block` refuses the mount outright.
    /// Any already-running engine is shut down first — one model per engine — and once
    /// this manager supervises nothing, a listener still on the port is somebody else's:
    /// the mount is refused with [`UnsupervisedListenerError`] rather than started over it.
    pub async fn mount(&self, model_id: &str) -> Result<()> {
        hf::validate_model_id(model_id)?;
        let settings = self.settings();
        let models_dir = expand_tilde(&settings.models_dir);
        let models = hf::list_local_models(&models_dir)?;
        let model: &LocalModel = models.iter().find(|m| m.id == model_id).with_context(|| {
            format!(
                "model '{model_id}' not found in {} (download it first)",
                models_dir.display()
            )
        })?;
        ensure!(
            model.complete,
            "model '{model_id}' is incomplete: a .part file remains or no .safetensors is present"
        );

        let (available, total) = measure();
        let gate = self.gate.evaluate(model.size_bytes, available, total);
        let blocked = gate.verdict == Verdict::Block;
        let block_message = gate.message.clone();
        *self.last_gate.lock().unwrap() = Some(gate);
        if blocked {
            bail!("memory gate BLOCK for '{model_id}': {block_message}");
        }

        let argv = build_serve_command(&settings, model_id);
        let mut state = self.state.lock().await;
        if let ManagerState::Mounting { model_id: current } = &*state {
            bail!("mount already in progress for '{current}'");
        }
        let previous = std::mem::replace(
            &mut *state,
            ManagerState::Mounting {
                model_id: model_id.to_string(),
            },
        );
        // The IDENTICAL configuration already has a supervisor: keep it and let its circuit
        // breaker judge — a crashed engine restarts with backoff, a crash loop trips the
        // breaker into Failed, a healthy engine is verified and kept. This is the crash
        // re-mount path (the swarm's ensure_loaded → mount); a different model or argv is a
        // deliberate change and gets a fresh supervisor.
        let supervised = match previous {
            ManagerState::Running {
                model_id: previous_model,
                sidecar,
                argv: previous_argv,
            } => {
                if previous_model == model_id && previous_argv == argv {
                    Some(sidecar)
                } else {
                    sidecar.shutdown().await;
                    None
                }
            }
            _ => None,
        };
        if supervised.is_none() && port_has_listener(settings.port) {
            *state = ManagerState::Stopped;
            return Err(UnsupervisedListenerError {
                port: settings.port,
            }
            .into());
        }
        drop(state);

        let base_url = format!("http://127.0.0.1:{}", settings.port);
        let expected_model_id = served_model_id(&settings, model_id);
        let state_arc = Arc::clone(&self.state);
        let model_id = model_id.to_string();
        tokio::spawn(async move {
            let started = match supervised {
                Some(sidecar) => sidecar.ensure_running().await.map(|()| sidecar),
                None => {
                    let mut config =
                        SidecarConfig::new("mlx-engine", argv.clone(), base_url, expected_model_id);
                    config.env = vec![("PATH".to_string(), sidecar_spawn_path())];
                    Sidecar::start(config).await.map(Box::new)
                }
            };
            match started {
                Ok(sidecar) => {
                    let mut state = state_arc.lock().await;
                    let still_mounting = matches!(
                        &*state,
                        ManagerState::Mounting { model_id: current } if *current == model_id
                    );
                    if still_mounting {
                        *state = ManagerState::Running {
                            model_id,
                            sidecar,
                            argv,
                        };
                    } else {
                        drop(state);
                        sidecar.shutdown().await;
                    }
                }
                Err(e) => {
                    let mut state = state_arc.lock().await;
                    let still_mounting = matches!(
                        &*state,
                        ManagerState::Mounting { model_id: current } if *current == model_id
                    );
                    if still_mounting {
                        *state = ManagerState::Failed {
                            model_id,
                            error: format!("{e:#}"),
                        };
                    }
                }
            }
        });
        Ok(())
    }

    /// Stop the engine if one is running; a mount still in flight sees the state change
    /// and shuts its freshly started sidecar down on arrival. When the manager supervises
    /// nothing but the configured port is still occupied (an engine orphaned by a previous
    /// goosed — supervision state is in-memory only), unmount reclaims the port by
    /// terminating the listeners per-pid: SIGTERM, a grace window, then SIGKILL.
    pub async fn unmount(&self) {
        let supervised = {
            let mut state = self.state.lock().await;
            match std::mem::replace(&mut *state, ManagerState::Stopped) {
                ManagerState::Running { sidecar, .. } => {
                    sidecar.shutdown().await;
                    true
                }
                _ => false,
            }
        };
        if !supervised {
            let port = self.settings().port;
            if port_has_listener(port) {
                reclaim_port(port).await;
            }
        }
    }

    pub async fn status(&self) -> EngineStatus {
        let settings = self.settings();
        let (available, total) = measure();
        let (gate_message, gate_verdict) = match self.last_gate.lock().unwrap().clone() {
            Some(g) => (
                Some(g.message),
                Some(
                    match g.verdict {
                        Verdict::Allow => "allow",
                        Verdict::Warn => "warn",
                        Verdict::Block => "block",
                    }
                    .to_string(),
                ),
            ),
            None => (None, None),
        };
        let mut status = EngineStatus {
            state: "stopped".to_string(),
            model_id: None,
            base_url: None,
            pid: None,
            context_window: None,
            tool_call_parser: None,
            served_model_id: None,
            probe_error: None,
            gate_message,
            gate_verdict,
            stray_listener_port: None,
            available_memory_gb: available as f64 / GIB as f64,
            total_memory_gb: total as f64 / GIB as f64,
            restart_required: false,
            last_error: None,
        };

        let running = {
            let state = self.state.lock().await;
            match &*state {
                ManagerState::Stopped => None,
                ManagerState::Mounting { model_id } => {
                    status.state = "mounting".to_string();
                    status.model_id = Some(model_id.clone());
                    None
                }
                ManagerState::Failed { model_id, error } => {
                    status.state = "failed".to_string();
                    status.model_id = Some(model_id.clone());
                    status.last_error = Some(error.clone());
                    None
                }
                ManagerState::Running {
                    model_id,
                    sidecar,
                    argv,
                } => {
                    status.state = "running".to_string();
                    status.model_id = Some(model_id.clone());
                    status.base_url = Some(sidecar.base_url().to_string());
                    status.pid = sidecar.pid().await;
                    Some((model_id.clone(), argv.clone()))
                }
            }
        };

        if running.is_none() && port_has_listener(settings.port) {
            status.stray_listener_port = Some(settings.port);
        }
        if let Some((running_model, running_argv)) = running {
            let desired_model = settings.model_id.as_deref().unwrap_or(&running_model);
            status.restart_required = build_serve_command(&settings, desired_model) != running_argv;
            let base_url = status.base_url.as_deref().expect("set for running state");
            match self.probe_model_info(base_url).await {
                Ok((served_model_id, context_window, tool_call_parser)) => {
                    status.served_model_id = served_model_id;
                    status.context_window = context_window;
                    status.tool_call_parser = tool_call_parser;
                }
                Err(e) => status.probe_error = Some(format!("{e:#}")),
            }
        }
        status
    }

    #[allow(clippy::type_complexity)]
    async fn probe_model_info(
        &self,
        base_url: &str,
    ) -> Result<(Option<String>, Option<u64>, Option<String>)> {
        let url = format!("{base_url}/v1/models");
        let resp = self
            .probe_client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        let status = resp.status();
        let body = resp.text().await.context("reading /v1/models body")?;
        ensure!(status.is_success(), "GET {url} returned HTTP {status}");
        let parsed: serde_json::Value =
            serde_json::from_str(&body).context("parsing /v1/models body")?;
        let model = parsed
            .get("data")
            .and_then(|d| d.get(0))
            .with_context(|| format!("/v1/models returned no data entries: {body}"))?;
        Ok((
            model.get("id").and_then(|v| v.as_str()).map(str::to_string),
            model.get("context_window").and_then(|v| v.as_u64()),
            model
                .get("tool_call_parser")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        ))
    }
}

impl Default for MlxEngineManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn global_manager() -> &'static MlxEngineManager {
    static MANAGER: OnceLock<MlxEngineManager> = OnceLock::new();
    MANAGER.get_or_init(MlxEngineManager::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_command_uses_served_model_name_alias_when_set() {
        let settings = EngineSettings {
            served_model_name: Some("workhorse-qwen3.5-9b-4bit-mlx".to_string()),
            ..Default::default()
        };
        let argv = build_serve_command(&settings, "mlx-community/Qwen3.5-9B-MLX-4bit");
        let pos = argv
            .iter()
            .position(|a| a == "--served-model-name")
            .unwrap();
        assert_eq!(argv[pos + 1], "workhorse-qwen3.5-9b-4bit-mlx");
        assert!(argv
            .iter()
            .any(|a| a.ends_with("mlx-community/Qwen3.5-9B-MLX-4bit")));
    }

    fn full_profile() -> ModelProfile {
        ModelProfile {
            temperature: Some(0.7),
            top_p: Some(0.95),
            top_k: Some(40),
            min_p: Some(0.05),
            repetition_penalty: Some(1.1),
            presence_penalty: Some(0.5),
            frequency_penalty: Some(0.25),
            context_limit: Some(32768),
        }
    }

    #[test]
    fn serve_command_golden_with_all_sampling_flags_from_profile() {
        let settings = EngineSettings {
            model_id: Some("mlx-community/Qwen3.5-9B-MLX-4bit".to_string()),
            models_dir: "/opt/models".to_string(),
            model_profiles: BTreeMap::from([(
                "mlx-community/Qwen3.5-9B-MLX-4bit".to_string(),
                full_profile(),
            )]),
            ..Default::default()
        };
        let argv = build_serve_command(&settings, "mlx-community/Qwen3.5-9B-MLX-4bit");
        assert_eq!(
            argv,
            vec![
                "uvx",
                "--from",
                "git+https://github.com/leanzero-srl/Rapid-MLX@v0.13.1",
                "rapid-mlx",
                "serve",
                "/opt/models/mlx-community/Qwen3.5-9B-MLX-4bit",
                "--port",
                "8090",
                "--served-model-name",
                "mlx-community/Qwen3.5-9B-MLX-4bit",
                "--enable-prefix-cache",
                "--max-concurrent-requests",
                "8",
                "--default-temperature",
                "0.7",
                "--default-top-p",
                "0.95",
                "--default-min-p",
                "0.05",
                "--default-repetition-penalty",
                "1.1",
                "--default-presence-penalty",
                "0.5",
                "--default-frequency-penalty",
                "0.25",
                "--default-top-k",
                "40",
            ]
        );
    }

    #[test]
    fn serve_command_omits_unset_sampling_flags_and_expands_tilde() {
        let settings = EngineSettings::default();
        let argv = build_serve_command(&settings, "pub/model");
        assert!(
            argv.iter().all(|a| !a.starts_with("--default-")),
            "absent profile must emit no sampling flags: {argv:?}"
        );
        let model_path = &argv[5];
        assert!(
            !model_path.starts_with('~'),
            "tilde was not expanded: {model_path}"
        );
        assert!(model_path.ends_with("/.goose/models/pub/model"));
    }

    #[test]
    fn serve_command_ignores_unmigrated_legacy_flats() {
        let settings = EngineSettings {
            model_id: Some("pub/model".to_string()),
            temperature: Some(0.7),
            presence_penalty: Some(1.2),
            ..Default::default()
        };
        let argv = build_serve_command(&settings, "pub/model");
        assert!(
            argv.iter().all(|a| !a.starts_with("--default-")),
            "legacy flats must not reach argv — profiles are the source of truth: {argv:?}"
        );
    }

    #[test]
    fn serve_command_gives_each_model_its_own_profile_flags() {
        let settings = EngineSettings {
            model_profiles: BTreeMap::from([
                (
                    "pub/alpha".to_string(),
                    ModelProfile {
                        temperature: Some(0.2),
                        top_k: Some(20),
                        ..Default::default()
                    },
                ),
                (
                    "pub/beta".to_string(),
                    ModelProfile {
                        presence_penalty: Some(1.2),
                        ..Default::default()
                    },
                ),
            ]),
            ..Default::default()
        };
        let alpha = build_serve_command(&settings, "pub/alpha");
        let beta = build_serve_command(&settings, "pub/beta");

        let flag_value = |argv: &[String], flag: &str| {
            argv.iter()
                .position(|a| a == flag)
                .map(|i| argv[i + 1].clone())
        };
        assert_eq!(
            flag_value(&alpha, "--default-temperature"),
            Some("0.2".to_string())
        );
        assert_eq!(
            flag_value(&alpha, "--default-top-k"),
            Some("20".to_string())
        );
        assert_eq!(flag_value(&alpha, "--default-presence-penalty"), None);

        assert_eq!(
            flag_value(&beta, "--default-presence-penalty"),
            Some("1.2".to_string())
        );
        assert_eq!(flag_value(&beta, "--default-temperature"), None);
        assert_eq!(flag_value(&beta, "--default-top-k"), None);
    }

    /// `status()` computes `restart_required = build_serve_command(&settings, mounted) != running_argv`;
    /// this test pins that comparison's per-model semantics: editing the MOUNTED model's
    /// profile changes its argv (flips restart_required), editing a DIFFERENT model's
    /// profile leaves the mounted argv identical (does not).
    #[test]
    fn profile_edits_flip_restart_argv_only_for_the_mounted_model() {
        let mounted = "pub/mounted";
        let mut settings = EngineSettings {
            model_id: Some(mounted.to_string()),
            model_profiles: BTreeMap::from([(
                mounted.to_string(),
                ModelProfile {
                    temperature: Some(0.7),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let running_argv = build_serve_command(&settings, mounted);

        settings.model_profiles.insert(
            "pub/other".to_string(),
            ModelProfile {
                temperature: Some(0.1),
                top_k: Some(5),
                ..Default::default()
            },
        );
        assert_eq!(
            build_serve_command(&settings, mounted),
            running_argv,
            "a different model's profile edit must not require a restart"
        );

        settings
            .model_profiles
            .get_mut(mounted)
            .unwrap()
            .temperature = Some(0.9);
        assert_ne!(
            build_serve_command(&settings, mounted),
            running_argv,
            "the mounted model's profile edit must require a restart"
        );
    }

    #[test]
    fn migrate_legacy_moves_flats_into_the_model_profile_once() {
        let mut settings = EngineSettings {
            model_id: Some("pub/model".to_string()),
            presence_penalty: Some(1.2),
            temperature: Some(0.7),
            context_limit: Some(32768),
            ..Default::default()
        };
        assert!(settings.migrate_legacy());

        let profile = &settings.model_profiles["pub/model"];
        assert_eq!(profile.presence_penalty, Some(1.2));
        assert_eq!(profile.temperature, Some(0.7));
        assert_eq!(profile.context_limit, Some(32768));
        assert_eq!(settings.presence_penalty, None);
        assert_eq!(settings.temperature, None);
        assert_eq!(settings.context_limit, None);

        assert!(!settings.migrate_legacy(), "second run must be a no-op");
    }

    /// The exact `mlx_engine` value persisted in the live config on 2026-08-31 (flat
    /// presence_penalty 1.2, explicit nulls, no model_profiles key): it must
    /// deserialize as-is and migrate the penalty into the mounted model's profile.
    #[test]
    fn migrate_legacy_handles_the_live_config_shape() {
        let live = r#"{
            "model_id": "mlx-community/Qwen3.5-9B-MLX-4bit",
            "served_model_name": "workhorse-qwen3.5-9b-4bit-mlx",
            "models_dir": "~/.goose/models",
            "port": 8090,
            "context_limit": null,
            "temperature": null,
            "top_p": null,
            "top_k": null,
            "min_p": null,
            "repetition_penalty": null,
            "presence_penalty": 1.2,
            "frequency_penalty": null,
            "spawn_command": [
                "uvx",
                "--from",
                "git+https://github.com/leanzero-srl/Rapid-MLX@v0.13.1",
                "rapid-mlx"
            ]
        }"#;
        let mut settings: EngineSettings = serde_json::from_str(live).unwrap();
        assert!(settings.migrate_legacy());
        assert_eq!(settings.presence_penalty, None);
        let profile = &settings.model_profiles["mlx-community/Qwen3.5-9B-MLX-4bit"];
        assert_eq!(profile.presence_penalty, Some(1.2));

        let argv = build_serve_command(&settings, "mlx-community/Qwen3.5-9B-MLX-4bit");
        let pos = argv
            .iter()
            .position(|a| a == "--default-presence-penalty")
            .expect("migrated penalty must reach the serve argv");
        assert_eq!(argv[pos + 1], "1.2");
    }

    #[test]
    fn migrate_legacy_without_model_id_leaves_flats_untouched() {
        let mut settings = EngineSettings {
            presence_penalty: Some(1.2),
            ..Default::default()
        };
        assert!(!settings.migrate_legacy());
        assert_eq!(settings.presence_penalty, Some(1.2));
        assert!(settings.model_profiles.is_empty());
    }

    #[test]
    fn migrate_legacy_never_clobbers_existing_profile_values() {
        let mut settings = EngineSettings {
            model_id: Some("pub/model".to_string()),
            temperature: Some(0.3),
            top_p: Some(0.9),
            model_profiles: BTreeMap::from([(
                "pub/model".to_string(),
                ModelProfile {
                    temperature: Some(0.8),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        assert!(settings.migrate_legacy());
        let profile = &settings.model_profiles["pub/model"];
        assert_eq!(
            profile.temperature,
            Some(0.8),
            "explicit profile value must win over the legacy flat"
        );
        assert_eq!(
            profile.top_p,
            Some(0.9),
            "unset profile field takes the flat"
        );
        assert_eq!(settings.temperature, None, "flats clear either way");
        assert_eq!(settings.top_p, None);
    }

    #[test]
    fn migrate_legacy_with_no_flats_is_a_no_op() {
        let mut settings = EngineSettings {
            model_id: Some("pub/model".to_string()),
            ..Default::default()
        };
        assert!(!settings.migrate_legacy());
        assert!(settings.model_profiles.is_empty());
    }

    #[tokio::test]
    async fn status_reports_a_stray_listener_on_the_configured_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let manager = MlxEngineManager::new();
        manager.set_settings(EngineSettings {
            port,
            ..Default::default()
        });
        let status = manager.status().await;
        assert_eq!(status.state, "stopped");
        assert_eq!(status.stray_listener_port, Some(port));
        drop(listener);
        let status = manager.status().await;
        assert_eq!(status.stray_listener_port, None);
    }

    #[tokio::test]
    async fn unmount_reclaims_an_unsupervised_listener() {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let mut orphan = std::process::Command::new("python3")
            .args([
                "-c",
                &format!(
                    "import http.server; http.server.HTTPServer(('127.0.0.1', {port}), \
                     http.server.BaseHTTPRequestHandler).serve_forever()"
                ),
            ])
            .spawn()
            .unwrap();
        for _ in 0..50 {
            if port_has_listener(port) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(port_has_listener(port), "orphan never came up");

        let manager = MlxEngineManager::new();
        manager.set_settings(EngineSettings {
            port,
            ..Default::default()
        });
        manager.unmount().await;
        assert!(!port_has_listener(port), "unmount did not reclaim the port");
        let _ = orphan.wait();
    }

    /// A fake engine launched through the REAL serve argv (`spawn_command` + `serve <dir>
    /// --port N --served-model-name X …`): it reads the port and the alias from argv and
    /// serves that alias in `/v1/models`, so the manager's own path is exercised end to end.
    const ARGV_FAKE_ENGINE: &str = r#"
import http.server, json, sys
argv = sys.argv
port = int(argv[argv.index("--port") + 1])
name = argv[argv.index("--served-model-name") + 1]
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = json.dumps({"object": "list", "data": [{"id": name}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass
http.server.HTTPServer(("127.0.0.1", port), H).serve_forever()
"#;

    async fn settle(manager: &MlxEngineManager) -> EngineStatus {
        loop {
            let status = manager.status().await;
            if status.state != "mounting" {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// S-L8: the circuit breaker now sits on the production re-mount path. A mount of the
    /// IDENTICAL configuration keeps the supervisor (a healthy engine is verified, not
    /// restarted); after the engine is killed, the same mount restarts it through
    /// `ensure_running`; a crash loop trips the breaker into a NAMED Failed state.
    #[tokio::test]
    async fn identical_mount_reuses_the_supervisor_and_a_crash_loop_trips_the_breaker() {
        let port = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap().port()
        };
        let tmp = tempfile::tempdir().unwrap();
        complete_small_model(tmp.path(), "pub/small");
        let manager = MlxEngineManager::new();
        manager.set_settings(EngineSettings {
            models_dir: tmp.path().to_string_lossy().into_owned(),
            port,
            served_model_name: Some("node-alias".to_string()),
            spawn_command: vec![
                "python3".to_string(),
                "-c".to_string(),
                ARGV_FAKE_ENGINE.to_string(),
            ],
            ..Default::default()
        });

        manager.mount("pub/small").await.unwrap();
        let status = settle(&manager).await;
        assert_eq!(status.state, "running", "{:?}", status.last_error);
        assert_eq!(status.served_model_id.as_deref(), Some("node-alias"));
        let first_pid = status.pid.unwrap();

        manager.mount("pub/small").await.unwrap();
        let status = settle(&manager).await;
        assert_eq!(status.state, "running");
        assert_eq!(
            status.pid,
            Some(first_pid),
            "a healthy identical mount must verify, not restart"
        );

        let mut pids = vec![first_pid];
        let mut tripped = None;
        for _ in 0..5 {
            let pid = *pids.last().unwrap();
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            tokio::time::sleep(Duration::from_millis(200)).await;
            manager.mount("pub/small").await.unwrap();
            let status = settle(&manager).await;
            match status.state.as_str() {
                "running" => {
                    let restarted = status.pid.unwrap();
                    assert!(!pids.contains(&restarted));
                    pids.push(restarted);
                }
                "failed" => {
                    tripped = status.last_error;
                    break;
                }
                other => panic!("unexpected state {other}"),
            }
        }
        let error = tripped.expect("a crash loop must trip the breaker into Failed");
        assert!(
            error.contains("circuit breaker open"),
            "Failed must NAME the breaker: {error}"
        );
        assert_eq!(pids.len(), 4, "three restarts, then the breaker");
        assert_eq!(manager.status().await.state, "failed");
        assert!(
            !port_has_listener(port),
            "the tripped supervisor must leave nothing on the port"
        );
        manager.unmount().await;
    }

    /// The real engine through the manager: `uvx … rapid-mlx serve` on a free port with an
    /// alias, driven to Running by the progress terminator, the alias checked in the
    /// catalog, then unmounted — port free, the wrapper's whole group gone. Reads a model
    /// from `GOOSE_SIDECAR_LIVE_MODELS_DIR` / `GOOSE_SIDECAR_LIVE_MODEL_ID` (never deletes).
    #[tokio::test]
    #[ignore = "spawns the real uvx/rapid-mlx engine; set GOOSE_SIDECAR_LIVE_MODELS_DIR and GOOSE_SIDECAR_LIVE_MODEL_ID"]
    async fn live_mount_of_the_real_engine_runs_and_unmounts_clean() {
        let models_dir = std::env::var("GOOSE_SIDECAR_LIVE_MODELS_DIR").unwrap();
        let model_id = std::env::var("GOOSE_SIDECAR_LIVE_MODEL_ID").unwrap();
        let port = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap().port()
        };
        let manager = MlxEngineManager::new();
        manager.set_settings(EngineSettings {
            models_dir,
            port,
            served_model_name: Some("live-alias".to_string()),
            ..Default::default()
        });

        let started = std::time::Instant::now();
        manager.mount(&model_id).await.unwrap();
        let status = settle(&manager).await;
        assert_eq!(status.state, "running", "{:?}", status.last_error);
        assert_eq!(status.served_model_id.as_deref(), Some("live-alias"));
        let leader = status.pid.unwrap();
        assert!(crate::owns_process_group(leader));
        let members = std::process::Command::new("pgrep")
            .args(["-g", &leader.to_string()])
            .output()
            .unwrap();
        let members = String::from_utf8_lossy(&members.stdout);
        eprintln!(
            "live: running after {:.1}s, leader {leader}, group members [{}], context_window {:?}, parser {:?}",
            started.elapsed().as_secs_f64(),
            members.split_whitespace().collect::<Vec<_>>().join(" "),
            status.context_window,
            status.tool_call_parser
        );
        assert!(
            members.split_whitespace().count() >= 2,
            "uv and its engine must both sit in the leader's group: [{members}]"
        );

        manager.unmount().await;
        assert!(!port_has_listener(port), "port still served after unmount");
        let leftover = std::process::Command::new("pgrep")
            .args(["-g", &leader.to_string()])
            .output()
            .unwrap();
        assert!(
            leftover.stdout.is_empty(),
            "group {leader} survived unmount: {}",
            String::from_utf8_lossy(&leftover.stdout)
        );
        assert_eq!(manager.status().await.state, "stopped");
    }

    fn complete_small_model(models_dir: &std::path::Path, id: &str) {
        let model_dir = models_dir.join(id);
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("config.json"), "{}").unwrap();
        std::fs::write(model_dir.join("model.safetensors"), "weights").unwrap();
    }

    /// S-H3: a listener this manager never started (a goosed-restart orphan, another
    /// process's engine) must REFUSE the mount, not be probed as our readiness.
    #[tokio::test]
    async fn mount_refuses_when_an_unsupervised_listener_holds_the_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let tmp = tempfile::tempdir().unwrap();
        complete_small_model(tmp.path(), "pub/small");

        let manager = MlxEngineManager::new();
        manager.set_settings(EngineSettings {
            models_dir: tmp.path().to_string_lossy().into_owned(),
            port,
            ..Default::default()
        });

        let err = manager.mount("pub/small").await.unwrap_err();
        assert_eq!(
            err.downcast_ref::<UnsupervisedListenerError>(),
            Some(&UnsupervisedListenerError { port }),
            "unexpected error: {err:#}"
        );
        assert_eq!(
            err.to_string(),
            format!("port {port} has an unsupervised listener — unmount/reclaim it first")
        );
        let status = manager.status().await;
        assert_eq!(
            status.state, "stopped",
            "a refused mount leaves nothing mounting"
        );
        assert_eq!(status.stray_listener_port, Some(port));
        drop(listener);
    }

    #[tokio::test]
    async fn mount_refuses_on_memory_gate_block_and_stays_stopped() {
        let tmp = tempfile::tempdir().unwrap();
        let model_dir = tmp.path().join("pub/huge");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("config.json"), "{}").unwrap();
        let weights = std::fs::File::create(model_dir.join("model.safetensors")).unwrap();
        weights.set_len(4096 * crate::GIB).unwrap();

        let manager = MlxEngineManager::new();
        manager.set_settings(EngineSettings {
            models_dir: tmp.path().to_string_lossy().into_owned(),
            ..Default::default()
        });

        let err = manager.mount("pub/huge").await.unwrap_err().to_string();
        assert!(err.contains("memory gate BLOCK"), "unexpected error: {err}");

        let status = manager.status().await;
        assert_eq!(status.state, "stopped");
        assert!(status.gate_message.unwrap().contains("exceeds available"));
    }

    #[tokio::test]
    async fn mount_refuses_missing_and_incomplete_models() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = MlxEngineManager::new();
        manager.set_settings(EngineSettings {
            models_dir: tmp.path().to_string_lossy().into_owned(),
            ..Default::default()
        });

        let err = manager.mount("pub/absent").await.unwrap_err().to_string();
        assert!(err.contains("not found"), "unexpected error: {err}");

        let partial = tmp.path().join("pub/partial");
        std::fs::create_dir_all(&partial).unwrap();
        std::fs::write(partial.join("config.json"), "{}").unwrap();
        std::fs::write(partial.join("model.safetensors"), "w").unwrap();
        std::fs::write(partial.join("model.safetensors.part"), "p").unwrap();
        let err = manager.mount("pub/partial").await.unwrap_err().to_string();
        assert!(err.contains("incomplete"), "unexpected error: {err}");
    }
}
