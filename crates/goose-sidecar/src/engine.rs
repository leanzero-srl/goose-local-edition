//! The in-house MLX engine manager: one supervised Rapid-MLX process, one mounted model.
//!
//! Mounting is asynchronous by design — `mount` validates the model and the memory gate,
//! flips to `Mounting`, and returns; a spawned task drives `Sidecar::start` to `Running`
//! or `Failed`. Callers poll `status()`, which also probes the live engine's `/v1/models`
//! for its context window and tool-call parser, and its `/v1/status` for the in-flight
//! request count — and never fabricates any of them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::fit::{self, FitVerdict, Need, NodeMemoryFacts, Verdict};
use crate::hf::{self, LocalModel};
use crate::kv_cache::{self, KvCacheMode};
use crate::{
    listening_pids, measure, port_has_listener, Sidecar, SidecarConfig, StartupWatch, GIB,
};

/// Rapid-MLX's `--max-concurrent-requests` is a HARD ADMISSION CAP, not a queue: the request past
/// it is answered `503 Server is busy (max concurrent requests reached)`. One source for the serve
/// argv and for every router that sizes a sidecar node's slots.
pub const MAX_CONCURRENT_REQUESTS: u32 = 8; // measured: 9th concurrent request got 503 (MLX busy-signal agent, 2026-09-02)

/// The prefix cache's share of the memory the engine finds FREE ONCE ITS WEIGHTS ARE IN
/// (`--cache-memory-percent`): Rapid-MLX builds the cache after the model load and multiplies
/// this by psutil's available bytes at that moment, so the Mac measures its own budget. The
/// engine default (0.20) gave the Studio 12,512,444,416 B, and ONE 49k-token agent turn with its
/// title, reviewer and canary requests filled 9,765,978,112 B of it (78%, /v1/status 2026-09-25):
/// a second session's prompt, or the next turn's own entries, had to evict the first. Receipt for the value, the Studio (M3 Ultra 96 GB,
/// Qwen3.8-27B Q8, /v1/status after one 49k turn): post-load free 62.56e9 B, engine base 30.44e9
/// (Metal active 40.21e9 − cache 9.77e9), a cold 49k prefill's peak +6.14e9, a live 49k sequence
/// 3.37e9, the engine's own pressure-eviction line 0.9 × 0.9 × the 83.49e9 GPU ceiling = 67.63e9.
/// Holding two measured turns needs ≥ 0.312; staying under that line with the cache full, one
/// prefill peak and two more live 49k sequences needs ≤ 0.389 — see
/// `the_prefix_cache_share_holds_two_turns_under_the_pressure_line`.
pub const PREFIX_CACHE_SHARE_OF_FREE: f64 = 0.35; // ratio: of the engine's post-load available memory; receipt above

/// Recurrent-state (GatedDeltaNet) and sliding-window caches cannot be trimmed, so Rapid-MLX
/// bounds how many such prefix entries it keeps by COUNT (`--hybrid-cache-entries`), auto-set to
/// 8 — a count that knows nothing of the byte budget above. Every request stores this many
/// (measured: 12 radix inserts for 4 requests on the Studio, 2026-09-25), and goose sends an
/// agent turn plus its title, reviewer and canary requests, so the 8 were full (8 of 8, one
/// eviction) after ONE user turn; raising the byte share alone would not keep a second.
pub const PREFIX_ENTRIES_PER_REQUEST: u32 = 3; // measured: 12 radix inserts / 4 requests, Studio /v1/status 2026-09-25

/// The non-trimmable entry count: every request the engine admits at once keeps its entries, so
/// the count never evicts before a full admission window has been stored — the byte share
/// governs everything beyond it.
pub fn hybrid_cache_entries() -> u32 {
    MAX_CONCURRENT_REQUESTS * PREFIX_ENTRIES_PER_REQUEST
}

/// v0.14.3-lz.5 = lz.4 + a request with no `max_tokens` generates until the model stops or the
/// context window is full (fork lz/unset-max-tokens): goose sends none on purpose, and lz.4 filled
/// it with the serve default 32768 — a hidden cap on every answer that also refused any prompt
/// within 32768 tokens of the window (`prompt + default > window` → 400). The budget is now the
/// room the counted prompt leaves; memory bounds decode instead (at the admission cap, after the
/// caches give back what they can, the longest generation stops as `length`). Q-65's class.
/// v0.14.3-lz.4 = lz.3 + a `logprobs` request on an MTP-mounted engine is answered instead of
/// aborting the whole process (fork ce15b39ff): the speculative paths yielded LAZY logprob rows
/// built on the engine's step-thread stream, and the route's `np.array` evaluated them on a thread
/// that has no such stream — a C++ throw inside the buffer protocol, `libc++abi: terminating`.
/// lz.4 schedules the rows on the step thread; a residual failure fails that request only.
/// lz.3 = lz.2 + quantized live KV on GatedDeltaNet hybrids (fork 999d43ea6): lz.2 refused
/// `--kv-cache-dtype int8|int4` on every qwen3_5 checkpoint pre-ready ("the loaded model is
/// incompatible: ArraysCache"); lz.3 compresses the full-attention layers and leaves the
/// linear-attention state alone, and prices the compressed cache in its admission gate. With no
/// `kv_cache` in the profile it runs the lz.2 code paths (the change is gated on the flag).
/// lz.2 = lz.1 + the `rapid_mlx_transient_tail` request extension (fork 42d207cf), measured in
/// evals/mlx-engine-bench/results/2026-09-23-phase1-*; the KV measurement is
/// evals/mlx-engine-bench/results/2026-09-24-kv-quant.
pub const ENGINE_LAUNCHER: [&str; 4] = [
    "uvx",
    "--from",
    "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.5",
    "rapid-mlx",
];

/// Every launcher this crate ever shipped as its default, oldest first. A persisted
/// `spawn_command` equal to one of these was never chosen by the owner — it is the default
/// the app wrote on first save — so it follows the current default (`migrate_launcher`).
/// A launcher NOT in this list is the owner's own and is never touched.
pub const SUPERSEDED_ENGINE_LAUNCHERS: &[[&str; 4]] = &[
    [
        "uvx",
        "--from",
        "git+https://github.com/leanzero-srl/Rapid-MLX@v0.13.1",
        "rapid-mlx",
    ],
    [
        "uvx",
        "--from",
        "git+https://github.com/leanzero-srl/Rapid-MLX@v0.13.4-lz.1",
        "rapid-mlx",
    ],
    [
        "uvx",
        "--from",
        "git+https://github.com/leanzero-srl/Rapid-MLX@v0.13.4-lz.2",
        "rapid-mlx",
    ],
    [
        "uvx",
        "--from",
        "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.1",
        "rapid-mlx",
    ],
    [
        "uvx",
        "--from",
        "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.2",
        "rapid-mlx",
    ],
    [
        "uvx",
        "--from",
        "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.3",
        "rapid-mlx",
    ],
    [
        "uvx",
        "--from",
        "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.4",
        "rapid-mlx",
    ],
];

/// The MTP head file an MTPLX-style artefact ships next to its trunk shards. Its presence
/// is the whole MTP auto-detection: the engine's injector probes exactly this name first.
pub const MTP_SIDECAR_FILE: &str = "mtp.safetensors";

/// Draft depth handed to the engine's MTP speculative decoder (`num_speculative_tokens`). It
/// is the CEILING of the engine's EV auto-K controller, which picks K in 0..=this per round
/// (scheduler.py `mtp_max_k`); the checkpoint caps it again (`mtplx_runtime.json`
/// `mtp_depth_max`, 3 on the Qwen3.8-27B MTPLX build). A/B 2026-09-23 on that 27B, greedy,
/// N=3 medians (evals/mlx-engine-bench/results/2026-09-23-phase0-k{3-env,2,1}): decode tok/s
/// short chat K3 22.8 / K2 23.0 / K1 20.2, 32k prompt K3 17.1 / K2 17.4 / K1 18.4 — K2 is
/// inside K3's noise on both, K1 trades -11% short for +8% long (under K3 the controller
/// already drops to K<=2 on 90%+ of the 32k rounds), so no depth beats 3 on both and it stays.
pub const MTP_SPECULATIVE_TOKENS: u32 = 3; // measured: evals/mlx-engine-bench/results/2026-09-23-phase0-k{3-env,2,1} (and Rapid-MLX cli.py's own K=3 MTP default)

/// The two files an mlx-lm LoRA/DoRA adapter directory must hold (`mlx_lm.lora --train`
/// writes both). The engine refuses `--adapter-path` without them (exit 2) — we refuse
/// earlier, at mount, with the missing names in the error.
pub const ADAPTER_REQUIRED_FILES: [&str; 2] = ["adapter_config.json", "adapters.safetensors"];

/// Per-model sampling and context settings. Sampling is per MODEL, not per engine:
/// each mounted model pulls its own profile from `EngineSettings::model_profiles`.
/// `context_limit` is profile state for goose's own context bookkeeping — Rapid-MLX
/// 0.13.1 has no context-length serve flag (`--max-tokens` caps generation, a
/// different knob), so it emits no argv.
///
/// The three serving-lane fields are OVERRIDES over what the model directory itself says
/// (`inspect_model_dir`): `None` means "auto" everywhere, so a persisted profile that
/// predates them keeps loading and keeps mounting the way the checkpoint dictates.
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
    /// `"mtp"` demands MTP speculative decoding (skipped with a warning when the directory
    /// has no `mtp.safetensors`), `"off"` refuses it, `None` = auto (on when the head file
    /// is there).
    pub speculative: Option<String>,
    /// Directory of an mlx-lm LoRA/DoRA adapter to fuse at load (`--adapter-path`);
    /// `~` expands. Validated at argv build: missing files fail the mount.
    pub adapter_path: Option<String>,
    /// `Some(false)` lets a vision-bearing checkpoint take the engine's MLLM lane;
    /// `None`/`Some(true)` pin it to the text lane (`--text-only`). No effect on a
    /// checkpoint whose config.json declares no vision.
    pub text_only: Option<bool>,
    /// Request-side reasoning switch, sent as `chat_template_kwargs.enable_thinking` on every turn
    /// of a session routed to this model. `None` = auto: nothing is sent and the engine decides
    /// (Rapid-MLX turns thinking OFF whenever a request carries tools). No argv effect.
    pub thinking: Option<ThinkingMode>,
    /// A level from the template's own effort vocabulary (`thinking::ThinkingCapabilities::
    /// effort_levels`), sent as `chat_template_kwargs.reasoning_effort`. `None` = the template's
    /// default. Locked per session: it rewrites the system prompt, so changing it mid-session
    /// would void the prefix cache. No argv effect.
    pub reasoning_effort: Option<String>,
    /// Compressed live KV cache (`--kv-cache-dtype int8|int4`). `None` = off: no flag, the
    /// engine's bf16 cache. Refused at argv build when the model's KV cannot take it
    /// (`kv_cache::check_mode_applies`); the engine refuses architectures it cannot quantize.
    pub kv_cache: Option<KvCacheMode>,
}

/// The explicit thinking choices; auto is the ABSENCE of a choice (`Option::None`), never a variant,
/// so a profile that never touched the switch sends exactly what it sent before the switch existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingMode {
    On,
    Off,
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
    /// A persisted `spawn_command` that is exactly a SUPERSEDED default follows the current
    /// default; returns the launcher it replaced so the caller can log the move. Without this
    /// the pin the app wrote on first save (`@v0.13.1`) ran forever on every existing install
    /// while the shipped default moved on — an engine upgrade that reached only fresh
    /// configs. An owner-edited launcher never matches and is left alone.
    pub fn migrate_launcher(&mut self) -> Option<Vec<String>> {
        let current: Vec<String> = ENGINE_LAUNCHER.iter().map(|s| s.to_string()).collect();
        if self.spawn_command == current {
            return None;
        }
        let superseded = SUPERSEDED_ENGINE_LAUNCHERS.iter().any(|old| {
            old.iter()
                .copied()
                .eq(self.spawn_command.iter().map(String::as_str))
        });
        if !superseded {
            return None;
        }
        let replaced = std::mem::replace(&mut self.spawn_command, current);
        Some(replaced)
    }

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
            ..Default::default()
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

/// The id an engine serving `model_id` advertises in `/v1/models`. `served_model_name` is the
/// alias of ONE model — `model_id`, the model the swarm node was set up for (AddNodeDialog and
/// `goose swarm` write the pair together) — so it applies only when `model_id` IS that model;
/// every other model is served under its own HF directory id. Applying the alias to any model put
/// the 27B's name on a Flash split (live 2026-09-24: `/v1/models` on :8091 answered
/// `mihai-qwen3.8-27b-atlassian-q8-mlx` while serving Qwen3.8-Flash-Next-4bit), so a swarm node
/// named for the 27B routed to a different model. The serve argv, the readiness check, the
/// distributed ranks and the router all derive from this one place.
pub fn served_model_id(settings: &EngineSettings, model_id: &str) -> String {
    match &settings.served_model_name {
        Some(alias) if settings.model_id.as_deref() == Some(model_id) => alias.clone(),
        _ => model_id.to_string(),
    }
}

/// What the model directory itself says about how it must be served. Read at argv build
/// time — not persisted — so a re-download (an MTP head appearing, a vision-config
/// checkpoint replacing a text one) changes the argv, and with it `restart_required`,
/// without anyone editing a setting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelDirFacts {
    /// `<dir>/mtp.safetensors` is a file.
    pub has_mtp_sidecar: bool,
    /// config.json declares `vision_config`, or is a `qwen3_5` `*ForConditionalGeneration`
    /// checkpoint — either way the engine's auto-detection may route it to the serialized
    /// single-request MLLM lane unless `--text-only` pins the text lane.
    pub vision_bearing: bool,
}

pub fn inspect_model_dir(dir: &Path) -> ModelDirFacts {
    let has_mtp_sidecar = dir.join(MTP_SIDECAR_FILE).is_file();
    let vision_bearing = match std::fs::read(dir.join("config.json")) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(config) => config_declares_vision(&config),
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "config.json is not JSON; assuming a text-only checkpoint");
                false
            }
        },
        Err(_) => false,
    };
    ModelDirFacts {
        has_mtp_sidecar,
        vision_bearing,
    }
}

fn config_declares_vision(config: &serde_json::Value) -> bool {
    if config.get("vision_config").is_some() {
        return true;
    }
    let qwen3_5 = config.get("model_type").and_then(|v| v.as_str()) == Some("qwen3_5");
    let conditional_generation = config
        .get("architectures")
        .and_then(|v| v.as_array())
        .map(|archs| {
            archs
                .iter()
                .filter_map(|a| a.as_str())
                .any(|a| a.contains("ForConditionalGeneration"))
        })
        .unwrap_or(false);
    qwen3_5 && conditional_generation
}

/// The `--speculative-config` value for an MTP head living in `model_dir`. The `model`
/// field is REQUIRED: the engine resolves the MTP sidecar from it (a directory is probed
/// for `mtp.safetensors` first); a bare `{"method":"mtp"}` on a local directory reaches
/// the injector with `sidecar=None` and hard-fails at boot.
fn mtp_speculative_config(model_dir: &Path) -> String {
    let dir = if model_dir.is_absolute() {
        model_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(model_dir))
            .unwrap_or_else(|_| model_dir.to_path_buf())
    };
    let dir_json = serde_json::to_string(&dir.to_string_lossy()).expect("a string serializes");
    format!(
        r#"{{"method":"mtp","model":{dir_json},"num_speculative_tokens":{MTP_SPECULATIVE_TOKENS}}}"#
    )
}

/// The adapter directory the profile names, expanded and PROVEN to be an mlx-lm adapter
/// (both required files present). A missing directory or file is an error that names it —
/// mount refuses rather than launching an engine that exits 2 on the same check.
fn validated_adapter_dir(raw: &str) -> Result<Option<PathBuf>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let dir = expand_tilde(raw);
    ensure!(
        dir.is_dir(),
        "adapter_path '{}' is not a directory",
        dir.display()
    );
    let missing: Vec<&str> = ADAPTER_REQUIRED_FILES
        .iter()
        .copied()
        .filter(|name| !dir.join(name).is_file())
        .collect();
    ensure!(
        missing.is_empty(),
        "adapter_path '{}' is not an mlx-lm adapter directory: missing {} (mlx_lm.lora --train writes both {})",
        dir.display(),
        missing.join(" and "),
        ADAPTER_REQUIRED_FILES.join(" + ")
    );
    Ok(Some(dir))
}

/// The engine argv for `model_id` under `settings`, read together with the model
/// directory (`inspect_model_dir`). Fails only when the profile names an adapter directory
/// that is not one, when checkpoint parser metadata cannot be read, or when the profile asks for
/// a compressed KV cache the model's KV layout cannot take.
pub fn build_serve_command(settings: &EngineSettings, model_id: &str) -> Result<Vec<String>> {
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
        "--cache-memory-percent".to_string(),
        PREFIX_CACHE_SHARE_OF_FREE.to_string(),
        "--hybrid-cache-entries".to_string(),
        hybrid_cache_entries().to_string(),
        "--max-concurrent-requests".to_string(),
        MAX_CONCURRENT_REQUESTS.to_string(),
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

    crate::model_parsers::append_checkpoint_parser_flags(&model_path, &mut argv)?;
    let facts = inspect_model_dir(&model_path);
    let speculative = profile
        .speculative
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase());
    let want_mtp = match speculative.as_deref() {
        Some("off") => false,
        Some("mtp") => {
            if !facts.has_mtp_sidecar {
                tracing::warn!(
                    model_id,
                    dir = %model_path.display(),
                    "profile asks for MTP but {MTP_SIDECAR_FILE} is missing; serving without speculative decoding"
                );
            }
            facts.has_mtp_sidecar
        }
        None => facts.has_mtp_sidecar,
        Some(other) => {
            tracing::warn!(
                model_id,
                value = other,
                "unknown profile.speculative value (expected \"mtp\" or \"off\"); treating as auto"
            );
            facts.has_mtp_sidecar
        }
    };
    if want_mtp {
        argv.push("--speculative-config".to_string());
        argv.push(mtp_speculative_config(&model_path));
    }
    if facts.vision_bearing && profile.text_only.unwrap_or(true) {
        argv.push("--text-only".to_string());
    }
    if let Some(raw) = profile.adapter_path.as_deref() {
        if let Some(dir) = validated_adapter_dir(raw)? {
            argv.push("--adapter-path".to_string());
            argv.push(dir.to_string_lossy().into_owned());
        }
    }
    if let Some(mode) = profile.kv_cache {
        kv_cache::check_mode_applies(&model_path, mode)?;
        argv.push("--kv-cache-dtype".to_string());
        argv.push(mode.engine_dtype().to_string());
    }
    Ok(argv)
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
        watch: Arc<StartupWatch>,
        weights_bytes: u64,
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
    /// Requests the live engine has accepted and not finished: `/v1/status`'s
    /// `num_running + num_waiting`. Measured 2026-09-02 on Rapid-MLX 0.13.1: an admitted
    /// request sits in `waiting` (running still 0) for an instant before its first batch
    /// slot, so running alone would call a loaded node idle. `None` means the engine did
    /// not report it — never a defaulted 0; `active_requests_error` says why.
    pub active_requests: Option<u32>,
    /// Why `active_requests` is `None` on a running engine: the `/v1/status` probe failed
    /// or its body lacked the counts. Kept apart from `probe_error` (the `/v1/models`
    /// probe) so a served id and a missing busy fact are two visible facts, not one.
    pub active_requests_error: Option<String>,
    pub gate_message: Option<String>,
    pub gate_verdict: Option<String>,
    /// Set when the manager supervises nothing but SOMETHING already listens on the
    /// configured port — an engine orphaned by a previous goosed. `unmount` reclaims it.
    pub stray_listener_port: Option<u16>,
    /// Free pages plus reclaimable file cache (`memory::measure`); 0 exactly when
    /// `memory_error` says the measurement failed.
    pub available_memory_gb: f64,
    pub total_memory_gb: f64,
    /// The part of `available_memory_gb` that is file cache the OS reclaims on demand.
    /// `None` where the platform source does not split it out (Linux).
    pub reclaimable_cache_gb: Option<f64>,
    /// Why the memory figures above are 0: the OS memory probe failed.
    pub memory_error: Option<String>,
    pub restart_required: bool,
    pub last_error: Option<String>,
    /// While a mount is in flight: how far the load has come. `None` otherwise.
    pub load: Option<EngineLoad>,
}

/// A mount in flight, measured — never a guessed percentage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineLoad {
    /// "makingRoom" (macOS is asked to reclaim memory before the gate judges again) |
    /// "starting" (the process runs; the engine has not said it is loading) | "loading" |
    /// "warming" (weights in; compiling kernels before it answers).
    pub phase: String,
    /// Resident bytes of the engine process (see `StartupWatch`); `None` before it exists.
    pub resident_bytes: Option<u64>,
    /// The model's bytes on disk — what a finished load holds (measured 0.985–0.994×).
    pub weights_bytes: u64,
}

/// Rapid-MLX's own words for where its start is (v0.14.3-lz.4, stderr, measured 2026-09-24):
/// "Loading model with BatchedEngine" / "Loading MLLM" open the weight load, "Warming up
/// (compiling Metal shaders)" the warm-up. Before either, the process is starting (uv resolving,
/// python importing MLX).
pub fn start_phase(stderr_tail: &[String]) -> &'static str {
    for line in stderr_tail.iter().rev() {
        if line.contains("Warming up") {
            return "warming";
        }
        if line.contains("Loading MLLM") || line.contains("Loading model") {
            return "loading";
        }
    }
    "starting"
}

/// The mount gate refused: the one fit rule's verdict on this Mac, typed so the ACP layer can hand
/// the desktop a structured refusal (and the split that would work) instead of a string.
#[derive(Debug, Clone)]
pub struct MountRefused {
    pub model_id: String,
    pub verdict: FitVerdict,
}

impl std::fmt::Display for MountRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "memory gate BLOCK for '{}': {}",
            self.model_id, self.verdict.message
        )
    }
}

impl std::error::Error for MountRefused {}

#[cfg(unix)]
pub fn local_gpu_ceiling() -> Result<u64> {
    crate::placement::chip::local_gpu_ceiling()
}

#[cfg(not(unix))]
pub fn local_gpu_ceiling() -> Result<u64> {
    anyhow::bail!("the GPU ceiling is read from Metal, which exists only on macOS")
}

/// The single engine's need for `model` — the placement planner's own (`single_engine_need`).
#[cfg(unix)]
fn single_engine_need(dir: &Path, weights_bytes: u64, kv_mode: Option<KvCacheMode>) -> Need {
    let facts = crate::placement::model::read_model_facts(dir).map_err(|e| format!("{e:#}"));
    crate::placement::planner::single_engine_need(
        weights_bytes,
        facts.as_ref().map_err(Clone::clone),
        kv_mode,
    )
}

#[cfg(not(unix))]
fn single_engine_need(_dir: &Path, weights_bytes: u64, _kv_mode: Option<KvCacheMode>) -> Need {
    Need::single_engine(
        weights_bytes,
        Err("model facts are read on macOS only".to_string()),
        0,
    )
}

/// A fixed, standard spawn PATH for the engine process. goosed's own PATH is a grab-bag of
/// goose-internal tool shims — the MCP `mcp-hermit` bootstrap AND the desktop-bundled
/// `ui/desktop/src/bin/uvx` wrapper — and inheriting it resolved `uvx` to those shims twice
/// on 2026-08-31 (a cold hermit python install, then a nested bash chain that never served).
/// A controlled environment makes resolution deterministic; if `uvx` is absent from these
/// standard locations the spawn fails loudly with exactly that message.
pub(crate) fn sidecar_spawn_path() -> String {
    "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string()
}

/// The engine's spawn environment, layered over what goosed inherits: the fixed PATH and the
/// telemetry kill switches Rapid-MLX honours (telemetry/state.py: `RAPID_MLX_TELEMETRY` falsy
/// or `DO_NOT_TRACK` truthy forces it OFF — a local-edition engine reports to no one).
fn sidecar_spawn_env() -> Vec<(String, String)> {
    vec![
        ("PATH".to_string(), sidecar_spawn_path()),
        ("DO_NOT_TRACK".to_string(), "1".to_string()),
        ("RAPID_MLX_TELEMETRY".to_string(), "0".to_string()),
    ]
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
    #[cfg(unix)]
    {
        tracing::warn!(
            port,
            ?pids,
            "reclaiming port from unsupervised engine (SIGTERM)"
        );
        for pid in &pids {
            unsafe { libc::kill(*pid as libc::pid_t, libc::SIGTERM) };
        }
        if crate::wait_port_clear(port).await {
            return;
        }
        tracing::warn!(port, ?pids, "reclaim: grace window expired (SIGKILL)");
        for pid in &pids {
            unsafe { libc::kill(*pid as libc::pid_t, libc::SIGKILL) };
        }
    }
    #[cfg(not(unix))]
    tracing::warn!(
        port,
        ?pids,
        "reclaim: signal delivery needs Unix; port left occupied"
    );
}

/// A supervised engine that ended: what ended, how, and its last words — and the restart
/// policy, stated (the single engine's is the distributed engine's with `restartOnFailure` off:
/// no silent restart; a Mount restarts it, behind the same crash breaker).
fn engine_exit_message(exit: &crate::SidecarExit) -> String {
    let pid = exit
        .pid
        .map(|pid| format!(" (pid {pid})"))
        .unwrap_or_default();
    format!(
        "the engine process{pid} exited: {} — not restarted automatically; Mount restarts it \
         (the crash breaker applies). Last log lines:\n{}",
        exit.status, exit.stderr_tail
    )
}

pub struct MlxEngineManager {
    state: Arc<Mutex<ManagerState>>,
    settings: StdMutex<EngineSettings>,
    last_gate: StdMutex<Option<FitVerdict>>,
    /// The model a mount is making room for (macOS compaction runs before the gate judges again).
    making_room: StdMutex<Option<(String, u64)>>,
    probe_client: reqwest::Client,
    #[cfg(test)]
    test_gpu_ceiling: StdMutex<Option<u64>>,
}

impl MlxEngineManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ManagerState::Stopped)),
            settings: StdMutex::new(EngineSettings::default()),
            last_gate: StdMutex::new(None),
            making_room: StdMutex::new(None),
            probe_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client with static configuration"),
            #[cfg(test)]
            test_gpu_ceiling: StdMutex::new(None),
        }
    }

    fn gpu_ceiling(&self) -> Result<u64> {
        #[cfg(test)]
        if let Some(bytes) = *self.test_gpu_ceiling.lock().unwrap() {
            return Ok(bytes);
        }
        local_gpu_ceiling()
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

    fn local_model(&self, settings: &EngineSettings, model_id: &str) -> Result<LocalModel> {
        hf::validate_model_id(model_id)?;
        let models_dir = expand_tilde(&settings.models_dir);
        let models = hf::list_local_models(&models_dir)?;
        let model = models
            .into_iter()
            .find(|m| m.id == model_id)
            .with_context(|| {
                format!(
                    "model '{model_id}' not found in {} (download it first)",
                    models_dir.display()
                )
            })?;
        Ok(model)
    }

    /// The one fit rule (`crate::fit`) for `model_id` on this Mac right now — the verdict the mount
    /// gate acts on and the desktop draws (`mlxEngine/status` `fitModelId`). A model mounted here
    /// counts as available, as the placement planner counts it: a mount replaces it (or keeps it,
    /// when it is this model).
    pub async fn mount_fit(&self, model_id: &str) -> Result<FitVerdict> {
        let settings = self.settings();
        let model = self.local_model(&settings, model_id)?;
        self.judge_model(&settings, &model).await
    }

    async fn judge_model(
        &self,
        settings: &EngineSettings,
        model: &LocalModel,
    ) -> Result<FitVerdict> {
        let reading = measure()?;
        let ceiling = self
            .gpu_ceiling()
            .context("reading the GPU ceiling the fit rule needs")?;
        let (freed, note) = self.mounted_footprint(settings).await;
        let need = single_engine_need(
            &expand_tilde(&settings.models_dir).join(&model.id),
            model.size_bytes,
            settings
                .model_profiles
                .get(&model.id)
                .and_then(|p| p.kv_cache),
        );
        let mut verdict = fit::judge(
            need,
            NodeMemoryFacts {
                available_bytes: reading.available_bytes.saturating_add(freed),
                total_bytes: reading.total_bytes,
                ceiling_bytes: ceiling,
            },
        );
        if let Some(note) = note {
            verdict.append(note);
        }
        Ok(verdict)
    }

    /// The running engine's resident bytes (what a mount gets back), with the sentence that says
    /// so — or 0 and the sentence that says why it could not be counted.
    async fn mounted_footprint(&self, settings: &EngineSettings) -> (u64, Option<String>) {
        let mounted = match &*self.state.lock().await {
            ManagerState::Running { model_id, .. } => model_id.clone(),
            _ => return (0, None),
        };
        #[cfg(unix)]
        let read = crate::placement::engine_resident_bytes(settings.port).await;
        #[cfg(not(unix))]
        let read: Result<u64> = Err(anyhow::anyhow!("process footprints are read on unix only"));
        match read {
            Ok(bytes) => (
                bytes,
                Some(format!(
                    "{mounted} is mounted here: its {:.1} GiB count as available, the mount \
                     replaces it",
                    bytes as f64 / GIB as f64
                )),
            ),
            Err(e) => (
                0,
                Some(format!(
                    "{mounted} is mounted here but its memory could not be read ({e:#}); it \
                     counts as in use"
                )),
            ),
        }
    }

    /// Validate the model and the one fit rule, flip to `Mounting`, and return; the engine start
    /// continues in a spawned task. A `Block` refuses the mount with [`MountRefused`] (after Make
    /// room had its chance). Any already-running engine is shut down first — one model per engine
    /// — and once this manager supervises nothing, a listener still on the port is somebody
    /// else's: the mount is refused with [`UnsupervisedListenerError`] rather than started over it.
    pub async fn mount(&self, model_id: &str) -> Result<()> {
        let settings = self.settings();
        let model = self.local_model(&settings, model_id)?;
        ensure!(
            model.complete,
            "model '{model_id}' is incomplete: a .part file remains or no .safetensors is present"
        );

        #[allow(unused_mut)]
        let mut gate = self.judge_model(&settings, &model).await?;
        #[cfg(unix)]
        if gate.verdict == Verdict::Block {
            gate = self.make_room_then_gate(&settings, &model, gate).await?;
        }
        *self.last_gate.lock().unwrap() = Some(gate.clone());
        if gate.verdict == Verdict::Block {
            return Err(MountRefused {
                model_id: model_id.to_string(),
                verdict: gate,
            }
            .into());
        }
        let weights_bytes = model.size_bytes;

        let argv = build_serve_command(&settings, model_id)?;
        let mut state = self.state.lock().await;
        if let ManagerState::Mounting {
            model_id: current, ..
        } = &*state
        {
            bail!("mount already in progress for '{current}'");
        }
        let watch = Arc::new(StartupWatch::default());
        let previous = std::mem::replace(
            &mut *state,
            ManagerState::Mounting {
                model_id: model_id.to_string(),
                watch: Arc::clone(&watch),
                weights_bytes,
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
                    config.env = sidecar_spawn_env();
                    config.startup_watch = Some(watch);
                    Sidecar::start(config).await.map(Box::new)
                }
            };
            match started {
                Ok(sidecar) => {
                    let mut state = state_arc.lock().await;
                    let still_mounting = matches!(
                        &*state,
                        ManagerState::Mounting { model_id: current, .. } if *current == model_id
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
                        ManagerState::Mounting { model_id: current, .. } if *current == model_id
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

    /// The gate said the model does not fit: when nothing of ours is loaded on this Mac and some
    /// amount of reclaimed memory could let it through, ask macOS to reclaim memory
    /// (`distributed::compaction`: pressure to the kernel's WARN, released at once, settled on
    /// progress) and judge again on the new reading. The outcome — freed, refused beside a loaded
    /// engine, or failed — is appended to the verdict's message either way, so the refusal the
    /// owner reads says what was tried. Status reports the mount as `makingRoom` meanwhile.
    #[cfg(unix)]
    async fn make_room_then_gate(
        &self,
        settings: &EngineSettings,
        model: &LocalModel,
        blocked: FitVerdict,
    ) -> Result<FitVerdict> {
        use crate::distributed::compaction::{compact_node, CompactionOutcome};
        let loaded = matches!(
            &*self.state.lock().await,
            ManagerState::Running { .. } | ManagerState::Mounting { .. }
        );
        if loaded || !blocked.could_ever_fit() {
            return Ok(blocked);
        }
        *self.making_room.lock().unwrap() = Some((model.id.clone(), model.size_bytes));
        let outcome = compact_node(&crate::distributed::SystemExec, None, "this Mac").await;
        *self.making_room.lock().unwrap() = None;
        Ok(match outcome {
            Ok(CompactionOutcome::Compacted(report)) => {
                let mut verdict = self.judge_model(settings, model).await?;
                verdict.append(format!("Make room ran first: {}", report.summary()));
                verdict
            }
            Ok(CompactionOutcome::Refused(refusal)) => {
                let mut verdict = blocked;
                verdict.append(format!(
                    "Make room did not run ({}): {}",
                    refusal.code, refusal.message
                ));
                verdict
            }
            Err(e) => {
                let mut verdict = blocked;
                verdict.append(format!("Make room failed: {e:#}"));
                verdict
            }
        })
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
        let (reading, memory_error) = match measure() {
            Ok(reading) => (Some(reading), None),
            Err(e) => (None, Some(format!("{e:#}"))),
        };
        let gib_of = |bytes: u64| bytes as f64 / GIB as f64;
        let (gate_message, gate_verdict) = match self.last_gate.lock().unwrap().clone() {
            Some(g) => (Some(g.message), Some(g.verdict.as_str().to_string())),
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
            active_requests: None,
            active_requests_error: None,
            gate_message,
            gate_verdict,
            stray_listener_port: None,
            available_memory_gb: reading.map_or(0.0, |r| gib_of(r.available_bytes)),
            total_memory_gb: reading.map_or(0.0, |r| gib_of(r.total_bytes)),
            reclaimable_cache_gb: reading.and_then(|r| r.reclaimable_cache_bytes.map(gib_of)),
            memory_error,
            restart_required: false,
            last_error: None,
            load: None,
        };

        let running = {
            let state = self.state.lock().await;
            match &*state {
                ManagerState::Stopped => None,
                ManagerState::Mounting {
                    model_id,
                    watch,
                    weights_bytes,
                } => {
                    status.state = "mounting".to_string();
                    status.model_id = Some(model_id.clone());
                    let seen = watch.seen();
                    status.load = Some(EngineLoad {
                        phase: start_phase(&seen.stderr_tail).to_string(),
                        resident_bytes: seen.resident_bytes,
                        weights_bytes: *weights_bytes,
                    });
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
                    status.model_id = Some(model_id.clone());
                    // The state is the PROCESS's, asked of the OS on every poll — never the
                    // flag the mount left (measured 2026-09-24: the engine SIGKILLed, its uvx
                    // launcher a zombie of goosed, status said `running` for minutes). The
                    // supervisor is kept, so a Mount of the same model restarts it through
                    // the crash breaker; nothing restarts on its own.
                    match sidecar.exited().await {
                        Ok(Some(exit)) => {
                            status.state = "failed".to_string();
                            status.last_error = Some(engine_exit_message(&exit));
                            None
                        }
                        Ok(None) => {
                            status.state = "running".to_string();
                            status.base_url = Some(sidecar.base_url().to_string());
                            status.pid = sidecar.pid().await;
                            Some((model_id.clone(), argv.clone()))
                        }
                        Err(e) => {
                            status.state = "failed".to_string();
                            status.last_error =
                                Some(format!("the engine process cannot be observed: {e:#}"));
                            None
                        }
                    }
                }
            }
        };

        if let Some((model_id, weights_bytes)) = self.making_room.lock().unwrap().clone() {
            status.state = "mounting".to_string();
            status.model_id = Some(model_id);
            status.load = Some(EngineLoad {
                phase: "makingRoom".to_string(),
                resident_bytes: None,
                weights_bytes,
            });
        }
        if running.is_none() && port_has_listener(settings.port) {
            status.stray_listener_port = Some(settings.port);
        }
        if let Some((running_model, running_argv)) = running {
            let desired_model = settings.model_id.as_deref().unwrap_or(&running_model);
            // The desired argv is rebuilt from the profile AND the model directory each poll,
            // so an MTP head that arrived, a vision checkpoint swapped in, or an adapter dir
            // that vanished all flip this. An adapter the profile names but that no longer
            // validates cannot be mounted as configured — that IS a restart-required fact,
            // and the remount says exactly what is missing.
            status.restart_required = match build_serve_command(&settings, desired_model) {
                Ok(desired) => desired != running_argv,
                Err(e) => {
                    tracing::warn!(model = desired_model, error = %format!("{e:#}"), "desired serve argv cannot be built; reporting restart required");
                    true
                }
            };
            let base_url = status.base_url.as_deref().expect("set for running state");
            let (model_info, active_requests) = tokio::join!(
                self.probe_model_info(base_url),
                self.probe_active_requests(base_url)
            );
            match model_info {
                Ok((served_model_id, context_window, tool_call_parser)) => {
                    status.served_model_id = served_model_id;
                    status.context_window = context_window;
                    status.tool_call_parser = tool_call_parser;
                }
                Err(e) => status.probe_error = Some(format!("{e:#}")),
            }
            match active_requests {
                Ok(count) => status.active_requests = Some(count),
                Err(e) => status.active_requests_error = Some(format!("{e:#}")),
            }
        }
        status
    }

    /// GET `/v1/status` — Rapid-MLX 0.13.1 `routes/health.py:304`, on the router that is
    /// auth-gated only when the engine was given an API key (the serve argv passes none).
    /// The same probe client and cadence as `/v1/models`; no clock of its own.
    async fn probe_active_requests(&self, base_url: &str) -> Result<u32> {
        let url = format!("{base_url}/v1/status");
        let resp = self
            .probe_client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        let status = resp.status();
        let body = resp.text().await.context("reading /v1/status body")?;
        ensure!(status.is_success(), "GET {url} returned HTTP {status}");
        parse_active_requests(&body)
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
        parse_model_info(&body)
    }
}

/// The first `/v1/models` entry as (served id, context_window, tool_call_parser) — the sidecar
/// serves one model, so the first entry is the model. Shared with every out-of-process reader of
/// the same catalog (the swarm chat router), so one parse rule describes the engine.
pub fn parse_model_info(body: &str) -> Result<(Option<String>, Option<u64>, Option<String>)> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).context("parsing /v1/models body")?;
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

impl Default for MlxEngineManager {
    fn default() -> Self {
        Self::new()
    }
}

/// The in-flight count from a `/v1/status` body: `num_running` (the scheduler's running
/// batch, `scheduler.py:8251 len(self.running)`) plus `num_waiting` (its admitted queue).
/// Both keys must be present as non-negative integers: a body without them is an engine
/// that does not report the fact, and the error names the missing key instead of reading 0.
pub fn parse_active_requests(body: &str) -> Result<u32> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).context("parsing /v1/status body")?;
    let count = |key: &str| -> Result<u32> {
        let value = parsed.get(key).with_context(|| {
            format!(
                "/v1/status body has no `{key}`: {}",
                body.chars().take(200).collect::<String>()
            )
        })?;
        let n = value.as_u64().with_context(|| {
            format!("/v1/status `{key}` is not a non-negative integer: {value}")
        })?;
        u32::try_from(n).with_context(|| format!("/v1/status `{key}` overflows u32: {n}"))
    };
    let running = count("num_running")?;
    let waiting = count("num_waiting")?;
    running
        .checked_add(waiting)
        .context("/v1/status num_running + num_waiting overflows u32")
}

pub fn global_manager() -> &'static MlxEngineManager {
    static MANAGER: OnceLock<MlxEngineManager> = OnceLock::new();
    MANAGER.get_or_init(MlxEngineManager::new)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_superseded_default_launcher_follows_the_current_default() {
        let mut settings = super::EngineSettings {
            spawn_command: super::SUPERSEDED_ENGINE_LAUNCHERS[0]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            ..Default::default()
        };
        let replaced = settings
            .migrate_launcher()
            .expect("the old default migrates");
        assert!(replaced[2].ends_with("@v0.13.1"), "{replaced:?}");
        assert_eq!(
            settings.spawn_command,
            super::ENGINE_LAUNCHER
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        assert!(
            settings.migrate_launcher().is_none(),
            "a second pass changes nothing"
        );
    }

    #[test]
    fn an_owner_edited_launcher_is_never_migrated() {
        let own = vec!["/opt/engines/rapid-mlx".to_string()];
        let mut settings = super::EngineSettings {
            spawn_command: own.clone(),
            ..Default::default()
        };
        assert!(settings.migrate_launcher().is_none());
        assert_eq!(settings.spawn_command, own);
    }

    use super::*;

    /// Metal exists only on macOS. On a Linux CI host the fit rule's ceiling is the host's whole
    /// RAM, so the RAM budget alone decides and the mount lifecycle under test is unchanged; on a
    /// Mac the real Metal ceiling is read.
    fn test_manager() -> MlxEngineManager {
        let manager = MlxEngineManager::new();
        #[cfg(not(target_os = "macos"))]
        {
            *manager.test_gpu_ceiling.lock().unwrap() = Some(measure().unwrap().total_bytes);
        }
        manager
    }

    /// Rapid-MLX v0.14.3-lz.4's own stderr on a 27B mount (2026-09-24), in order.
    #[test]
    fn the_start_phase_is_the_engines_own_words() {
        let lines = [
            "INFO:rapid_mlx.gdn_prefill:[gdn_prefill] blocked-seq GDN prefill kernel installed",
            "INFO:rapid_mlx.server:Loading model with BatchedEngine: /m",
            "INFO:rapid_mlx.models.mllm:Loading MLLM: /m",
            "INFO:rapid_mlx.models.mllm:MLLM loaded successfully: /m",
            "INFO:rapid_mlx.server:Warming up (compiling Metal shaders)...",
            "INFO:rapid_mlx.server:Warmup complete (0.0s)",
        ]
        .map(String::from);
        assert_eq!(start_phase(&lines[..1]), "starting");
        assert_eq!(start_phase(&lines[..2]), "loading");
        assert_eq!(start_phase(&lines[..4]), "loading");
        assert_eq!(start_phase(&lines), "warming");
        assert_eq!(start_phase(&[]), "starting");
    }

    #[test]
    fn serve_command_uses_served_model_name_alias_when_set() {
        let settings = EngineSettings {
            model_id: Some("mlx-community/Qwen3.5-9B-MLX-4bit".to_string()),
            served_model_name: Some("workhorse-qwen3.5-9b-4bit-mlx".to_string()),
            ..Default::default()
        };
        let argv = build_serve_command(&settings, "mlx-community/Qwen3.5-9B-MLX-4bit").unwrap();
        let pos = argv
            .iter()
            .position(|a| a == "--served-model-name")
            .unwrap();
        assert_eq!(argv[pos + 1], "workhorse-qwen3.5-9b-4bit-mlx");
        assert!(argv
            .iter()
            .any(|a| a.ends_with("mlx-community/Qwen3.5-9B-MLX-4bit")));
    }

    /// The alias names ONE model. Live 2026-09-24: a Flash split served under the 27B's alias
    /// because the alias applied to any model id handed in.
    #[test]
    fn the_served_alias_belongs_to_its_own_model_only() {
        let settings = EngineSettings {
            model_id: Some("Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string()),
            served_model_name: Some("mihai-qwen3.8-27b-atlassian-q8-mlx".to_string()),
            ..Default::default()
        };
        assert_eq!(
            served_model_id(&settings, "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx"),
            "mihai-qwen3.8-27b-atlassian-q8-mlx"
        );
        assert_eq!(
            served_model_id(&settings, "rapid-mlx/Qwen3.8-Flash-Next-4bit"),
            "rapid-mlx/Qwen3.8-Flash-Next-4bit"
        );
        let argv = build_serve_command(&settings, "rapid-mlx/Qwen3.8-Flash-Next-4bit").unwrap();
        let pos = argv
            .iter()
            .position(|a| a == "--served-model-name")
            .unwrap();
        assert_eq!(argv[pos + 1], "rapid-mlx/Qwen3.8-Flash-Next-4bit");
        let no_model = EngineSettings {
            served_model_name: Some("orphan-alias".to_string()),
            ..Default::default()
        };
        assert_eq!(served_model_id(&no_model, "pub/small"), "pub/small");
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
            ..Default::default()
        }
    }

    /// The receipt behind `PREFIX_CACHE_SHARE_OF_FREE`, in the Studio's measured bytes (M3 Ultra
    /// 96 GB serving Qwen3.8-27B Q8, /v1/status 2026-09-25 after one 49,016-token agent turn):
    /// the share must hold two measured turns, and with the cache full, one cold 49k prefill's
    /// peak and two more live 49k sequences the engine must stay under its OWN pressure-eviction
    /// line — above it the engine sheds prefix entries and calls `mx.clear_cache()` per entry,
    /// the eviction churn of the IOGPU panic recipe (research D3).
    #[test]
    fn the_prefix_cache_share_holds_two_turns_under_the_pressure_line() {
        const ENGINE_DEFAULT_SHARE: f64 = 0.20;
        const DEFAULT_BUDGET: f64 = 12_512_444_416.0;
        const ONE_TURN: f64 = 9_765_978_112.0;
        const METAL_ACTIVE: f64 = 40.21e9;
        const PREFILL_PEAK_RISE: f64 = 46.35e9 - 40.21e9;
        const M3_ULTRA_CEILING: f64 = 83_494_174_720.0;
        // Rapid-MLX's --gpu-memory-utilization default and its metal_pressure_evict_fraction.
        const ENGINE_CAP_SHARE: f64 = 0.90;
        const PRESSURE_EVICT_SHARE: f64 = 0.90;
        // Qwen3.8-27B: 16 full-attention layers × 4 KV heads × 256 head dim × K,V × bf16, and
        // 48 GatedDeltaNet layers × (48 × 128 × 128 f32 state + 3 × 10,240 bf16 conv state).
        const KV_BYTES_PER_TOKEN: f64 = 16.0 * 4.0 * 256.0 * 2.0 * 2.0;
        const RECURRENT_STATE: f64 = 48.0 * (48.0 * 128.0 * 128.0 * 4.0 + 3.0 * 10_240.0 * 2.0);
        const PROMPT_TOKENS: f64 = 49_016.0;

        let free_after_load = DEFAULT_BUDGET / ENGINE_DEFAULT_SHARE;
        let base = METAL_ACTIVE - ONE_TURN;
        let live_sequence = PROMPT_TOKENS * KV_BYTES_PER_TOKEN + RECURRENT_STATE;
        let pressure_line = M3_ULTRA_CEILING * ENGINE_CAP_SHARE * PRESSURE_EVICT_SHARE;
        let cache = PREFIX_CACHE_SHARE_OF_FREE * free_after_load;

        assert_eq!(
            KV_BYTES_PER_TOKEN, 65_536.0,
            "the fit rule's own 27B figure"
        );
        assert!(
            cache >= 2.0 * ONE_TURN,
            "{cache:.3e} B cannot hold two measured {ONE_TURN:.3e} B turns"
        );
        let worst = base + cache + PREFILL_PEAK_RISE + 2.0 * live_sequence;
        assert!(
            worst <= pressure_line,
            "{worst:.3e} B crosses the engine's pressure-eviction line {pressure_line:.3e} B"
        );
        let floor = 2.0 * ONE_TURN / free_after_load;
        let ceiling =
            (pressure_line - base - PREFILL_PEAK_RISE - 2.0 * live_sequence) / free_after_load;
        assert!(
            (0.31..0.32).contains(&floor) && (0.38..0.39).contains(&ceiling),
            "the admissible band moved: [{floor:.3}, {ceiling:.3}]"
        );
    }

    #[test]
    fn the_hybrid_entry_count_keeps_a_full_admission_window() {
        assert_eq!(hybrid_cache_entries(), 24);
        assert_eq!(
            hybrid_cache_entries(),
            MAX_CONCURRENT_REQUESTS * PREFIX_ENTRIES_PER_REQUEST
        );
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
        let argv = build_serve_command(&settings, "mlx-community/Qwen3.5-9B-MLX-4bit").unwrap();
        assert_eq!(
            argv,
            vec![
                "uvx",
                "--from",
                "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.5",
                "rapid-mlx",
                "serve",
                "/opt/models/mlx-community/Qwen3.5-9B-MLX-4bit",
                "--port",
                "8090",
                "--served-model-name",
                "mlx-community/Qwen3.5-9B-MLX-4bit",
                "--enable-prefix-cache",
                "--cache-memory-percent",
                "0.35",
                "--hybrid-cache-entries",
                "24",
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
        let argv = build_serve_command(&settings, "pub/model").unwrap();
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
        let argv = build_serve_command(&settings, "pub/model").unwrap();
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
        let alpha = build_serve_command(&settings, "pub/alpha").unwrap();
        let beta = build_serve_command(&settings, "pub/beta").unwrap();

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

    // -----------------------------------------------------------------------
    // Serving-lane flags read from the model DIRECTORY: MTP head, vision config, adapter.
    // -----------------------------------------------------------------------

    /// A models dir holding `pub/model` with the given files; `config.json` is always
    /// written (list_local_models requires it) with the given body.
    fn model_dir_with(
        config_json: &str,
        extra_files: &[&str],
    ) -> (tempfile::TempDir, EngineSettings) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("pub").join("model");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), config_json).unwrap();
        for name in extra_files {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let settings = EngineSettings {
            models_dir: root.path().to_string_lossy().into_owned(),
            ..Default::default()
        };
        (root, settings)
    }

    /// The exact config.json shape of mlx-community/Qwen3.5-9B-MLX-4bit and the
    /// lmstudio-community Qwen3.8-27B-MLX-8bit artefact (read 2026-09-06): qwen3_5,
    /// Qwen3_5ForConditionalGeneration, a vision_config block.
    const QWEN3_5_VISION_CONFIG: &str = r#"{"model_type":"qwen3_5","architectures":["Qwen3_5ForConditionalGeneration"],"vision_config":{"depth":27},"text_config":{"hidden_size":4096}}"#;
    const PLAIN_TEXT_CONFIG: &str =
        r#"{"model_type":"qwen3","architectures":["Qwen3ForCausalLM"]}"#;

    fn flag_value(argv: &[String], flag: &str) -> Option<String> {
        argv.iter()
            .position(|a| a == flag)
            .map(|i| argv[i + 1].clone())
    }

    #[test]
    fn mtp_head_plus_vision_config_yields_speculative_config_with_the_dir_and_text_only() {
        let (root, settings) = model_dir_with(QWEN3_5_VISION_CONFIG, &[MTP_SIDECAR_FILE]);
        let argv = build_serve_command(&settings, "pub/model").unwrap();
        let dir = root.path().join("pub").join("model");
        let expected_json = format!(
            r#"{{"method":"mtp","model":"{}","num_speculative_tokens":3}}"#,
            dir.display()
        );
        assert_eq!(
            flag_value(&argv, "--speculative-config"),
            Some(expected_json.clone()),
            "{argv:?}"
        );
        // The JSON is one argv element (no shell — the JSON's quotes are literal).
        let json: serde_json::Value = serde_json::from_str(&expected_json).unwrap();
        assert_eq!(json["method"], "mtp");
        assert_eq!(json["model"], dir.to_string_lossy().as_ref());
        assert_eq!(json["num_speculative_tokens"], 3);
        assert!(argv.iter().any(|a| a == "--text-only"), "{argv:?}");
        assert!(!argv.iter().any(|a| a == "--adapter-path"), "{argv:?}");
        // The lane flags come AFTER the base argv, which is unchanged.
        let base = build_serve_command(
            &EngineSettings {
                models_dir: settings.models_dir.clone(),
                ..Default::default()
            },
            "pub/model",
        )
        .unwrap();
        assert_eq!(&argv[..base.len() - 1], &base[..base.len() - 1]);
        assert_eq!(
            &argv[17..],
            &[
                "--speculative-config".to_string(),
                expected_json,
                "--text-only".to_string()
            ]
        );
    }

    #[test]
    fn speculative_off_suppresses_mtp_even_with_the_head_present() {
        let (_root, mut settings) = model_dir_with(PLAIN_TEXT_CONFIG, &[MTP_SIDECAR_FILE]);
        settings.model_profiles.insert(
            "pub/model".to_string(),
            ModelProfile {
                speculative: Some("off".to_string()),
                ..Default::default()
            },
        );
        let argv = build_serve_command(&settings, "pub/model").unwrap();
        assert!(
            !argv.iter().any(|a| a == "--speculative-config"),
            "{argv:?}"
        );
        assert!(!argv.iter().any(|a| a == "--text-only"), "{argv:?}");
    }

    #[test]
    fn the_spawn_env_turns_engine_telemetry_off() {
        let env: BTreeMap<String, String> = sidecar_spawn_env().into_iter().collect();
        assert_eq!(env["PATH"], sidecar_spawn_path());
        assert_eq!(env["DO_NOT_TRACK"], "1");
        assert_eq!(env["RAPID_MLX_TELEMETRY"], "0");
    }

    #[test]
    fn speculative_mtp_without_the_head_file_is_skipped_not_fatal() {
        let (_root, mut settings) = model_dir_with(PLAIN_TEXT_CONFIG, &[]);
        settings.model_profiles.insert(
            "pub/model".to_string(),
            ModelProfile {
                speculative: Some("mtp".to_string()),
                ..Default::default()
            },
        );
        let argv = build_serve_command(&settings, "pub/model").unwrap();
        assert!(
            !argv.iter().any(|a| a == "--speculative-config"),
            "{argv:?}"
        );
    }

    #[test]
    fn text_only_false_lets_a_vision_checkpoint_take_the_mllm_lane() {
        let (_root, mut settings) = model_dir_with(QWEN3_5_VISION_CONFIG, &[]);
        settings.model_profiles.insert(
            "pub/model".to_string(),
            ModelProfile {
                text_only: Some(false),
                ..Default::default()
            },
        );
        let argv = build_serve_command(&settings, "pub/model").unwrap();
        assert!(!argv.iter().any(|a| a == "--text-only"), "{argv:?}");
    }

    #[test]
    fn a_qwen3_5_conditional_generation_config_without_vision_config_still_pins_the_text_lane() {
        let cfg = r#"{"model_type":"qwen3_5","architectures":["Qwen3_5ForConditionalGeneration"]}"#;
        let (_root, settings) = model_dir_with(cfg, &[]);
        let argv = build_serve_command(&settings, "pub/model").unwrap();
        assert!(argv.iter().any(|a| a == "--text-only"), "{argv:?}");
        let facts = inspect_model_dir(&expand_tilde(&settings.models_dir).join("pub/model"));
        assert_eq!(
            facts,
            ModelDirFacts {
                has_mtp_sidecar: false,
                vision_bearing: true
            }
        );
    }

    #[test]
    fn a_valid_adapter_dir_reaches_argv_expanded() {
        let (root, mut settings) = model_dir_with(PLAIN_TEXT_CONFIG, &[]);
        let adapter = root.path().join("lora");
        std::fs::create_dir_all(&adapter).unwrap();
        for name in ADAPTER_REQUIRED_FILES {
            std::fs::write(adapter.join(name), b"x").unwrap();
        }
        settings.model_profiles.insert(
            "pub/model".to_string(),
            ModelProfile {
                adapter_path: Some(adapter.to_string_lossy().into_owned()),
                ..Default::default()
            },
        );
        let argv = build_serve_command(&settings, "pub/model").unwrap();
        assert_eq!(
            flag_value(&argv, "--adapter-path"),
            Some(adapter.to_string_lossy().into_owned()),
            "{argv:?}"
        );
    }

    #[test]
    fn an_adapter_dir_missing_its_files_fails_the_build_naming_them() {
        let (root, mut settings) = model_dir_with(PLAIN_TEXT_CONFIG, &[]);
        let adapter = root.path().join("lora");
        std::fs::create_dir_all(&adapter).unwrap();
        std::fs::write(adapter.join("adapter_config.json"), b"{}").unwrap();
        settings.model_profiles.insert(
            "pub/model".to_string(),
            ModelProfile {
                adapter_path: Some(adapter.to_string_lossy().into_owned()),
                ..Default::default()
            },
        );
        let err = build_serve_command(&settings, "pub/model").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("adapters.safetensors"), "{msg}");
        assert!(!msg.contains("missing adapter_config.json"), "{msg}");

        let missing_dir = root.path().join("nope");
        settings
            .model_profiles
            .get_mut("pub/model")
            .unwrap()
            .adapter_path = Some(missing_dir.to_string_lossy().into_owned());
        let err = build_serve_command(&settings, "pub/model").unwrap_err();
        assert!(format!("{err:#}").contains("not a directory"), "{err:#}");

        // A blank adapter path is "none", never an error.
        settings
            .model_profiles
            .get_mut("pub/model")
            .unwrap()
            .adapter_path = Some("   ".to_string());
        let argv = build_serve_command(&settings, "pub/model").unwrap();
        assert!(!argv.iter().any(|a| a == "--adapter-path"), "{argv:?}");
    }

    /// A plain text checkpoint (no MTP head, no vision config, no adapter) mounts with the
    /// argv this crate produced before the lane flags existed — byte for byte, plus the prefix
    /// cache's two sizing flags, which every model carries.
    #[test]
    fn a_plain_model_dir_keeps_the_pre_lane_argv_byte_identical() {
        let (root, settings) = model_dir_with(PLAIN_TEXT_CONFIG, &[]);
        let argv = build_serve_command(&settings, "pub/model").unwrap();
        let model_path = root.path().join("pub").join("model");
        assert_eq!(
            argv,
            vec![
                "uvx",
                "--from",
                "rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@v0.14.3-lz.5",
                "rapid-mlx",
                "serve",
                &model_path.to_string_lossy(),
                "--port",
                "8090",
                "--served-model-name",
                "pub/model",
                "--enable-prefix-cache",
                "--cache-memory-percent",
                "0.35",
                "--hybrid-cache-entries",
                "24",
                "--max-concurrent-requests",
                "8",
            ]
        );
    }

    #[test]
    fn a_persisted_profile_without_the_lane_fields_loads_as_auto() {
        let json = r#"{"temperature":0.7,"top_k":40}"#;
        let profile: ModelProfile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.speculative, None);
        assert_eq!(profile.adapter_path, None);
        assert_eq!(profile.text_only, None);
        assert_eq!(profile.temperature, Some(0.7));
        let full: ModelProfile = serde_json::from_str(
            r#"{"speculative":"mtp","adapter_path":"~/lora","text_only":false}"#,
        )
        .unwrap();
        assert_eq!(full.speculative.as_deref(), Some("mtp"));
        assert_eq!(full.adapter_path.as_deref(), Some("~/lora"));
        assert_eq!(full.text_only, Some(false));
        assert!(!full.is_empty());
        assert_eq!(profile.thinking, None);
        assert_eq!(profile.reasoning_effort, None);
    }

    #[test]
    fn thinking_fields_round_trip_and_never_touch_the_argv() {
        let profile: ModelProfile =
            serde_json::from_str(r#"{"thinking":"on","reasoning_effort":"low"}"#).unwrap();
        assert_eq!(profile.thinking, Some(ThinkingMode::On));
        assert_eq!(profile.reasoning_effort.as_deref(), Some("low"));
        assert_eq!(
            serde_json::to_value(&profile).unwrap()["thinking"],
            serde_json::json!("on")
        );
        assert!(serde_json::from_str::<ModelProfile>(r#"{"thinking":"auto"}"#).is_err());

        let model = "pub/model";
        let mut settings = EngineSettings {
            model_id: Some(model.to_string()),
            ..Default::default()
        };
        let before = build_serve_command(&settings, model).unwrap();
        settings.model_profiles.insert(model.to_string(), profile);
        assert_eq!(build_serve_command(&settings, model).unwrap(), before);
    }

    /// The owner's 27B shape as far as the KV facts read it: 16 of 64 layers full attention.
    const QWEN3_5_KV_CONFIG: &str = r#"{"model_type":"qwen3_5","text_config":{"num_hidden_layers":64,"num_attention_heads":24,"num_key_value_heads":4,"head_dim":256,"full_attention_interval":4,"dtype":"bfloat16"}}"#;

    #[test]
    fn a_kv_cache_choice_reaches_the_argv_as_the_engine_dtype_and_off_sends_nothing() {
        let (_root, mut settings) = model_dir_with(QWEN3_5_KV_CONFIG, &[]);
        let off = build_serve_command(&settings, "pub/model").unwrap();
        assert!(!off.iter().any(|a| a == "--kv-cache-dtype"), "{off:?}");
        for (mode, dtype) in [(KvCacheMode::Int8, "int8"), (KvCacheMode::Int4, "int4")] {
            settings.model_profiles.insert(
                "pub/model".to_string(),
                ModelProfile {
                    kv_cache: Some(mode),
                    ..Default::default()
                },
            );
            let argv = build_serve_command(&settings, "pub/model").unwrap();
            assert_eq!(
                flag_value(&argv, "--kv-cache-dtype").as_deref(),
                Some(dtype)
            );
            assert_eq!(&argv[..off.len()], &off[..], "the flag only appends");
        }
    }

    #[test]
    fn a_kv_cache_the_model_cannot_take_fails_the_build_naming_why() {
        let (_root, mut settings) = model_dir_with(
            r#"{"num_hidden_layers":2,"num_attention_heads":4,"num_key_value_heads":4,"head_dim":80,"dtype":"bfloat16"}"#,
            &[],
        );
        settings.model_profiles.insert(
            "pub/model".to_string(),
            ModelProfile {
                kv_cache: Some(KvCacheMode::Int8),
                ..Default::default()
            },
        );
        let err = build_serve_command(&settings, "pub/model").unwrap_err();
        assert!(format!("{err:#}").contains("head_dim 80"), "{err:#}");
    }

    #[test]
    fn kv_cache_round_trips_lowercase_and_a_profile_without_it_loads_as_off() {
        let profile: ModelProfile = serde_json::from_str(r#"{"kv_cache":"int4"}"#).unwrap();
        assert_eq!(profile.kv_cache, Some(KvCacheMode::Int4));
        assert_eq!(
            serde_json::to_value(&profile).unwrap()["kv_cache"],
            serde_json::json!("int4")
        );
        assert!(serde_json::from_str::<ModelProfile>(r#"{"kv_cache":"bf16"}"#).is_err());
        let legacy: ModelProfile = serde_json::from_str(r#"{"temperature":0.7}"#).unwrap();
        assert_eq!(legacy.kv_cache, None);
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
        let running_argv = build_serve_command(&settings, mounted).unwrap();

        settings.model_profiles.insert(
            "pub/other".to_string(),
            ModelProfile {
                temperature: Some(0.1),
                top_k: Some(5),
                ..Default::default()
            },
        );
        assert_eq!(
            build_serve_command(&settings, mounted).unwrap(),
            running_argv,
            "a different model's profile edit must not require a restart"
        );

        settings
            .model_profiles
            .get_mut(mounted)
            .unwrap()
            .temperature = Some(0.9);
        assert_ne!(
            build_serve_command(&settings, mounted).unwrap(),
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
                "git+https://github.com/leanzero-srl/Rapid-MLX@v0.13.4-lz.1",
                "rapid-mlx"
            ]
        }"#;
        let mut settings: EngineSettings = serde_json::from_str(live).unwrap();
        assert!(settings.migrate_legacy());
        assert_eq!(settings.presence_penalty, None);
        let profile = &settings.model_profiles["mlx-community/Qwen3.5-9B-MLX-4bit"];
        assert_eq!(profile.presence_penalty, Some(1.2));

        let argv = build_serve_command(&settings, "mlx-community/Qwen3.5-9B-MLX-4bit").unwrap();
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
        let manager = test_manager();
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

        let manager = test_manager();
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

    /// `ARGV_FAKE_ENGINE` with Rapid-MLX 0.13.1's `/v1/status` shape (routes/health.py:323)
    /// mid-generation: one request in the running batch, two admitted and waiting.
    const ARGV_FAKE_ENGINE_BUSY: &str = r#"
import http.server, json, sys
argv = sys.argv
port = int(argv[argv.index("--port") + 1])
name = argv[argv.index("--served-model-name") + 1]
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/v1/status":
            body = json.dumps({"status": "generating", "model": name, "num_running": 1, "num_waiting": 2}).encode()
        else:
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

    /// The measured bodies (2026-09-02, real engine): idle is an EXPLICIT 0, generating
    /// under a 10-request burst read `8 running / 0 waiting` and, an instant after
    /// admission, `0 running / 2 waiting`. A body without the counts is an absence, never 0.
    #[test]
    fn parse_active_requests_sums_running_and_waiting_and_refuses_absence() {
        assert_eq!(
            parse_active_requests(r#"{"status":"idle","num_running":0,"num_waiting":0}"#).unwrap(),
            0
        );
        assert_eq!(
            parse_active_requests(r#"{"status":"generating","num_running":8,"num_waiting":0}"#)
                .unwrap(),
            8
        );
        assert_eq!(
            parse_active_requests(r#"{"status":"idle","num_running":0,"num_waiting":2}"#).unwrap(),
            2,
            "admitted-and-waiting requests are in flight even while the batch is empty"
        );

        let no_running = parse_active_requests(r#"{"object":"list","data":[{"id":"x"}]}"#)
            .unwrap_err()
            .to_string();
        assert!(no_running.contains("no `num_running`"), "{no_running}");
        let no_waiting = parse_active_requests(r#"{"num_running":1}"#)
            .unwrap_err()
            .to_string();
        assert!(no_waiting.contains("no `num_waiting`"), "{no_waiting}");
        let negative = parse_active_requests(r#"{"num_running":-1,"num_waiting":0}"#)
            .unwrap_err()
            .to_string();
        assert!(
            negative.contains("not a non-negative integer"),
            "{negative}"
        );
        let not_json = format!("{:#}", parse_active_requests("<html>").unwrap_err());
        assert!(not_json.contains("parsing /v1/status body"), "{not_json}");
    }

    /// Q1: the status carries the engine's own in-flight count when it reports one, and an
    /// engine that answers `/v1/status` WITHOUT the counts leaves `active_requests` None
    /// with the absence named — the busy fact is read, never invented.
    #[tokio::test]
    async fn status_carries_the_engines_in_flight_count_and_never_fabricates_it() {
        let tmp = tempfile::tempdir().unwrap();
        complete_small_model(tmp.path(), "pub/small");
        let manager = test_manager();
        let port = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap().port()
        };
        manager.set_settings(EngineSettings {
            models_dir: tmp.path().to_string_lossy().into_owned(),
            port,
            model_id: Some("pub/small".to_string()),
            served_model_name: Some("busy-alias".to_string()),
            spawn_command: vec![
                "python3".to_string(),
                "-c".to_string(),
                ARGV_FAKE_ENGINE_BUSY.to_string(),
            ],
            ..Default::default()
        });
        manager.mount("pub/small").await.unwrap();
        let status = settle(&manager).await;
        assert_eq!(status.state, "running", "{:?}", status.last_error);
        assert_eq!(status.served_model_id.as_deref(), Some("busy-alias"));
        assert_eq!(status.active_requests, Some(3), "1 running + 2 waiting");
        assert_eq!(status.active_requests_error, None);
        manager.unmount().await;

        let port = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap().port()
        };
        manager.set_settings(EngineSettings {
            models_dir: tmp.path().to_string_lossy().into_owned(),
            port,
            model_id: Some("pub/small".to_string()),
            served_model_name: Some("mute-alias".to_string()),
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
        assert_eq!(
            status.served_model_id.as_deref(),
            Some("mute-alias"),
            "the /v1/models probe is untouched by a mute /v1/status"
        );
        assert_eq!(status.probe_error, None);
        assert_eq!(status.active_requests, None, "absence is None, never 0");
        let absence = status.active_requests_error.expect("the absence is named");
        assert!(absence.contains("no `num_running`"), "{absence}");
        manager.unmount().await;
        assert_eq!(manager.status().await.active_requests, None);
    }

    /// Measured 2026-09-24 on the workhorse: the engine SIGKILLed, and status kept saying
    /// `running` (the stored flag) with its launcher left a zombie. The very next poll must say
    /// `failed`, naming the pid, the signal, the restart policy and the engine's last log lines;
    /// the process is reaped; and a Mount of the same model restarts it (no silent restart).
    #[cfg(unix)]
    #[tokio::test]
    async fn a_killed_engine_is_failed_on_the_next_poll_and_a_mount_restarts_it() {
        let port = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap().port()
        };
        let tmp = tempfile::tempdir().unwrap();
        complete_small_model(tmp.path(), "pub/small");
        let manager = test_manager();
        manager.set_settings(EngineSettings {
            models_dir: tmp.path().to_string_lossy().into_owned(),
            port,
            spawn_command: vec![
                "python3".to_string(),
                "-c".to_string(),
                format!("import sys; print('engine log: loaded', file=sys.stderr, flush=True)\n{ARGV_FAKE_ENGINE}"),
            ],
            ..Default::default()
        });
        manager.mount("pub/small").await.unwrap();
        let status = settle(&manager).await;
        assert_eq!(status.state, "running", "{:?}", status.last_error);
        let pid = status.pid.unwrap();

        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let failed = loop {
            let status = manager.status().await;
            if status.state == "failed" {
                break status;
            }
            assert_eq!(status.state, "running");
            assert!(
                std::time::Instant::now() < deadline,
                "the dead engine still reads running"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let error = failed.last_error.expect("a named failure");
        assert!(
            error.contains(&format!("(pid {pid}) exited: signal: 9")),
            "{error}"
        );
        assert!(
            error.contains("not restarted automatically; Mount restarts it"),
            "{error}"
        );
        assert!(
            error.contains("engine log: loaded"),
            "the last log lines: {error}"
        );
        assert_eq!(failed.pid, None);
        assert_eq!(failed.model_id.as_deref(), Some("pub/small"));
        let zombie = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&zombie.stdout).contains('Z'),
            "the dead engine is reaped, not left a zombie"
        );
        assert_eq!(
            manager.status().await.state,
            "failed",
            "stays failed: nothing restarts"
        );

        manager.mount("pub/small").await.unwrap();
        let restarted = settle(&manager).await;
        assert_eq!(restarted.state, "running", "{:?}", restarted.last_error);
        assert_ne!(restarted.pid, Some(pid));
        manager.unmount().await;
    }

    /// S-L8: the circuit breaker now sits on the production re-mount path. A mount of the
    /// IDENTICAL configuration keeps the supervisor (a healthy engine is verified, not
    /// restarted); after the engine is killed, the same mount restarts it through
    /// `ensure_running`; a crash loop trips the breaker into a NAMED Failed state.
    #[cfg(unix)]
    #[tokio::test]
    async fn identical_mount_reuses_the_supervisor_and_a_crash_loop_trips_the_breaker() {
        let port = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap().port()
        };
        let tmp = tempfile::tempdir().unwrap();
        complete_small_model(tmp.path(), "pub/small");
        let manager = test_manager();
        manager.set_settings(EngineSettings {
            models_dir: tmp.path().to_string_lossy().into_owned(),
            port,
            model_id: Some("pub/small".to_string()),
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
    /// `GOOSE_SIDECAR_LIVE_KV_CACHE=int8|int4` mounts with that profile KV cache; the engine's own
    /// `rapid_mlx_kv_cache_dtype` gauge must then name it (bf16 when unset).
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "spawns the real uvx/rapid-mlx engine; set GOOSE_SIDECAR_LIVE_MODELS_DIR and GOOSE_SIDECAR_LIVE_MODEL_ID"]
    async fn live_mount_of_the_real_engine_runs_and_unmounts_clean() {
        let models_dir = std::env::var("GOOSE_SIDECAR_LIVE_MODELS_DIR").unwrap();
        let model_id = std::env::var("GOOSE_SIDECAR_LIVE_MODEL_ID").unwrap();
        let kv_cache: Option<KvCacheMode> = std::env::var("GOOSE_SIDECAR_LIVE_KV_CACHE")
            .ok()
            .map(|v| serde_json::from_value(serde_json::Value::String(v)).unwrap());
        let port = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap().port()
        };
        let manager = test_manager();
        manager.set_settings(EngineSettings {
            models_dir,
            port,
            model_id: Some(model_id.clone()),
            served_model_name: Some("live-alias".to_string()),
            model_profiles: BTreeMap::from([(
                model_id.clone(),
                ModelProfile {
                    kv_cache,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });

        let fit = manager.mount_fit(&model_id).await.unwrap();
        eprintln!("live: fit {:?} — {}", fit.verdict, fit.message);
        let started = std::time::Instant::now();
        manager.mount(&model_id).await.unwrap();
        // The load as status reports it, every change of phase or resident bytes.
        let mut loads: Vec<EngineLoad> = Vec::new();
        let status = loop {
            let status = manager.status().await;
            if status.state != "mounting" {
                break status;
            }
            if let Some(load) = status.load {
                if loads.last() != Some(&load) {
                    loads.push(load);
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        assert_eq!(status.state, "running", "{:?}", status.last_error);
        assert_eq!(status.served_model_id.as_deref(), Some("live-alias"));
        assert!(status.load.is_none(), "a running engine reports no load");
        let gib = |b: u64| format!("{:.2}", b as f64 / GIB as f64);
        eprintln!(
            "live: load samples [{}]",
            loads
                .iter()
                .map(|l| format!("{} {}", l.phase, l.resident_bytes.map_or("-".into(), gib)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        assert!(loads.iter().any(|l| l.phase == "loading"), "{loads:?}");
        let weights = loads.last().unwrap().weights_bytes;
        let most = loads.iter().filter_map(|l| l.resident_bytes).max().unwrap();
        assert!(
            most as f64 / weights as f64 > 0.9,
            "the engine's resident bytes reached {} of {} on disk",
            gib(most),
            gib(weights)
        );
        let leader = status.pid.unwrap();
        assert!(crate::owns_process_group(leader));
        let members = std::process::Command::new("pgrep")
            .args(["-g", &leader.to_string()])
            .output()
            .unwrap();
        let members = String::from_utf8_lossy(&members.stdout);
        eprintln!(
            "live: running after {:.1}s, leader {leader}, group members [{}], context_window {:?}, parser {:?}, active_requests {:?} (error {:?})",
            started.elapsed().as_secs_f64(),
            members.split_whitespace().collect::<Vec<_>>().join(" "),
            status.context_window,
            status.tool_call_parser,
            status.active_requests,
            status.active_requests_error
        );
        assert_eq!(
            status.active_requests,
            Some(0),
            "an idle real engine reports an EXPLICIT zero; error: {:?}",
            status.active_requests_error
        );
        assert!(
            members.split_whitespace().count() >= 2,
            "uv and its engine must both sit in the leader's group: [{members}]"
        );
        let metrics = reqwest::get(format!("http://127.0.0.1:{port}/metrics"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let active_dtype = metrics
            .lines()
            .find(|l| l.starts_with("rapid_mlx_kv_cache_dtype{") && l.ends_with(" 1"))
            .map(str::to_string);
        eprintln!("live: engine reports {active_dtype:?}");
        let expected = kv_cache.map_or("bf16", KvCacheMode::engine_dtype);
        assert_eq!(
            active_dtype,
            Some(format!(
                "rapid_mlx_kv_cache_dtype{{dtype=\"{expected}\"}} 1"
            ))
        );

        // lz.4: a `logprobs` request on an MTP-mounted engine aborted the whole process
        // (libc++abi "There is no Stream(gpu, 1) in current thread"). It must be answered — or
        // refused with a 400 naming logprobs — and the SAME engine must still be serving.
        let models: serde_json::Value = serde_json::from_str(
            &reqwest::get(format!("http://127.0.0.1:{port}/v1/models"))
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
        )
        .unwrap();
        let speculative = models["data"][0]["speculative_decoding"].clone();
        let reply = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .header("content-type", "application/json")
            .body(
                serde_json::json!({
                    "model": "live-alias",
                    "messages": [{"role": "user", "content": "Say hi."}],
                    "max_tokens": 8,
                    "logprobs": true,
                    "top_logprobs": 2,
                })
                .to_string(),
            )
            .send()
            .await
            .expect("the engine must answer a logprobs request, not drop the connection");
        let code = reply.status();
        let body = reply.text().await.unwrap();
        let logprobs = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["choices"][0]["logprobs"]["content"].as_array().cloned());
        eprintln!(
            "live: logprobs request → {code}, speculative {speculative}, {} logprob entries, first {:?}",
            logprobs.as_ref().map_or(0, Vec::len),
            logprobs.as_ref().and_then(|l| l.first())
        );
        if code.is_success() {
            assert!(
                logprobs.is_some_and(|l| !l.is_empty()),
                "a 200 must carry per-token logprobs: {body}"
            );
        } else {
            assert_eq!(code, reqwest::StatusCode::BAD_REQUEST, "{body}");
            assert!(
                body.contains("logprobs"),
                "a refusal names logprobs: {body}"
            );
        }
        let after = manager.status().await;
        assert_eq!(after.state, "running", "{:?}", after.last_error);
        assert_eq!(
            after.pid,
            Some(leader),
            "the engine was restarted, not kept"
        );
        assert!(port_has_listener(port), "the engine stopped serving");

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

        let manager = test_manager();
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

        let manager = test_manager();
        manager.set_settings(EngineSettings {
            models_dir: tmp.path().to_string_lossy().into_owned(),
            ..Default::default()
        });

        // 4 TiB exceeds any Mac's RAM, so the gate refuses without asking macOS to make room.
        let err = manager.mount("pub/huge").await.unwrap_err();
        let refused = err
            .downcast_ref::<MountRefused>()
            .expect("a gate refusal is typed, so the ACP layer can structure it");
        assert_eq!(refused.verdict.verdict, Verdict::Block);
        assert!(!refused.verdict.could_ever_fit());
        let err = err.to_string();
        assert!(err.contains("memory gate BLOCK"), "unexpected error: {err}");
        assert!(
            !err.contains("Make room"),
            "no compaction for a model no RAM holds: {err}"
        );

        let status = manager.status().await;
        assert_eq!(status.state, "stopped");
        assert_eq!(status.gate_verdict.as_deref(), Some("block"));
        let message = status.gate_message.unwrap();
        assert!(
            message.contains("budget") && message.contains("short"),
            "{message}"
        );
        let fit = manager.mount_fit("pub/huge").await.unwrap();
        assert_eq!(
            (fit.verdict, &fit.need),
            (Verdict::Block, &refused.verdict.need),
            "the status verdict and the mount are one rule on one need"
        );
    }

    #[tokio::test]
    async fn mount_refuses_missing_and_incomplete_models() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = test_manager();
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
