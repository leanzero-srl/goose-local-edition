//! The in-house MLX engine manager: one supervised Rapid-MLX process, one mounted model.
//!
//! Mounting is asynchronous by design — `mount` validates the model and the memory gate,
//! flips to `Mounting`, and returns; a spawned task drives `Sidecar::start` to `Running`
//! or `Failed`. Callers poll `status()`, which also probes the live engine's `/v1/models`
//! for its context window and tool-call parser and never fabricates either.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::hf::{self, LocalModel};
use crate::{measure, MemoryGate, Sidecar, SidecarConfig, Verdict, GIB};

pub const ENGINE_LAUNCHER: [&str; 4] = [
    "uvx",
    "--from",
    "git+https://github.com/leanzero-srl/Rapid-MLX@v0.13.1",
    "rapid-mlx",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineSettings {
    pub model_id: Option<String>,
    pub models_dir: String,
    pub port: u16,
    pub context_limit: Option<u32>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub min_p: Option<f64>,
    pub repetition_penalty: Option<f64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub spawn_command: Vec<String>,
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
            spawn_command: ENGINE_LAUNCHER.iter().map(|s| s.to_string()).collect(),
        }
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

pub fn build_serve_command(settings: &EngineSettings, model_id: &str) -> Vec<String> {
    let model_path = expand_tilde(&settings.models_dir).join(model_id);
    let mut argv = settings.spawn_command.clone();
    argv.extend([
        "serve".to_string(),
        model_path.to_string_lossy().into_owned(),
        "--port".to_string(),
        settings.port.to_string(),
        "--served-model-name".to_string(),
        model_id.to_string(),
        "--enable-prefix-cache".to_string(),
        "--max-concurrent-requests".to_string(),
        "8".to_string(),
    ]);
    let float_flags = [
        ("--default-temperature", settings.temperature),
        ("--default-top-p", settings.top_p),
        ("--default-min-p", settings.min_p),
        ("--default-repetition-penalty", settings.repetition_penalty),
        ("--default-presence-penalty", settings.presence_penalty),
        ("--default-frequency-penalty", settings.frequency_penalty),
    ];
    for (flag, value) in float_flags {
        if let Some(value) = value {
            argv.push(flag.to_string());
            argv.push(value.to_string());
        }
    }
    if let Some(top_k) = settings.top_k {
        argv.push("--default-top-k".to_string());
        argv.push(top_k.to_string());
    }
    argv
}

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
    pub probe_error: Option<String>,
    pub gate_message: Option<String>,
    pub available_memory_gb: f64,
    pub total_memory_gb: f64,
    pub restart_required: bool,
    pub last_error: Option<String>,
}

pub struct MlxEngineManager {
    state: Arc<Mutex<ManagerState>>,
    settings: StdMutex<EngineSettings>,
    last_gate_message: StdMutex<Option<String>>,
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
            last_gate_message: StdMutex::new(None),
            gate,
            probe_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client with static configuration"),
        }
    }

    pub fn set_settings(&self, settings: EngineSettings) {
        *self.settings.lock().unwrap() = settings;
    }

    pub fn settings(&self) -> EngineSettings {
        self.settings.lock().unwrap().clone()
    }

    /// Validate the model and the memory gate, flip to `Mounting`, and return; the engine
    /// start continues in a spawned task. A gate `Block` refuses the mount outright.
    /// Any already-running engine is shut down first — one model per engine.
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
        if gate.verdict == Verdict::Block {
            *self.last_gate_message.lock().unwrap() = Some(gate.message.clone());
            bail!("memory gate BLOCK for '{model_id}': {}", gate.message);
        }
        *self.last_gate_message.lock().unwrap() = Some(gate.message);

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
        if let ManagerState::Running { sidecar, .. } = previous {
            sidecar.shutdown().await;
        }
        drop(state);

        let argv = build_serve_command(&settings, model_id);
        let base_url = format!("http://127.0.0.1:{}", settings.port);
        let state_arc = Arc::clone(&self.state);
        let model_id = model_id.to_string();
        tokio::spawn(async move {
            let config = SidecarConfig::new("mlx-engine", argv.clone(), base_url);
            match Sidecar::start(config).await {
                Ok(sidecar) => {
                    let mut state = state_arc.lock().await;
                    let still_mounting = matches!(
                        &*state,
                        ManagerState::Mounting { model_id: current } if *current == model_id
                    );
                    if still_mounting {
                        *state = ManagerState::Running {
                            model_id,
                            sidecar: Box::new(sidecar),
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
    /// and shuts its freshly started sidecar down on arrival.
    pub async fn unmount(&self) {
        let mut state = self.state.lock().await;
        if let ManagerState::Running { sidecar, .. } =
            std::mem::replace(&mut *state, ManagerState::Stopped)
        {
            sidecar.shutdown().await;
        }
    }

    pub async fn status(&self) -> EngineStatus {
        let settings = self.settings();
        let (available, total) = measure();
        let gate_message = self.last_gate_message.lock().unwrap().clone();
        let mut status = EngineStatus {
            state: "stopped".to_string(),
            model_id: None,
            base_url: None,
            pid: None,
            context_window: None,
            tool_call_parser: None,
            probe_error: None,
            gate_message,
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

        if let Some((running_model, running_argv)) = running {
            let desired_model = settings.model_id.as_deref().unwrap_or(&running_model);
            status.restart_required = build_serve_command(&settings, desired_model) != running_argv;
            let base_url = status.base_url.as_deref().expect("set for running state");
            match self.probe_model_info(base_url).await {
                Ok((context_window, tool_call_parser)) => {
                    status.context_window = context_window;
                    status.tool_call_parser = tool_call_parser;
                }
                Err(e) => status.probe_error = Some(format!("{e:#}")),
            }
        }
        status
    }

    async fn probe_model_info(&self, base_url: &str) -> Result<(Option<u64>, Option<String>)> {
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
    fn serve_command_golden_with_all_sampling_flags() {
        let settings = EngineSettings {
            model_id: Some("mlx-community/Qwen3.5-9B-MLX-4bit".to_string()),
            models_dir: "/opt/models".to_string(),
            port: 8090,
            context_limit: Some(32768),
            temperature: Some(0.7),
            top_p: Some(0.95),
            top_k: Some(40),
            min_p: Some(0.05),
            repetition_penalty: Some(1.1),
            presence_penalty: Some(0.5),
            frequency_penalty: Some(0.25),
            spawn_command: ENGINE_LAUNCHER.iter().map(|s| s.to_string()).collect(),
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
            "unset sampling fields must emit no flags: {argv:?}"
        );
        let model_path = &argv[5];
        assert!(
            !model_path.starts_with('~'),
            "tilde was not expanded: {model_path}"
        );
        assert!(model_path.ends_with("/.goose/models/pub/model"));
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
