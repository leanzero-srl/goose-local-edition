//! `goose swarm` — local multi-device swarm (goose-local-edition).
//!
//! `goose swarm run "<task>"` plans on the smart model then dispatches subtasks across the LM Link
//! device pool with the goose-swarm weighted work-queue scheduler. `goose swarm pool` manages the
//! pool (devices, weights, enable/disable) via an interactive menu, persisted in the Goose config.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use console::style;
use futures::StreamExt;
use goose::agents::{
    Agent, AgentConfig, AgentEvent, ExtensionConfig, GoosePlatform, SessionConfig,
};
use goose::config::permission::PermissionManager;
use goose::config::{Config, GooseMode};
use goose::conversation::message::{Message, MessageContent};
use goose::providers::base::Provider;
use goose::recipe::Response;
use goose::session::session_manager::SessionType;
use goose::session::SessionManager;
use goose_swarm::{
    deterministic_verdict, is_split_candidate, ChildSpec, Dag, DeviceCfg, DispatchError,
    DispatchRequest, EventSink, Judge, JudgeConfig, JudgeInput, JudgeOutcome, JudgeRequest,
    NullSink, PreReviewOutput, PreReviewRequest, PreReviewer, ReplanContext, Replanner, Scheduler,
    SwarmEvent, TaskDispatcher, TaskRunOutput, TaskSpec, ToolCallRecord, Verdict,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const FINAL_OUTPUT_TOOL: &str = "recipe__final_output";
const SWARM_CONFIG_KEY: &str = "swarm";

// ---------------------------------------------------------------------------------------------
// Pool config (persisted under the `swarm` key in ~/.config/goose/config.yaml)
// ---------------------------------------------------------------------------------------------

fn default_endpoint() -> String {
    "http://localhost:1234".to_string()
}
fn default_planner() -> String {
    "qwen/qwen3.6-27b".to_string()
}

fn default_instances() -> u32 {
    1
}
fn default_host() -> Option<String> {
    None
}
fn default_worker_max_turns() -> u32 {
    40
}
fn default_max_attempts() -> u32 {
    3
}
fn default_planner_also_works() -> bool {
    true
}
fn default_planner_weight() -> u32 {
    1
}
fn default_max_research() -> u32 {
    4
}
fn default_dynamic_replan() -> bool {
    true
}
fn default_max_replans() -> u32 {
    2
}
fn default_research_scouts() -> bool {
    true
}
fn default_parallel_planning() -> bool {
    true
}
fn default_worker_timeout_secs() -> u64 {
    // A generous HANG failsafe — only a genuine infinite stall should ever reach it (slow local models
    // are expected; this must never trip on mere slowness). On trip, the task re-routes to another
    // device. 0 = disabled (no timer; a true hang then needs a manual Ctrl-C).
    900
}
fn default_planner_timeout_secs() -> u64 {
    // Generous HANG failsafe for planner-side calls (architect / scouts / research / replan). 150s and
    // 360s both risked killing legitimately-slow work on local hardware; 900s only catches a true
    // infinite stall (best-of-N already fans the skeleton across devices). 0 = disabled.
    900
}
fn default_best_of_n_skeletons() -> usize {
    1
}

/// Imposed sampling parameters for the local models — the lever for steadying weak models (lower
/// temperature for more deterministic tool-calling, etc.). `temperature` is a first-class ModelConfig
/// field; `top_p`/`top_k`/`min_p`/`repeat_penalty` are merged into the request body (LM Studio accepts
/// them). All None = use the model/server defaults (no change).
#[derive(Clone, Default)]
pub struct SamplingParams {
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<i32>,
    min_p: Option<f32>,
    repeat_penalty: Option<f32>,
}

/// When the swarm runs a parallel research phase before planning.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ResearchPlanningMode {
    Off,
    /// Always run the parallel research phase before planning (keeps the fleet busy during planning).
    #[default]
    On,
    /// Research only when the working dir already has source files (an amendment).
    Auto,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SwarmDevice {
    pub id: String,
    pub model_id: String,
    /// Max concurrent tasks routed to this device (one model instance serves several via LM Studio's PARALLEL).
    pub weight: u32,
    pub enabled: bool,
    /// How many instances of this model goose may load on the device. Default 1 — goose never
    /// spins up extra instances unless you raise this.
    #[serde(default = "default_instances")]
    pub instances: u32,
    /// Physical host (lms ps DEVICE column). Informational/display only — routing is by model_id.
    #[serde(default = "default_host")]
    pub host: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SwarmConfig {
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_planner")]
    pub planner_model: String,
    #[serde(default)]
    pub devices: Vec<SwarmDevice>,
    /// Max turns per worker agent (knob: raise if workers hit the cap before finishing).
    #[serde(default = "default_worker_max_turns")]
    pub worker_max_turns: u32,
    /// Max dispatch attempts per task before it fails (knob: raise for flaky LM Link).
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// MCP extensions (by builder name) every worker gets: "context7" | "web-search" | "doc-processor".
    /// Secrets are read from the environment at runtime, never stored here.
    #[serde(default)]
    pub worker_extensions: Vec<String>,
    /// After planning, also use the planner model as a WORKER so the smartest model isn't idle
    /// (and hard subtasks can route to it). Default true.
    #[serde(default = "default_planner_also_works")]
    pub planner_also_works: bool,
    /// Worker weight for the planner model when it pitches in (default 1 — it's the dense, slower model).
    #[serde(default = "default_planner_weight")]
    pub planner_weight: u32,
    /// Effective context-window cap (GOOSE_LOCAL_CONTEXT_CAP) applied to every agent; None = off.
    #[serde(default)]
    pub context_cap: Option<u32>,
    /// Parallel research-planning phase before plan(). Auto = only when the cwd has source files.
    #[serde(default)]
    pub research_planning: ResearchPlanningMode,
    /// Hard cap on parallel research workers (bounds latency + curbs make-work).
    #[serde(default = "default_max_research")]
    pub max_research_questions: u32,
    /// When workers idle mid-run, let the planner inject more parallel work to fill the tail.
    #[serde(default = "default_dynamic_replan")]
    pub dynamic_replan: bool,
    /// Max dynamic-replan rounds per run (bounds latency + make-work).
    #[serde(default = "default_max_replans")]
    pub max_replans: u32,
    /// Use parallel fixed-lens SCOUTS for research (no serial scoping call) instead of the planner
    /// scoping questions first. On by default — maximizes parallelism during the research phase.
    #[serde(default = "default_research_scouts")]
    pub research_scouts: bool,
    /// Parallel planning: the 27B drafts a skeleton, then the fleet details every subtask in parallel
    /// (vs the 27B writing the whole plan alone). On by default — maximizes parallelism in the PLAN phase.
    #[serde(default = "default_parallel_planning")]
    pub parallel_planning: bool,
    /// Per-task wall-clock cap (seconds): a worker exceeding this is treated as hung and re-routed
    /// to another device, so one stuck model never stalls the whole run.
    #[serde(default = "default_worker_timeout_secs")]
    pub worker_timeout_secs: u64,
    /// Wall-clock cap (seconds) for PLANNER-side agent calls (architect / solo plan / scouts /
    /// research / replan). Shorter than worker tasks — these should be quick, so a longer hang is a
    /// stall to recover from fast (fallback to solo plan / skip / empty).
    #[serde(default = "default_planner_timeout_secs")]
    pub planner_timeout_secs: u64,
    /// May the swarm run `lms load` to spin up models? Default FALSE — use only already-resident
    /// models and never auto-load. Turn on (pool menu) to let the swarm pre-warm + JIT re-warm.
    #[serde(default)]
    pub allow_model_load: bool,
    /// How many SKELETON candidates to draft in parallel and pick the structurally-best from (1 = the
    /// single-draft default, no change). >1 is a plan-quality experiment — latency-neutral, the fleet
    /// drafts in parallel and a pure-Rust scorer (no LLM) picks the widest valid plan.
    #[serde(default = "default_best_of_n_skeletons")]
    pub best_of_n_skeletons: usize,
    /// Imposed sampling parameters for the local models (None = server/model default). Tuned to steady
    /// weak models — e.g. a low temperature for deterministic tool-calling.
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub top_k: Option<i32>,
    #[serde(default)]
    pub min_p: Option<f32>,
    #[serde(default)]
    pub repeat_penalty: Option<f32>,
    /// Hard cap (chars) on any single tool result fed back to a worker (None = swarm default 8000).
    /// The big lever against context-bloat prefill stalls on local models.
    #[serde(default)]
    pub max_tool_response_chars: Option<u32>,
    /// Per-SCOUT wall-clock budget (seconds) — a research scout exceeding this returns partial so it
    /// cannot monopolize a node and idle the fleet behind the scout barrier.
    #[serde(default = "default_scout_budget_secs")]
    pub scout_budget_secs: u64,
    /// All worker models are the SAME model (same weights + tokenizer, quant aside). When true the
    /// planner is told fragments produced independently on different nodes WILL mesh consistently, so
    /// it splits more aggressively — enabling more parallel planning + execution without divergence risk.
    #[serde(default)]
    pub homogeneous_models: bool,
    /// Per-host throughput weights (device-id SUBSTRING -> weight; higher = faster host gets a larger
    /// share of tasks). E.g. {"worksmacstudio":3,"mihai":2,"gabee":1}. Empty = equal split.
    #[serde(default)]
    pub speed_weights: std::collections::HashMap<String, u32>,
}

fn default_scout_budget_secs() -> u64 {
    120
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            endpoint: default_endpoint(),
            planner_model: default_planner(),
            devices: vec![
                SwarmDevice {
                    id: "mac".to_string(),
                    model_id: "qwen/qwen3.6-35b-a3b".to_string(),
                    weight: 2,
                    enabled: true,
                    instances: 1,
                    host: None,
                },
                SwarmDevice {
                    id: "macbook".to_string(),
                    model_id: "qwen3.6-35b-a3b-mtp-holo3-qwopus-qx86-hi-mlx".to_string(),
                    weight: 1,
                    enabled: true,
                    instances: 1,
                    host: None,
                },
            ],
            worker_max_turns: default_worker_max_turns(),
            max_attempts: default_max_attempts(),
            worker_extensions: Vec::new(),
            planner_also_works: default_planner_also_works(),
            planner_weight: default_planner_weight(),
            context_cap: None,
            research_planning: ResearchPlanningMode::On,
            max_research_questions: default_max_research(),
            dynamic_replan: default_dynamic_replan(),
            max_replans: default_max_replans(),
            research_scouts: default_research_scouts(),
            parallel_planning: default_parallel_planning(),
            worker_timeout_secs: default_worker_timeout_secs(),
            planner_timeout_secs: default_planner_timeout_secs(),
            allow_model_load: false,
            best_of_n_skeletons: default_best_of_n_skeletons(),
            temperature: None,
            top_p: None,
            top_k: None,
            min_p: None,
            repeat_penalty: None,
            max_tool_response_chars: None,
            scout_budget_secs: default_scout_budget_secs(),
            homogeneous_models: false,
            speed_weights: std::collections::HashMap::new(),
        }
    }
}

fn load_config() -> SwarmConfig {
    Config::global()
        .get_param::<SwarmConfig>(SWARM_CONFIG_KEY)
        .unwrap_or_default()
}

fn save_config(cfg: &SwarmConfig) -> Result<()> {
    Config::global()
        .set_param(SWARM_CONFIG_KEY, cfg)
        .map_err(|e| anyhow!("failed to save swarm config: {e}"))
}

// ---------------------------------------------------------------------------------------------
// CLI surface
// ---------------------------------------------------------------------------------------------

#[derive(clap::Subcommand, Debug)]
pub enum SwarmCommand {
    /// Plan a task and run it across the swarm device pool.
    Run {
        /// The task to plan and run.
        prompt: String,
        /// Final report format: `text` (default) or `json` (the enriched RunReport to stdout).
        #[arg(long = "output-format", default_value = "text")]
        output_format: String,
        /// Path for the structured JSONL event log (default: <cwd>/.swarm/run-<id>.jsonl).
        #[arg(long = "log-file")]
        log_file: Option<PathBuf>,
        /// Disable the JSONL event log.
        #[arg(long = "no-log")]
        no_log: bool,
        /// Override per-worker max turns (default: the pool's worker_max_turns).
        #[arg(long = "max-turns")]
        max_turns: Option<u32>,
        /// Enable an MCP worker extension for this run (repeatable): context7 | web-search | doc-processor.
        #[arg(long = "mcp")]
        mcp: Vec<String>,
        /// Force the research-planning phase on/off for this run (overrides the configured mode).
        #[arg(long = "research")]
        research: Option<bool>,
        /// Force dynamic replanning on/off for this run (overrides the configured setting).
        #[arg(long = "dynamic-replan")]
        dynamic_replan: Option<bool>,
        /// Draft N plan skeletons in parallel and pick the best (overrides config; 1 = off).
        #[arg(long = "best-of-n")]
        best_of_n: Option<usize>,
    },
    /// View and manage the swarm device pool (interactive menu when no subcommand is given).
    Pool {
        #[command(subcommand)]
        command: Option<PoolCommand>,
    },
}

/// Options for a `goose swarm run`.
pub struct RunOpts {
    pub prompt: String,
    pub output_format: String,
    pub log_file: Option<PathBuf>,
    pub no_log: bool,
    pub max_turns: Option<u32>,
    pub mcp: Vec<String>,
    pub research: Option<bool>,
    pub dynamic_replan: Option<bool>,
    pub best_of_n: Option<usize>,
}

#[derive(clap::Subcommand, Debug)]
pub enum PoolCommand {
    /// Print the current pool.
    Show,
    /// Add a device.
    Add {
        id: String,
        model_id: String,
        #[arg(default_value_t = 1)]
        weight: u32,
        #[arg(default_value_t = 1)]
        instances: u32,
    },
    /// Remove a device by id.
    Rm { id: String },
    /// Set a device's weight.
    Weight { id: String, weight: u32 },
    /// Enable a device.
    Enable { id: String },
    /// Disable a device.
    Disable { id: String },
    /// Probe the live fleet (lms ps + the endpoint's model ids).
    Probe,
    /// Import every model loaded across the fleet (parses `lms ps`) as pool entries.
    Import {
        #[arg(long, default_value_t = 1)]
        weight: u32,
        #[arg(long)]
        disabled: bool,
    },
}

pub async fn handle_swarm(cmd: SwarmCommand) -> Result<()> {
    match cmd {
        SwarmCommand::Run {
            prompt,
            output_format,
            log_file,
            no_log,
            max_turns,
            mcp,
            research,
            dynamic_replan,
            best_of_n,
        } => {
            run_swarm(RunOpts {
                prompt,
                output_format,
                log_file,
                no_log,
                max_turns,
                mcp,
                research,
                dynamic_replan,
                best_of_n,
            })
            .await
        }
        SwarmCommand::Pool { command } => match command {
            None => pool_menu(),
            Some(pc) => pool_op(pc),
        },
    }
}

// ---------------------------------------------------------------------------------------------
// Pool management
// ---------------------------------------------------------------------------------------------

fn show_pool(cfg: &SwarmConfig) {
    println!("\n{}", style(" swarm pool ").on_cyan().black().bold());
    println!("  endpoint   {}", style(&cfg.endpoint).cyan());
    println!(
        "  planner    {}   also-works {} (w{})",
        style(&cfg.planner_model).green(),
        if cfg.planner_also_works {
            style("on").green().bold()
        } else {
            style("off").red().bold()
        },
        cfg.planner_weight
    );
    println!(
        "  limits     worker-max-turns {}   max-attempts {}   context-cap {}",
        style(cfg.worker_max_turns).cyan(),
        style(cfg.max_attempts).cyan(),
        match cfg.context_cap {
            Some(c) => style(c.to_string()).cyan(),
            None => style("off".to_string()).dim(),
        }
    );
    println!(
        "  research   {:?}   max-questions {}   mode {}",
        cfg.research_planning,
        style(cfg.max_research_questions).cyan(),
        if cfg.research_scouts {
            style("scouts(parallel)").green().bold()
        } else {
            style("questions").cyan()
        }
    );
    println!(
        "  planning   {}",
        if cfg.parallel_planning {
            style("parallel (skeleton + fleet detailing)")
                .green()
                .bold()
        } else {
            style("solo 27B").cyan()
        }
    );
    println!(
        "  identical  {}",
        if cfg.homogeneous_models {
            style("yes — split aggressively (same tokenizer)")
                .green()
                .bold()
        } else {
            style("no").dim()
        }
    );
    if !cfg.speed_weights.is_empty() {
        let mut sw: Vec<String> = cfg
            .speed_weights
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        sw.sort();
        println!(
            "  speed-wt   {}",
            style(format!("{} (faster host → more tasks)", sw.join(" "))).green()
        );
    }
    println!(
        "  skeletons  {}",
        if cfg.best_of_n_skeletons > 1 {
            style(format!(
                "best-of-{} (parallel draft + structural score)",
                cfg.best_of_n_skeletons
            ))
            .green()
            .bold()
        } else {
            style("single".to_string()).dim()
        }
    );
    println!(
        "  idle-cap   worker {}s · planner {}s (NO-PROGRESS window, not wall-clock; a stalled stream re-routes / falls back)",
        style(cfg.worker_timeout_secs).cyan(),
        style(cfg.planner_timeout_secs).cyan()
    );
    println!(
        "  models     load {}",
        if cfg.allow_model_load {
            style("ON (swarm may lms-load / pre-warm)").green().bold()
        } else {
            style("OFF (resident models only — no auto spin-up)")
                .yellow()
                .bold()
        }
    );
    {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = cfg.temperature {
            parts.push(format!("temp={v}"));
        }
        if let Some(v) = cfg.top_p {
            parts.push(format!("top_p={v}"));
        }
        if let Some(v) = cfg.top_k {
            parts.push(format!("top_k={v}"));
        }
        if let Some(v) = cfg.min_p {
            parts.push(format!("min_p={v}"));
        }
        if let Some(v) = cfg.repeat_penalty {
            parts.push(format!("rep={v}"));
        }
        let s = if parts.is_empty() {
            style("model defaults".to_string()).dim()
        } else {
            style(parts.join(" ")).cyan().bold()
        };
        println!("  sampling   {s}");
    }
    println!(
        "  replan     {}   max-rounds {}",
        if cfg.dynamic_replan {
            style("on").green().bold()
        } else {
            style("off").red().bold()
        },
        style(cfg.max_replans).cyan()
    );
    println!(
        "  mcp        {}",
        if cfg.worker_extensions.is_empty() {
            style("none".to_string()).dim()
        } else {
            style(cfg.worker_extensions.join(", ")).cyan()
        }
    );
    if cfg.devices.is_empty() {
        println!("  {}", style("(no devices — add one)").yellow());
        return;
    }
    for d in &cfg.devices {
        let state = if d.enabled {
            style("enabled ").green().bold()
        } else {
            style("disabled").red().bold()
        };
        println!(
            "  {state}  {:<10} weight {}  ×{} inst  {}{}",
            style(&d.id).bold(),
            style(d.weight).cyan().bold(),
            style(d.instances).cyan(),
            style(&d.model_id).dim(),
            style(
                d.host
                    .as_deref()
                    .map(|h| format!("  @{h}"))
                    .unwrap_or_default()
            )
            .dim()
        );
    }
}

fn pool_menu() -> Result<()> {
    let mut cfg = load_config();
    cliclack::intro(style(" goose swarm pool ").on_cyan().black())?;
    loop {
        show_pool(&cfg);
        let action: &str = cliclack::select("Manage the device pool")
            .item("add", "Add a device", "")
            .item("weight", "Set a device weight", "")
            .item("instances", "Set device instance count", "copies to load")
            .item("toggle", "Enable / disable a device", "")
            .item("remove", "Remove a device", "")
            .item("planner", "Set the planner model", "")
            .item("planner-worker", "Planner-also-works on/off + weight", "")
            .item("endpoint", "Set the LM Link endpoint", "")
            .item("max-turns", "Set worker max-turns", "")
            .item("max-attempts", "Set max dispatch attempts", "")
            .item(
                "context-cap",
                "Set context-window cap",
                "GOOSE_LOCAL_CONTEXT_CAP",
            )
            .item("research", "Research-planning mode", "off / on / auto")
            .item(
                "scouts",
                "Research method",
                "parallel scouts vs serial scoping",
            )
            .item(
                "planning",
                "Planning method",
                "parallel fleet detailing vs solo 27B",
            )
            .item(
                "homogeneous",
                "Identical models",
                "same tokenizer -> split aggressively",
            )
            .item(
                "best-of-n",
                "Best-of-N skeletons",
                "draft N plans in parallel, pick the best (1 = off)",
            )
            .item(
                "model-load",
                "Allow model loading",
                "let the swarm lms-load / pre-warm (off = resident only)",
            )
            .item("max-research", "Max research questions / lenses", "")
            .item(
                "replan",
                "Dynamic replan on/off",
                "fill idle workers mid-run",
            )
            .item("max-replans", "Max replan rounds", "")
            .item(
                "mcp",
                "Toggle worker MCP extensions",
                "context7 / web-search / doc-processor",
            )
            .item("probe", "Probe the live fleet", "lms ps + endpoint models")
            .item(
                "import",
                "Import loaded models from fleet",
                "lms ps -> pool entries",
            )
            .item("save", "Save & exit", "")
            .item("quit", "Quit without saving", "")
            .interact()?;
        match action {
            "add" => {
                let id: String = cliclack::input("Device id (e.g. workhorse)").interact()?;
                let model_id: String =
                    cliclack::input("LM Link model id (must be unique)").interact()?;
                let weight: String = cliclack::input("Weight (max concurrent tasks)")
                    .default_input("1")
                    .interact()?;
                let weight: u32 = weight.trim().parse().unwrap_or(1).max(1);
                let instances: String = cliclack::input("Instances to load on this device")
                    .default_input("1")
                    .interact()?;
                let instances: u32 = instances.trim().parse().unwrap_or(1).max(1);
                cfg.devices.retain(|d| d.id != id);
                cfg.devices.push(SwarmDevice {
                    id,
                    model_id,
                    weight,
                    enabled: true,
                    instances,
                    host: None,
                });
            }
            "weight" => {
                if let Some(id) = pick_device(&cfg, "Set weight for which device?")? {
                    let weight: String =
                        cliclack::input(format!("New weight for {id}")).interact()?;
                    let weight: u32 = weight.trim().parse().unwrap_or(1).max(1);
                    if let Some(d) = cfg.devices.iter_mut().find(|d| d.id == id) {
                        d.weight = weight;
                    }
                }
            }
            "instances" => {
                if let Some(id) = pick_device(&cfg, "Set instances for which device?")? {
                    let n: String = cliclack::input(format!("Instances for {id}")).interact()?;
                    let n: u32 = n.trim().parse().unwrap_or(1).max(1);
                    if let Some(d) = cfg.devices.iter_mut().find(|d| d.id == id) {
                        d.instances = n;
                    }
                }
            }
            "toggle" => {
                if let Some(id) = pick_device(&cfg, "Enable/disable which device?")? {
                    if let Some(d) = cfg.devices.iter_mut().find(|d| d.id == id) {
                        d.enabled = !d.enabled;
                    }
                }
            }
            "remove" => {
                if let Some(id) = pick_device(&cfg, "Remove which device?")? {
                    cfg.devices.retain(|d| d.id != id);
                }
            }
            "planner" => {
                let m: String = cliclack::input("Planner model id")
                    .default_input(&cfg.planner_model)
                    .interact()?;
                cfg.planner_model = m;
            }
            "planner-worker" => {
                cfg.planner_also_works = cliclack::confirm("Planner also works as a worker?")
                    .initial_value(cfg.planner_also_works)
                    .interact()?;
                if cfg.planner_also_works {
                    let w: String = cliclack::input("Planner worker weight")
                        .default_input(&cfg.planner_weight.to_string())
                        .interact()?;
                    cfg.planner_weight = w.trim().parse().unwrap_or(1).max(1);
                }
            }
            "endpoint" => {
                let e: String = cliclack::input("LM Link endpoint")
                    .default_input(&cfg.endpoint)
                    .interact()?;
                cfg.endpoint = e;
            }
            "max-turns" => {
                let v: String = cliclack::input("Worker max-turns")
                    .default_input(&cfg.worker_max_turns.to_string())
                    .interact()?;
                cfg.worker_max_turns = v.trim().parse().unwrap_or(40).max(1);
            }
            "max-attempts" => {
                let v: String = cliclack::input("Max dispatch attempts")
                    .default_input(&cfg.max_attempts.to_string())
                    .interact()?;
                cfg.max_attempts = v.trim().parse().unwrap_or(3).max(1);
            }
            "context-cap" => {
                let cur = cfg
                    .context_cap
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "0".to_string());
                let v: String = cliclack::input("Context cap tokens (0 = off)")
                    .default_input(&cur)
                    .interact()?;
                let parsed: u32 = v.trim().parse().unwrap_or(0);
                cfg.context_cap = if parsed == 0 { None } else { Some(parsed) };
            }
            "research" => {
                let m: &str = cliclack::select("Research-planning mode")
                    .item("off", "off", "")
                    .item("on", "on", "")
                    .item("auto", "auto", "on for amendments only")
                    .interact()?;
                cfg.research_planning = match m {
                    "off" => ResearchPlanningMode::Off,
                    "on" => ResearchPlanningMode::On,
                    _ => ResearchPlanningMode::Auto,
                };
            }
            "scouts" => {
                cfg.research_scouts =
                    cliclack::confirm("Use parallel fixed-lens scouts (no serial scoping call)?")
                        .initial_value(cfg.research_scouts)
                        .interact()?;
            }
            "planning" => {
                cfg.parallel_planning =
                    cliclack::confirm("Parallel planning (27B skeleton + fleet detailing)?")
                        .initial_value(cfg.parallel_planning)
                        .interact()?;
            }
            "homogeneous" => {
                cfg.homogeneous_models = cliclack::confirm(
                    "All worker models identical (same tokenizer)? Enables aggressive splitting",
                )
                .initial_value(cfg.homogeneous_models)
                .interact()?;
            }
            "best-of-n" => {
                let v: String =
                    cliclack::input("How many skeleton candidates to draft in parallel (1 = off)")
                        .default_input(&cfg.best_of_n_skeletons.to_string())
                        .interact()?;
                cfg.best_of_n_skeletons = v.trim().parse().unwrap_or(1).clamp(1, 5);
            }
            "model-load" => {
                cfg.allow_model_load = cliclack::confirm(
                    "Allow the swarm to load models (lms load / pre-warm)? Off = use only resident models",
                )
                .initial_value(cfg.allow_model_load)
                .interact()?;
            }
            "max-research" => {
                let v: String = cliclack::input("Max research questions / scout lenses")
                    .default_input(&cfg.max_research_questions.to_string())
                    .interact()?;
                cfg.max_research_questions = v.trim().parse().unwrap_or(4).clamp(1, 8);
            }
            "replan" => {
                cfg.dynamic_replan =
                    cliclack::confirm("Dynamic replanning (fill idle workers mid-run)?")
                        .initial_value(cfg.dynamic_replan)
                        .interact()?;
            }
            "max-replans" => {
                let v: String = cliclack::input("Max replan rounds")
                    .default_input(&cfg.max_replans.to_string())
                    .interact()?;
                cfg.max_replans = v.trim().parse().unwrap_or(2).clamp(0, 6);
            }
            "mcp" => {
                let choice: &str = cliclack::select("Toggle which worker MCP extension?")
                    .item("context7", "context7", "")
                    .item("web-search", "web-search", "")
                    .item("doc-processor", "doc-processor", "")
                    .interact()?;
                if let Some(pos) = cfg.worker_extensions.iter().position(|x| x == choice) {
                    cfg.worker_extensions.remove(pos);
                } else {
                    cfg.worker_extensions.push(choice.to_string());
                }
            }
            "probe" => probe_fleet(),
            "import" => match probe_lms_processes() {
                Ok(procs) if !procs.is_empty() => {
                    let summary = import_processes(&mut cfg, &procs, 1, true);
                    print_import_summary(&summary);
                }
                Ok(_) => println!("  {}", style("(no models loaded on the fleet)").yellow()),
                Err(e) => println!("  (lms ps failed: {e})"),
            },
            "save" => {
                save_config(&cfg)?;
                cliclack::outro(style("pool saved").green())?;
                break;
            }
            "quit" => {
                cliclack::outro(style("not saved").yellow())?;
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

fn pick_device(cfg: &SwarmConfig, prompt: &str) -> Result<Option<String>> {
    if cfg.devices.is_empty() {
        println!("  {}", style("(no devices)").yellow());
        return Ok(None);
    }
    let mut sel = cliclack::select(prompt.to_string());
    for d in &cfg.devices {
        sel = sel.item(d.id.clone(), &d.id, &d.model_id);
    }
    Ok(Some(sel.interact()?))
}

fn pool_op(pc: PoolCommand) -> Result<()> {
    let mut cfg = load_config();
    match pc {
        PoolCommand::Show => {
            show_pool(&cfg);
            return Ok(());
        }
        PoolCommand::Probe => {
            probe_fleet();
            return Ok(());
        }
        PoolCommand::Import { weight, disabled } => {
            let procs = probe_lms_processes()?;
            let summary = import_processes(&mut cfg, &procs, weight, !disabled);
            print_import_summary(&summary);
        }
        PoolCommand::Add {
            id,
            model_id,
            weight,
            instances,
        } => {
            cfg.devices.retain(|d| d.id != id);
            cfg.devices.push(SwarmDevice {
                id,
                model_id,
                weight: weight.max(1),
                enabled: true,
                instances: instances.max(1),
                host: None,
            });
        }
        PoolCommand::Rm { id } => cfg.devices.retain(|d| d.id != id),
        PoolCommand::Weight { id, weight } => {
            if let Some(d) = cfg.devices.iter_mut().find(|d| d.id == id) {
                d.weight = weight.max(1);
            }
        }
        PoolCommand::Enable { id } => {
            if let Some(d) = cfg.devices.iter_mut().find(|d| d.id == id) {
                d.enabled = true;
            }
        }
        PoolCommand::Disable { id } => {
            if let Some(d) = cfg.devices.iter_mut().find(|d| d.id == id) {
                d.enabled = false;
            }
        }
    }
    save_config(&cfg)?;
    show_pool(&cfg);
    Ok(())
}

fn probe_fleet() {
    println!("\n{}", style("lms ps:").bold());
    match ProcCommand::new("lms").arg("ps").output() {
        Ok(out) => print!("{}", String::from_utf8_lossy(&out.stdout)),
        Err(e) => println!("  (lms ps failed: {e})"),
    }
    println!("{}", style("endpoint model ids:").bold());
    match ProcCommand::new("curl")
        .args(["-s", "--max-time", "6", "http://localhost:1234/v1/models"])
        .output()
    {
        Ok(out) => {
            let body = String::from_utf8_lossy(&out.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
                    for m in arr {
                        if let Some(id) = m.get("id").and_then(|i| i.as_str()) {
                            println!("  {id}");
                        }
                    }
                }
            }
        }
        Err(e) => println!("  (curl failed: {e})"),
    }
}

/// Count currently-loaded instances of a model across the fleet (`lms ps`).
fn loaded_instance_count(model_id: &str) -> usize {
    match ProcCommand::new("lms").arg("ps").output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains(model_id))
            .count(),
        Err(_) => 0,
    }
}

/// Ensure up to `instances` copies of a model are loaded — and NEVER more than already present, so
/// repeated runs / pre-warms don't stack duplicate instances (the cause of "3 instances on one box").
/// Default `instances` is 1, so goose never spins up extras unless the user raises it.
fn ensure_loaded(model_id: &str, instances: u32) {
    let want = instances.max(1) as usize;
    let have = loaded_instance_count(model_id);
    for _ in have..want {
        let _ = ProcCommand::new("lms")
            .args(["load", model_id, "-y", "--ttl", "3600"])
            .output();
    }
}

// ---------------------------------------------------------------------------------------------
// Fleet import — parse `lms ps` and add every loaded model (one per machine, or several) to the pool
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct LmsProcess {
    identifier: String,
    status: String,
    device: Option<String>,
    /// LM Studio's PARALLEL column — how many requests this model instance serves at once. The swarm
    /// uses it as the device weight so dispatch concurrency tracks the user's LM Studio concurrency.
    parallel: Option<u32>,
}

/// Parse `lms ps` output (a plain whitespace-aligned table). Splits data rows on runs of >=2 spaces
/// (so "29.53 GB" stays one field) and reads DEVICE by its header column index. Errs if no header.
fn parse_lms_ps(raw: &str) -> Result<Vec<LmsProcess>> {
    let ansi = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    let gap = regex::Regex::new(r"\s{2,}").unwrap();
    let clean = ansi.replace_all(raw, "");
    let lines: Vec<&str> = clean.lines().collect();
    let header = lines
        .iter()
        .position(|l| l.contains("IDENTIFIER") && l.contains("DEVICE"))
        .ok_or_else(|| anyhow!("lms ps: header (IDENTIFIER/DEVICE) not found"))?;
    let cols: Vec<&str> = lines[header].split_whitespace().collect();
    let device_idx = cols.iter().position(|c| *c == "DEVICE").unwrap_or(6);
    let status_idx = cols.iter().position(|c| *c == "STATUS").unwrap_or(2);
    let parallel_idx = cols.iter().position(|c| *c == "PARALLEL");
    let mut out = Vec::new();
    for line in &lines[header + 1..] {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<String> = gap
            .split(line.trim())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if f.is_empty() {
            continue;
        }
        out.push(LmsProcess {
            identifier: f[0].clone(),
            status: f.get(status_idx).cloned().unwrap_or_default(),
            device: f.get(device_idx).cloned().filter(|s| !s.is_empty()),
            parallel: parallel_idx
                .and_then(|i| f.get(i))
                .and_then(|s| s.parse::<u32>().ok()),
        });
    }
    Ok(out)
}

fn probe_lms_processes() -> Result<Vec<LmsProcess>> {
    let out = ProcCommand::new("lms")
        .arg("ps")
        .output()
        .map_err(|e| anyhow!("lms ps failed: {e}"))?;
    parse_lms_ps(&String::from_utf8_lossy(&out.stdout))
}

fn short_model(identifier: &str) -> String {
    identifier
        .rsplit('/')
        .next()
        .unwrap_or(identifier)
        .to_lowercase()
        .chars()
        .take(28)
        .collect()
}

/// "Auto-use what's loaded": build the worker pool from the models currently resident on the fleet
/// (`lms ps`) so the swarm runs on what's actually loaded, not (possibly stale) configured model_ids.
/// Returns (pool, planner_model). An empty pool means the fleet has nothing loaded (caller bootstraps
/// or bails). Weights are inherited from a matching configured device by model_id; else default 1.
fn reconcile_pool_with_fleet(cfg: &SwarmConfig) -> (Vec<SwarmDevice>, Option<String>) {
    let procs = match probe_lms_processes() {
        Ok(p) => p,
        Err(_) => return (Vec::new(), None),
    };
    // One worker per DISTINCT loaded identifier (LM Link routes by identifier); first host wins.
    let mut seen = std::collections::HashSet::new();
    let mut resident: Vec<&LmsProcess> = Vec::new();
    for p in &procs {
        if !p.identifier.is_empty() && seen.insert(p.identifier.clone()) {
            resident.push(p);
        }
    }
    if resident.is_empty() {
        return (Vec::new(), None);
    }
    let pool: Vec<SwarmDevice> = resident
        .iter()
        .map(|p| SwarmDevice {
            id: gen_entry_id(cfg, p.device.as_deref(), &p.identifier),
            model_id: p.identifier.clone(),
            // Weight = an explicit configured override for this model_id (user wins), else LM Studio's
            // PARALLEL for this instance so swarm dispatch concurrency tracks the user's LM Studio
            // concurrency setting (set PARALLEL=1 there and a node runs one task at a time), else 1.
            weight: cfg
                .devices
                .iter()
                .find(|d| d.model_id == p.identifier)
                .map(|d| d.weight)
                .or(p.parallel)
                .unwrap_or(1)
                .max(1),
            enabled: true,
            instances: 1,
            host: p.device.clone(),
        })
        .collect();
    // Planner: keep the configured planner if it is resident; else pick the best resident model for the
    // hardest job (the architect skeleton). QUALITY outranks speed here: a low-quant model (q5/q4/q3/q2)
    // fails the structured skeleton, so prefer a NOT-low-quant model FIRST, then the fastest host
    // (highest speed_weight). speed_weight keys match device+identifier (some identifiers omit the host).
    let planner_rank = |p: &&LmsProcess| -> (u8, u32) {
        let ident = p.identifier.to_lowercase();
        let quant_ok = u8::from(
            !(ident.contains("q2_")
                || ident.contains("q3_")
                || ident.contains("q4_")
                || ident.contains("q5")),
        );
        let hay = format!("{} {}", p.device.as_deref().unwrap_or(""), ident);
        let speed = cfg
            .speed_weights
            .iter()
            .find(|(pat, _)| hay.contains(pat.as_str()))
            .map(|(_, w)| *w)
            .unwrap_or(1);
        (quant_ok, speed)
    };
    let planner = if resident.iter().any(|p| p.identifier == cfg.planner_model) {
        Some(cfg.planner_model.clone())
    } else {
        resident
            .iter()
            .filter(|p| {
                let n = p.identifier.to_lowercase();
                n.contains("27b") || n.contains("dense") || n.contains("coder")
            })
            .max_by_key(|p| planner_rank(p))
            .or_else(|| resident.iter().max_by_key(|p| planner_rank(p)))
            .map(|p| p.identifier.clone())
    };
    (pool, planner)
}

fn gen_entry_id(cfg: &SwarmConfig, device: Option<&str>, identifier: &str) -> String {
    let dev = device
        .map(|d| d.split('.').next().unwrap_or(d).to_lowercase())
        .unwrap_or_default();
    let base = if dev.is_empty() {
        short_model(identifier)
    } else {
        format!("{dev}-{}", short_model(identifier))
    };
    let mut id = base.clone();
    let mut n = 2;
    while cfg.devices.iter().any(|d| d.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

struct ImportSummary {
    added: Vec<SwarmDevice>,
    skipped_existing: Vec<String>,
    /// (model_id, kept_device, dropped_device) — same identifier loaded on two hosts.
    skipped_collision: Vec<(String, String, String)>,
}

/// Add loaded models as pool entries. Dedup by identifier first (the SAME identifier on two hosts
/// cannot be routed by LM Link → keep the first, flag the rest), then skip identifiers already pooled.
fn import_processes(
    cfg: &mut SwarmConfig,
    procs: &[LmsProcess],
    default_weight: u32,
    enabled: bool,
) -> ImportSummary {
    let mut summary = ImportSummary {
        added: Vec::new(),
        skipped_existing: Vec::new(),
        skipped_collision: Vec::new(),
    };
    let mut kept: HashMap<String, String> = HashMap::new();
    for p in procs {
        let dev_label = p.device.clone().unwrap_or_else(|| "?".to_string());
        if let Some(prev) = kept.get(&p.identifier) {
            summary
                .skipped_collision
                .push((p.identifier.clone(), prev.clone(), dev_label));
            continue;
        }
        kept.insert(p.identifier.clone(), dev_label);
        if cfg.devices.iter().any(|d| d.model_id == p.identifier) {
            summary.skipped_existing.push(p.identifier.clone());
            continue;
        }
        let dev = SwarmDevice {
            id: gen_entry_id(cfg, p.device.as_deref(), &p.identifier),
            model_id: p.identifier.clone(),
            weight: default_weight.max(1),
            enabled,
            instances: 1,
            host: p.device.clone(),
        };
        cfg.devices.push(dev.clone());
        summary.added.push(dev);
    }
    summary
}

fn print_import_summary(s: &ImportSummary) {
    for d in &s.added {
        println!(
            "  {} {:<14} {}{}",
            style("+ added").green().bold(),
            style(&d.id).bold(),
            style(&d.model_id).dim(),
            d.host
                .as_deref()
                .map(|h| format!("  @{h}"))
                .unwrap_or_default()
        );
    }
    for m in &s.skipped_existing {
        println!("  {} {} (already in pool)", style("· skip").dim(), m);
    }
    for (m, keep, drop) in &s.skipped_collision {
        println!(
            "  {} {} on {} — same model_id already taken by {} (LM Link can't distinguish)",
            style("! collision").red().bold(),
            m,
            drop,
            keep
        );
    }
}

/// GOOSE_SWARM_ASK_REPLAN gate. After the user answers the clarify questions, the swarm can either REUSE the
/// first plan (relying on the answers already appended to research_findings, which every worker prompt injects)
/// or RE-PLAN from scratch with the answers folded in. A full re-plan is a ~15-20min tax (skeleton re-draft +
/// re-detailing every subtask) that only pays off when the answers change the plan STRUCTURE; when the ASK was
/// about semantics the reused plan is identical in shape and the workers still see the answers. DEFAULT is now
/// REUSE (skip the re-plan): an A/B on the same ASKING helpdesk spec (UNIQ12 re-plan vs UNIQ12b skip) produced
/// TWO equally-correct full-win apps while the skip saved ~15min — the re-plan's confidence boost (69->88) did
/// NOT yield a better app. Opt INTO the re-plan with GOOSE_SWARM_ASK_REPLAN=1 (also on/true/yes). N=1 with a
/// draft-variance confound (the skip arm reused a higher-confidence 78 plan), so the default stays a knob.
fn ask_replan_enabled(v: Option<String>) -> bool {
    match v {
        Some(s) => matches!(
            s.trim().to_lowercase().as_str(),
            "1" | "on" | "true" | "yes"
        ),
        None => false,
    }
}

/// integrate-verify runs the PROGRAM end-to-end; it does NOT need the unit-test subtask, and a FAILING test
/// must NOT block it. Otherwise the run reports FAILED while integrate-verify never ran to check whether the
/// app actually works (the dependency-blocked false-negative: observed on UNIQ6, where a failed `tests` task
/// blocked integrate-verify so the app's real bug went uncaught and the run looked failed for the wrong
/// reason). Strip test-subtask ids from integrate-verify's `depends_on` so it runs regardless of the tests;
/// it still depends on the real module/entry subtasks. Returns how many deps were stripped (for logging).
fn strip_integrate_verify_test_deps(plan: &mut serde_json::Value, lang: TargetLang) -> usize {
    let Some(arr) = plan.get("subtasks").and_then(|s| s.as_array()) else {
        return 0;
    };
    let is_test_subtask = |s: &serde_json::Value| -> bool {
        let id = s.get("id").and_then(|i| i.as_str()).unwrap_or("");
        if id == "integrate-verify" {
            return false;
        }
        let files: Vec<&str> = s
            .get("files")
            .and_then(|f| f.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        id.contains("test") || (!files.is_empty() && files.iter().all(|f| lang.is_test_file(f)))
    };
    let test_ids: std::collections::HashSet<String> = arr
        .iter()
        .filter(|s| is_test_subtask(s))
        .filter_map(|s| s.get("id").and_then(|i| i.as_str()).map(String::from))
        .collect();
    if test_ids.is_empty() {
        return 0;
    }
    let mut stripped = 0;
    if let Some(arr) = plan.get_mut("subtasks").and_then(|s| s.as_array_mut()) {
        for s in arr.iter_mut() {
            if s.get("id").and_then(|i| i.as_str()) == Some("integrate-verify") {
                if let Some(deps) = s.get_mut("depends_on").and_then(|d| d.as_array_mut()) {
                    let before = deps.len();
                    deps.retain(|d| d.as_str().map(|x| !test_ids.contains(x)).unwrap_or(true));
                    stripped += before - deps.len();
                }
            }
        }
    }
    stripped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrate_verify_does_not_block_on_tests() {
        // A failing `tests` subtask must not block integrate-verify (UNIQ6: tests failed -> integrate-verify
        // never ran -> the app's real bug went uncaught + the run looked failed for the wrong reason).
        let mut plan: serde_json::Value = serde_json::from_str(
            r#"{"subtasks":[
                {"id":"core","depends_on":[],"files":["core.py"]},
                {"id":"cli-entry","depends_on":["core"],"files":["cli.py"]},
                {"id":"tests","depends_on":["core"],"files":["tests/test_core.py"]},
                {"id":"integrate-verify","depends_on":["core","cli-entry","tests"],"files":[]}
            ]}"#,
        )
        .unwrap();
        let stripped = strip_integrate_verify_test_deps(&mut plan, TargetLang::Python);
        assert_eq!(stripped, 1, "the single 'tests' dep should be stripped");
        let iv = plan["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "integrate-verify")
            .unwrap();
        let deps: Vec<&str> = iv["depends_on"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|d| d.as_str())
            .collect();
        assert_eq!(
            deps,
            vec!["core", "cli-entry"],
            "real module/entry deps stay; tests dep removed"
        );
    }

    #[tokio::test]
    async fn ast_review_counts_from_pkg_import_submodule_as_wired() {
        // A package whose __main__ wires its cli via `from pkg import cli` must NOT be flagged unwired
        // (the observed UNIQ12 false-positive); a genuinely orphaned module still must be. Skips if python3
        // is unavailable (run_ast_review returns ran=false then).
        let dir = std::env::temp_dir().join(format!("goose_ast_review_{}", std::process::id()));
        let pkg = dir.join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("__init__.py"), "").unwrap();
        std::fs::write(pkg.join("__main__.py"), "from pkg import cli\ncli.main()\n").unwrap();
        std::fs::write(pkg.join("cli.py"), "def main():\n    return 0\n").unwrap();
        std::fs::write(pkg.join("orphan.py"), "x = 1\n").unwrap();
        let res = run_ast_review(&dir).await;
        let _ = std::fs::remove_dir_all(&dir);
        if !res.ran {
            return; // python3 not available in this environment
        }
        let joined = res.findings.join("\n");
        assert!(
            !joined.contains("module 'pkg.cli' is imported by no"),
            "cli wired via `from pkg import cli` must not be flagged UNWIRED: {joined}"
        );
        assert!(
            joined.contains("pkg.orphan"),
            "a genuinely orphaned module must still be flagged: {joined}"
        );
    }

    #[test]
    fn multifile_stub_note_fires_only_for_multifile_non_entry() {
        // Multi-file non-entry module (the plan-shopping case) -> stub-first note; entry, single-file, and
        // disabled -> empty.
        let note = multifile_stub_note(
            &["recipes/plan.py".into(), "recipes/shopping.py".into()],
            true,
        );
        assert!(note.contains("STUB-FIRST") && note.contains("COMPILING STUB"));
        // A file set that includes the entry is covered by skeleton_note -> empty here.
        assert!(multifile_stub_note(&["pkg/cli.py".into(), "pkg/util.py".into()], true).is_empty());
        assert!(
            multifile_stub_note(&["pkg/__main__.py".into(), "pkg/x.py".into()], true).is_empty()
        );
        // Single-file -> empty (skeleton-first was a wash on simple single-file tasks).
        assert!(multifile_stub_note(&["pkg/only.py".into()], true).is_empty());
        // Disabled -> empty.
        assert!(multifile_stub_note(&["a.py".into(), "b.py".into()], false).is_empty());
    }

    #[test]
    fn cli_contract_note_fires_only_for_entry_when_enabled() {
        // Entry file + enabled -> a non-empty CLI-structure contract mentioning nested/global/units.
        let note = cli_contract_note(true, true);
        assert!(note.contains("CLI STRUCTURE CONTRACT"));
        assert!(note.contains("NESTED") && note.contains("GLOBAL"));
        // POSITIONAL-vs-flag + no-rename rules (UNIQ16 drifted positionals to flags + renamed --from/--to).
        assert!(note.contains("POSITIONAL") && note.contains("do NOT rename"));
        // Keyword-subcommand-name rule (UNIQ26 registered `import_` for the spec's `import` -> `store import` failed).
        assert!(note.contains("reserved word") && note.contains("add_parser(\"import\")"));
        // Disabled, or no entry file among the owned set -> empty (no-op, byte-identical default-off path).
        assert!(cli_contract_note(true, false).is_empty());
        assert!(cli_contract_note(false, true).is_empty());
    }

    #[test]
    fn ask_replan_defaults_off_and_opts_in() {
        // Default (unset) now REUSES the plan (skip the re-plan, per the UNIQ12/UNIQ12b A/B); opt INTO the
        // re-plan only with an explicit on-value.
        assert!(
            !ask_replan_enabled(None),
            "unset defaults OFF (reuse plan / skip re-plan)"
        );
        assert!(ask_replan_enabled(Some("1".into())));
        assert!(ask_replan_enabled(Some("on".into())));
        assert!(ask_replan_enabled(Some("true".into())));
        assert!(ask_replan_enabled(Some("YES".into())), "case-insensitive");
        assert!(!ask_replan_enabled(Some("0".into())));
        assert!(!ask_replan_enabled(Some("off".into())));
        assert!(
            !ask_replan_enabled(Some("anything".into())),
            "unknown values skip — only explicit on-values re-plan"
        );
        assert!(!ask_replan_enabled(Some(" no ".into())));
    }

    #[test]
    fn score_skeleton_prefers_wider_flatter_plan() {
        let wc = 3;
        let wide = goose_swarm::specs_from_plan_json(
            r#"{"subtasks":[
                {"id":"a","depends_on":[],"files":["a.py"]},
                {"id":"b","depends_on":[],"files":["b.py"]},
                {"id":"c","depends_on":[],"files":["c.py"]},
                {"id":"integrate-verify","depends_on":["a","b","c"],"files":["t.py"]}
            ]}"#,
        )
        .unwrap();
        let deep = goose_swarm::specs_from_plan_json(
            r#"{"subtasks":[
                {"id":"a","depends_on":[],"files":["a.py"]},
                {"id":"b","depends_on":["a"],"files":["b.py"]},
                {"id":"c","depends_on":["b"],"files":["c.py"]},
                {"id":"integrate-verify","depends_on":["a","b","c"],"files":["t.py"]}
            ]}"#,
        )
        .unwrap();
        let sw = score_skeleton(&wide, wc).unwrap();
        let sd = score_skeleton(&deep, wc).unwrap();
        assert!(sw > sd, "wider/flatter should win: wide={sw} deep={sd}");
        // a dep on an unknown task is not a valid DAG -> None (so it can never be picked).
        let bad = goose_swarm::specs_from_plan_json(
            r#"{"subtasks":[{"id":"x","depends_on":["nope"],"files":["x.py"]}]}"#,
        )
        .unwrap();
        assert!(score_skeleton(&bad, wc).is_none());
    }

    #[test]
    fn parse_confidence_extracts_score_and_uncertainties() {
        assert_eq!(
            parse_confidence("72|missing error handling; no CLI test").unwrap(),
            (72, "missing error handling; no CLI test".to_string())
        );
        assert_eq!(
            parse_confidence("SCORE: 85 | parser edge cases").unwrap().0,
            85
        );
        assert_eq!(parse_confidence("100").unwrap(), (100, String::new()));
        // a stray "out of 100" must not be read as the score — first integer wins.
        assert_eq!(
            parse_confidence("I rate it 40 out of 100|risky").unwrap().0,
            40
        );
        // clamp + no-digit guard.
        assert_eq!(parse_confidence("130|over").unwrap().0, 100);
        assert!(parse_confidence("no digits here").is_none());
    }

    #[test]
    fn parse_judge_reply_handles_qwen_formats() {
        // Healthy: qwen echoes the field labels and reorders OK/HIGH/LOW — all must read OK (no kill).
        for ok in [
            "VERDICT|CONFIDENCE|OK|HIGH",
            "VERDICT|OK|LOW|",
            "VERDICT|CONFIDENCE|HIGH|OK",
            "VERDICT|LOW|",
            "VERDICT|HIGH|OK|done",
        ] {
            assert_eq!(
                parse_judge_reply(ok).verdict,
                Verdict::Ok,
                "should be OK: {ok}"
            );
        }
        // A real catch with NO verdict keyword — just HIGH + a corrective hint — must become actionable
        // (this is the qwen format that was silently dropped before).
        let caught = parse_judge_reply(
            "VERDICT|HIGH|STOP retrying failing commands — write rules.py directly with a parser",
        );
        assert_ne!(
            caught.verdict,
            Verdict::Ok,
            "keyword-less HIGH+hint must act"
        );
        assert!(caught.confidence >= 0.8);
        assert!(
            caught.hint.contains("rules.py"),
            "hint must be the correction, not an echoed label"
        );
        // Explicit keyword still classifies, and the hint skips echoed labels.
        let oread = parse_judge_reply("VERDICT|CONFIDENCE|OVER_READING|HIGH|write the file now");
        assert_eq!(oread.verdict, Verdict::OverReading);
        assert_eq!(oread.hint, "write the file now");
        // HIGH but no real correction -> stays OK (a vague reply can never kill a healthy worker).
        assert_eq!(parse_judge_reply("VERDICT|HIGH|").verdict, Verdict::Ok);
    }

    #[test]
    fn smoke_pytest_collect_interpretation() {
        use CollectVerdict::*;
        assert_eq!(interpret_pytest_collect(Some(0), "collected 12 items"), Ok);
        // exit 5 = "no tests collected" — not an error.
        assert_eq!(
            interpret_pytest_collect(Some(5), "no tests ran in 0.01s"),
            Ok
        );
        // pytest not installed -> inconclusive, never a failure.
        assert_eq!(
            interpret_pytest_collect(Some(1), "ModuleNotFoundError: No module named pytest"),
            PytestMissing
        );
        // a real collection error becomes a finding carrying the traceback tail.
        match interpret_pytest_collect(
            Some(2),
            "ERROR collecting foo.py\nImportError: cannot import name 'bar' from 'baz'",
        ) {
            Errors(t) => assert!(t.contains("ImportError"), "tail must carry the error: {t}"),
            other => panic!("expected Errors, got {other:?}"),
        }
    }

    #[test]
    fn smoke_pytest_run_interpretation() {
        use TestRunVerdict::*;
        // all pass.
        assert_eq!(interpret_pytest_run(Some(0), "12 passed in 0.3s"), Pass);
        // exit 5 = no tests collected -> inconclusive, not a failure.
        assert_eq!(
            interpret_pytest_run(Some(5), "no tests ran in 0.01s"),
            NoTests
        );
        // pytest not installed -> inconclusive, never a failure.
        assert_eq!(
            interpret_pytest_run(Some(1), "ModuleNotFoundError: No module named 'pytest'"),
            PytestMissing
        );
        // a real RUNTIME failure (the class --help/collect-only miss) becomes a finding with the tail.
        match interpret_pytest_run(
            Some(1),
            "test_nested_rollback FAILED\nsqlite3.ProgrammingError: You can only execute one statement at a time",
        ) {
            Failures(t) => assert!(
                t.contains("ProgrammingError"),
                "tail must carry the runtime failure: {t}"
            ),
            other => panic!("expected Failures, got {other:?}"),
        }
    }

    #[test]
    fn smoke_entry_package_detection() {
        // top-level package with __main__.py is runnable via `python3 -m pkg`.
        assert_eq!(
            entry_package_from_paths(&[
                "chaos_fern/ifs.py".into(),
                "chaos_fern/__main__.py".into()
            ]),
            Some("chaos_fern".to_string())
        );
        // a src/ layout is also detected.
        assert_eq!(
            entry_package_from_paths(&["src/byte_oracle/__main__.py".into()]),
            Some("byte_oracle".to_string())
        );
        // a flat tree with no __main__.py package -> no `-m` entry point (a finding).
        assert_eq!(
            entry_package_from_paths(&["cli.py".into(), "models.py".into()]),
            None
        );
        // a root-level __main__.py is NOT a `-m` package (needs a different invocation).
        assert_eq!(entry_package_from_paths(&["__main__.py".into()]), None);
    }

    #[test]
    fn smoke_tail_lines_keeps_last_nonblank_in_order() {
        let s = "a\n\nb\nc\n\n";
        assert_eq!(tail_lines(s, 2), "b\nc");
        assert_eq!(tail_lines(s, 10), "a\nb\nc");
    }

    #[tokio::test]
    async fn fanout_caps_one_call_per_device() {
        use std::sync::atomic::AtomicUsize;
        let devices = vec!["d0".to_string(), "d1".to_string(), "d2".to_string()];
        let max_per_device = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
        let inflight = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
        let total = Arc::new(AtomicUsize::new(0));
        let max_total = Arc::new(AtomicUsize::new(0));
        let items: Vec<usize> = (0..9).collect();
        let (mpd, inf, tot, mtot) = (
            max_per_device.clone(),
            inflight.clone(),
            total.clone(),
            max_total.clone(),
        );
        let results = fanout_over_fleet(devices, items, move |i, dev| {
            let (mpd, inf, tot, mtot) = (mpd.clone(), inf.clone(), tot.clone(), mtot.clone());
            async move {
                let cur = {
                    let mut g = inf.lock().unwrap();
                    let e = g.entry(dev.clone()).or_insert(0);
                    *e += 1;
                    *e
                };
                {
                    let mut m = mpd.lock().unwrap();
                    let e = m.entry(dev.clone()).or_insert(0);
                    if cur > *e {
                        *e = cur;
                    }
                }
                let t = tot.fetch_add(1, Ordering::SeqCst) + 1;
                mtot.fetch_max(t, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                tot.fetch_sub(1, Ordering::SeqCst);
                *inf.lock().unwrap().get_mut(&dev).unwrap() -= 1;
                i * 2
            }
        })
        .await;
        assert_eq!(results.len(), 9, "every item returns a result");
        for (dev, &m) in max_per_device.lock().unwrap().iter() {
            assert!(
                m <= 1,
                "device {dev} ran {m} concurrent calls; must be <= 1"
            );
        }
        assert!(
            max_total.load(Ordering::SeqCst) <= 3,
            "no more than 3 concurrent across a 3-device fleet"
        );
        assert_eq!(
            max_per_device.lock().unwrap().len(),
            3,
            "work-stealing should use every device"
        );
    }

    #[test]
    fn model_active_params_and_weak_bump() {
        // MoE active marker wins over the dense total (a3b = 3B active, weaker than the 35B total).
        assert_eq!(model_active_params_b("qwen/qwen3.6-35b-a3b"), Some(3));
        // dense size when there is no active marker.
        assert_eq!(model_active_params_b("qwen/qwen3.6-27b"), Some(27));
        assert_eq!(model_active_params_b("llama-3.1-8b-instruct"), Some(8));
        // Mixtral-style NxMb -> the per-expert size M as the active proxy.
        assert_eq!(model_active_params_b("mixtral-8x7b-instruct"), Some(7));
        assert_eq!(model_active_params_b("some-unsized-model"), None);
        // Weaker (fewer active params) -> bigger bump (ask sooner); strong -> no bump.
        assert_eq!(ask_floor_weak_bump(Some(27)), 5);
        assert_eq!(ask_floor_weak_bump(Some(3)), 15); // a3b MoE
        assert_eq!(ask_floor_weak_bump(Some(70)), 0); // strong
        assert_eq!(ask_floor_weak_bump(None), 5);
        assert!(ask_floor_weak_bump(Some(3)) > ask_floor_weak_bump(Some(27)));
    }

    #[test]
    fn detect_language_defaults_python_and_honors_cues() {
        // No cue -> Python (the validated baseline default).
        assert_eq!(
            detect_language("a CLI markdown to HTML renderer", &[]),
            TargetLang::Python
        );
        // Explicit spec cues win.
        assert_eq!(
            detect_language("build a TypeScript CLI todo app", &[]),
            TargetLang::TypeScript
        );
        assert_eq!(
            detect_language("a Rust CLI using cargo", &[]),
            TargetLang::Rust
        );
        assert_eq!(
            detect_language("a golang command line tool", &[]),
            TargetLang::Go
        );
        // A named-but-unprofiled language is honored (generic), never forced to Python.
        assert_eq!(detect_language("a Ruby CLI gem", &[]), TargetLang::Other);
        // APP8 regression: an explicit LANG=Python wins over ".json" (which contains ".js") — previously
        // mis-detected as TypeScript and the JSON validator was built in the wrong language.
        assert_eq!(
            detect_language(
                "LANG=Python — a CLI JSON-schema validator: validate SCHEMA.json DATA.json",
                &[]
            ),
            TargetLang::Python
        );
        // ".json" with no explicit language is NOT TypeScript (word-boundary ext match) -> default Python.
        assert_eq!(
            detect_language("a CLI that reads config.json and prints a report", &[]),
            TargetLang::Python
        );
        // a real .js file mention IS TypeScript; node.js name IS TypeScript.
        assert_eq!(
            detect_language("a CLI whose entry is bin/cli.js", &[]),
            TargetLang::TypeScript
        );
        assert_eq!(
            detect_language("a node.js CLI that validates data.json", &[]),
            TargetLang::TypeScript
        );
        // Amendment: the existing files' extensions are the strongest signal, overriding a bare spec.
        assert_eq!(
            detect_language("add a --json flag", &["index.ts".into(), "util.ts".into()]),
            TargetLang::TypeScript
        );
        assert_eq!(
            detect_language(
                "add a --json flag",
                &["cli.py".into(), "detector.py".into()]
            ),
            TargetLang::Python
        );
    }

    #[test]
    fn target_lang_profile_python_is_unchanged_others_translate() {
        // Python keeps the exact original scaffolding and an EMPTY directive (prompt byte-identical).
        assert!(TargetLang::Python.directive().is_empty());
        assert!(TargetLang::Python.entry_clause().contains("cli.py"));
        assert_eq!(TargetLang::Python.test_cmd(), "python3 -m pytest");
        // Non-Python: forceful directive + language-correct entry point + test runner, no Python mandate.
        let ts = TargetLang::TypeScript;
        assert!(ts.directive().contains("TARGET LANGUAGE: TypeScript"));
        assert!(ts.entry_clause().contains("index.ts") && !ts.entry_clause().contains("cli.py"));
        assert!(ts.test_cmd().contains("vitest") || ts.test_cmd().contains("npm"));
        assert!(TargetLang::Rust.entry_clause().contains("main.rs"));
        assert_eq!(TargetLang::Go.test_cmd(), "go test ./...");
        // Source/test-file predicates: Python arm = the original `.py` behavior; non-Python is language-correct.
        assert!(
            TargetLang::Python.is_source_file("foo.py")
                && !TargetLang::Python.is_source_file("foo.ts")
        );
        assert!(
            ts.is_source_file("foo.ts")
                && ts.is_source_file("foo.tsx")
                && !ts.is_source_file("foo.py")
        );
        assert!(
            TargetLang::Python.is_test_file("test_foo.py")
                && TargetLang::Python.is_test_file("conftest.py")
        );
        assert!(ts.is_test_file("foo.test.ts") && !ts.is_test_file("foo.ts"));
        assert!(!TargetLang::Other.is_source_file("foo.rb"));
    }

    #[test]
    fn parse_ast_review_reads_findings_and_degrades() {
        let r = parse_ast_review(
            r#"{"modules": 6, "findings": ["app.orphan is imported by no non-test module"]}"#,
        );
        assert!(r.ran);
        assert_eq!(r.modules, 6);
        assert_eq!(r.findings.len(), 1);
        // a reviewer hiccup (non-JSON stderr/traceback) must degrade to ran=false, never a hard failure.
        let bad = parse_ast_review("Traceback (most recent call last): SyntaxError");
        assert!(!bad.ran);
        assert!(bad.findings.is_empty());
    }

    #[test]
    fn ast_fix_description_carries_unwired_findings() {
        let d = ast_fix_description(&[
            "module 'sched.store' is imported by no non-test module — built-but-unwired"
                .to_string(),
        ]);
        assert!(d.contains("sched.store"));
        assert!(d.contains("WIRE"));
        assert!(d.contains("SMALLEST"));
    }

    #[test]
    fn ast_fix_description_carries_stub_findings() {
        let d = ast_fix_description(&[
            "function 'compute_total' in module 'app.core' is a STUB/UNIMPLEMENTED (body is only pass / ... \
             / raise NotImplementedError / a docstring) — implement it FULLY per the spec"
                .to_string(),
        ]);
        assert!(d.contains("app.core") && d.contains("compute_total"));
        assert!(d.contains("IMPLEMENT"));
        assert!(d.contains("STUB"));
        assert!(d.contains("SMALLEST"));
    }

    #[test]
    fn smoke_fix_description_carries_findings_and_targets() {
        let d = smoke_fix_description(
            &[
                "pytest --collect-only errors: ImportError cannot import name bar from baz"
                    .to_string(),
            ],
            TargetLang::Python,
        );
        assert!(d.contains("ImportError cannot import name bar from baz"));
        assert!(d.contains("--collect-only"));
        assert!(d.contains("--help"));
        assert!(d.contains("SMALLEST"));
        // Language-aware: the TS variant names the npm build + node entry, not pytest.
        let ts = smoke_fix_description(
            &["`npm run build` failed".to_string()],
            TargetLang::TypeScript,
        );
        assert!(ts.contains("npm run build"));
        assert!(ts.contains("node "));
        assert!(!ts.contains("pytest"));
    }

    #[test]
    fn ts_entry_detection_and_crash_signature() {
        // bin as an object -> first value, ./ stripped.
        let pkg: serde_json::Value =
            serde_json::from_str(r#"{"bin":{"calc":"./dist/bin/calc.js"}}"#).unwrap();
        assert_eq!(
            ts_entry_from_package_json(&pkg),
            Some("dist/bin/calc.js".to_string())
        );
        // main fallback.
        let pkg2: serde_json::Value = serde_json::from_str(r#"{"main":"dist/index.js"}"#).unwrap();
        assert_eq!(
            ts_entry_from_package_json(&pkg2),
            Some("dist/index.js".to_string())
        );
        // scripts.start -> the source-file token, incl. a tsx .ts entry (run-script apps).
        let pkg3: serde_json::Value =
            serde_json::from_str(r#"{"scripts":{"start":"node dist/bin/calc.js"}}"#).unwrap();
        assert_eq!(
            ts_entry_from_package_json(&pkg3),
            Some("dist/bin/calc.js".to_string())
        );
        let pkg3b: serde_json::Value =
            serde_json::from_str(r#"{"scripts":{"start":"tsx src/index.ts"}}"#).unwrap();
        assert_eq!(
            ts_entry_from_package_json(&pkg3b),
            Some("src/index.ts".to_string())
        );
        // nothing declared.
        let pkg4: serde_json::Value = serde_json::from_str(r#"{"name":"x"}"#).unwrap();
        assert_eq!(ts_entry_from_package_json(&pkg4), None);
        assert!(has_run_script(&pkg3b));
        assert!(!has_run_script(&pkg4));
        // a real stack frame (`at ... :line`) vs help prose ("at least one of") that also starts with "at ".
        assert!(has_stack_frame(
            "RangeError: bad\n    at tokenize (/x/dist/tok.js:5:11)"
        ));
        assert!(!has_stack_frame(
            "Usage:\n    at least one of --foo/--bar is required"
        ));
        // crash = NONZERO exit AND a stack frame. A clean exit (success=true) is never a crash, even with a
        // stack-looking line; a clean nonzero rejection with no frame is not a crash either.
        assert!(looks_like_runtime_crash(
            "RangeError: Invalid array length\n    at tokenize (/x/dist/tok.js:5:11)",
            false
        ));
        assert!(!looks_like_runtime_crash(
            "RangeError: Invalid array length\n    at tokenize (/x/dist/tok.js:5:11)",
            true // exited 0 -> not a crash
        ));
        assert!(!looks_like_runtime_crash(
            "error: unknown flag --help",
            false
        ));
        assert!(looks_like_rust_panic(
            "thread 'main' panicked at src/main.rs:10:5:\nindex out of bounds",
            false
        ));
        assert!(!looks_like_rust_panic("thread 'main' panicked at x", true));
        assert!(!looks_like_rust_panic("Usage: mycli [OPTIONS]", false));
    }

    #[test]
    fn speculative_copy_helpers_isolated_and_traversal_guarded() {
        use std::fs;
        let base = tempfile::TempDir::new().unwrap();
        let real = base.path().join("real");
        fs::create_dir_all(real.join("sub")).unwrap();
        fs::create_dir_all(real.join("node_modules")).unwrap();
        fs::write(real.join("a.py"), "real-a").unwrap();
        fs::write(real.join("sub/b.py"), "real-b").unwrap();
        fs::write(real.join("node_modules/junk.js"), "junk").unwrap();
        // shadow = cp -r excluding heavy dirs.
        let shadow = base.path().join("shadow");
        copy_tree_excluding(&real, &shadow).unwrap();
        assert_eq!(fs::read_to_string(shadow.join("a.py")).unwrap(), "real-a");
        assert_eq!(
            fs::read_to_string(shadow.join("sub/b.py")).unwrap(),
            "real-b"
        );
        assert!(
            !shadow.join("node_modules").exists(),
            "heavy dirs excluded from the shadow"
        );
        // the twin edits a.py + adds c.py inside the shadow only.
        fs::write(shadow.join("a.py"), "twin-a").unwrap();
        fs::write(shadow.join("c.py"), "twin-c").unwrap();
        // BEFORE promote: the real tree is untouched by the twin's shadow writes.
        assert_eq!(fs::read_to_string(real.join("a.py")).unwrap(), "real-a");
        assert!(!real.join("c.py").exists());
        // promote ONLY the owned files.
        let n = copy_owned_files(&shadow, &real, &["a.py".to_string(), "c.py".to_string()]);
        assert_eq!(n, 2);
        assert_eq!(fs::read_to_string(real.join("a.py")).unwrap(), "twin-a");
        assert_eq!(fs::read_to_string(real.join("c.py")).unwrap(), "twin-c");
        assert_eq!(
            fs::read_to_string(real.join("sub/b.py")).unwrap(),
            "real-b",
            "a NON-owned file is never touched by promote"
        );
        // path-traversal / absolute paths are rejected -> nothing written outside the real tree.
        fs::write(shadow.join("evil"), "evil").unwrap();
        let n2 = copy_owned_files(
            &shadow,
            &real,
            &["../SENTINEL".to_string(), "/tmp/abs-escape".to_string()],
        );
        assert_eq!(n2, 0, "traversal + absolute owned paths are rejected");
        assert!(
            !base.path().join("SENTINEL").exists(),
            "promote never escapes the real tree"
        );
    }

    #[test]
    fn frozen_interfaces_block_noop_when_empty() {
        assert_eq!(frozen_interfaces_block(""), "");
        assert_eq!(frozen_interfaces_block("   \n  "), "");
        let block = frozen_interfaces_block("def add(a: int, b: int) -> int: ...");
        assert!(block.contains("FROZEN MODULE INTERFACES"));
        assert!(
            block.contains("def add(a: int, b: int) -> int: ..."),
            "the stub bundle must be embedded verbatim"
        );
    }

    #[test]
    fn render_pillars_block_noop_when_empty_else_renders() {
        // Empty -> empty string, so the GOOSE_SWARM_GOALS injection is a true no-op (byte-identical off-path).
        assert_eq!(render_pillars_block(&Pillars::default()), "");
        let p = Pillars {
            pillars: vec![Pillar {
                id: "P1".to_string(),
                goal: "The command is invoked as `report budget`, not `budget report`.".to_string(),
                check: None,
            }],
        };
        let block = render_pillars_block(&p);
        assert!(block.contains("APP PILLARS"));
        assert!(
            block.contains("P1: The command is invoked as `report budget`"),
            "each pillar's goal must be embedded verbatim"
        );
    }

    #[test]
    fn complete_rounds_defaults_and_clamps() {
        assert_eq!(complete_rounds_from(None), 2); // default
        assert_eq!(complete_rounds_from(Some("4".to_string())), 4);
        assert_eq!(complete_rounds_from(Some("99".to_string())), 6); // clamped high
        assert_eq!(complete_rounds_from(Some("0".to_string())), 1); // clamped low
        assert_eq!(complete_rounds_from(Some("nope".to_string())), 2); // unparseable -> default
    }

    #[test]
    fn extract_file_prefers_source_over_test_and_none_when_absent() {
        let f = "tests/test_cli.py:9: in test_add\n    from spendlog.cli import main\n\
                 spendlog/cli.py:3: in <module>\n    import missing\nE   ModuleNotFoundError"
            .to_string();
        assert_eq!(
            extract_file_from_finding(&f).as_deref(),
            Some("spendlog/cli.py"),
            "a non-test source frame is the fix target"
        );
        assert_eq!(
            extract_file_from_finding("no python3 -m app entry point found"),
            None
        );
        // `File "path", line N` shape (pytest full traceback / rust).
        assert_eq!(
            extract_file_from_finding("File \"app/core.py\", line 7, in run").as_deref(),
            Some("app/core.py")
        );
    }

    #[test]
    fn group_findings_by_file_partitions_dedups_and_serializes() {
        let findings = vec![
            "tests/test_a.py:5: in test_x\n    assert foo() == 1\nE   AssertionError".to_string(),
            "spendlog/cli.py:12: in cmd_add\n    raise ValueError\nE   ValueError: boom"
                .to_string(),
            "spendlog/cli.py:40: in cmd_budget\n    x\nE   KeyError".to_string(), // SAME file
            "spendlog/cli.py:12: in cmd_add\n    raise ValueError\nE   ValueError: boom"
                .to_string(), // dup
            "no python3 -m spendlog entry point found".to_string(),               // unassigned
        ];
        let (groups, unassigned) = group_findings_by_file(&findings);
        let cli = groups
            .iter()
            .find(|g| g.file == "spendlog/cli.py")
            .expect("cli.py group");
        // The two DISTINCT cli.py findings collapse into ONE group (same-file serialize); the dup is dropped.
        assert_eq!(cli.findings.len(), 2);
        assert!(groups.iter().any(|g| g.file == "tests/test_a.py"));
        assert_eq!(unassigned.len(), 1); // the file-less finding
                                         // Partition invariant: every group names a distinct file.
        let files: std::collections::HashSet<_> = groups.iter().map(|g| &g.file).collect();
        assert_eq!(files.len(), groups.len());
    }

    #[test]
    fn scout_lenses_select_correctly() {
        // greenfield drops the amendment-only `codebase` lens.
        let g: Vec<&str> = select_lenses(false, 4).iter().map(|l| l.id).collect();
        assert_eq!(g, vec!["libraries", "architecture", "edge-cases"]);
        // amendments include codebase, and it is first so a low clamp keeps it.
        let a: Vec<&str> = select_lenses(true, 4).iter().map(|l| l.id).collect();
        assert_eq!(
            a,
            vec!["codebase", "libraries", "architecture", "edge-cases"]
        );
        assert_eq!(
            select_lenses(true, 2)
                .iter()
                .map(|l| l.id)
                .collect::<Vec<_>>(),
            vec!["codebase", "libraries"]
        );
        // max clamps up to at least 1 even if 0 is passed.
        assert_eq!(select_lenses(false, 0).len(), 1);
    }

    // Captured verbatim from a real `lms ps` (5 models; the macbook 'Local' hosts two).
    const FIXTURE: &str = "\nIDENTIFIER                                      MODEL                                           STATUS        SIZE        CONTEXT    PARALLEL    DEVICE                TTL     \nqwen/qwen3.6-27b                                qwen/qwen3.6-27b                                GENERATING    29.53 GB    262144     4           WorksMacStudio.lan    1h / 1h \nqwen/qwen3.6-35b-a3b                            qwen/qwen3.6-35b-a3b                            IDLE          29.09 GB    200000     4           Mac.lan                       \nqwen3.6-35b-a3b-mtp-holo3-qwopus-qx86-hi-mlx    qwen3.6-35b-a3b-mtp-holo3-qwopus-qx86-hi-mlx    IDLE          39.51 GB    128000     4           Local                         \nqwopus3.6-27b-coder-mlx                         qwopus3.6-27b-coder-mlx                         IDLE          28.60 GB    128000     4           Local                         \nqwopus3.6-35b-a3b-v1-mtp                        qwopus3.6-35b-a3b-v1-mtp                        IDLE          38.70 GB    262144     4           WorksMacStudio.lan    17m / 1h\n";

    fn empty_cfg() -> SwarmConfig {
        SwarmConfig {
            devices: vec![],
            ..SwarmConfig::default()
        }
    }

    #[test]
    fn parses_real_lms_ps_fixture() {
        let procs = parse_lms_ps(FIXTURE).unwrap();
        assert_eq!(procs.len(), 5);
        assert_eq!(procs[0].identifier, "qwen/qwen3.6-27b");
        assert_eq!(procs[0].status, "GENERATING");
        assert_eq!(procs[0].device.as_deref(), Some("WorksMacStudio.lan"));
        assert_eq!(
            procs[0].parallel,
            Some(4),
            "reads the PARALLEL column as the device weight source"
        );
        let local = procs
            .iter()
            .filter(|p| p.device.as_deref() == Some("Local"))
            .count();
        assert_eq!(local, 2, "the macbook (Local) hosts two distinct models");
    }

    #[test]
    fn import_adds_all_distinct_models() {
        let mut cfg = empty_cfg();
        let procs = parse_lms_ps(FIXTURE).unwrap();
        let s = import_processes(&mut cfg, &procs, 2, true);
        assert_eq!(s.added.len(), 5);
        assert!(s.skipped_existing.is_empty());
        assert!(s.skipped_collision.is_empty());
        assert_eq!(cfg.devices.len(), 5);
        assert!(cfg
            .devices
            .iter()
            .all(|d| d.weight == 2 && d.host.is_some()));
    }

    #[test]
    fn import_skips_existing_and_flags_collision() {
        let mut cfg = empty_cfg();
        cfg.devices.push(SwarmDevice {
            id: "x".into(),
            model_id: "qwen/qwen3.6-27b".into(),
            weight: 1,
            enabled: true,
            instances: 1,
            host: None,
        });
        let procs = vec![
            LmsProcess {
                identifier: "qwen/qwen3.6-27b".into(),
                status: "IDLE".into(),
                device: Some("Mac.lan".into()),
                parallel: None,
            },
            LmsProcess {
                identifier: "dup-model".into(),
                status: "IDLE".into(),
                device: Some("Mac.lan".into()),
                parallel: None,
            },
            LmsProcess {
                identifier: "dup-model".into(),
                status: "IDLE".into(),
                device: Some("Local".into()),
                parallel: None,
            },
        ];
        let s = import_processes(&mut cfg, &procs, 1, true);
        assert_eq!(s.skipped_existing, vec!["qwen/qwen3.6-27b".to_string()]);
        assert_eq!(s.added.len(), 1);
        assert_eq!(s.skipped_collision.len(), 1);
    }

    #[test]
    fn missing_header_errors() {
        assert!(parse_lms_ps("no table here\njust text").is_err());
    }
}

// ---------------------------------------------------------------------------------------------
// Structured observability — JSONL event log + per-run capture
// ---------------------------------------------------------------------------------------------

/// A tool is from an MCP extension (vs a goose builtin) if it is namespaced `{ext}__{tool}` and the
/// prefix is not a known builtin/platform namespace.
fn is_mcp_tool(name: &str) -> bool {
    name.contains("__")
        && !name.starts_with("developer__")
        && !name.starts_with("recipe__")
        && !name.starts_with("platform__")
}

/// Per-run JSONL event sink. All writes go through one locked, line-flushed writer; a monotonic
/// `seq` gives a total order even though scheduler events and CLI-native events interleave.
struct JsonlSink {
    writer: Mutex<std::io::BufWriter<std::fs::File>>,
    run_id: String,
    seq: AtomicU64,
}

impl JsonlSink {
    fn new(path: &Path, run_id: String) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self {
            writer: Mutex::new(std::io::BufWriter::new(file)),
            run_id,
            seq: AtomicU64::new(0),
        })
    }

    fn write_line(&self, mut value: serde_json::Value) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "ts".to_string(),
                serde_json::json!(chrono::Utc::now().to_rfc3339()),
            );
            obj.insert("run_id".to_string(), serde_json::json!(self.run_id));
            obj.insert("seq".to_string(), serde_json::json!(seq));
        }
        if let Ok(mut w) = self.writer.lock() {
            let _ = serde_json::to_writer(&mut *w, &value);
            let _ = w.write_all(b"\n");
            let _ = w.flush();
        }
    }
}

impl EventSink for JsonlSink {
    fn emit(&self, event: &SwarmEvent) {
        if let Ok(value) = serde_json::to_value(event) {
            self.write_line(value);
        }
    }
    fn write_value(&self, value: serde_json::Value) {
        self.write_line(value);
    }
}

/// What `run_agent` returns: streamed text, the typed final_output (if any), the session id for
/// trace lookup, and the tool calls the agent made.
struct RunAgentOut {
    text: String,
    final_output: Option<String>,
    session_id: String,
    tool_calls: Vec<ToolCallRecord>,
}

#[derive(Clone)]
struct ResearchQuestion {
    id: String,
    question: String,
    kind: String,
}

struct ResearchFinding {
    question: String,
    kind: String,
    findings: String,
}

/// A fixed research angle a SCOUT investigates in parallel (no serial scoping call needed). The
/// `codebase` lens is amendment-only; it is listed first so it survives a low `max` clamp.
struct ScoutLens {
    id: &'static str,
    title: &'static str,
    brief: &'static str,
    tool_hint: &'static str,
    amendment_only: bool,
}

const SCOUT_LENSES: &[ScoutLens] = &[
    ScoutLens {
        id: "codebase",
        title: "Existing codebase",
        brief: "Investigate the EXISTING code in the working directory: structure, key files, conventions, and exactly where the requested change must hook in.",
        tool_hint: "Use the developer shell tools (ls, grep, cat) to read the existing code.",
        amendment_only: true,
    },
    ScoutLens {
        id: "libraries",
        title: "Libraries & APIs",
        brief: "Identify the key libraries/frameworks this task needs and look up their REAL current API: function/class names, signatures, minimal usage snippets, and gotchas.",
        tool_hint: "Use the context7 tools (resolve-library-id then get-library-docs) and web-search.",
        amendment_only: false,
    },
    ScoutLens {
        id: "architecture",
        title: "Architecture & data model",
        brief: "Propose the module/file breakdown, the data model/types, and how the pieces fit — a skeleton the planner can decompose from.",
        tool_hint: "Reason from the task; use web-search only to confirm conventions.",
        amendment_only: false,
    },
    ScoutLens {
        id: "edge-cases",
        title: "Edge cases & testing",
        brief: "Enumerate the tricky edge cases, failure modes, and the concrete tests that would prove the task is done correctly.",
        tool_hint: "Reason from the task; use web-search for domain specifics if needed.",
        amendment_only: false,
    },
];

/// The lenses to run for this task: drop amendment-only lenses on greenfield, then clamp to `max`.
fn select_lenses(is_amendment: bool, max: u32) -> Vec<&'static ScoutLens> {
    SCOUT_LENSES
        .iter()
        .filter(|l| !l.amendment_only || is_amendment)
        .take(max.max(1) as usize)
        .collect()
}

/// M6 plan confidence via SELF-CONSISTENCY: how much the N drafted skeleton candidates AGREE on shape.
/// Verbalized self-confidence is overconfident, but agreement across independent drafts is a calibrated
/// signal — when the drafts diverge, the model doesn't really know how to decompose this (a cue to
/// research more before committing). Pure-Rust, 0–100, plus a one-line reason.
fn plan_agreement(candidates: &[Vec<goose_swarm::TaskSpec>]) -> (u8, String) {
    if candidates.len() < 2 {
        return (60, "single draft — no cross-check".to_string());
    }
    // Subtask-count agreement (tight spread = high).
    let counts: Vec<usize> = candidates.iter().map(Vec::len).collect();
    let spread = counts.iter().max().unwrap() - counts.iter().min().unwrap();
    let count_score: u32 = match spread {
        0 => 40,
        1 => 28,
        2..=3 => 14,
        _ => 0,
    };
    // Owned-file-set agreement: mean pairwise Jaccard across candidates.
    let file_sets: Vec<std::collections::BTreeSet<&str>> = candidates
        .iter()
        .map(|c| {
            c.iter()
                .flat_map(|t| t.owned_files.iter().map(String::as_str))
                .collect()
        })
        .collect();
    let mut jsum = 0.0f64;
    let mut jn = 0u32;
    for (a, sa) in file_sets.iter().enumerate() {
        for sb in &file_sets[a + 1..] {
            let inter = sa.intersection(sb).count();
            let uni = sa.union(sb).count().max(1);
            jsum += inter as f64 / uni as f64;
            jn += 1;
        }
    }
    let jacc = if jn > 0 { jsum / f64::from(jn) } else { 0.0 };
    let file_score = (jacc * 45.0) as u32;
    // Independent-task-count agreement (the parallel shape the planner settled on).
    let indeps: Vec<usize> = candidates
        .iter()
        .map(|c| c.iter().filter(|t| t.deps.is_empty()).count())
        .collect();
    let indep_spread = indeps.iter().max().unwrap() - indeps.iter().min().unwrap();
    let indep_score: u32 = u32::from(indep_spread <= 1) * 15;
    let conf = (count_score + file_score + indep_score).min(100) as u8;
    (
        conf,
        format!(
            "{} drafts agree: count spread {spread}, file-overlap {:.0}%",
            candidates.len(),
            jacc * 100.0
        ),
    )
}

/// Score a candidate plan SKELETON for best-of-N selection. Pure-Rust, no LLM. Returns `None` if the
/// skeleton is not a valid DAG (validity borrowed from the same `Dag::from_specs` the live path uses,
/// so a scored candidate is guaranteed loadable). Higher = a wider, flatter, less-conflicting plan:
/// rewards independent (zero-dep) parallel subtasks + adequate count, penalizes deep dependency chains,
/// overlapping files, and chokepoints (one task most others depend on).
fn score_skeleton(specs: &[goose_swarm::TaskSpec], worker_count: usize) -> Option<i64> {
    goose_swarm::Dag::from_specs(specs.to_vec()).ok()?;
    let wc = worker_count.max(1) as i64;
    let n = specs.len() as i64;
    if n == 0 {
        return None;
    }
    // Parallel width: independent (zero-dep) subtasks, excluding the integrate-verify sink.
    let independent = specs
        .iter()
        .filter(|s| s.deps.is_empty() && s.id != "integrate-verify")
        .count() as i64;
    let indep_score = independent.min(wc) * 10;
    // Longest dependency chain (DAG validated above, so acyclic) — penalize depth beyond 2.
    let deps_of: std::collections::HashMap<&str, &[String]> = specs
        .iter()
        .map(|s| (s.id.as_str(), s.deps.as_slice()))
        .collect();
    let mut depth: std::collections::HashMap<&str, i64> =
        deps_of.keys().map(|k| (*k, 0i64)).collect();
    for _ in 0..specs.len() {
        let mut changed = false;
        for (id, ds) in &deps_of {
            let d = ds
                .iter()
                .filter_map(|x| depth.get(x.as_str()).copied())
                .max()
                .map(|m| m + 1)
                .unwrap_or(0);
            if d > depth[id] {
                depth.insert(id, d);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let max_depth = depth.values().copied().max().unwrap_or(0);
    let depth_pen = (max_depth - 2).max(0) * 5;
    // File overlap: files claimed by >1 subtask (scheduler serializes them — a quality, not validity, hit).
    let mut files: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for s in specs {
        for f in &s.owned_files {
            *files.entry(f.as_str()).or_insert(0) += 1;
        }
    }
    let overlap_pen = files.values().filter(|&&c| c > 1).count() as i64 * 3;
    // Chokepoint: the most-depended-on task. Penalize when it exceeds ~half the fleet width.
    let mut fan_in: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for s in specs {
        for d in &s.deps {
            *fan_in.entry(d.as_str()).or_insert(0) += 1;
        }
    }
    let max_fan_in = fan_in.values().copied().max().unwrap_or(0);
    let choke_pen = if max_fan_in > (wc / 2).max(1) {
        max_fan_in * 2
    } else {
        0
    };
    // Size sanity: want at least worker_count subtasks to fill the fleet.
    let size_score = if n >= wc { 5 } else { -(wc - n) * 2 };
    Some(indep_score + size_score - depth_pen - overlap_pen - choke_pen)
}

/// True if the working dir already contains source (a marker file or a source-extension file within
/// ~2 levels) — i.e. an amendment, which is what flips research-planning Auto to on.
fn working_dir_has_sources(dir: &Path) -> bool {
    const SRC_EXT: &[&str] = &[
        "rs", "py", "ts", "tsx", "js", "jsx", "go", "java", "rb", "c", "cpp", "h", "hpp", "cs",
        "swift", "kt",
    ];
    const MARKERS: &[&str] = &[
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "pom.xml",
    ];
    const SKIP: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        ".venv",
        ".swarm",
        "__pycache__",
    ];
    fn walk(dir: &Path, depth: u32) -> bool {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if depth == 0 || name.starts_with('.') || SKIP.contains(&name.as_str()) {
                    continue;
                }
                if walk(&p, depth - 1) {
                    return true;
                }
            } else if MARKERS.contains(&name.as_str())
                || p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| SRC_EXT.contains(&e))
                    .unwrap_or(false)
            {
                return true;
            }
        }
        false
    }
    walk(dir, 2)
}

// ---------------------------------------------------------------------------------------------
// MCP worker extensions — built at runtime from env vars (no secrets stored on disk)
// ---------------------------------------------------------------------------------------------

/// Build a worker MCP extension by name, reading secrets from the environment. Returns None (with a
/// note) if a required secret env var is missing, so the run proceeds without that extension.
fn build_worker_extension(name: &str) -> Option<ExtensionConfig> {
    match name {
        "context7" => {
            let key = std::env::var("CONTEXT7_API_KEY").ok().or_else(|| {
                eprintln!("(skipping context7: set CONTEXT7_API_KEY)");
                None
            })?;
            Some(ExtensionConfig::Stdio {
                name: "context7".to_string(),
                description: "Upstash Context7 library docs".to_string(),
                cmd: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@upstash/context7-mcp".to_string(),
                    "--api-key".to_string(),
                    key,
                ],
                envs: Default::default(),
                env_keys: vec![],
                timeout: Some(120),
                cwd: None,
                bundled: None,
                available_tools: vec![],
            })
        }
        "web-search" => {
            let bearer = std::env::var("WEBSEARCH_BEARER").ok().or_else(|| {
                eprintln!("(skipping web-search: set WEBSEARCH_BEARER)");
                None
            })?;
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {bearer}"));
            if let Ok(k) = std::env::var("SERPER_KEY") {
                headers.insert("X-Serper-Key".to_string(), k);
            }
            if let Ok(k) = std::env::var("GITHUB_TOKEN") {
                headers.insert("X-GitHub-Token".to_string(), k);
            }
            Some(ExtensionConfig::StreamableHttp {
                name: "web-search".to_string(),
                description: "Web search + GitHub".to_string(),
                uri: std::env::var("WEBSEARCH_URI").unwrap_or_else(|_| {
                    "https://worksmacstudio.tailfc4700.ts.net:8443/mcp".to_string()
                }),
                envs: Default::default(),
                env_keys: vec![],
                headers,
                timeout: Some(120),
                socket: None,
                bundled: None,
                available_tools: vec![],
            })
        }
        "doc-processor" => {
            let bearer = std::env::var("DOCPROC_BEARER").ok().or_else(|| {
                eprintln!("(skipping doc-processor: set DOCPROC_BEARER)");
                None
            })?;
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {bearer}"));
            Some(ExtensionConfig::StreamableHttp {
                name: "doc-processor".to_string(),
                description: "Document processor".to_string(),
                uri: std::env::var("DOCPROC_URI").unwrap_or_else(|_| {
                    "https://worksmacstudio.tailfc4700.ts.net:10000/mcp".to_string()
                }),
                envs: Default::default(),
                env_keys: vec![],
                headers,
                timeout: Some(120),
                socket: None,
                bundled: None,
                available_tools: vec![],
            })
        }
        other => {
            eprintln!("(unknown worker extension: {other})");
            None
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Dispatcher (M1.1) — drives one Goose agent per task over the shared lmstudio provider
// ---------------------------------------------------------------------------------------------

pub struct GooseAgentDispatcher {
    provider: Arc<dyn Provider>,
    session_manager: Arc<SessionManager>,
    permission_manager: Arc<PermissionManager>,
    working_dir: PathBuf,
    worker_max_turns: u32,
    /// MCP extensions added to each WORKER agent (not the planner). Built from env at run start.
    worker_extensions: Vec<ExtensionConfig>,
    /// The planner model (also used by the dynamic Replanner impl).
    planner_model: String,
    /// Per-task wall-clock cap (seconds): a worker exceeding this is treated as hung and re-routed.
    worker_timeout_secs: u64,
    /// Shorter wall-clock cap (seconds) for planner-side calls via `run_agent_timed`.
    planner_timeout_secs: u64,
    /// Whether the swarm may `lms load` a model (gates the transient re-warm on dispatch errors).
    allow_model_load: bool,
    /// Imposed sampling parameters applied to every model call (steadies weak local models).
    sampling: SamplingParams,
    /// Frozen module-interface contracts (signature-only stubs) injected into EVERY worker prompt to kill
    /// cross-module drift. Empty until the GOOSE_SWARM_CONTRACTS stub pass populates it (stage 2b); set
    /// once before the EXECUTE phase, then read by every worker. Empty -> the injection is a no-op.
    contracts: std::sync::OnceLock<String>,
    /// APP PILLARS (GOOSE_SWARM_GOALS): a small set of distilled, app-level acceptance criteria (the
    /// non-negotiable goals + interface/invariant shape) injected — as a pre-rendered block — into EVERY
    /// worker prompt so modules cohere to the same north star through context compaction. Distilled once
    /// at plan time (post-plan), set before EXECUTE. Empty -> the injection is a no-op (flag off).
    pillars: std::sync::OnceLock<String>,
    /// SPECULATIVE EXECUTION (GOOSE_SWARM_SPECULATE): per-twin shadow workspace + its owned files, keyed by
    /// task_id. A twin's agent is rooted here (NOT the real tree); on a twin win the scheduler calls
    /// `promote_speculative` which copies only these owned files back. Empty unless the flag is on.
    spec_shadows: Mutex<HashMap<String, (tempfile::TempDir, Vec<String>)>>,
}

impl GooseAgentDispatcher {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        working_dir: PathBuf,
        worker_max_turns: u32,
        worker_extensions: Vec<ExtensionConfig>,
        planner_model: String,
        worker_timeout_secs: u64,
        planner_timeout_secs: u64,
        allow_model_load: bool,
        sampling: SamplingParams,
    ) -> Result<Self> {
        let provider = goose::providers::create("lmstudio", vec![]).await?;
        let session_root = std::env::temp_dir().join("goose-swarm-sessions");
        std::fs::create_dir_all(&session_root)?;
        // Use the global session store so each worker's full trace is fetchable by its logged
        // session_id (Hidden type keeps them out of normal listings).
        let session_manager = Arc::new(SessionManager::instance());
        let permission_manager = Arc::new(PermissionManager::new(session_root));
        Ok(Self {
            provider,
            session_manager,
            permission_manager,
            working_dir,
            worker_max_turns,
            worker_extensions,
            planner_model,
            worker_timeout_secs,
            planner_timeout_secs,
            allow_model_load,
            sampling,
            contracts: std::sync::OnceLock::new(),
            pillars: std::sync::OnceLock::new(),
            spec_shadows: Mutex::new(HashMap::new()),
        })
    }

    /// Build the isolated SHADOW workspace for a speculative twin: a cp -r of the real tree (heavy dirs
    /// excluded) into a fresh TempDir, stored in `spec_shadows[task_id]` with the twin's owned files so a
    /// later `promote_speculative` can copy exactly those back. Returns the shadow path. On any IO error the
    /// caller MUST bail the twin (never fall back to the real tree — that would let two writers collide).
    fn make_shadow(
        &self,
        task_id: &str,
        owned_files: &[String],
        real_root: &Path,
    ) -> std::io::Result<PathBuf> {
        let tmp = tempfile::TempDir::new()?;
        copy_tree_excluding(real_root, tmp.path())?;
        let path = tmp.path().to_path_buf();
        self.spec_shadows
            .lock()
            .unwrap()
            .insert(task_id.to_string(), (tmp, owned_files.to_vec()));
        Ok(path)
    }

    /// `run_agent` wrapped in a wall-clock timeout (`worker_timeout_secs`). Used for PLANNER-side
    /// calls (architect / solo plan / scouts / research / replan) so an agent that hangs at the
    /// protocol level cannot stall the run forever — every caller degrades on Err (fallback plan,
    /// skip scout, empty research).
    async fn run_agent_timed(
        &self,
        model_id: &str,
        system_prompt: String,
        user_text: String,
        response: Option<Response>,
        max_turns: u32,
        extensions: &[ExtensionConfig],
    ) -> Result<RunAgentOut> {
        // Idle-based, not wall-clock: planner_timeout_secs is a NO-PROGRESS window. A slow but
        // progressing architect / detailer / scout runs to completion; only a stalled stream aborts.
        self.run_agent(
            model_id,
            system_prompt,
            user_text,
            response,
            max_turns,
            extensions,
            self.planner_timeout_secs,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    async fn run_agent(
        &self,
        model_id: &str,
        system_prompt: String,
        user_text: String,
        response: Option<Response>,
        max_turns: u32,
        extensions: &[ExtensionConfig],
        idle_secs: u64,
        activity_key: Option<&str>,
    ) -> Result<RunAgentOut> {
        // Normal path: the agent writes the REAL project tree (self.working_dir).
        self.run_agent_in(
            self.working_dir.clone(),
            model_id,
            system_prompt,
            user_text,
            response,
            max_turns,
            extensions,
            idle_secs,
            activity_key,
        )
        .await
    }

    /// Like `run_agent` but the agent's file/shell tools are rooted at `work_dir` (the session working_dir).
    /// For a SPECULATIVE twin this is an isolated shadow copy, so the twin never writes the real tree; for
    /// every normal call `work_dir == self.working_dir`, so behavior is unchanged.
    #[allow(clippy::too_many_arguments)]
    async fn run_agent_in(
        &self,
        work_dir: PathBuf,
        model_id: &str,
        system_prompt: String,
        user_text: String,
        response: Option<Response>,
        max_turns: u32,
        extensions: &[ExtensionConfig],
        idle_secs: u64,
        // When Some(task_id), emit a per-turn activity heartbeat to `.swarm/activity/<task_id>.json` so
        // the idle-model judge can see how many actions this worker has taken — letting it catch a
        // thrashing (many-actions, zero-output) worker by BEHAVIOR instead of waiting on the clock.
        // None for planner-side calls (architect/detailer/scout/judge), which are not judged.
        activity_key: Option<&str>,
    ) -> Result<RunAgentOut> {
        let agent_config = AgentConfig::new(
            self.session_manager.clone(),
            self.permission_manager.clone(),
            None,
            GooseMode::Auto,
            true,
            GoosePlatform::GooseCli,
        );
        let agent = Agent::with_config(agent_config);

        let session = self
            .session_manager
            .create_session(
                work_dir.clone(),
                "swarm-task".to_string(),
                SessionType::Hidden,
                GooseMode::default(),
            )
            .await?;
        let session_id = session.id.clone();

        let mut model_config =
            goose::model_config::model_config_from_user_config("lmstudio", model_id)?;
        // Follow LM Studio's own temperature: pass the sampling temperature through verbatim, which is
        // None unless the swarm config explicitly sets one. None clears any inherited GOOSE_TEMPERATURE
        // default so the request omits temperature entirely and the LM Studio per-model setting applies.
        model_config = model_config.with_temperature(self.sampling.temperature);
        let mut extra = std::collections::HashMap::new();
        if let Some(v) = self.sampling.top_p {
            extra.insert("top_p".to_string(), serde_json::json!(v));
        }
        if let Some(v) = self.sampling.top_k {
            extra.insert("top_k".to_string(), serde_json::json!(v));
        }
        if let Some(v) = self.sampling.min_p {
            extra.insert("min_p".to_string(), serde_json::json!(v));
        }
        if let Some(v) = self.sampling.repeat_penalty {
            extra.insert("repeat_penalty".to_string(), serde_json::json!(v));
        }
        if !extra.is_empty() {
            model_config = model_config.with_merged_request_params(extra);
        }
        agent
            .update_provider(self.provider.clone(), model_config, &session_id)
            .await
            .map_err(|e| anyhow!("update_provider: {e}"))?;

        agent
            .add_extension(
                ExtensionConfig::Builtin {
                    name: "developer".to_string(),
                    display_name: None,
                    description: String::new(),
                    timeout: None,
                    bundled: Some(true),
                    available_tools: vec![],
                },
                &session_id,
            )
            .await
            .map_err(|e| anyhow!("add developer extension: {e}"))?;

        // Worker MCP extensions (none for the planner). A failed connection is non-fatal so a down
        // server doesn't kill the task.
        for ext in extensions {
            if let Err(e) = agent.add_extension(ext.clone(), &session_id).await {
                eprintln!("(worker extension add failed: {e})");
            }
        }

        agent.apply_recipe_components(response, true).await;
        agent.override_system_prompt(system_prompt).await;

        let user_message = Message::user().with_text(user_text);
        let session_config = SessionConfig {
            id: session_id.clone(),
            schedule_id: None,
            max_turns: Some(max_turns),
            retry_config: None,
        };

        let mut stream = agent
            .reply(user_message, session_config, None)
            .await
            .map_err(|e| anyhow!("agent.reply: {e}"))?;

        let mut texts: Vec<String> = Vec::new();
        let mut final_output: Option<String> = None;
        let mut pending: HashMap<String, (String, bool)> = HashMap::new();
        let mut tool_calls: Vec<ToolCallRecord> = Vec::new();
        // Per-turn activity heartbeat for the judge (worker calls only). Reset to 0 at the start of every
        // attempt so a re-dispatch never inherits a prior attempt's count. Best-effort: a failed write
        // just means the judge falls back to its time-based checks.
        let activity_file = activity_key.map(|k| {
            // Use work_dir (== self.working_dir for a normal task, == the shadow for a speculative twin) so a
            // twin's heartbeat stays inside its shadow rather than touching the real tree's .swarm/activity.
            let dir = work_dir.join(".swarm").join("activity");
            let _ = std::fs::create_dir_all(&dir);
            dir.join(format!("{k}.json"))
        });
        if let Some(p) = &activity_file {
            let _ = std::fs::write(
                p,
                "{\"tool_calls\":0,\"errors\":0,\"recent\":[],\"last_text\":\"\"}",
            );
        }
        // IDLE-based watchdog: kill the task only if NO agent event arrives for `idle_secs` (a genuinely
        // stalled stream), NOT on total wall-clock — a slow-but-progressing local model emits an event
        // every turn and must be allowed to finish. idle_secs == 0 disables the watchdog.
        let idle = std::time::Duration::from_secs(if idle_secs == 0 { 86_400 } else { idle_secs });
        // Optional graceful wall-clock cap for the heavy `integrate-verify` SINK worker. A healthy sink
        // can legitimately run ~1400s; a pathological one blows past the run budget with no way for the
        // scheduler to finalize it — the judge's repeated "ok" verdict is a no-op and the watchdog is
        // idle-based, so a still-emitting sink never trips it. GOOSE_SWARM_SINK_CAP_SECS>0 finalizes the
        // sink as DONE on expiry (NOT an error/re-route) so the run terminates cleanly and the
        // deterministic smoke gate backstops correctness. Unset/0 = OFF ⇒ byte-identical default path;
        // only the sink task is ever affected.
        let sink_deadline = if activity_key == Some("integrate-verify") {
            std::env::var("GOOSE_SWARM_SINK_CAP_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|&s| s > 0)
                .map(|s| tokio::time::Instant::now() + std::time::Duration::from_secs(s))
        } else {
            None
        };
        loop {
            // HARD wall-clock ceiling: finalize the moment the sink deadline passes, regardless of whether
            // the sink is still emitting. The wait/timeout path below only reaches the deadline check on an
            // event GAP (the `Err(_)` arm), so a CONTINUOUSLY-active integrate-verify (steady tokens/tools)
            // can otherwise run well past the deadline before a gap lets the cap fire. Checking here at the
            // top makes GOOSE_SWARM_SINK_CAP_SECS a true ceiling. No-op when unset (sink_deadline == None).
            if sink_deadline.is_some_and(|dl| tokio::time::Instant::now() >= dl) {
                eprintln!(
                    "↳ integrate-verify hit the sink wall-clock cap — finalizing as done (smoke gate backstops)"
                );
                break;
            }
            // Wait at most `idle`, but no later than the sink cap (when set) so the cap fires promptly.
            let wait = match sink_deadline {
                Some(dl) => idle.min(dl.saturating_duration_since(tokio::time::Instant::now())),
                None => idle,
            };
            let ev = match tokio::time::timeout(wait, stream.next()).await {
                Ok(Some(ev)) => ev,
                Ok(None) => break,
                Err(_) => {
                    // Distinguish the sink wall-clock cap from a genuine idle stall: on the cap, finalize
                    // as DONE (the app files are already built; the sink owns no deliverables) instead of
                    // re-routing, so the run can terminate; otherwise re-route as before.
                    if sink_deadline.is_some_and(|dl| tokio::time::Instant::now() >= dl) {
                        eprintln!(
                            "↳ integrate-verify hit the sink wall-clock cap — finalizing as done (smoke gate backstops)"
                        );
                        break;
                    }
                    return Err(anyhow!(
                        "agent stalled — no progress for {idle_secs}s (no token/tool activity)"
                    ));
                }
            };
            match ev {
                Ok(AgentEvent::Message(msg)) => {
                    for content in &msg.content {
                        match content {
                            MessageContent::Text(t) => texts.push(t.text.clone()),
                            MessageContent::ToolRequest(req) => {
                                if let Ok(tc) = req.tool_call.as_ref() {
                                    let name = tc.name.to_string();
                                    if name == FINAL_OUTPUT_TOOL {
                                        final_output = Some(
                                            serde_json::to_string(&tc.arguments)
                                                .unwrap_or_default(),
                                        );
                                    }
                                    let mcp = is_mcp_tool(&name);
                                    pending.insert(req.id.clone(), (name, mcp));
                                }
                            }
                            MessageContent::ToolResponse(resp) => {
                                if let Some((name, is_mcp)) = pending.remove(&resp.id) {
                                    let ok = resp
                                        .tool_result
                                        .as_ref()
                                        .map(|r| !r.is_error.unwrap_or(false))
                                        .unwrap_or(false);
                                    tool_calls.push(ToolCallRecord {
                                        name,
                                        is_mcp,
                                        ok: Some(ok),
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => return Err(anyhow!("agent stream error: {e}")),
            }
            if let Some(p) = &activity_file {
                // A digest of what the worker is actually DOING — the judge reads this as the worker's
                // live "log": how many actions, how many ERRORED, the last few tool calls, and the worker's
                // most recent reasoning. This is what lets the semantic judge see a worker re-running a
                // failing test, looping on the same error, or exploring without producing.
                let errors = tool_calls.iter().filter(|t| t.ok == Some(false)).count();
                let recent: Vec<String> = tool_calls
                    .iter()
                    .rev()
                    .take(6)
                    .rev()
                    .map(|t| {
                        format!(
                            "{} {}",
                            t.name,
                            if t.ok == Some(false) { "ERR" } else { "ok" }
                        )
                    })
                    .collect();
                let lt = texts.last().cloned().unwrap_or_default();
                let n = lt.chars().count();
                let last_text: String = if n > 400 {
                    lt.chars().skip(n - 400).collect()
                } else {
                    lt
                };
                let digest = serde_json::json!({
                    "tool_calls": tool_calls.len(),
                    "errors": errors,
                    "recent": recent,
                    "last_text": last_text,
                });
                let _ = std::fs::write(p, digest.to_string());
            }
        }
        // Requests with no response (e.g. a max-turns cutoff): record with unknown ok.
        for (_id, (name, is_mcp)) in pending {
            tool_calls.push(ToolCallRecord {
                name,
                is_mcp,
                ok: None,
            });
        }

        // Stream delivers incremental Text chunks; concatenate to reconstruct the message text.
        Ok(RunAgentOut {
            text: texts.concat(),
            final_output,
            session_id,
            tool_calls,
        })
    }

    /// Ask the planner for a small set of INDEPENDENT research questions to resolve before planning.
    /// Degrades to an empty list on any error (research is optional, never aborts the run).
    async fn research_questions(
        &self,
        planner_model: &str,
        user_prompt: &str,
        max_q: u32,
        is_amendment: bool,
    ) -> Result<Vec<ResearchQuestion>> {
        let codebase = if is_amendment {
            " You MAY also include \"codebase\" questions to investigate the EXISTING code in the working dir."
        } else {
            ""
        };
        let system = format!(
            "You scope a coding task BEFORE planning. Emit AT MOST {max_q} INDEPENDENT research questions whose \
             answers would MATERIALLY change the plan: \"library_docs\" (look up a library's real API via its docs) \
             or \"web\" (a fact to look up).{codebase} Ask ONLY what you cannot already answer; if the task is \
             self-contained, return an EMPTY questions list. Do NOT invent make-work. Then call the final_output tool."
        );
        let response = Some(Response {
            json_schema: Some(research_schema()),
        });
        let out = match self
            .run_agent_timed(
                planner_model,
                system,
                format!("Task: {user_prompt}"),
                response,
                8,
                &[],
            )
            .await
        {
            Ok(o) => o,
            Err(_) => return Ok(Vec::new()),
        };
        let Some(fo) = out.final_output else {
            return Ok(Vec::new());
        };
        #[derive(serde::Deserialize)]
        struct Q {
            id: String,
            question: String,
            kind: String,
        }
        #[derive(serde::Deserialize)]
        struct Qs {
            #[serde(default)]
            questions: Vec<Q>,
        }
        let parsed: Qs = match serde_json::from_str(&fo) {
            Ok(p) => p,
            Err(_) => return Ok(Vec::new()),
        };
        Ok(parsed
            .questions
            .into_iter()
            .take(max_q as usize)
            .map(|q| ResearchQuestion {
                id: q.id,
                question: q.question,
                kind: q.kind,
            })
            .collect())
    }

    /// GOOSE_SWARM_ASK: generate crisp INTERROGATIVE clarifying questions to ask the USER when plan
    /// confidence is low. The answers should change HOW the program is built (scope/IO/formats/acceptance),
    /// not be make-work. Returns an empty vec on any failure or a self-contained task — the caller falls
    /// back to a generic question so a below-floor plan ALWAYS asks (never proceeds on a default).
    async fn clarify_questions(
        &self,
        planner_model: &str,
        user_prompt: &str,
        plan_json: &str,
        uncertainties: &str,
        conf: u8,
        max_q: u32,
    ) -> Vec<String> {
        let unc = if uncertainties.trim().is_empty() {
            "(none stated)".to_string()
        } else {
            uncertainties.trim().to_string()
        };
        let plan_excerpt: String = plan_json.chars().take(2000).collect();
        let system = format!(
            "A weak local model just drafted a plan for a coding task but its confidence is LOW ({conf}/100). \
             Ask the USER AT MOST {max_q} crisp, specific, INTERROGATIVE questions whose answers would most \
             change HOW the program is built — its scope, inputs/outputs, file formats, or acceptance criteria \
             — NOT trivia or anything already pinned down by the task. Ask ONLY what the USER alone can decide \
             — do NOT ask facts that can be looked up in docs or on the web (the swarm researches those itself). \
             Each question must be answerable in one sentence and END WITH '?'. If the task is genuinely self- \
             contained and nothing would change the build, return an EMPTY questions list — do NOT invent \
             make-work. Then call the final_output tool."
        );
        let user = format!(
            "Task: {user_prompt}\n\nThe model's stated uncertainties: {unc}\n\nThe drafted plan (excerpt):\n{plan_excerpt}"
        );
        let response = Some(Response {
            json_schema: Some(clarify_schema()),
        });
        let out = match self
            .run_agent_timed(planner_model, system, user, response, 8, &[])
            .await
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };
        let Some(fo) = out.final_output else {
            return Vec::new();
        };
        #[derive(serde::Deserialize)]
        struct Qs {
            #[serde(default)]
            questions: Vec<String>,
        }
        let parsed: Qs = match serde_json::from_str(&fo) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        parsed
            .questions
            .into_iter()
            .map(|q| q.trim().to_string())
            // Enforce the interrogative contract the prompt demands: a real question ends with '?'.
            // Declarative junk (headers, statements) falls through to the next cascade tier instead of
            // being surfaced to the user as a "question".
            .filter(|q| q.chars().count() >= 8 && q.ends_with('?'))
            .take(max_q as usize)
            .collect()
    }

    /// Run the research questions IN PARALLEL across the fleet (round-robin over worker models), each
    /// with the research MCP extensions. A failed research worker degrades to a note, never blocks.
    async fn run_research(
        self: &Arc<Self>,
        questions: Vec<ResearchQuestion>,
        research_extensions: Arc<Vec<ExtensionConfig>>,
        worker_models: Vec<String>,
    ) -> Vec<ResearchFinding> {
        if worker_models.is_empty() {
            return Vec::new();
        }
        let me = self.clone();
        // One research call per device (work-stealing): a weight-1 node never has a second queued.
        fanout_over_fleet(worker_models, questions, move |q, model| {
            let me = me.clone();
            let exts = research_extensions.clone();
            async move {
                let started = std::time::Instant::now();
                eprintln!(
                    "  {} research {} ({}) → {}",
                    style("▸").cyan().bold(),
                    style(&q.id).bold(),
                    q.kind,
                    model
                );
                let tool_hint = match q.kind.as_str() {
                    "library_docs" => "Use the context7 tools (resolve-library-id then get-library-docs).",
                    "web" => "Use the web-search tool.",
                    "codebase" => "Use shell/grep to inspect the existing code in the working directory.",
                    _ => "Use whatever tools fit.",
                };
                let system = format!(
                    "You are a RESEARCH worker. Answer EXACTLY the question below with a concise, factual summary \
                     (key API names, short snippets, file refs). {tool_hint} Do NOT write or modify any project files."
                );
                let findings = match me
                    .run_agent_timed(&model, system, q.question.clone(), None, 12, &exts)
                    .await
                {
                    Ok(o) => o.text,
                    Err(e) => format!("(research failed: {e})"),
                };
                eprintln!(
                    "  {} research {} ({:.0}s)",
                    style("✓").green().bold(),
                    style(&q.id).bold(),
                    started.elapsed().as_secs_f64()
                );
                ResearchFinding {
                    question: q.question,
                    kind: q.kind,
                    findings,
                }
            }
        })
        .await
    }

    /// Fan out fixed-lens SCOUTS IN PARALLEL across the fleet — each self-directs its lens with no
    /// serial scoping call. Returns the same `ResearchFinding` shape as `run_research` so the planner
    /// and the findings-join are unchanged.
    async fn run_scouts(
        self: &Arc<Self>,
        user_prompt: &str,
        is_amendment: bool,
        max_lenses: u32,
        research_extensions: Arc<Vec<ExtensionConfig>>,
        worker_models: Vec<String>,
        scout_budget: u64,
    ) -> Vec<ResearchFinding> {
        if worker_models.is_empty() {
            return Vec::new();
        }
        let me = self.clone();
        let prompt = user_prompt.to_string();
        let lenses = select_lenses(is_amendment, max_lenses);
        // One scout per device (work-stealing): a weight-1 node never has a second scout queued.
        fanout_over_fleet(worker_models, lenses, move |lens, model| {
            let me = me.clone();
            let exts = research_extensions.clone();
            let prompt = prompt.clone();
            async move {
                let started = std::time::Instant::now();
                eprintln!(
                    "  {} scout {} → {}",
                    style("▸").cyan().bold(),
                    style(lens.id).bold(),
                    model
                );
                let system = format!(
                    "You are a SCOUT investigating ONE aspect of a coding task to inform the planner. \
                     Your lens is \"{}\": {} {} Return a CONCISE, factual brief (key facts, API names, \
                     short snippets, file refs, and a suggested breakdown for your lens) as your TEXT \
                     RESPONSE ONLY. You have NO write task: do NOT create, write, or edit ANY file \
                     (no .md brief, no notes, no scratch) — read-only investigation, then report in your \
                     message. Do NOT produce the full plan. To read text use `cat`; `python3` not `python`. \
                     Keep it LEAN: never dump full docs/help()/pydoc text into your context; for \
                     standard-library modules just name the relevant APIs in one line. A few hundred words \
                     is plenty — large context is very slow on local models. \
                     STAY in the current working directory: for a NEW/empty project there is nothing on \
                     disk to investigate, so reason from the task itself; NEVER `ls`/`cat` parent or \
                     sibling directories — they are unrelated projects. Finish FAST.",
                    lens.title, lens.brief, lens.tool_hint
                );
                let findings = match tokio::time::timeout(
                    std::time::Duration::from_secs(scout_budget),
                    me.run_agent_timed(&model, system, format!("Task: {prompt}"), None, 12, &exts),
                )
                .await
                {
                    Ok(Ok(o)) => o.text,
                    Ok(Err(e)) => format!("(scout failed: {e})"),
                    Err(_) => format!(
                        "(scout '{}' exceeded {}s budget — skipped to keep the fleet moving)",
                        lens.id, scout_budget
                    ),
                };
                eprintln!(
                    "  {} scout {} ({:.0}s)",
                    style("✓").green().bold(),
                    style(lens.id).bold(),
                    started.elapsed().as_secs_f64()
                );
                ResearchFinding {
                    question: lens.title.to_string(),
                    kind: lens.id.to_string(),
                    findings,
                }
            }
        })
        .await
    }

    /// GOOSE_SWARM_CONTRACTS (2b): freeze the contract before EXECUTE. Set once; every worker reads it.
    pub fn set_contracts(&self, bundle: String) {
        let _ = self.contracts.set(bundle);
    }

    /// GOOSE_SWARM_GOALS: freeze the rendered app-PILLARS block before EXECUTE. Set once; every worker reads it.
    pub fn set_pillars(&self, block: String) {
        let _ = self.pillars.set(block);
    }

    /// GOOSE_SWARM_GOALS (part 1): distill the app's non-negotiable PILLARS from the spec + research + the
    /// chosen plan, as a small set of imperative acceptance criteria. One planner call with a forced JSON
    /// schema (mirrors `plan()`); grounded on the actual decomposition so the pillars reflect what will be
    /// built. Bounded to <=7. Any failure -> empty Pillars (the injection then no-ops). Returns the pillars
    /// (the confidence slot is reserved for the later clarify-if-thin gate).
    async fn distill_pillars(
        &self,
        planner_model: &str,
        user_prompt: &str,
        research_findings: &str,
        plan_json: &str,
    ) -> Pillars {
        let system = "You are distilling the PILLARS of an app about to be built by a swarm of parallel \
            workers that each see only their own module. Output the 3-7 load-bearing acceptance criteria the \
            FINISHED program MUST satisfy — each ONE short imperative sentence. Capture: (a) the EXACT \
            interface the spec advertises — command names and ARGUMENT ORDER verbatim (if the spec says \
            `report budget`, the pillar says `report budget`, never `budget report`); (b) the invariants that \
            make modules agree — the shared store/file, the units (e.g. money to 2 decimals), the entry point; \
            (c) any correctness rule the spec states. Do NOT restate implementation detail or invent features. \
            Prefer the spec's literal words. Output ONLY the JSON object; no prose, no code fences."
            .to_string();
        let response = Some(Response {
            json_schema: Some(pillars_schema()),
        });
        let research_block = if research_findings.trim().is_empty() {
            String::new()
        } else {
            format!("## Research findings\n{research_findings}\n\n")
        };
        let user = format!(
            "{research_block}## App spec\n{user_prompt}\n\n## The chosen plan (architecture already decided)\n{plan_json}\n\nDistill the pillars now."
        );
        let out = match self
            .run_agent_timed(planner_model, system, user, response, 8, &[])
            .await
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("  pillars: distillation failed ({e}) — skipping");
                return Pillars::default();
            }
        };
        let raw = out.final_output.unwrap_or_default();
        let mut p: Pillars = serde_json::from_str(&raw).unwrap_or_default();
        p.pillars.truncate(7);
        p
    }

    /// Generate signature-only interface stubs per module IN PARALLEL across the fleet and assemble them
    /// into one frozen-contract bundle, so parallel workers build against the SAME interfaces (kills the
    /// cross-module drift that passing isolation tests hide). One call per module, work-stolen over the
    /// fleet; a slow/empty/failed stub just drops out of the bundle.
    async fn generate_contracts(
        self: &Arc<Self>,
        modules: Vec<TaskSpec>,
        worker_models: Vec<String>,
        goal: &str,
    ) -> String {
        let goal = goal.to_string();
        let me = self.clone();
        let stubs = fanout_over_fleet(worker_models, modules, move |spec, model| {
            let me = me.clone();
            let goal = goal.clone();
            async move {
                let files = spec
                    .owned_files
                    .iter()
                    .filter(|f| f.ends_with(".py"))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                eprintln!(
                    "  {} contract {} → {}",
                    style("▸").cyan().bold(),
                    style(&spec.id).bold(),
                    model
                );
                let system = "You are defining the PUBLIC INTERFACE of ONE module BEFORE it is \
                    implemented, so parallel workers agree on the contract. Output ONLY Python signature \
                    stubs for the listed files: every public function and class the module will expose, \
                    with EXACT names, full type-annotated signatures, and a ONE-LINE docstring each, with \
                    `...` as the body. ALSO — if this module owns a DATABASE SCHEMA (it creates tables, a \
                    SQLite/SQL schema, or defines the persisted record shape), append a `# SCHEMA` comment \
                    block listing each TABLE and its EXACT column names (and types), because every module \
                    that reads or writes those tables MUST use the SAME column names — a drift (one module \
                    using `league_id` while another uses `league`, or `home_team` vs `home`) is a top \
                    integration failure that passing isolation unit-tests hide. NO implementations, NO \
                    private helpers, NO prose, NO code fences. You have file/shell tools but MUST NOT use \
                    them: do NOT create, write, or edit ANY file — put the stubs in your reply TEXT only. \
                    Keep it tight."
                    .to_string();
                let user = format!(
                    "Overall program: {goal}\n\nModule subtask [{}]: {}\nFiles it owns: {files}\n\n\
                     Emit signature-only stubs, each file preceded by a `# <path>` header.",
                    spec.id, spec.description
                );
                let stub = match tokio::time::timeout(
                    std::time::Duration::from_secs(75),
                    me.run_agent(&model, system, user, None, 6, &[], 0, None),
                )
                .await
                {
                    Ok(Ok(o)) if !o.text.trim().is_empty() => o.text,
                    _ => String::new(),
                };
                (spec.id, stub)
            }
        })
        .await;
        let mut bundle = String::new();
        for (id, stub) in stubs {
            let stub = stub.trim();
            if !stub.is_empty() {
                bundle.push_str(&format!("### module: {id}\n{stub}\n\n"));
            }
        }
        bundle
    }

    /// Parallel planning: the 27B drafts a STRUCTURAL SKELETON (brief one-line descriptions) fast, then
    /// the fleet writes every subtask's implementation-ready spec IN PARALLEL, and we assemble the final
    /// plan deterministically. Returns the same plan JSON `plan()` would — callers fall back to `plan()`
    /// on Err. The skeleton itself is a valid plan, so a total detailer failure degrades gracefully.
    #[allow(clippy::too_many_arguments)]
    async fn parallel_plan(
        self: &Arc<Self>,
        planner_model: &str,
        worker_models: Vec<String>,
        user_prompt: &str,
        plan_schema: serde_json::Value,
        worker_count: usize,
        research_findings: &str,
        best_of_n: usize,
        homogeneous: bool,
    ) -> Result<(String, Option<u8>, String)> {
        let homo_hint = if homogeneous {
            "ALL worker nodes run the SAME model (identical weights + tokenizer), so files produced \
             independently on different nodes mesh consistently (same naming priors, same conventions). \
             Split AGGRESSIVELY into many fine independent subtasks — do NOT fear interface divergence. "
        } else {
            ""
        };
        let research_block = if research_findings.is_empty() {
            String::new()
        } else {
            format!("## Prior research findings (use these; do NOT re-research)\n{research_findings}\n\n")
        };
        let lang = detect_language(user_prompt, &[]);
        let lang_directive = lang.directive();
        let entry_clause = lang.entry_clause();
        let test_cmd = lang.test_cmd();
        let no_compile = if lang == TargetLang::Python {
            "(NOT py_compile) "
        } else {
            ""
        };
        let system = format!("You are the ARCHITECT on the smart model. {lang_directive}Produce a PLAN SKELETON ONLY — do NOT write code. \
            You already have any needed research findings — plan DIRECTLY from the task and call final_output FAST; do NOT \
            explore the filesystem or read other directories (a new project has nothing on disk; never read sibling projects). {homo_hint}\n\
            There are {worker_count} worker devices that run in PARALLEL. Decompose into a SMALL number of COHESIVE subtasks — \
            aim for about 2x to 3x {worker_count} total (e.g. ~6-9 for a 3-device fleet), NOT one per command/function. GROUP \
            several related commands or functions into ONE module subtask, and related tests into ONE test subtask. These models \
            are SLOW (minutes per subtask), so too many tiny subtasks serialize and dominate wall-clock while adding no real \
            parallelism past the fleet width — a handful of well-scoped subtasks finishes far sooner than 18 micro-ones. Still keep \
            subtasks INDEPENDENT with NON-OVERLAPPING files and minimal ordering; only add a dependency when a subtask genuinely \
            needs another's output. AVOID deep chains and chokepoints: keep dependency depth <= 2; if shared types/data-models are \
            needed, put them in ONE TINY early subtask so dependents unblock fast — never make most subtasks depend on a single big one.\n\
            MODULAR ARCHITECTURE (hard rule) — keep FILES small and single-responsibility. A subtask may (and for any non-trivial \
            module SHOULD) own SEVERAL small files, ONE concern each (e.g. a parser subtask owns `lexer.py`+`parser.py`+`ast.py`; a \
            models subtask owns `user.py`+`account.py`), NOT one big catch-all file. NEVER assign a single monolithic file that does \
            many unrelated things — split by responsibility. This keeps subtask COUNT low (good for the slow fleet) while the \
            architecture stays modular and readable. Put any logic used by more than one subtask in the ONE early shared subtask and \
            have the others IMPORT it — NEVER let two subtasks each implement the same thing; duplicate implementations of one \
            algorithm are a real defect (two copies drift apart and one silently goes wrong).\n\
            DELIVER ONLY THE APP: decompose the program's actual FUNCTIONALITY — its logic modules, the runnable entry point, and its \
            tests, nothing else. Do NOT add project-scaffolding subtasks: NO CI/workflow config, LICENSE, README/docs, \
            pyproject/setup/packaging, .gitignore, or pre-commit hooks — UNLESS the request explicitly asks for them. They are not the \
            deliverable, they waste the slow fleet, and the weak model tends to claim such a file done without ever writing it.\n\
            DECIDE THE LAYOUT FIRST and pick ONE convention, applied to EVERY file — do NOT mix: EITHER a single package \
            directory `pkgname/` that holds ALL modules AND the cli (imports like `from pkgname.models import X`), with tests \
            under `tests/`; OR fully FLAT (every .py at the project root, imports like `from models import X`). NEVER put the cli \
            in a package while its modules sit at the root. Every subtask's `files` and every import MUST match the one chosen \
            layout exactly.\n\
            AMENDMENT — if the manifest below already lists project files, you are EDITING an existing app, NOT rebuilding it: to \
            ADD a feature, EDIT the EXISTING files IN PLACE. Every subtask that touches existing behavior MUST own the EXACT \
            existing path (e.g. `src/notes/models.py`), and imports MUST match the real modules. NEVER create a PARALLEL renamed \
            module that duplicates one that already exists (do NOT add `render_ascii.py` beside an existing `renderer.py`, or \
            `fern.py` beside `ifs.py`), and NEVER rewire the entry point away from the existing modules — that abandons the working \
            originals as dead/unwired duplicates and breaks the existing tests. NEVER invent a new filename (e.g. `note.py`) for a \
            module that already exists (e.g. `models.py`). Create NEW files ONLY for genuinely-new functionality the existing \
            modules do not already provide (plus a test for it).\n\
            {entry_clause}\n\
            For each subtask provide: id (kebab-case), description (ONE short line — a fuller spec is written separately, keep \
            it terse here), difficulty (\"easy\"|\"hard\"), model (\"qwen/qwen3.6-27b\" if hard else \"qwen/qwen3.6-35b-a3b\"), \
            depends_on (list of ids; empty if independent), files (paths it owns; non-overlapping).\n\
            UNLESS the task is purely text, ALWAYS add a FINAL subtask id \"integrate-verify\" depending_on EVERY other subtask, \
            difficulty \"hard\": be EFFICIENT (do not re-read every file; rely on the test run). It RUNS `{test_cmd}` \
            {no_compile}and fixes EVERY failure until GREEN — INCLUDING a pre-existing test that now fails because this \
            change intentionally altered behavior (e.g. a new field appears in a serialized dict): in that case EDIT that \
            existing test to assert the new correct output. Do not stall — make the whole suite pass. Then a GOLDEN-VALUE CHECK: \
            for EACH command/subcommand the spec advertises (NOT only the default one), run it on a concrete input the spec gives \
            or directly implies, and verify the ACTUAL output equals the SPECIFIC value the spec implies — not merely that it \
            starts or exits 0. For a MULTI-OUTPUT command (one with a --count N, or that lists N results) verify ALL N outputs are \
            correct AND, where the semantics require distinct results (e.g. the NEXT N occurrences of a schedule), that they are \
            genuinely distinct at the right granularity, not near-duplicates. Derive the expected value from the spec's semantics; \
            do NOT invent an output just to make the check pass. A green test suite does NOT prove correctness: a real failure mode \
            is a code path producing WRONG output (wrong constants, off-by-one, wrong granularity) while every shape-only test \
            passes — fix the ROOT CAUSE if the actual output is wrong. RUN the program \
            through its BUILT, ADVERTISED entry point — if the language compiles, BUILD it first (e.g. `npm run build` \
            for TypeScript, `cargo build` for Rust) and run the BUILT artifact (e.g. `node dist/cli.js`), NOT the source \
            via tsx/ts-node — using the EXACT commands the spec advertises (the SAME subcommands and argument shapes shown \
            in the goal; do NOT silently redesign the interface into flags). When you run the BUILT entry directly, the \
            spec LEADING program/bin name is the program ITSELF, never an argument: spec `app build x` runs as \
            `node dist/cli.js build x` or `python3 -m app build x`, NEVER `node dist/cli.js app build x` (mis-prefixing \
            the bin name makes a WORKING app look broken). If a build step or a build config the entry \
            needs (e.g. a tsconfig.json) is MISSING so the advertised entry will not build or run, that is a FAILURE — add it. \
            ALSO confirm the \
            spec's HEADLINE deliverable is actually REACHABLE and surfaced through the default command — a feature whose \
            module exists but is never WIRED into the entry point (so the spec's main ask never appears in the output) is a \
            FAILURE: wire it. Reports PASS/FAIL honestly. \
            Its own files must NOT overlap the others. Then call the final_output tool with the plan.");
        let user_msg = format!("{research_block}Plan this task: {user_prompt}");
        // Models to draw skeleton drafts from: planner first (so best_of_n=1 == today exactly), then
        // the fleet workers round-robin.
        let draft_models: Vec<String> = std::iter::once(planner_model.to_string())
            .chain(worker_models.iter().cloned())
            .collect();
        let n = best_of_n.max(1);
        if n > 1 {
            eprintln!(
                "  drafting {} skeleton candidate(s) IN PARALLEL, picking the structurally-best (deterministic, no LLM judge)",
                n
            );
        }
        let mut handles = Vec::new();
        for i in 0..n {
            let me = self.clone();
            let model = draft_models[i % draft_models.len()].clone();
            let sys = system.clone();
            let um = user_msg.clone();
            let schema = plan_schema.clone();
            handles.push(tokio::spawn(async move {
                // Wall-clock cap per skeleton draft. The planner watchdog is IDLE-based (no-progress),
                // so a runaway SINGLE generation on a slow local (non-q5) model can stream for 20+ min
                // without ever going idle, hanging the whole run before execute starts. On timeout the
                // draft is dropped; best-of-N (and the solo-planner fallback) then take over.
                tokio::time::timeout(
                    std::time::Duration::from_secs(480),
                    me.run_agent_timed(
                        &model,
                        sys,
                        um,
                        Some(Response {
                            json_schema: Some(schema),
                        }),
                        12,
                        &[],
                    ),
                )
                .await
                .ok()
                .and_then(|r| r.ok())
                .and_then(|o| o.final_output)
            }));
        }
        let mut candidates: Vec<String> = Vec::new();
        for h in handles {
            if let Ok(Some(j)) = h.await {
                candidates.push(j);
            }
        }
        // Pick the best skeleton with a PURE-RUST structural scorer (validity borrowed from the same
        // Dag::from_specs the live path uses) — no LLM in the merge/select path. n==1 keeps the old
        // behavior exactly (use the single draft as-is). On no valid candidate, Err -> solo plan().
        let (skeleton, agreement_conf): (String, Option<u8>) = if n == 1 {
            (
                candidates
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow!("architect produced no skeleton"))?,
                None,
            )
        } else {
            let mut best: Option<(i64, String)> = None;
            let mut valid_specs: Vec<Vec<goose_swarm::TaskSpec>> = Vec::new();
            for (i, c) in candidates.into_iter().enumerate() {
                let specs = match goose_swarm::specs_from_plan_json(&c) {
                    Ok(s) => s,
                    Err(_) => {
                        eprintln!("  · candidate {i}: invalid JSON — skipped");
                        continue;
                    }
                };
                match score_skeleton(&specs, worker_count) {
                    Some(score) => {
                        eprintln!(
                            "  · candidate {i}: score {score} ({} subtasks)",
                            specs.len()
                        );
                        valid_specs.push(specs);
                        if best.as_ref().map(|(b, _)| score > *b).unwrap_or(true) {
                            best = Some((score, c));
                        }
                    }
                    None => eprintln!("  · candidate {i}: invalid DAG — skipped"),
                }
            }
            match best {
                Some((score, json)) => {
                    // M6: plan confidence from cross-draft AGREEMENT (self-consistency is calibrated where
                    // verbalized confidence is overconfident). Low agreement = the model doesn't really know
                    // how to decompose this — a signal to research more before committing (M6 step 3).
                    let (conf, reason) = plan_agreement(&valid_specs);
                    eprintln!(
                        "  {} picked best skeleton (score {score}) — plan confidence {conf}/100 ({reason})",
                        style("✓").green().bold()
                    );
                    (json, Some(conf))
                }
                None => return Err(anyhow!("no valid skeleton among {n} candidates")),
            }
        };
        // M6 step2: a SEPARATE, deliberately harsh self-rating of the chosen plan. Verbalized confidence is
        // systematically overconfident, so it is the SECONDARY signal (0.3) behind the calibrated
        // cross-draft agreement (0.7). One extra planner call, only on the best-of-N path (the opt-in
        // plan-quality experiment) so single-draft runs keep their old latency exactly.
        // Hoisted out of `if n > 1` so the confidence + the model's stated uncertainties are RETURNED to the
        // caller (the GOOSE_SWARM_ASK gate consumes them). n==1 keeps its old latency (no verbalized call)
        // and yields the inert agreement default — the ask gate forces best_of_n>=2 anyway.
        let (plan_conf, uncertainties): (Option<u8>, String) = if n > 1 {
            let verbalized = self
                .verbalized_confidence(planner_model, user_prompt, &skeleton)
                .await;
            let final_conf = match (agreement_conf, verbalized.as_ref()) {
                (Some(a), Some((v, _))) => {
                    Some(((f32::from(a) * 0.7) + (f32::from(*v) * 0.3)).round() as u8)
                }
                (Some(a), None) => Some(a),
                (None, Some((v, _))) => Some(*v),
                (None, None) => Some(60),
            };
            let unc = verbalized
                .as_ref()
                .map(|(_, u)| u.clone())
                .unwrap_or_default();
            if let Some((v, u)) = &verbalized {
                eprintln!(
                    "  plan self-confidence {v}/100 (verbalized, discounted){}",
                    if u.is_empty() {
                        String::new()
                    } else {
                        format!(" — uncertainties: {u}")
                    }
                );
            }
            if let Some(fc) = final_conf {
                eprintln!("  {} final plan confidence {fc}/100", style("◆").cyan());
            }
            (final_conf, unc)
        } else {
            (agreement_conf, String::new())
        };
        let mut v: serde_json::Value = serde_json::from_str(&skeleton)?;
        // Deterministically ensure a final integrate-verify sink: the weak architect sometimes OMITS it
        // despite the prompt, and without it nothing smoke-runs the program end-to-end — so a broken entry
        // point (Click `ctx.obj` None, argparse `dest=` on a positional, a bad import) SHIPS green because
        // unit tests bypass the CLI. Inject one depending on every other subtask if it is missing.
        if let Some(arr) = v.get_mut("subtasks").and_then(|s| s.as_array_mut()) {
            let has_iv = arr
                .iter()
                .any(|s| s.get("id").and_then(|i| i.as_str()) == Some("integrate-verify"));
            if !has_iv && arr.len() > 1 {
                let ids: Vec<serde_json::Value> =
                    arr.iter().filter_map(|s| s.get("id").cloned()).collect();
                let iv_desc = format!(
                    "Integrate every module and VERIFY the whole program works end-to-end: run the test suite ({}), then BUILD + ACTUALLY RUN the program's ADVERTISED entry point ({}) AND run EVERY command/usage the SPEC advertises — the exact example invocations from the goal, with the SAME subcommands and argument shapes the spec shows (do NOT redesign the interface into flags). INVOCATION: when you run the BUILT entry directly, the spec LEADING program/bin name is the program ITSELF, never an argument — spec `app build x` runs as `node dist/cli.js build x` or `python3 -m app build x`, NEVER `node dist/cli.js app build x`; mis-prefixing the bin name makes a WORKING app look broken. For EACH command do a GOLDEN-VALUE CHECK: feed a concrete input the spec gives or implies and confirm the ACTUAL output equals the SPECIFIC value the spec implies (not just exit 0); for a MULTI-OUTPUT command (--count N / a list of N) confirm all N are correct AND genuinely distinct at the right granularity where the semantics require it (e.g. the next N occurrences). Do NOT invent an expected output to pass the check. FIX any build error, missing build config (e.g. a tsconfig.json the build needs), runtime crash, OR wrong output (wrong constants/off-by-one/wrong granularity) at the ROOT CAUSE. A green test suite does NOT prove the CLI runs or is correct, and running the source directly does NOT prove the BUILT/advertised entry works.",
                    lang.test_cmd(),
                    lang.entry_run_example()
                );
                arr.push(serde_json::json!({
                    "id": "integrate-verify",
                    "description": iv_desc,
                    "depends_on": ids,
                    "files": [],
                    "difficulty": "hard",
                    "model": "qwen/qwen3.6-27b"
                }));
                eprintln!("  · injected missing integrate-verify sink (architect omitted it)");
            }
        }
        // A failing unit-test subtask must not BLOCK integrate-verify (it runs the program, not the tests) —
        // else the run reports FAILED while integrate-verify never ran to confirm whether the app works.
        let stripped = strip_integrate_verify_test_deps(&mut v, lang);
        if stripped > 0 {
            eprintln!(
                "  · integrate-verify no longer waits on the test subtask(s) ({stripped} dep(s) stripped) — a failing test will not hide whether the app actually runs"
            );
        }
        let items: Vec<(usize, String, String, String)> = v
            .get("subtasks")
            .and_then(|s| s.as_array())
            .ok_or_else(|| anyhow!("skeleton has no subtasks array"))?
            .iter()
            .enumerate()
            .map(|(i, st)| {
                // The EXACT paths this subtask owns — passed to the detailer so its spec refers to them
                // verbatim. Without this the detailer invents a filename that contradicts the owned_files
                // (e.g. spec says formula_parser.py while the plan owns parser.py); the worker follows the
                // spec, never writes the owned file, and the task fails its owned-file check every attempt.
                let files = st["files"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|f| f.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                (
                    i,
                    st["id"].as_str().unwrap_or("").to_string(),
                    st["description"].as_str().unwrap_or("").to_string(),
                    files,
                )
            })
            .collect();
        if items.is_empty() {
            return Err(anyhow!("skeleton had zero subtasks"));
        }
        eprintln!(
            "  skeleton: {} subtask(s) → detailing IN PARALLEL across the fleet:",
            items.len()
        );
        let wm = if worker_models.is_empty() {
            vec![planner_model.to_string()]
        } else {
            worker_models
        };
        let goal = user_prompt.to_string();
        let findings = research_findings.to_string();
        let me = self.clone();
        // One detail call per device (work-stealing): a weight-1 node never has a second detail queued
        // behind the first. Each item grabs the next free node, so the fleet stays busy without
        // over-dispatching; on timeout/empty/error we fall back to the architect's brief line.
        let results = fanout_over_fleet(wm, items, move |(idx, id, brief, files), model| {
            let me = me.clone();
            let goal = goal.clone();
            let findings = findings.clone();
            async move {
                let started = std::time::Instant::now();
                eprintln!(
                    "  {} detail {} → {}",
                    style("▸").cyan().bold(),
                    style(&id).bold(),
                    model
                );
                let fb = if findings.is_empty() {
                    String::new()
                } else {
                    format!("\n\nResearch findings:\n{findings}")
                };
                let system = "You are detailing ONE subtask of a larger plan into a precise, implementation-ready \
                    spec for the worker who will build it: exact function/class names and signatures, key logic, the \
                    files it owns, edge cases to handle, and what its tests must check. Use the EXACT file paths the \
                    subtask owns (given below) verbatim — NEVER invent, rename, or pluralize a filename. Be concrete \
                    and self-contained, and BRIEF — about 150 words, no preamble. Output ONLY the spec prose; do NOT \
                    write code files or restate the whole project."
                    .to_string();
                let files_line = if files.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n\nThis subtask owns EXACTLY these file(s): {files}\nYour spec MUST refer to the \
                         worker's files by these EXACT paths — do NOT use a different name (e.g. do not write \
                         formula_parser.py when the owned file is parser.py): a mismatched filename makes the \
                         worker write the wrong file and the task FAILS its owned-file check on every attempt."
                    )
                };
                let user = format!("Overall goal: {goal}\n\nThis subtask: [{id}] {brief}{files_line}{fb}");
                let desc = match tokio::time::timeout(
                    std::time::Duration::from_secs(75),
                    me.run_agent(&model, system, user, None, 6, &[], 0, None),
                )
                .await
                {
                    Ok(Ok(o)) if !o.text.trim().is_empty() => o.text,
                    _ => brief,
                };
                eprintln!(
                    "  {} detail {} ({:.0}s)",
                    style("✓").green().bold(),
                    style(&id).bold(),
                    started.elapsed().as_secs_f64()
                );
                (idx, desc)
            }
        })
        .await;
        for (idx, desc) in results {
            v["subtasks"][idx]["description"] = serde_json::Value::String(desc);
        }
        Ok((v.to_string(), plan_conf, uncertainties))
    }

    pub async fn plan(
        &self,
        planner_model: &str,
        user_prompt: &str,
        plan_schema: serde_json::Value,
        worker_count: usize,
        research_findings: &str,
    ) -> Result<(String, Option<u8>, String)> {
        let lang = detect_language(user_prompt, &[]);
        let test_cmd = lang.test_cmd();
        let system = format!("You are the PLANNER on the smart model. Produce a PLAN ONLY — do NOT write code.\n\
            There are {worker_count} worker devices that run in PARALLEL — decompose into MANY small INDEPENDENT subtasks \
            (split by file / module / feature) and aim for AT LEAST {worker_count} independent subtasks (one or more per worker; more is better) \
            with NON-OVERLAPPING files and NO ordering dependency, so no worker sits idle. Only add a dependency when a subtask genuinely \
            needs another's output; a wide independent set is the goal.\n\
            For each subtask provide: id (kebab-case), description (a precise self-contained spec), difficulty (\"easy\"|\"hard\"), \
            model (\"qwen/qwen3.6-27b\" if hard else \"qwen/qwen3.6-35b-a3b\"), depends_on (list of ids; empty if independent), \
            files (paths it owns; non-overlapping across parallel subtasks).\n\
            UNLESS the task is purely text with nothing to integrate, ALWAYS add a FINAL subtask id \"integrate-verify\" \
            that depends_on EVERY other subtask, difficulty \"hard\", model \"qwen/qwen3.6-27b\": it integrates the produced \
            files, RUNS `{test_cmd}`, and fixes EVERY failure until GREEN — including a pre-existing test that now \
            fails because the change intentionally altered behavior (EDIT that existing test to assert the new output; do not \
            stall). Then BUILD + RUN the program's ADVERTISED entry point (build first if it compiles — e.g. `npm run \
            build` for TypeScript, `cargo build` for Rust — and run the BUILT artifact, NOT the source via tsx/ts-node) \
            using the EXACT commands the SPEC advertises (the SAME subcommands and argument shapes; do NOT redesign the \
            interface into flags). When you run the BUILT entry directly, the spec LEADING program/bin name is the \
            program ITSELF, never an argument: spec `app build x` runs as `node dist/cli.js build x` or `python3 -m \
            app build x`, NEVER `node dist/cli.js app build x` (mis-prefixing the bin name fails a WORKING app). \
            GOLDEN-VALUE CHECK each command: feed a spec-given/implied input and confirm the ACTUAL \
            output equals the SPECIFIC value the spec implies (not just exit 0); for a MULTI-OUTPUT command (--count N / a list) \
            confirm all N are correct AND distinct at the right granularity where required; do NOT invent an expected output. \
            FIX wrong output (wrong constants/off-by-one/wrong granularity) at the ROOT CAUSE, and ADD any missing build config \
            the entry needs (e.g. tsconfig.json). Reports PASS/FAIL; its files must NOT overlap the others.\n\
            Also produce a short integration note. Then call the final_output tool with the plan.");
        let response = Some(Response {
            json_schema: Some(plan_schema),
        });
        let research_block = if research_findings.is_empty() {
            String::new()
        } else {
            format!("## Prior research findings (use these; do NOT re-research)\n{research_findings}\n\n")
        };
        let out = self
            .run_agent_timed(
                planner_model,
                system,
                format!("{research_block}Plan this task: {user_prompt}"),
                response,
                15,
                &[],
            )
            .await?;
        let plan = out
            .final_output
            .ok_or_else(|| anyhow!("planner did not produce a final_output plan"))?;
        // Solo planner has no cross-draft confidence; the ask gate forces the best-of-N path instead.
        Ok((plan, None, String::new()))
    }
}

/// Language-aware per-file syntax check, dispatched on extension. `.py` -> the Python ast.parse check
/// verbatim (byte-identical); other languages have no cheap parse-only per-file check (tsc/rustc/etc. are
/// project-level), so they skip cleanly (None) and rely on the language's own build/test step.
async fn syntax_error(path: &Path) -> Option<String> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => py_syntax_error(path).await,
        _ => None,
    }
}

/// Syntax-check a Python file without polluting `__pycache__` (ast.parse, not py_compile). Returns the
/// last error line on a SyntaxError, `None` if it parses.
async fn py_syntax_error(path: &Path) -> Option<String> {
    let out = tokio::process::Command::new("python3")
        .arg("-c")
        .arg("import ast,sys; ast.parse(open(sys.argv[1]).read())")
        .arg(path)
        .output()
        .await
        .ok()?;
    if out.status.success() {
        None
    } else {
        Some(
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("syntax error")
                .trim()
                .to_string(),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// GOOSE_SWARM_SMOKE — deterministic end-to-end smoke gate (Track A #1, off by default).
//
// After the scheduler finishes, the HARNESS (not the weak model) runs ground-truth oracles on the
// produced tree: `pytest --collect-only -q` imports every module + test and surfaces the
// cross-module ImportError that isolation-only unit tests miss, and `python3 -m <pkg> --help`
// confirms the CLI entry point actually runs. Both need zero model intelligence and fire precisely
// on the multi-module apps where a weak fleet only reports "it runs / PASS" without ever invoking
// the binary. Findings are emitted to the run jsonl as a `smoke` event for the eval to read.
// ---------------------------------------------------------------------------------------------

/// Verdict of `pytest --collect-only`: the project imports cleanly, pytest is unavailable (so the
/// check is inconclusive, NOT a failure), or there are real collection errors (the finding).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
enum CollectVerdict {
    Ok,
    PytestMissing,
    Errors(String),
}

/// Interpret a `python3 -m pytest --collect-only -q` run from its exit code + combined output. Pure
/// (no I/O) so it is unit-tested without spawning Python. Exit 5 ("no tests collected") is NOT an
/// error; a missing pytest module makes the check inconclusive, never a failure.
fn interpret_pytest_collect(code: Option<i32>, output: &str) -> CollectVerdict {
    let low = output.to_lowercase();
    if low.contains("no module named pytest") || low.contains("no module named 'pytest'") {
        return CollectVerdict::PytestMissing;
    }
    match code {
        Some(0) | Some(5) => CollectVerdict::Ok,
        _ => {
            let tail = tail_lines(output, 40);
            CollectVerdict::Errors(if tail.is_empty() {
                "pytest collection failed".to_string()
            } else {
                tail
            })
        }
    }
}

/// Verdict of RUNNING the generated test suite (`pytest -q`) — the deterministic RUNTIME oracle that
/// exercises real code paths `--help` + `--collect-only` never touch. This is the exact class the two
/// other gates are blind to: the broken_code judge only COMPILES (a runtime crash on an un-run path
/// passes), and import-only smoke never invokes a command (verified: UNIQ21 shipped a member-list
/// crash + broken export that `--help` never hit). The generated tests ARE the model's representative
/// invocations, so running them needs zero command-synthesis. Passing, an absent/unavailable pytest
/// (inconclusive — never a failure), no tests, or real failures (the finding).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
enum TestRunVerdict {
    Pass,
    NoTests,
    PytestMissing,
    Failures(String),
}

/// Interpret a `python3 -m pytest -q` run from its exit code + combined output. Pure (no I/O) so it is
/// unit-tested without spawning Python. Exit 0 = all pass; exit 5 = no tests collected (inconclusive, not
/// a failure); a missing pytest module is inconclusive; any other non-zero is a real test failure/error
/// (the finding). Mirrors `interpret_pytest_collect`'s "missing/none is never a failure" rule so the gate
/// only ever fails on a genuine, reproducible runtime failure.
fn interpret_pytest_run(code: Option<i32>, output: &str) -> TestRunVerdict {
    let low = output.to_lowercase();
    if low.contains("no module named pytest") || low.contains("no module named 'pytest'") {
        return TestRunVerdict::PytestMissing;
    }
    match code {
        Some(0) => TestRunVerdict::Pass,
        Some(5) => TestRunVerdict::NoTests,
        _ => {
            let tail = tail_lines(output, 40);
            TestRunVerdict::Failures(if tail.is_empty() {
                "pytest reported test failures".to_string()
            } else {
                tail
            })
        }
    }
}

/// The last `n` non-blank lines of `s`, in original order — captures a traceback tail for a hint.
fn tail_lines(s: &str, n: usize) -> String {
    let mut lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines.drain(..start);
    lines.join("\n")
}

/// The runnable package for `python3 -m <pkg>`: the shallowest top-level (`pkg/__main__.py`) or
/// `src/`-layout (`src/pkg/__main__.py`) directory owning a `__main__.py`. `rel_paths` are `.py`
/// paths relative to the project root. `None` means there is NO module entry point — itself a smoke
/// finding (an unrunnable app), which is exactly the built-but-unwired failure class.
fn entry_package_from_paths(rel_paths: &[String]) -> Option<String> {
    let mut pkgs: Vec<String> = rel_paths
        .iter()
        .filter_map(|p| {
            let p = p.replace('\\', "/");
            let segs: Vec<&str> = p.split('/').collect();
            let pkg = match segs.as_slice() {
                [pkg, "__main__.py"] => pkg,
                ["src", pkg, "__main__.py"] => pkg,
                _ => return None,
            };
            (!pkg.starts_with('.')).then(|| pkg.to_string())
        })
        .collect();
    pkgs.sort();
    pkgs.dedup();
    pkgs.into_iter().next()
}

/// Recursively collect `.py` files under `root` (to ~3 levels), skipping vendored/build/cache dirs.
fn collect_py_files(root: &Path) -> Vec<PathBuf> {
    const SKIP: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        ".venv",
        ".swarm",
        "__pycache__",
    ];
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if depth == 0 || name.starts_with('.') || SKIP.contains(&name.as_str()) {
                    continue;
                }
                walk(&p, depth - 1, out);
            } else if p.extension().and_then(|e| e.to_str()) == Some("py") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 3, &mut out);
    out
}

/// Outcome of the smoke gate, serialized into the run jsonl `smoke` event.
#[derive(Debug, Clone, Serialize)]
struct SmokeResult {
    ran: bool,
    py_files: usize,
    collect: Option<CollectVerdict>,
    tests: Option<TestRunVerdict>,
    entry_package: Option<String>,
    entry_ok: Option<bool>,
    findings: Vec<String>,
}

impl SmokeResult {
    fn passed(&self) -> bool {
        self.ran && self.findings.is_empty()
    }
    /// The gate did not apply to this tree (no recognized build) — never a failure.
    fn skipped() -> Self {
        SmokeResult {
            ran: false,
            py_files: 0,
            collect: None,
            tests: None,
            entry_package: None,
            entry_ok: None,
            findings: vec![],
        }
    }
}

/// Run the deterministic end-to-end smoke oracles on the produced tree at `root`. No-ops
/// (`ran=false`) when there is no Python. A missing `python3`/`pytest` is inconclusive, never a
/// failure; the findings are cross-module import errors and an entry point that fails or is absent.
async fn run_smoke_gate(root: &Path, lang: TargetLang) -> SmokeResult {
    // Dispatch by language: TS/Rust get their own build+run oracles (below). Go/Other have no profile yet
    // -> skip cleanly. Python falls through to the unchanged pytest/`-m` logic below — byte-identical.
    match lang {
        TargetLang::TypeScript => return smoke_typescript(root).await,
        TargetLang::Rust => return smoke_rust(root).await,
        TargetLang::Python => {}
        TargetLang::Go | TargetLang::Other => return SmokeResult::skipped(),
    }
    let py = collect_py_files(root);
    if py.is_empty() {
        return SmokeResult {
            ran: false,
            py_files: 0,
            collect: None,
            tests: None,
            entry_package: None,
            entry_ok: None,
            findings: vec![],
        };
    }
    let mut findings: Vec<String> = Vec::new();

    // 1) collect-only — imports every module + test, surfacing cross-module ImportError.
    let mut collect_cmd = tokio::process::Command::new("python3");
    collect_cmd
        .args(["-m", "pytest", "--collect-only", "-q"])
        .current_dir(root);
    let collect = match smoke_output(collect_cmd, 90).await {
        Some(out) => {
            let combined = combined_output(&out);
            let v = interpret_pytest_collect(out.status.code(), &combined);
            if let CollectVerdict::Errors(ref t) = v {
                findings.push(format!(
                    "pytest --collect-only errors (cross-module import?):\n{t}"
                ));
            }
            Some(v)
        }
        None => None, // python3 missing / timed out -> inconclusive, not a failure
    };

    // 1b) RUN the generated tests — the runtime oracle. `--help`/`--collect-only` never execute a real
    // code path; the suite does, catching the runtime-crash class both other gates are blind to. Gated on
    // a clean collect (Ok) so an import error is reported once as its own finding; a real failure becomes a
    // finding that feeds the SAME corrective re-dispatch as the collect/entry findings.
    let tests = if matches!(collect, Some(CollectVerdict::Ok)) {
        let mut run_cmd = tokio::process::Command::new("python3");
        run_cmd.args(["-m", "pytest", "-q"]).current_dir(root);
        match smoke_output(run_cmd, 120).await {
            Some(out) => {
                let combined = combined_output(&out);
                let v = interpret_pytest_run(out.status.code(), &combined);
                if let TestRunVerdict::Failures(ref t) = v {
                    findings.push(format!(
                        "`pytest -q` failed — the generated tests exercise runtime paths that \
                         `--help`/`--collect-only` never invoke:\n{t}"
                    ));
                }
                Some(v)
            }
            None => None, // pytest missing / timed out -> inconclusive, not a failure
        }
    } else {
        None
    };

    // 2) entry point — `python3 -m <pkg> --help` must exit 0.
    let rel: Vec<String> = py
        .iter()
        .filter_map(|p| {
            p.strip_prefix(root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
        })
        .collect();
    let entry_package = entry_package_from_paths(&rel);
    let entry_ok = if let Some(ref pkg) = entry_package {
        let existing = std::env::var("PYTHONPATH").unwrap_or_default();
        let pythonpath = if existing.is_empty() {
            "src".to_string()
        } else {
            format!("src:{existing}")
        };
        let mut help_cmd = tokio::process::Command::new("python3");
        help_cmd
            .args(["-m", pkg.as_str(), "--help"])
            .current_dir(root)
            .env("PYTHONPATH", pythonpath);
        match smoke_output(help_cmd, 30).await {
            Some(out) => {
                let ok = out.status.success();
                if !ok {
                    let combined = combined_output(&out);
                    findings.push(format!(
                        "`python3 -m {pkg} --help` failed (exit {}):\n{}",
                        out.status.code().unwrap_or(-1),
                        tail_lines(&combined, 40)
                    ));
                }
                Some(ok)
            }
            None => None,
        }
    } else {
        findings.push(
            "no `python3 -m <pkg>` entry point (no package with __main__.py) — the app may be \
             unrunnable"
                .to_string(),
        );
        None
    };

    SmokeResult {
        ran: true,
        py_files: py.len(),
        collect,
        tests,
        entry_package,
        entry_ok,
        findings,
    }
}

/// Run a smoke subcommand with a HARD TIMEOUT + null stdin, so a produced server/REPL/daemon that ignores
/// `--help` (or a build that waits on input) can never hang the whole run at the finish line. Returns None on
/// spawn error OR timeout (inconclusive — never a finding); the child is killed on drop when the timeout fires.
async fn smoke_output(mut cmd: tokio::process::Command, secs: u64) -> Option<std::process::Output> {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    match tokio::time::timeout(std::time::Duration::from_secs(secs), cmd.output()).await {
        Ok(Ok(out)) => Some(out),
        _ => None, // spawn error or timed out -> inconclusive
    }
}

/// stdout+stderr of a finished smoke process, lossily decoded.
fn combined_output(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A real V8/Node stack frame: a line whose trimmed form starts with `at ` and carries a `:<digit>` location
/// (`at fn (file:line:col)` / `at file:line:col`), NOT ordinary indented help prose like "at least one of
/// --foo" or "at most 3 items" (no `:<digit>`). Distinguishes a true uncaught crash from a usage message.
fn has_stack_frame(output: &str) -> bool {
    output.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("at ")
            && t.as_bytes()
                .windows(2)
                .any(|w| w[0] == b':' && w[1].is_ascii_digit())
    })
}

/// A Node uncaught-exception signature: the process exited NONZERO AND its output has a real stack frame. A
/// clean `--help` (exit 0) or a clean nonzero rejection with no stack is NOT a crash, so the gate never flags
/// a benign exit code (it intentionally misses caught-error/no-stack bugs like APP6 — that is golden-value).
fn looks_like_runtime_crash(output: &str, success: bool) -> bool {
    !success && has_stack_frame(output)
}

/// A Rust panic: the process exited NONZERO AND printed a panic message (vs a clean usage/exit).
fn looks_like_rust_panic(output: &str, success: bool) -> bool {
    !success && (output.contains("panicked at") || output.contains("thread 'main' panicked"))
}

/// The runnable entry path from a parsed package.json: `bin` (string or first object value), else `main`,
/// else the first source-file token in `scripts.start` (`.js/.mjs/.cjs` OR `.ts/.tsx` for tsx/ts-node apps).
/// Leading `./` stripped. None if none declared. A `.ts/.tsx` entry is detected but NOT run with bare `node`.
fn ts_entry_from_package_json(pkg: &serde_json::Value) -> Option<String> {
    let norm = |s: &str| s.trim_start_matches("./").to_string();
    if let Some(bin) = pkg.get("bin") {
        if let Some(s) = bin.as_str() {
            return Some(norm(s));
        }
        if let Some(v) = bin.as_object().and_then(|o| o.values().next()?.as_str()) {
            return Some(norm(v));
        }
    }
    if let Some(m) = pkg.get("main").and_then(|v| v.as_str()) {
        return Some(norm(m));
    }
    if let Some(start) = pkg
        .get("scripts")
        .and_then(|s| s.get("start"))
        .and_then(|v| v.as_str())
    {
        for tok in start.split_whitespace() {
            if [".js", ".mjs", ".cjs", ".ts", ".tsx"]
                .iter()
                .any(|e| tok.ends_with(e))
            {
                return Some(norm(tok));
            }
        }
    }
    None
}

/// True if package.json declares a way to RUN the app other than a built `node` entry (a start/dev/serve
/// script) — used to suppress the "no entry" finding for tsx/ts-node apps that legitimately have no bin/main.
fn has_run_script(pkg: &serde_json::Value) -> bool {
    pkg.get("scripts")
        .and_then(|s| s.as_object())
        .map(|o| {
            ["start", "dev", "serve"]
                .iter()
                .any(|k| o.get(*k).and_then(|v| v.as_str()).is_some())
        })
        .unwrap_or(false)
}

/// Deterministic TypeScript/Node smoke oracle: `npm run build` must succeed, the package.json entry artifact
/// must EXIST after the build (catches built-but-unwired), and running it (`node <entry> --help`) must not
/// CRASH at runtime (catches APP6-class "compiles but throws on every input"). A missing `npm`/`node`, or no
/// package.json, is inconclusive (ran=false), never a failure.
async fn smoke_typescript(root: &Path) -> SmokeResult {
    let pkg_path = root.join("package.json");
    let pkg: serde_json::Value = match std::fs::read_to_string(&pkg_path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(_) => return SmokeResult::skipped(),
        },
        Err(_) => return SmokeResult::skipped(),
    };
    let mut findings: Vec<String> = Vec::new();
    // 1) build (only if a build script is declared — many tiny TS CLIs run via tsx with no build step).
    let has_build = pkg
        .get("scripts")
        .and_then(|s| s.get("build"))
        .and_then(|v| v.as_str())
        .is_some();
    // A missing node_modules would fail `npm run build`/the entry run SPURIOUSLY (a false finding). If deps
    // are declared but not installed, best-effort `npm install --include=dev` (so an inherited
    // NODE_ENV=production cannot drop the build toolchain); if that fails (offline), skip the whole TS gate
    // as inconclusive rather than report a phantom build failure.
    let needs_deps = pkg.get("dependencies").is_some() || pkg.get("devDependencies").is_some();
    if needs_deps && !root.join("node_modules").exists() {
        let mut c = tokio::process::Command::new("npm");
        c.args(["install", "--no-audit", "--no-fund", "--include=dev"])
            .current_dir(root);
        match smoke_output(c, 180).await {
            Some(out) if out.status.success() => {}
            _ => return SmokeResult::skipped(), // cannot install deps / timed out -> inconclusive
        }
    }
    if has_build {
        let mut c = tokio::process::Command::new("npm");
        c.args(["run", "build"]).current_dir(root);
        match smoke_output(c, 180).await {
            Some(out) if !out.status.success() => {
                let combined = combined_output(&out);
                findings.push(format!(
                    "`npm run build` failed (exit {}):\n{}",
                    out.status.code().unwrap_or(-1),
                    tail_lines(&combined, 40)
                ));
            }
            Some(_) => {}
            None => return SmokeResult::skipped(), // npm missing / timed out -> inconclusive
        }
    }
    // 2) entry: confirm it exists, and for a BUILT (.js/.mjs/.cjs) entry that it runs without an uncaught
    // crash. A .ts/.tsx entry (tsx/ts-node) is NOT run with bare `node` — node cannot parse TS on the common
    // LTS, which would throw a SyntaxError and FALSE-flag a healthy app; we only confirm its presence.
    let entry_ok = match ts_entry_from_package_json(&pkg) {
        Some(entry_rel) => {
            let entry_path = root.join(&entry_rel);
            let is_js = [".js", ".mjs", ".cjs"]
                .iter()
                .any(|e| entry_rel.ends_with(e));
            if !entry_path.exists() {
                // Only a missing BUILT artifact is a real unwired-entry finding. A missing .ts source with a
                // run script (tsx) is left inconclusive, not flagged.
                if is_js {
                    findings.push(format!(
                        "the package.json entry `{entry_rel}` is missing after build — the app is unrunnable \
                         (built-but-unwired entry point)"
                    ));
                    Some(false)
                } else {
                    None
                }
            } else if is_js {
                let mut c = tokio::process::Command::new("node");
                c.arg(&entry_path).arg("--help").current_dir(root);
                match smoke_output(c, 30).await {
                    Some(out) => {
                        let combined = combined_output(&out);
                        if looks_like_runtime_crash(&combined, out.status.success()) {
                            findings.push(format!(
                                "running the entry `node {entry_rel} --help` CRASHES at runtime:\n{}",
                                tail_lines(&combined, 40)
                            ));
                            Some(false)
                        } else {
                            Some(true)
                        }
                    }
                    None => None, // node missing / timed out -> inconclusive
                }
            } else {
                None // a .ts/.tsx entry: present, but not run with bare node (inconclusive)
            }
        }
        None => {
            // No bin/main/start entry. If a start/dev/serve script exists the app is runnable via tsx -> not
            // unwired; only flag a true no-way-to-run.
            if has_run_script(&pkg) {
                None
            } else {
                findings.push(
                    "no package.json bin/main/start entry — the app may be unrunnable".to_string(),
                );
                None
            }
        }
    };
    SmokeResult {
        ran: true,
        py_files: 0,
        collect: None,
        tests: None,
        entry_package: None,
        entry_ok,
        findings,
    }
}

/// Deterministic Rust smoke oracle: `cargo build` must succeed and `cargo run -- --help` must not PANIC.
/// A missing `cargo` or no Cargo.toml is inconclusive (ran=false), never a failure.
async fn smoke_rust(root: &Path) -> SmokeResult {
    if !root.join("Cargo.toml").exists() {
        return SmokeResult::skipped();
    }
    let mut findings: Vec<String> = Vec::new();
    let mut build = tokio::process::Command::new("cargo");
    build.args(["build", "--quiet"]).current_dir(root);
    match smoke_output(build, 240).await {
        Some(out) if !out.status.success() => {
            let combined = combined_output(&out);
            findings.push(format!(
                "`cargo build` failed:\n{}",
                tail_lines(&combined, 40)
            ));
            // Can't run an entry that didn't build — report the build failure and stop here.
            return SmokeResult {
                ran: true,
                py_files: 0,
                collect: None,
                tests: None,
                entry_package: None,
                entry_ok: Some(false),
                findings,
            };
        }
        Some(_) => {}
        None => return SmokeResult::skipped(), // cargo missing / timed out -> inconclusive
    }
    let mut run = tokio::process::Command::new("cargo");
    run.args(["run", "--quiet", "--", "--help"])
        .current_dir(root);
    let entry_ok = match smoke_output(run, 60).await {
        Some(out) => {
            let combined = combined_output(&out);
            if looks_like_rust_panic(&combined, out.status.success()) {
                findings.push(format!(
                    "`cargo run -- --help` PANICS at runtime:\n{}",
                    tail_lines(&combined, 40)
                ));
                Some(false)
            } else {
                Some(true)
            }
        }
        None => None,
    };
    SmokeResult {
        ran: true,
        py_files: 0,
        collect: None,
        tests: None,
        entry_package: None,
        entry_ok,
        findings,
    }
}

/// Run `items` across the fleet with at most ONE call in flight PER DEVICE (work-stealing: each item
/// grabs the next free device-model and returns it on completion). This bounds the planning-phase
/// fan-outs (detailing, scouts, best-of-N, research) to the per-device capacity the EXECUTE scheduler
/// already honors, so a weight-1 node never has a second request queued behind the first. Results come
/// back in item order. `devices` is the list of distinct worker model-ids (one per node).
async fn fanout_over_fleet<T, R, F, Fut>(devices: Vec<String>, items: Vec<T>, f: F) -> Vec<R>
where
    T: Send + 'static,
    R: Send + 'static,
    F: Fn(T, String) -> Fut + Clone + Send + 'static,
    Fut: std::future::Future<Output = R> + Send + 'static,
{
    use std::collections::VecDeque;
    let devices = if devices.is_empty() {
        vec![String::new()]
    } else {
        devices
    };
    // permits == pool size, so a permit holder is always guaranteed a free device to pop.
    let permits = Arc::new(tokio::sync::Semaphore::new(devices.len()));
    let pool = Arc::new(Mutex::new(
        devices.into_iter().collect::<VecDeque<String>>(),
    ));
    let mut handles = Vec::with_capacity(items.len());
    for item in items {
        let permits = permits.clone();
        let pool = pool.clone();
        let f = f.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permits
                .acquire_owned()
                .await
                .expect("fleet semaphore never closed");
            let dev = {
                pool.lock()
                    .unwrap()
                    .pop_front()
                    .expect("a device is free whenever a permit is held")
            };
            let out = f(item, dev.clone()).await;
            pool.lock().unwrap().push_back(dev);
            out
        }));
    }
    let mut results = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(r) = h.await {
            results.push(r);
        }
    }
    results
}

/// Parse the semantic judge's one-line `VERDICT|CONFIDENCE|hint` reply. Conservative: anything not a
/// clearly-flagged problem reads as OK, so a vague weak-model reply can never kill a healthy worker.
/// CONFIDENCE gates agency — the judge acts (kill + correct) only on a verdict it marks HIGH.
fn parse_judge_reply(s: &str) -> JudgeOutcome {
    let upper = s.to_uppercase();
    // The correction is the LAST pipe-segment that is real free text — not a field LABEL, verdict word, or
    // confidence token. qwen-class models often echo the labels (e.g. `VERDICT|CONFIDENCE|BROKEN_CODE|HIGH|
    // <fix>`), so naive "segment after the verdict" grabs a label; taking the last non-token segment is
    // robust to both that and the terse `VERDICT|CONFIDENCE|hint` / `VERDICT|hint` forms.
    let is_token = |seg: &str| {
        matches!(
            seg.to_uppercase().trim(),
            "VERDICT"
                | "CONFIDENCE"
                | "CONF"
                | "HINT"
                | "OK"
                | "BROKEN_CODE"
                | "BROKEN CODE"
                | "LOOPING"
                | "OVER_READING"
                | "OVER READING"
                | "SPEC_DRIFT"
                | "SPEC DRIFT"
                | "HIGH"
                | "LOW"
        )
    };
    let hint = s
        .split('|')
        .map(|h| h.trim())
        .rfind(|h| !h.is_empty() && !is_token(h));
    // Confidence gates AGENCY: the judge acts (kill + re-dispatch with the correction) only when it marks
    // the verdict HIGH; an unsure/LOW verdict is logged (observed) but never kills. TUNABLE: drop the HIGH
    // mapping below intervene_confidence (0.8) to revert to advisory-only if it mis-fires live.
    let confidence = if upper.contains("HIGH") { 0.85 } else { 0.5 };
    let verdict = if upper.contains("BROKEN_CODE") || upper.contains("BROKEN CODE") {
        Verdict::BrokenCode
    } else if upper.contains("LOOPING") {
        Verdict::Looping
    } else if upper.contains("OVER_READING") || upper.contains("OVER READING") {
        Verdict::OverReading
    } else if upper.contains("SPEC_DRIFT") || upper.contains("SPEC DRIFT") {
        Verdict::SpecDrift
    } else {
        // No explicit verdict keyword. qwen-class models routinely express a real problem as just
        // `VERDICT|HIGH|<corrective hint>` with no keyword — dropping that would make the semantic judge
        // inert on this fleet. Treat it as an actionable problem ONLY when the model did NOT call it OK,
        // marked HIGH confidence, AND gave a substantive correction; anything else reads as healthy so a
        // vague reply still can't kill a good worker. (Recoverable if wrong: a re-dispatch with a hint,
        // capped per task — revert via the HIGH mapping above if false-positives show up live.)
        let said_ok = s.split('|').any(|p| p.trim().eq_ignore_ascii_case("ok"));
        let substantive = hint.map(|h| h.len() >= 16).unwrap_or(false);
        if !said_ok && upper.contains("HIGH") && substantive {
            Verdict::SpecDrift
        } else {
            return JudgeOutcome::ok();
        }
    };
    JudgeOutcome {
        verdict,
        confidence,
        hint: hint
            .map(|h| h.to_string())
            .unwrap_or_else(|| "Your output does not match the spec — correct it now.".to_string()),
        proposed_split: None,
    }
}

impl GooseAgentDispatcher {
    /// M3: ask the idle judge model to PARTITION an over-long task's files into 2–4 independent children.
    /// Returns the parsed children (the scheduler re-validates the partition before applying), or None if
    /// the reply can't be parsed into >= 2 children — the judge then falls back to its normal review.
    async fn propose_split(&self, req: &JudgeRequest) -> Option<Vec<ChildSpec>> {
        let owns = req.owned_files.join(", ");
        let system = "You split an over-long coding subtask into smaller INDEPENDENT pieces so several \
            workers can finish it in parallel. You are given the files the task owns. Partition those files \
            into 2 to 4 child subtasks. RULES: every file goes in EXACTLY ONE child; together the children \
            cover ALL the listed files; introduce NO new files. Prefer fully independent children (empty \
            depends_on); add a dependency only if one file genuinely cannot be written before another. Reply \
            with ONLY a JSON array and no prose: \
            [{\"id\":\"short-kebab-id\",\"files\":[\"path\"],\"depends_on\":[]}]."
            .to_string();
        let user = format!(
            "GOAL: {goal}\nThe subtask \"{desc}\" owns these files and is taking too long for one worker:\n  \
             {owns}\nPartition them now as a JSON array.",
            goal = req.goal,
            desc = req.description,
        );
        let text = tokio::time::timeout(
            std::time::Duration::from_secs(self.planner_timeout_secs.max(90)),
            self.run_agent(&req.judge_model_id, system, user, None, 2, &[], 0, None),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|o| o.text)?;
        // Extract the JSON array even if the model wrapped it in prose. `get` (not slice indexing) returns
        // None on an inverted/invalid range, so a reply with no array just falls through to the review.
        let start = text.find('[')?;
        let end = text.rfind(']')?;
        let json = text.get(start..=end)?;
        let children: Vec<ChildSpec> = serde_json::from_str(json).ok()?;
        (children.len() >= 2).then_some(children)
    }

    /// M6 step2: a deliberately HARSH self-rating of the chosen plan. Verbalized confidence is systematically
    /// overconfident, so the prompt pushes the model to subtract for anything unverified; the caller weights
    /// this BELOW the calibrated cross-draft agreement. Returns (0–100, semicolon-joined uncertainties) or
    /// None on call/parse failure.
    async fn verbalized_confidence(
        &self,
        model: &str,
        goal: &str,
        plan_json: &str,
    ) -> Option<(u8, String)> {
        let system = "You critique a software PLAN's completeness and correctness. You are KNOWN to be \
            OVERCONFIDENT — be brutally harsh and SUBTRACT for anything unverified, vague, or likely to break \
            at integration. Reply with EXACTLY one line `SCORE|uncertainties`: SCORE is 0-100 (confidence \
            that the plan, executed well, yields a COMPLETE and CORRECT program); uncertainties = the 1-3 \
            biggest risks, semicolon-separated (empty if none)."
            .to_string();
        let plan: String = plan_json.chars().take(2500).collect();
        let user = format!(
            "GOAL: {goal}\n\nPLAN (subtask skeleton JSON):\n{plan}\n\nYour one-line score:"
        );
        let text = tokio::time::timeout(
            std::time::Duration::from_secs(self.planner_timeout_secs.max(90)),
            self.run_agent(model, system, user, None, 2, &[], 0, None),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|o| o.text)?;
        parse_confidence(&text)
    }
}

/// M5: read all `.swarm/prereview/<task>.json` findings under `cwd` into a worker-prompt block for the
/// integrate-verify sink (CONFIRM + FIX). Returns "" when the dir is absent or no findings were recorded.
fn read_prereview_findings(cwd: &std::path::Path) -> String {
    let entries = match std::fs::read_dir(cwd.join(".swarm").join("prereview")) {
        Ok(e) => e,
        Err(_) => return String::new(),
    };
    let mut findings = String::new();
    for e in entries.flatten() {
        if let Some(v) = std::fs::read_to_string(e.path())
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        {
            if let (Some(t), Some(f)) = (
                v.get("task_id").and_then(|x| x.as_str()),
                v.get("findings").and_then(|x| x.as_str()),
            ) {
                findings.push_str(&format!("- {t}: {f}\n"));
            }
        }
    }
    if findings.is_empty() {
        String::new()
    } else {
        format!(
            "## Pre-review findings — an idle reviewer flagged likely defects in completed work; CONFIRM \
             each against the spec and FIX it before you finish:\n{findings}\n"
        )
    }
}

/// Parse the harsh self-rating reply `SCORE|uncertainties` (M6 step2). Tolerant: the score is the first
/// integer found on the first digit-bearing line (clamped 0–100); uncertainties is whatever follows `|`.
fn parse_confidence(reply: &str) -> Option<(u8, String)> {
    let line = reply
        .trim()
        .lines()
        .find(|l| l.chars().any(|c| c.is_ascii_digit()))?;
    let (score_part, unc) = match line.split_once('|') {
        Some((a, b)) => (a, b.trim().to_string()),
        None => (line, String::new()),
    };
    let first_num = score_part
        .split(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())?;
    let n: u32 = first_num.parse().ok()?;
    Some((n.min(100) as u8, unc))
}

#[async_trait]
impl Judge for GooseAgentDispatcher {
    async fn judge(&self, req: JudgeRequest) -> JudgeOutcome {
        // M3: split-enable is OFF in the default; GOOSE_SWARM_SPLIT=1 turns task-splitting on at runtime
        // so it can be proven live (M4) without a recompile, mirroring the judge/pre-review env gates.
        let cfg = JudgeConfig {
            split_enabled: std::env::var("GOOSE_SWARM_SPLIT")
                .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
                .unwrap_or(false),
            // GOOSE_SWARM_SPLIT_SECS overrides the too-big threshold (default 900s) so a live M4 proof can
            // trigger a split on a moderate task without waiting ~15 min for one to cross the default.
            split_threshold_secs: std::env::var("GOOSE_SWARM_SPLIT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| JudgeConfig::default().split_threshold_secs),
            ..JudgeConfig::default()
        };
        let cwd = std::env::current_dir().unwrap_or_else(|_| self.working_dir.clone());
        let mut file_contents: Vec<(String, String)> = Vec::new();
        let mut compile_errors: Vec<(String, String)> = Vec::new();
        let mut any_owned_written = false;
        let mut newest_mtime: Option<std::time::SystemTime> = None;
        for f in &req.owned_files {
            let path = cwd.join(f);
            if let Ok(meta) = path.metadata() {
                if meta.len() > 0 {
                    any_owned_written = true;
                }
                if let Ok(mt) = meta.modified() {
                    newest_mtime = Some(match newest_mtime {
                        Some(n) if n > mt => n,
                        _ => mt,
                    });
                }
            }
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if !contents.trim().is_empty() {
                    if let Some(err) = syntax_error(&path).await {
                        compile_errors.push((f.clone(), err));
                    }
                }
                file_contents.push((f.clone(), contents));
            }
        }
        let secs_since_last_write = newest_mtime
            .and_then(|mt| mt.elapsed().ok())
            .map(|d| d.as_secs());
        // The worker's live activity digest (.swarm/activity/<task_id>.json): action count, error count,
        // recent tool calls, and last reasoning. tool_calls feeds the deterministic over-read check; the
        // whole digest is the worker's "log" that the semantic review reads below.
        let digest = std::fs::read_to_string(
            cwd.join(".swarm")
                .join("activity")
                .join(format!("{}.json", req.task_id)),
        )
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        let worker_tool_calls = digest
            .as_ref()
            .and_then(|v| v.get("tool_calls").and_then(|n| n.as_u64()))
            .map(|n| n as u32);
        let input = JudgeInput {
            task_id: req.task_id.clone(),
            description: req.description.clone(),
            owned_files: req.owned_files.clone(),
            file_contents,
            compile_errors,
            elapsed_secs: req.elapsed_secs,
            any_owned_written,
            secs_since_last_write,
            worker_tool_calls,
            // Threaded from the scheduler's per-task split generation so the split cap holds (a child of a
            // split carries split_count >= 1 and is never re-split).
            split_count: req.split_count,
        };
        // Phase 1: cheap, unambiguous signals (won't-compile, no-output-while-old) act without a model.
        if let Some(out) = deterministic_verdict(&input, &cfg) {
            return out;
        }
        // No idle device was free for the model review (fleet saturated — weight-1 with every node busy).
        // The cheap deterministic checks above already ran without a model; skip the LLM review rather than
        // queue it behind a busy worker. This is what lets the judge still catch a stuck worker mid-fan-out.
        if req.judge_model_id.trim().is_empty() {
            return JudgeOutcome::ok();
        }
        // M3 (gated by split_enabled): a too-big PRODUCING task — ask this idle node to PARTITION its files
        // into independent children instead of letting it crawl. The scheduler RE-VALIDATES the partition
        // before applying, so a malformed proposal is harmless; on any parse failure we fall through to the
        // normal semantic review and the worker keeps running.
        if is_split_candidate(&input, &cfg) {
            if let Some(children) = self.propose_split(&req).await {
                return JudgeOutcome::split(children);
            }
        }
        // Phase 2: SEMANTIC review on the idle node. Reached only after the deterministic checks pass —
        // this is where the (weak) model adds JUDGEMENT the cheap signals can't: given the goal and what
        // the rest of the run has already done, is this worker on a healthy path, or is it broken /
        // looping on an error / drifting / re-doing finished work? It reads the worker's files-so-far,
        // its live activity log, AND the high-level run state, then passes or returns a correction.
        let acts = input.worker_tool_calls.unwrap_or(0);
        if input.file_contents.is_empty() && acts < 4 {
            return JudgeOutcome::ok(); // nothing meaningful to assess yet
        }
        let files_block = if input.file_contents.is_empty() {
            "(no file written yet)".to_string()
        } else {
            input
                .file_contents
                .iter()
                .map(|(p, c)| {
                    let body: String = c.chars().take(1800).collect();
                    format!("### {p}\n```\n{body}\n```")
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        // The worker's live "log": what it has been doing and whether its actions are erroring.
        let trace_block = digest
            .as_ref()
            .map(|d| {
                let errors = d.get("errors").and_then(|n| n.as_u64()).unwrap_or(0);
                let recent: Vec<String> = d
                    .get("recent")
                    .and_then(|r| r.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let last = d.get("last_text").and_then(|t| t.as_str()).unwrap_or("");
                format!(
                    "actions taken: {acts} ({errors} errored)\nrecent actions: {}\nworker's last reasoning: {}",
                    if recent.is_empty() {
                        "(none)".to_string()
                    } else {
                        recent.join(", ")
                    },
                    if last.is_empty() { "(none)" } else { last }
                )
            })
            .unwrap_or_else(|| format!("actions taken: {acts}"));
        // High-level state of the rest of the run — so the judge reviews this worker in context.
        let done_block = if req.done.is_empty() {
            "    (none yet)".to_string()
        } else {
            req.done
                .iter()
                .map(|(id, brief)| format!("    - {id}: {}", brief.replace('\n', " ")))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let remaining_str = if req.remaining.is_empty() {
            "(none)".to_string()
        } else {
            req.remaining.join(", ")
        };
        let failed_str = if req.failed.is_empty() {
            "(none)".to_string()
        } else {
            req.failed.join(", ")
        };
        let owns_str = if req.owned_files.is_empty() {
            "(works across the whole layout)".to_string()
        } else {
            req.owned_files.join(", ")
        };
        let system = "You are the SUPERVISOR of one worker on a shared multi-agent code build, running on \
            a spare node. You are given the overall GOAL, the high-level state of the whole run (what is \
            already done, still running, and failed), the worker's own SUBTASK spec, the file(s) it has \
            produced so far, and its live ACTIVITY LOG (recent actions, how many errored, its last \
            reasoning). Use ALL of it plus your own judgement to decide ONE thing: is this worker on a \
            healthy path to finish its subtask and move the goal forward, or has it gone wrong? Mid-write, \
            incomplete code is NORMAL — never flag merely-unfinished work. Flag ONLY a clear problem you \
            can SEE evidence for: code that cannot satisfy the spec, repeating the same failing \
            action/error, exploring without producing, re-doing a task already DONE, or depending on a \
            FAILED task. Give a concrete CORRECTION the worker can act on. BE CONSERVATIVE — a wrong kill \
            wastes real work, so when unsure say OK. Reply with EXACTLY one line `VERDICT|CONFIDENCE|hint`: \
            VERDICT one of OK, BROKEN_CODE, LOOPING, SPEC_DRIFT; CONFIDENCE one of HIGH or LOW (HIGH only \
            when you are sure and can point to the evidence); hint = a short, concrete correction (empty \
            for OK)."
            .to_string();
        // GOOSE_SWARM_GOALS (part 5): give the judge the app's PILLARS so its existing SPEC_DRIFT verdict is
        // grounded in the concrete acceptance criteria (a wrong command name/interface is now a nameable
        // drift, not a vague "quality" call). Conservative: still HIGH-confidence + visible-evidence only.
        let pillars_block = if goals_enabled() {
            self.pillars
                .get()
                .map(|p| {
                    format!(
                        "\n{p}(If this worker's code CLEARLY violates one of the pillars above — a wrong \
                         command name/argument order, or a different shared data shape — that is SPEC_DRIFT; \
                         still require HIGH confidence + visible evidence, never flag merely-unfinished work.)"
                    )
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        let user = format!(
            "GOAL: {goal}{pillars_block}\n\nRUN STATE:\n  done:\n{done}\n  still running: {rem}\n  failed: {fail}\n\n\
             THIS WORKER's subtask: {desc}\n  owns files: {owns}\n\nFiles produced so far:\n{files}\n\n\
             Worker activity log:\n{trace}\n\nYour one-line verdict:",
            goal = req.goal,
            done = done_block,
            rem = remaining_str,
            fail = failed_str,
            desc = req.description,
            owns = owns_str,
            files = files_block,
            trace = trace_block,
        );
        match tokio::time::timeout(
            std::time::Duration::from_secs(self.planner_timeout_secs.max(90)),
            self.run_agent(&req.judge_model_id, system, user, None, 2, &[], 0, None),
        )
        .await
        {
            Ok(Ok(o)) => {
                // Research log: record EVERY semantic review (including the OK ones) so the judge's
                // behaviour can actually be studied — when it ran and what it concluded. A semantic OK is
                // otherwise indistinguishable from a deterministic OK in the verdict event.
                let log = cwd.join(".swarm").join("semantic_reviews.log");
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log)
                {
                    use std::io::Write;
                    let reply: String = o.text.replace('\n', " ").chars().take(240).collect();
                    let _ = writeln!(f, "{}\t{}s\t{}", req.task_id, req.elapsed_secs, reply);
                }
                parse_judge_reply(&o.text)
            }
            _ => JudgeOutcome::ok(),
        }
    }
}

#[async_trait]
impl PreReviewer for GooseAgentDispatcher {
    async fn pre_review(&self, req: PreReviewRequest) -> PreReviewOutput {
        let none = PreReviewOutput {
            had_findings: false,
            summary: String::new(),
        };
        let cwd = std::env::current_dir().unwrap_or_else(|_| self.working_dir.clone());
        let mut files_block = String::new();
        for f in &req.owned_files {
            if let Ok(c) = std::fs::read_to_string(cwd.join(f)) {
                if c.trim().is_empty() {
                    continue;
                }
                let body: String = c.chars().take(2400).collect();
                files_block.push_str(&format!("### {f}\n```\n{body}\n```\n\n"));
            }
        }
        if files_block.is_empty() {
            return none; // nothing on disk to review
        }
        let system = "You CORRECTNESS-review one COMPLETED subtask of a larger build, BEFORE final \
            integration, on a spare node. A passing test suite does NOT prove correctness: the deepest \
            failure mode is code whose DEFAULT/primary path produces WRONG output (wrong constants/params) \
            or a spec deliverable that is built but never WIRED into the program's entry point. Read the \
            files against the GOAL and the subtask, and find ANY concrete defect of that kind. Reply with \
            EXACTLY one line `STATUS|findings`: STATUS is OK or ISSUES; findings = specific, actionable \
            corrections (what is wrong + which file), empty when OK. Be conservative — only ISSUES when you \
            can point to a real defect."
            .to_string();
        // GOOSE_SWARM_GOALS (part 5): let the correctness pre-review catch a concrete pillar violation
        // (wrong interface/command name, or a deliverable not wired to the pillar's entry) as an ISSUE.
        let pillars_block = if goals_enabled() {
            self.pillars
                .get()
                .map(|p| {
                    format!(
                        "\n{p}(Flag as an ISSUE any concrete violation of a pillar above — a wrong command \
                         name/argument order, or a shared data shape that disagrees with a pillar — naming the \
                         file. Stay conservative: only a defect you can point to.)"
                    )
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        let user = format!(
            "GOAL: {goal}{pillars_block}\n\nSUBTASK: {desc}\n\nFiles produced:\n{files}\n\nYour one-line review:",
            goal = req.goal,
            desc = req.description,
            files = files_block,
        );
        let text = tokio::time::timeout(
            std::time::Duration::from_secs(self.planner_timeout_secs.max(90)),
            self.run_agent(&req.reviewer_model_id, system, user, None, 2, &[], 0, None),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|o| o.text)
        .unwrap_or_default();
        let (status, findings) = text.trim().split_once('|').unwrap_or(("OK", ""));
        let findings = findings.trim();
        let had_findings = status.to_uppercase().contains("ISSUE") && !findings.is_empty();
        if had_findings {
            // Persist for integrate-verify to consume (M5 increment 2b wires the injection).
            let dir = cwd.join(".swarm").join("prereview");
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(
                dir.join(format!("{}.json", req.task_id)),
                serde_json::json!({"task_id": req.task_id, "findings": findings}).to_string(),
            );
        }
        PreReviewOutput {
            had_findings,
            summary: findings.chars().take(200).collect(),
        }
    }
}

/// Embedded model-free AST wiring reviewer (GOOSE_SWARM_REVIEW). Parses the produced Python tree and
/// flags BUILT-BUT-UNWIRED modules (a non-test logic module that no non-test module imports — a
/// duplicate-impl / dead-feature smell SMOKE cannot see because the app still RUNS via the duplicate).
/// Undefined-import drift is intentionally NOT checked here: SMOKE's collect-only catches real undefined
/// imports with zero false positives (it actually runs the import), whereas a STATIC drift check
/// false-positives on re-exports + star-imports (it flagged a re-exported RenderConfig in the clean
/// mdhtml WIN). Validated: 0 findings on well-wired apps (chaos-fern, the mdhtml WIN), true-positive on a
/// known-unwired tree, and it surfaced real dead/duplicate code in a "clean" example a manual review missed.
const AST_REVIEW_SCRIPT: &str = r##"
import ast, json, os, sys

root = sys.argv[1]
SKIP = {".git", "node_modules", "target", ".venv", ".swarm", "__pycache__"}

mods = {}
for dirpath, dirs, files in os.walk(root):
    dirs[:] = [d for d in dirs if d not in SKIP and not d.startswith(".")]
    for f in files:
        if f.endswith(".py"):
            full = os.path.join(dirpath, f)
            rel = os.path.relpath(full, root)
            mods[".".join(rel[:-3].split(os.sep))] = full


def base(mod):
    return mod.split(".")[-1]


def is_test(mod):
    b = base(mod)
    return b.startswith("test_") or b.endswith("_test") or b == "conftest"


def localmatch(name):
    if not name:
        return None
    if name in mods:
        return name
    for m in mods:
        if m.endswith("." + name) or base(m) == name:
            return m
    return None


imported_by_nontest = set()
for mod, path in mods.items():
    if is_test(mod):
        continue
    try:
        tree = ast.parse(open(path, encoding="utf-8").read())
    except Exception:
        continue
    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom):
            lm = localmatch(node.module)
            if lm:
                imported_by_nontest.add(lm)
            # `from PKG import MOD` (or `from . import MOD`) can import a local SUBMODULE, not just a
            # name inside PKG. Count PKG.MOD / MOD so a module wired ONLY this way (a very common entry
            # pattern, e.g. __main__ doing `from pkg import cli`) is not falsely flagged built-but-unwired.
            for alias in node.names:
                cand = (node.module + "." + alias.name) if node.module else alias.name
                sm = localmatch(cand) or localmatch(alias.name)
                if sm:
                    imported_by_nontest.add(sm)
        elif isinstance(node, ast.Import):
            for alias in node.names:
                lm = localmatch(alias.name)
                if lm:
                    imported_by_nontest.add(lm)

findings = []
for mod in mods:
    if base(mod) in ("__init__", "__main__") or is_test(mod):
        continue
    if mod not in imported_by_nontest:
        findings.append(
            "module '%s' is imported by no non-test module — built-but-unwired (dead or unreachable from the app)"
            % mod
        )


def _strip_doc(body):
    if (
        body
        and isinstance(body[0], ast.Expr)
        and isinstance(getattr(body[0], "value", None), ast.Constant)
        and isinstance(body[0].value.value, str)
    ):
        return body[1:]
    return body


def _is_abstract(fn):
    # @abstractmethod/@abstractproperty are intentionally unimplemented; @overload/@typing.overload
    # signatures REQUIRE a `...`/`pass` body (structural, never implemented). Never flag these.
    for d in fn.decorator_list:
        n = d.id if isinstance(d, ast.Name) else (d.attr if isinstance(d, ast.Attribute) else None)
        if n in ("abstractmethod", "abstractproperty", "overload"):
            return True
    return False


def _is_protocol_class(cls):
    # A typing.Protocol class's methods have structural `...`/`pass` bodies (interface declarations).
    for b in cls.bases:
        n = b.id if isinstance(b, ast.Name) else (b.attr if isinstance(b, ast.Attribute) else None)
        if n == "Protocol":
            return True
    return False


def _is_stub(fn):
    # Dunders (an empty __init__ is fine) and @abstractmethod are legitimately trivial — never flag them.
    if fn.name.startswith("__") and fn.name.endswith("__"):
        return False
    if _is_abstract(fn):
        return False
    body = _strip_doc(fn.body)
    if not body:
        return True
    if len(body) == 1:
        s = body[0]
        if isinstance(s, ast.Pass):
            return True
        if (
            isinstance(s, ast.Expr)
            and isinstance(getattr(s, "value", None), ast.Constant)
            and s.value.value is Ellipsis
        ):
            return True
        if isinstance(s, ast.Raise):
            exc = s.exc
            nm = None
            if isinstance(exc, ast.Name):
                nm = exc.id
            elif isinstance(exc, ast.Call) and isinstance(exc.func, ast.Name):
                nm = exc.func.id
            if nm == "NotImplementedError":
                return True
    return False


# STUB/FAKE/UNIMPLEMENTED detection: a non-test logic function whose whole body is pass / ... /
# raise NotImplementedError / just a docstring is unimplemented — a passing test suite can hide it (the
# test may never call it, or assert nothing). Flag it so the review can drive a real implementation.
for mod, path in mods.items():
    if is_test(mod):
        continue
    try:
        tree = ast.parse(open(path, encoding="utf-8").read())
    except Exception:
        continue
    # Methods directly defined on a typing.Protocol class are structural interface declarations whose
    # `...`/`pass` body is correct — collect them (by node identity within THIS parse) to skip.
    protocol_methods = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef) and _is_protocol_class(node):
            for item in node.body:
                if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    protocol_methods.add(id(item))
    for node in ast.walk(tree):
        if (
            isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
            and id(node) not in protocol_methods
            and _is_stub(node)
        ):
            findings.append(
                "function '%s' in module '%s' is a STUB/UNIMPLEMENTED (body is only pass / ... / raise NotImplementedError / a docstring) — implement it FULLY per the spec"
                % (node.name, mod)
            )

print(json.dumps({"modules": len(mods), "findings": sorted(set(findings))}))
"##;

/// Outcome of the AST review, serialized into the run jsonl `review` event.
#[derive(Debug, Clone, Serialize)]
struct AstReviewResult {
    ran: bool,
    modules: usize,
    findings: Vec<String>,
}

/// Parse the AST reviewer's JSON stdout. Pure — unit-tested. Any parse failure degrades to `ran=false`
/// (advisory: a reviewer hiccup never fails the run).
fn parse_ast_review(stdout: &str) -> AstReviewResult {
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        modules: usize,
        #[serde(default)]
        findings: Vec<String>,
    }
    match serde_json::from_str::<Raw>(stdout.trim()) {
        Ok(r) => AstReviewResult {
            ran: true,
            modules: r.modules,
            findings: r.findings,
        },
        Err(_) => AstReviewResult {
            ran: false,
            modules: 0,
            findings: vec![],
        },
    }
}

/// Run the model-free AST wiring review over the produced tree. No-op (`ran=false`) when there is
/// no Python or python3 is unavailable. Advisory — emits findings, never blocks the run.
async fn run_ast_review(root: &Path) -> AstReviewResult {
    if collect_py_files(root).is_empty() {
        return AstReviewResult {
            ran: false,
            modules: 0,
            findings: vec![],
        };
    }
    match tokio::process::Command::new("python3")
        .arg("-c")
        .arg(AST_REVIEW_SCRIPT)
        .arg(root)
        .output()
        .await
    {
        Ok(o) if o.status.success() => parse_ast_review(&String::from_utf8_lossy(&o.stdout)),
        _ => AstReviewResult {
            ran: false,
            modules: 0,
            findings: vec![],
        },
    }
}

/// Build the worker instruction for the GOOSE_SWARM_REVIEW corrective fix from the model-free findings —
/// BUILT-BUT-UNWIRED modules (wire them) and/or STUB/UNIMPLEMENTED functions (implement them fully). Pure —
/// unit-tested. The finding strings are self-describing, so one prompt covers both defect classes.
fn ast_fix_description(findings: &[String]) -> String {
    format!(
        "A model-free review found defects a passing test suite can hide. Findings:\n{}\n\nFix EACH:\n\
         - BUILT-BUT-UNWIRED module (no non-test code imports it): WIRE it into the app — make the entry \
         point / CLI IMPORT and USE it instead of duplicating its logic inline (load a store on startup and \
         save THROUGH it on every mutation; call a runner module to execute work; etc.).\n\
         - STUB/UNIMPLEMENTED function (body is only pass / ... / raise NotImplementedError / a docstring): \
         IMPLEMENT it FULLY per the spec — real working logic that returns the correct result, with NO pass \
         / ... / NotImplementedError / TODO left behind. A function the tests never exercise still must work.\n\
         Make the SMALLEST change that resolves the findings (do NOT rewrite working code), then RUN the \
         relevant command to confirm the feature works end to end (e.g. add an item, then list it in a fresh \
         process).",
        findings.join("\n")
    )
}

/// Target programming language for a swarm run, inferred from the spec (and, for amendments, the
/// existing files). Keeps the swarm from being Python-specific: the architect/worker scaffolding is
/// templated per language. Python is the no-cue default (the validated baseline + the weak fleet's
/// strongest training); any other language is honored when the spec or existing files call for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TargetLang {
    Python,
    TypeScript,
    Rust,
    Go,
    Other,
}

/// SPECULATIVE shadow: recursively copy the project tree `src` -> `dst`, SKIPPING heavy/irrelevant dirs so a
/// twin gets the source to read but the copy stays cheap. Best-effort — an unreadable entry is skipped, never
/// fatal. Used only on the speculative path (GOOSE_SWARM_SPECULATE); never touches the real tree.
fn copy_tree_excluding(src: &Path, dst: &Path) -> std::io::Result<()> {
    const SKIP: &[&str] = &[
        "node_modules",
        "target",
        "dist",
        ".git",
        ".swarm",
        ".venv",
        "__pycache__",
        "build",
    ];
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)?.flatten() {
        let name = entry.file_name();
        if SKIP.iter().any(|s| **s == *name.to_string_lossy()) {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => {
                let _ = copy_tree_excluding(&from, &to);
            }
            Ok(ft) if ft.is_file() => {
                let _ = std::fs::copy(&from, &to);
            }
            _ => {} // skip symlinks / other
        }
    }
    Ok(())
}

/// SPECULATIVE promote: copy ONLY `files` (the winning twin's owned, relative paths) from its shadow `from`
/// into the real tree `to`. SAFETY: rejects any absolute or parent-escaping (`..`) path so it can NEVER write
/// outside `to`; creates parent dirs; NEVER deletes; touches nothing but the listed owned files. Returns the
/// count promoted. This is the only place a twin's work reaches the real tree.
fn copy_owned_files(from: &Path, to: &Path, files: &[String]) -> usize {
    let mut promoted = 0;
    for f in files {
        let rel = Path::new(f);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue; // never escape `to`
        }
        let src = from.join(rel);
        let dst = to.join(rel);
        if !src.is_file() {
            continue;
        }
        if let Some(parent) = dst.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                continue;
            }
            // Defense-in-depth: the RESOLVED destination parent must stay inside `to` — defeats a symlinked
            // path component that the absolute/`..` textual checks miss (a pre-existing symlink at an owned
            // prefix could otherwise let std::fs::copy write THROUGH it outside the real tree).
            match (parent.canonicalize(), to.canonicalize()) {
                (Ok(cp), Ok(ct)) if cp.starts_with(&ct) => {}
                _ => continue, // cannot confirm containment -> skip
            }
        }
        if std::fs::copy(&src, &dst).is_ok() {
            promoted += 1;
        }
    }
    promoted
}

/// True if `s` mentions a `.<ext>` file at a word boundary (the char after the ext is not alphanumeric), so
/// ".js" matches "cli.js" but NOT "schema.json", and ".ts" matches "a.ts" but not "a.tsx". `s` is ASCII-lower.
fn mentions_ext(s: &str, ext: &str) -> bool {
    let needle = format!(".{ext}");
    let bytes = s.as_bytes();
    s.match_indices(&needle).any(|(i, _)| {
        let after = i + needle.len();
        after >= bytes.len() || !bytes[after].is_ascii_alphanumeric()
    })
}

/// Detect the target language. Existing files (an amendment) are the strongest signal; otherwise an EXPLICIT
/// language name in the spec wins, then weaker word-boundary file-extension cues; default Python otherwise.
fn detect_language(spec: &str, existing_files: &[String]) -> TargetLang {
    if !existing_files.is_empty() {
        let ext_of = |p: &str| {
            p.rsplit('.')
                .next()
                .filter(|e| *e != p)
                .unwrap_or("")
                .to_lowercase()
        };
        let n = |e: &str| existing_files.iter().filter(|p| ext_of(p) == e).count();
        let (py, ts, rs, go) = (n("py"), n("ts") + n("tsx") + n("js"), n("rs"), n("go"));
        let top = [py, ts, rs, go].into_iter().max().unwrap_or(0);
        if top > 0 {
            if ts == top {
                return TargetLang::TypeScript;
            }
            if rs == top {
                return TargetLang::Rust;
            }
            if go == top {
                return TargetLang::Go;
            }
            return TargetLang::Python;
        }
    }
    let s = spec.to_lowercase();
    // EXPLICIT language declarations win over incidental file-extension mentions: a Python app whose spec
    // says "validate SCHEMA.json" must NOT be read as TypeScript just because ".json" contains ".js" (the
    // exact APP8 failure — a LANG=Python JSON validator was built in TypeScript).
    if s.contains("python") || s.contains("pytest") {
        return TargetLang::Python;
    }
    if s.contains("typescript")
        || s.contains("javascript")
        || s.contains("node.js")
        || s.contains("nodejs")
    {
        return TargetLang::TypeScript;
    }
    if s.contains("rust") || s.contains("cargo") {
        return TargetLang::Rust;
    }
    if s.contains("golang") {
        return TargetLang::Go;
    }
    // Weaker file-extension / tool cues — matched at a word BOUNDARY so ".js" does not match ".json" and
    // ".ts" does not match ".tsx".
    if mentions_ext(&s, "ts")
        || mentions_ext(&s, "tsx")
        || mentions_ext(&s, "js")
        || s.contains("vitest")
        || s.contains(" jest")
        || s.contains("npm ")
    {
        return TargetLang::TypeScript;
    }
    if mentions_ext(&s, "rs") {
        return TargetLang::Rust;
    }
    if mentions_ext(&s, "go") || s.contains(" go ") {
        return TargetLang::Go;
    }
    if mentions_ext(&s, "py") {
        return TargetLang::Python;
    }
    // A named-but-unprofiled language: still honor it (generic non-Python guidance), never force Python.
    if s.contains("ruby")
        || s.contains("java")
        || s.contains("c#")
        || s.contains("c++")
        || s.contains("php")
        || s.contains("swift")
        || s.contains("kotlin")
        || s.contains("scala")
        || s.contains("elixir")
        || s.contains("haskell")
    {
        return TargetLang::Other;
    }
    TargetLang::Python
}

impl TargetLang {
    fn name(self) -> &'static str {
        match self {
            TargetLang::Python => "Python",
            TargetLang::TypeScript => "TypeScript",
            TargetLang::Rust => "Rust",
            TargetLang::Go => "Go",
            TargetLang::Other => "the target language",
        }
    }

    /// A forceful directive prepended to the architect prompt. EMPTY for Python so the validated Python
    /// prompt stays byte-identical; for every other language it names the target and tells the model the
    /// Python-looking examples below are illustrative — translate them.
    fn directive(self) -> String {
        if self == TargetLang::Python {
            return String::new();
        }
        format!(
            "TARGET LANGUAGE: {n}. Build the ENTIRE program in {n} using that language's idiomatic conventions \
             — its file extensions, module/import system, project layout, runnable entry point and standard \
             test runner. Any Python-looking names or commands in the guidance below (e.g. `cli.py`, \
             `python3 -m`, `pytest`, `.py`) are ILLUSTRATIVE ONLY — translate them to idiomatic {n}; do NOT \
             emit Python. ",
            n = self.name()
        )
    }

    /// The runnable-entry-point mandate, language-specific. The Python text is the original verbatim.
    fn entry_clause(self) -> &'static str {
        match self {
            TargetLang::Python => "If the request is a CLI / command-line tool (says 'CLI', 'command', 'command-line'), you MUST include a subtask that \
            writes the RUNNABLE ENTRY POINT — a `cli.py` (argparse or click) that wires the logic modules into actual commands \
            AND a `__main__.py` so `python3 -m <pkg> ...` runs it. The logic modules + tests ALONE are NOT a usable CLI; never \
            omit the entry point.",
            TargetLang::TypeScript => "If the request is a CLI / command-line tool, you MUST include a subtask that writes the RUNNABLE ENTRY POINT — a \
            `src/index.ts` (or `src/cli.ts`) using a real argument parser (commander/yargs, or `process.argv`) wired into actual \
            commands, PLUS a `package.json` with a `bin` and/or `scripts` entry so the CLI runs from the shell (e.g. \
            `npx tsx src/index.ts ...`). The logic modules + tests ALONE are NOT a usable CLI; never omit the entry point.",
            TargetLang::Rust => "If the request is a CLI / command-line tool, you MUST include a subtask that writes the RUNNABLE ENTRY POINT — a \
            `src/main.rs` with a real argument parser (clap, or std::env::args) wired into actual commands, PLUS the `Cargo.toml` \
            `[[bin]]`/deps so `cargo run -- ...` runs it. The library modules + tests ALONE are NOT a usable CLI; never omit it.",
            TargetLang::Go => "If the request is a CLI / command-line tool, you MUST include a subtask that writes the RUNNABLE ENTRY POINT — a \
            `main.go` (package main, with the `flag` package or os.Args) wired into actual commands so `go run . ...` runs it. The \
            packages + tests ALONE are NOT a usable CLI; never omit the entry point.",
            TargetLang::Other => "If the request is a CLI / command-line tool, you MUST include a subtask that writes the RUNNABLE ENTRY POINT in the \
            target language — the idiomatic executable that wires the logic modules into actual shell commands. The logic modules + \
            tests ALONE are NOT a usable program; never omit the entry point.",
        }
    }

    /// The command the integrate-verify subtask runs to execute the test suite.
    fn test_cmd(self) -> &'static str {
        match self {
            TargetLang::Python => "python3 -m pytest",
            TargetLang::TypeScript => {
                "the project's configured test runner (e.g. `npm test`, `npx vitest run`, or `npx jest`)"
            }
            TargetLang::Rust => "cargo test",
            TargetLang::Go => "go test ./...",
            TargetLang::Other => "the project's standard test runner for the target language",
        }
    }

    /// A concrete "run the built entry point" example for prompts (integrate-verify / worker). Python arm
    /// is the original `python3 -m <package> --help` verbatim.
    fn entry_run_example(self) -> &'static str {
        match self {
            TargetLang::Python => "python3 -m <package> --help",
            TargetLang::TypeScript => {
                "npm install && npm run build (or `npx tsc`), then run the BUILT entry — \
                 `node <the package.json bin/main target, e.g. dist/cli.js> --help`. Do NOT run the .ts \
                 source via tsx/ts-node: that bypasses the build and HIDES a missing tsconfig.json / no-dist \
                 failure, so the advertised `node dist/...`/bin would be broken for the user."
            }
            TargetLang::Rust => "cargo run -- --help",
            TargetLang::Go => "go run . --help",
            TargetLang::Other => "the program's runnable entry point with --help",
        }
    }

    /// Is `f` a source file in this language? Used to pick which sibling files' APIs get injected into a
    /// worker prompt. The Python arm is the original `.py` check verbatim (byte-identical behavior).
    fn is_source_file(self, f: &str) -> bool {
        match self {
            TargetLang::Python => f.ends_with(".py"),
            TargetLang::TypeScript => {
                f.ends_with(".ts") || f.ends_with(".tsx") || f.ends_with(".js")
            }
            TargetLang::Rust => f.ends_with(".rs"),
            TargetLang::Go => f.ends_with(".go"),
            TargetLang::Other => false,
        }
    }

    /// Is `base` (a file name) a TEST file? Excluded from dependency-API injection. Python arm verbatim.
    fn is_test_file(self, base: &str) -> bool {
        match self {
            TargetLang::Python => {
                base.starts_with("test_") || base.ends_with("_test.py") || base == "conftest.py"
            }
            TargetLang::TypeScript => {
                base.ends_with(".test.ts")
                    || base.ends_with(".spec.ts")
                    || base.ends_with(".test.js")
                    || base.ends_with(".spec.js")
            }
            TargetLang::Rust => base.ends_with("_test.rs"),
            TargetLang::Go => base.ends_with("_test.go"),
            TargetLang::Other => false,
        }
    }
}

/// GOOSE_SWARM_COMPLETE_PARALLEL: a group of verify findings that all name the SAME file, so exactly one
/// fix agent ever writes that file (same-file failures serialize by construction).
struct FileGroup {
    file: String,
    findings: Vec<String>,
}

/// Pull the fix-target source file out of a deterministic pytest/tooling finding. Findings are built from
/// `tail_lines` of real pytest/`-m` output (not model text), so the `path.py:N: in ...` and
/// `File "path.py", line N` shapes are stable. Prefers a NON-test source frame (the thing to fix); falls
/// back to the last file seen. Returns None when the finding names no code file (e.g. a missing entry point).
fn extract_file_from_finding(finding: &str) -> Option<String> {
    let is_code = |p: &str| {
        (p.ends_with(".py") || p.ends_with(".rs") || p.ends_with(".ts"))
            && !p.is_empty()
            && !p.contains(' ')
    };
    let mut last: Option<String> = None;
    let mut src: Option<String> = None;
    for raw in finding.lines() {
        let line = raw.trim();
        let cand: Option<&str> = if let Some((_, rest)) = line.split_once("File \"") {
            rest.split('"').next()
        } else {
            line.split(':').next().map(|t| t.trim())
        };
        if let Some(p) = cand.map(|p| p.trim()) {
            if is_code(p) {
                last = Some(p.to_string());
                if !p.contains("test") && !p.contains("conftest") {
                    src = Some(p.to_string());
                }
            }
        }
    }
    src.or(last)
}

/// Dedup + group findings by the file they name so each file becomes ONE fix agent (writes partitioned,
/// same-file findings serialized). Returns (groups in first-seen order, unassigned findings that name no
/// file) — the unassigned bucket gets a single serial fallback fix so a file-less finding is not dropped.
fn group_findings_by_file(findings: &[String]) -> (Vec<FileGroup>, Vec<String>) {
    let mut order: Vec<String> = Vec::new();
    let mut by_file: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut unassigned: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for f in findings {
        if !seen.insert(f.clone()) {
            continue;
        }
        match extract_file_from_finding(f) {
            Some(file) => {
                if !by_file.contains_key(&file) {
                    order.push(file.clone());
                }
                by_file.entry(file).or_default().push(f.clone());
            }
            None => unassigned.push(f.clone()),
        }
    }
    let groups = order
        .into_iter()
        .map(|file| {
            let findings = by_file.remove(&file).unwrap_or_default();
            FileGroup { file, findings }
        })
        .collect();
    (groups, unassigned)
}

/// Build the worker instruction for the GOOSE_SWARM_SMOKE corrective re-dispatch from the smoke findings.
/// Pure — unit-tested. Asks for the SMALLEST root-cause fix that makes collect-only + the `-m` entry pass.
fn smoke_fix_description(findings: &[String], lang: TargetLang) -> String {
    let verify = match lang {
        TargetLang::TypeScript => {
            "`npm run build` (no build errors) AND running the built entry point (e.g. \
             `node <entry-from-package.json> --help`) WITHOUT a runtime crash/uncaught exception"
        }
        TargetLang::Rust => "`cargo build` (no errors) AND `cargo run -- --help` WITHOUT a panic",
        _ => {
            "`python3 -m pytest --collect-only -q` (no collection/import errors) AND \
             `python3 -m <package> --help` (exit 0)"
        }
    };
    format!(
        "The integrated app FAILS a deterministic end-to-end smoke check the harness just ran. Findings:\n{}\n\n\
         FIX THE ROOT CAUSE directly — edit the offending file(s) in this project so that {verify} succeed. \
         Do NOT add features or rewrite working modules; make the SMALLEST change that resolves the findings, \
         then run those commands yourself to confirm before finishing.",
        findings.join("\n")
    )
}

/// Format the frozen module-interface contracts bundle for injection into a worker prompt. An empty (or
/// whitespace) bundle yields an empty string, so the GOOSE_SWARM_CONTRACTS injection is a true no-op
/// until the stub pass populates it. Pure — unit-tested without a model.
fn frozen_interfaces_block(bundle: &str) -> String {
    if bundle.trim().is_empty() {
        return String::new();
    }
    format!(
        "\n## FROZEN MODULE INTERFACES — the agreed contract (build against these EXACTLY)\n\
         These are the signature-only stubs every sibling module WILL expose. Import and call them with \
         these EXACT names + signatures, and keep shared data shapes identical; do NOT invent a different \
         signature or re-shape a shared value. A mismatch here is the #1 cause of passing-unit-tests but a \
         broken end-to-end integration.\n{bundle}\n"
    )
}

/// APP PILLARS (GOOSE_SWARM_GOALS): a small set of distilled, app-level acceptance criteria injected into
/// EVERY worker so the whole fleet builds toward the same north star even after context compaction.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Pillar {
    id: String,
    /// ONE imperative acceptance criterion the finished app MUST satisfy — the exact interface/command shape,
    /// a shared invariant (same store, same units), or the runnable entry — captured so workers cannot
    /// silently redesign it. This is precisely what drifts today (e.g. `report budget` built as `budget report`).
    goal: String,
    /// Optional runnable check hint, consumed only by the later review-against-pillars step. None for v1.
    #[serde(default)]
    check: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
struct Pillars {
    pillars: Vec<Pillar>,
}

/// GOOSE_SWARM_GOALS (default OFF): distill app-level pillars at plan time and inject them into every worker.
fn goals_enabled() -> bool {
    std::env::var("GOOSE_SWARM_GOALS")
        .map(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

/// Render the pillars as a worker-prompt block. Empty pillars -> empty string (a true no-op), so injection is
/// inert when the flag is off or nothing was distilled. Pure — unit-testable without a model.
fn render_pillars_block(p: &Pillars) -> String {
    if p.pillars.is_empty() {
        return String::new();
    }
    let body = p
        .pillars
        .iter()
        .map(|x| format!("- {}: {}", x.id, x.goal))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "\n## APP PILLARS — the app-wide acceptance criteria (NON-NEGOTIABLE; they outrank local convenience)\n\
         Your module MUST conform to these EXACTLY — the command/argument shape, the shared store, the units. \
         If your subtask touches a pillar, satisfy it VERBATIM; do NOT redesign the interface for convenience \
         (do NOT, for example, flip a `noun verb` command into `verb noun`). These are the whole app's contract:\n{body}\n"
    )
}

/// GOOSE_SWARM_CLI_CONTRACT (default ON): whether to inject the CLI-STRUCTURE contract into the entry worker.
fn cli_contract_enabled() -> bool {
    std::env::var("GOOSE_SWARM_CLI_CONTRACT")
        .map(|v| {
            !matches!(
                v.trim().to_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            )
        })
        .unwrap_or(true)
}

/// The entry file DEFINES the app's command-line interface, and it is the module the weak worker most often
/// drifts on the SHAPE of — verified twice: UNIQ9 built `checkin NAME DATE` (positional) instead of the spec's
/// `checkin NAME --date DATE`; UNIQ10 built flat `group-add` + per-command positional db + cents display instead
/// of the spec's nested `group add` + a GLOBAL `--db` before the subcommand + dollars. In both the ENGINE was
/// correct but the interface violated the spec, so spec-drift review failed the entry (and blocked its
/// dependents). This note freezes the interface CONTRACT for the entry worker: preserve the spec's exact command
/// tree, option placement, units and value syntax. Pure + unit-tested.
fn cli_contract_note(has_entry_file: bool, enabled: bool) -> String {
    if !enabled || !has_entry_file {
        return String::new();
    }
    "\nCLI STRUCTURE CONTRACT (your entry file IS the command-line interface — match the spec's SHAPE exactly; \
     spec-drift review verifies this and FAILS a working-but-wrong-shaped CLI):\n\
     - NESTED subcommands stay NESTED: if the spec writes `group add NAME` / `member add GROUP NAME`, implement \
       a `group` command WITH an `add` subcommand — NOT a flat hyphenated `group-add`.\n\
     - GLOBAL options stay GLOBAL: if the spec shows an option BEFORE the subcommand (e.g. `--db PATH init`), \
       parse it at the top level so it works before ANY subcommand — NOT as a per-command positional argument.\n\
     - Match each argument's POSITIONAL-vs-FLAG form EXACTLY as the spec writes it: a BARE word after the \
       subcommand (e.g. `product add SKU`, `warehouse add NAME`, `stock level SKU`) is a POSITIONAL argument — keep \
       it positional, do NOT convert it into a `--sku`/`--name` flag; conversely a `--flag VALUE` stays a flag, not \
       a positional. Converting the spec's positionals into flags (or vice-versa) is a spec-drift FAILURE even when \
       the logic is correct.\n\
     - Use the spec's EXACT option and command names — do NOT rename or 'improve' them: `--from`/`--to` must stay \
       `--from`/`--to` (not `--source`/`--dest`), `--reorder` must stay `--reorder` (not `--reorder-level`). Match \
       value UNITS (dollars with 2 decimals vs raw cents) and share/pair SYNTAX (`name=value`, not `name:value`). A \
       CLI that computes correctly but does not accept the spec's exact invocations is a spec-drift FAILURE — do not \
       silently re-shape the interface for convenience.\n\
     - Subcommand NAMES passed to add_parser() are STRINGS, not Python identifiers: use the spec's EXACT subcommand \
       name even when it is a Python reserved word — write `add_parser(\"import\")`, `add_parser(\"class\")`, \
       `add_parser(\"del\")`, NOT `\"import_\"`/`\"import2\"`/`\"import_cmd\"`. Trailing-underscore keyword-avoidance \
       is for Python VARIABLE/function names ONLY (`import_parser = subparsers.add_parser(\"import\")` is correct); \
       the CLI-facing subcommand string must stay verbatim so `prog import --file` works. Renaming the subcommand \
       `import` to `import_` makes the spec's `import` invocation fail = spec-drift.\n"
        .to_string()
}

/// Non-entry MULTI-FILE modules are the other over-read failure class (verified UNIQ13 plan-shopping, which owns
/// plan.py + shopping.py and needs 4 sibling modules: across 3 attempts it ran ls/tree/find/cat exploring the
/// layout + reading deps but NEVER wrote an owned file, so the no-write over-read timeout killed each attempt and
/// cascade-failed the run — 2nd instance after the UNIQ9 tests-writer). The entry gets skeleton_note; give non-entry
/// multi-file owners the same MECHANICAL fix: write a COMPILING STUB of each owned file FIRST (which flips
/// any_owned_written true and exempts the over-read timeout), then read deps + fill. Scoped to multi-file only —
/// single-file skeleton-first was a same-spec-A/B WASH. Empty when an owned file is the entry (skeleton_note covers
/// it). Gated on GOOSE_SWARM_SKELETON_FIRST (passed in as `enabled`). Pure + unit-tested.
fn multifile_stub_note(owned_files: &[String], enabled: bool) -> String {
    let is_entry = |f: &str| {
        f.ends_with("cli.py")
            || f.ends_with("__main__.py")
            || f.ends_with("main.rs")
            || f.ends_with("index.ts")
            || f.ends_with("cli.ts")
            || f.ends_with("main.go")
    };
    if !enabled || owned_files.len() <= 1 || owned_files.iter().any(|f| is_entry(f.as_str())) {
        return String::new();
    }
    "\nSTUB-FIRST (you own MULTIPLE non-entry files): do NOT run ls/tree/find or read every dependency before \
     producing — a weak worker that explores first burns its budget and is KILLED for over-reading before it \
     writes anything (a whole task lost). Your FIRST actions must be a `write` for EACH owned file emitting a \
     COMPILING STUB: the imports it needs plus every public function/class with its real signature and a `pass` \
     body. Once the files EXIST you are exempt from the over-read timeout — THEN read only the specific dependency \
     APIs you need (injected below under 'API of …') and fill each body with a focused `edit`. Never finish with a \
     `pass`/stub body still in place."
        .to_string()
}

#[async_trait]
impl TaskDispatcher for GooseAgentDispatcher {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
        // Detect the target language from this subtask's manifest (extensions are language-correct after the
        // architect plans them) + its description. Python (the no-cue / .py default) keeps every prompt arm
        // below byte-identical; other languages get the right scaffolding via the TargetLang profile.
        let lang = detect_language(&req.description, &req.all_files);
        // SPECULATIVE twin isolation: a twin runs rooted at a SHADOW copy so it never writes the real tree; a
        // normal task uses the real working dir (byte-identical). If a twin's shadow cannot be built, BAIL the
        // twin (Transient) rather than fall back to the real tree — two writers there would corrupt it.
        let real_root = std::env::current_dir().unwrap_or_else(|_| self.working_dir.clone());
        let root: PathBuf = if req.speculative {
            match self.make_shadow(&req.task_id, &req.owned_files, &real_root) {
                Ok(shadow) => shadow,
                Err(e) => {
                    return Err(DispatchError::Transient(format!(
                        "speculative shadow setup failed: {e}"
                    )))
                }
            }
        } else {
            real_root
        };
        let context_block = if req.context_slice.is_empty() {
            String::new()
        } else {
            format!(
                "## Relevant context from completed dependencies\n{}\n\n",
                req.context_slice
            )
        };
        // Hand the worker the AGREED layout: the full file manifest (so imports match where modules
        // actually live) and its OWN exact paths (so it never writes a divergent copy to the cwd root).
        // Gated on the manifest, not owned_files — integrate-verify owns nothing but most needs the map.
        let layout_block = if req.all_files.is_empty() {
            String::new()
        } else {
            let cwd = root.display().to_string();
            let manifest = req
                .all_files
                .iter()
                .map(|f| format!("  {f}"))
                .collect::<Vec<_>>()
                .join("\n");
            let owned_part = if req.owned_files.is_empty() {
                "You own no single file — you work ACROSS this whole layout. Confirm EVERY file listed \
                 above actually exists on disk and the tests cover each module. CRITICAL: a green pytest \
                 suite does NOT prove the program works — unit tests usually call functions directly and \
                 NEVER invoke the CLI/entry point, so a broken argparse, a bad import, or a crashing \
                 `main()` passes every test yet fails on every real invocation. You MUST actually RUN the \
                 program end-to-end: invoke its entry point (e.g. `python3 -m <package> --help`, then one \
                 real command from the spec with real arguments) and confirm it prints sane output and does \
                 NOT raise. If the entry point crashes, FIX the offending file — a program whose CLI cannot \
                 run is a FAILURE no matter how many unit tests pass. ENTRY WIRING (the #1 integration \
                 failure on a multi-module app): the CLI entry MUST import and REGISTER every command/\
                 subcommand the spec advertises — run `--help` and confirm EVERY advertised command is \
                 listed and actually invokable; a command group/parser that defines no commands, or omits \
                 some, means the modules exist but the program is UNUSABLE — wire them all. Report any \
                 missing file, unregistered command, or runtime crash.\n\n"
                    .to_string()
            } else {
                // Pre-create each owned file's parent directory so the worker NEVER needs mkdir — workers
                // have spammed `mkdir` 27x on a nested path and paralysed the task (0 writes). Deterministic
                // beats nudging.
                for f in &req.owned_files {
                    if let Some(parent) = std::path::Path::new(&cwd).join(f).parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                }
                let owned = req
                    .owned_files
                    .iter()
                    .map(|f| format!("  {cwd}/{f}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                // Multi-file tasks fail by writing the first owned file, forgetting the rest, then
                // claiming done — the completion guard retries but the worker repeats it. Call it out.
                let multi_note = if req.owned_files.len() > 1 {
                    format!(
                        "\nYOU OWN {n} FILES — you MUST write EVERY one. The classic multi-file failure is \
                         writing the first and forgetting the rest, then claiming done: this task is NOT \
                         complete until ALL {n} paths above exist and are non-empty. Write them one after \
                         another and verify each is on disk before you finish.",
                        n = req.owned_files.len()
                    )
                } else {
                    String::new()
                };
                // GOOSE_SWARM_SKELETON_FIRST (direction A — atomic writes): a multi-command ENTRY/wiring file
                // makes the worker front-load ~5k tokens planning the whole file then dump it in one write,
                // which trips the deterministic over_read kill (gated on !any_owned_written) and surfaces a
                // bad import only after a full rewrite. When ON, instruct a SKELETON-FIRST build for the entry
                // file: write the compiling structure (imports + every command registered with a placeholder
                // body) FIRST, confirm it imports, THEN fill each body. This OVERRIDES the one-write rule for
                // the entry file ONLY (resolved locally so non-entry workers see the unchanged rule). DEFAULT
                // ON (opt-out with GOOSE_SWARM_SKELETON_FIRST=0): a same-spec A/B (bookmark CLI) showed it is a
                // WASH on simple apps (identical quality + 0 over_read + total time within noise both ways) and
                // it helps on COMPLEX multi-command entries where the front-load actually fires (UNIQ3 ETL),
                // so on-by-default is not-worse and beneficial-where-it-matters. The kill-on-mid-fill hazard (a
                // skeleton with placeholder bodies accepted as done) is backstopped by integrate-verify + the
                // smoke gate, which run the entry end-to-end; the note also forbids finishing on a stub.
                let skeleton_first = std::env::var("GOOSE_SWARM_SKELETON_FIRST")
                    .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
                    .unwrap_or(true);
                let is_entry_file = |f: &str| {
                    f.ends_with("cli.py")
                        || f.ends_with("__main__.py")
                        || f.ends_with("main.rs")
                        || f.ends_with("index.ts")
                        || f.ends_with("cli.ts")
                        || f.ends_with("main.go")
                };
                let skeleton_note = if skeleton_first
                    && req.owned_files.iter().any(|f| is_entry_file(f))
                {
                    format!(
                            "\nSKELETON-FIRST (OVERRIDES the 'write the whole file in ONE write' rule below, \
                             for your ENTRY/wiring file ONLY): your entry file wires many commands, so do NOT \
                             plan the entire file then dump it in one write — that front-loads thinking, burns \
                             turns, and hides a bad import until the very end. Instead: (1) your FIRST `write` \
                             emits the COMPILING SKELETON — every import plus every command/subcommand the spec \
                             advertises REGISTERED, each with a placeholder body (`pass` / `todo!()` / \
                             `throw new Error('todo')`); (2) run `{check}` ONCE and confirm it imports and \
                             lists EVERY command; (3) THEN fill each handler body with a focused `edit`. You \
                             MUST finish with EVERY body fully implemented — a skeleton with placeholder bodies \
                             left in is NOT done and will fail verification. Write any NON-entry owned file \
                             complete in one write as usual.",
                            check = lang.entry_run_example()
                        )
                } else {
                    String::new()
                };
                let cli_note = cli_contract_note(
                    req.owned_files.iter().any(|f| is_entry_file(f)),
                    cli_contract_enabled(),
                );
                let multifile_note = multifile_stub_note(&req.owned_files, skeleton_first);
                format!(
                    "YOU OWN — write EXACTLY these ABSOLUTE paths, and write NOTHING outside them. Their \
                     parent directories ALREADY EXIST (pre-created for you) — NEVER run `mkdir` at all (it \
                     just wastes turns):\n{owned}{multi_note}{skeleton_note}{cli_note}{multifile_note}\n\
                     WRITE FIRST. Your spec above is the COMPLETE contract — your VERY FIRST action must be to \
                     `write` your owned file(s) IN FULL from it. Do NOT `ls`/`find`/`tree`/`cat` to 'understand \
                     the API', hunt for tests, or 'see the current state of the project': the PROJECT FILE \
                     LAYOUT above IS the complete structure (there is nothing on disk to discover), tests are a \
                     SEPARATE subtask, and the API of EVERY dependency you import is ALREADY injected below \
                     under 'API of …' — read it THERE, NEVER `cat` the module. Cat-ing files whose APIs are \
                     already injected only bloats your context until you LOOP — repeating 'let me write the \
                     file' over and over without ever emitting the write. Implement from the spec + injected \
                     APIs, THEN run `python3 -m pytest` to check. A turn that ends without every owned file \
                     written and non-empty FAILS and is retried — exploring/cat-ing instead of writing is the \
                     #1 way workers burn their whole budget and produce nothing.\n\n"
                )
            };
            // M5: inject idle-node PRE-REVIEW findings into the integrate-verify sink — whether or not it
            // happens to own files (a model-authored sink can own files) — so it CONFIRMS + FIXES the
            // flagged defects, not merely greens the suite. read_prereview_findings returns "" if none.
            let owned_part = if req.owned_files.is_empty() || req.task_id == "integrate-verify" {
                format!(
                    "{owned_part}{}",
                    read_prereview_findings(std::path::Path::new(&cwd))
                )
            } else {
                owned_part
            };
            // Inject the CURRENT content of any owned file that already exists (an AMENDMENT: you are
            // EDITING it) so the worker need not re-`cat` it. Integration/wire-into-existing-file tasks
            // otherwise over-read the whole project; handing them the file up front cuts the dominant tail.
            let mut existing_block = String::new();
            for f in &req.owned_files {
                let p = std::path::Path::new(&cwd).join(f);
                if let Ok(content) = std::fs::read_to_string(&p) {
                    if !content.trim().is_empty() {
                        let capped: String = content.chars().take(12000).collect();
                        let note = if content.chars().count() > 12000 {
                            " [truncated — head only; cat the rest only if needed]"
                        } else {
                            ""
                        };
                        existing_block.push_str(&format!(
                            "## CURRENT content of {f}{note} — this file ALREADY EXISTS (you were re-dispatched, \
                             or it is an amendment). Do NOT rewrite it from scratch — that re-does finished work \
                             and risks another timeout. FIRST run the program/tests to check whether it already \
                             satisfies the spec; if it does, report DONE immediately. Otherwise edit ONLY the real \
                             defect, from here. Do NOT `cat` it again:\n```\n{capped}\n```\n\n"
                        ));
                    }
                }
            }
            // Inject the CURRENT content of the task's DEPENDENCY source files (already-built modules it
            // imports from) so cli/integration tasks need not `cat` them — a cli-edit-delete task over-read
            // 16 deps and paralysed at 82 msgs / 0 writes. Only files that EXIST on disk (i.e. completed
            // deps); skip owned files (already injected above), test files, and non-`.py`. Capped per-file
            // and total to bound context on slow local models.
            let owned_set: std::collections::HashSet<&String> = req.owned_files.iter().collect();
            let mut dep_block = String::new();
            let mut dep_budget: usize = 14000;
            for f in &req.all_files {
                if dep_budget == 0 {
                    break;
                }
                if owned_set.contains(f) || !lang.is_source_file(f) {
                    continue;
                }
                let base = std::path::Path::new(f)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if lang.is_test_file(base) {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(std::path::Path::new(&cwd).join(f)) {
                    let trimmed = content.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let capped: String = trimmed.chars().take(dep_budget.min(3500)).collect();
                    dep_budget = dep_budget.saturating_sub(capped.chars().count());
                    dep_block.push_str(&format!(
                        "## API of {f} (a dependency you import — use it from here, do NOT `cat` it):\n```\n{capped}\n```\n\n"
                    ));
                }
            }
            format!(
                "## PROJECT FILE LAYOUT — the agreed plan\n\
                 Every module lives at EXACTLY these paths; import from here, NEVER invent another \
                 location or write a second copy at the project root:\n{manifest}\n{owned_part}{existing_block}{dep_block}"
            )
        };
        let contracts_on = std::env::var("GOOSE_SWARM_CONTRACTS")
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
            .unwrap_or(false);
        // GOOSE_SWARM_CONTRACTS: inject the frozen sibling-module interfaces so every parallel worker
        // builds against ONE agreed contract (kills cross-module drift). No-op until the stub pass (2b)
        // populates the bundle, so this is safe to ship ahead of the generator.
        let contracts_block = if contracts_on {
            self.contracts
                .get()
                .map(|b| frozen_interfaces_block(b))
                .unwrap_or_default()
        } else {
            String::new()
        };
        // GOOSE_SWARM_GOALS: the app-level PILLARS block (pre-rendered), injected into EVERY worker so the
        // whole fleet holds the same acceptance criteria through compaction. Empty until distilled -> no-op.
        let pillars_block = if goals_enabled() {
            self.pillars.get().cloned().unwrap_or_default()
        } else {
            String::new()
        };
        let worker_directive = lang.directive();
        let system_prompt = format!(
            "You are a WORKER on a local AI swarm. {worker_directive}Complete EXACTLY the task below using your tools, \
             in the current working directory. Write correct, minimal code; do nothing beyond the task. \
             When finished, briefly state what you produced.\n\
             SMALL MODULAR FILES (hard rule): write SMALL, single-responsibility files — ONE clear concern each. If you own \
             several files, write each one focused and short; NEVER cram everything into one big monolithic file (a 300+-line \
             do-everything file is wrong — split it by responsibility into the files you own). REUSE the modules whose API is \
             injected below: IMPORT and call them, do NOT re-implement their logic — re-coding an algorithm another module already \
             provides produces two copies that drift and one silently breaks.\n\
             \n\
             TOOLS & ENVIRONMENT — follow exactly, this avoids wasted calls:\n\
             - To READ a text file, use the shell tool: `cat <path>`. There is NO `read` tool, and \
             `read_image` is ONLY for images (png/jpeg/gif/webp) — never call it on source/text files.\n\
             - Keep tool OUTPUT SMALL: NEVER dump full `help()`/pydoc or whole large files into the chat — \
             use `head`, `grep`, or read only the specific lines/symbols you need. Large context is very \
             slow on local models and degrades quality.\n\
             - If a tool result says it was too large and was saved to a `goose_mcp_responses` temp file, \
             do NOT `cat` that temp file — reading it just re-truncates into ANOTHER temp file and you will \
             loop forever. Instead re-read the ORIGINAL file with `sed -n '1,120p'`/`grep`/`head` to get \
             only the part you need.\n\
             - Run Python with `python3`, never bare `python`.\n\
             - Testing a Click CLI: construct `CliRunner()` with NO arguments — the `mix_stderr` kwarg was \
             REMOVED in Click 8.2+, so `CliRunner(mix_stderr=False)` raises `TypeError` and breaks the whole \
             test file. stdout+stderr are already combined in `result.output`. (Your Click knowledge may be \
             out of date — when an import/TypeError says an argument was removed, drop it, do not fight it.)\n\
             - NEVER run `cd`. You are ALREADY in the working directory — run commands directly there \
             (e.g. `python3 -m pytest`, `cat src/foo.py`). Repeated `cd` into the same dir just burns turns.\n\
             - EVERY path you pass to write/edit MUST be ABSOLUTE (start with `/`); never a relative path.\n\
             - Write each file COMPLETE in ONE `write` and move on. Do NOT write a rough draft then refine \
             it with a chain of small `edit`s — plan the whole file first, then write it once. Every extra \
             round-trip costs ~30-60s on a local model and is the main reason tasks run slow.\n\
             - If a test or command fails unexpectedly, RE-READ the relevant file with `cat` BEFORE \
             forming any theory. Do NOT speculate about bytecode/.pyc/caching/compilation — check reality.\n\
             - Create ONLY the files your task owns; never leave scratch, notes, or plan files behind.\n\
             - If your task TESTS or BUILDS ON another module, `cat` that module's ACTUAL file for its \
             exact API, signatures, and behaviour (e.g. the precise SM-2 / formula constants) before \
             writing code — do NOT guess from the dependency summary; independent guesses DIVERGE and the \
             tests then disagree with the implementation.\n\
             - STAY INSIDE the current working directory. NEVER `cd`, `ls`, or `cat` files in PARENT or \
             SIBLING directories — they are unrelated projects. If the directory is empty, that is \
             expected for a new project: just create your files, do not go looking elsewhere.\n\
             - NEVER read the swarm's/harness's OWN artifacts that may sit in this directory or its parent: \
             `out.json`, any `*out.json`, `*progress*.log`, `plan.json`, `prompt.txt`, or the `.swarm/` \
             folder. They are run logs / the plan / the task prompt — NOT project files; cat-ing them tells \
             you nothing and wastes turns (workers have looped 10+ times on `plan.json`). Ignore them \
             completely and also do NOT create a `plan.json`.\n\
             - DON'T OVER-READ. You ALREADY have the file manifest and your dependencies' specs/outputs \
             above — that is enough to start. Read AT MOST the ONE file you will edit (for an amendment), \
             then ACT. Do NOT re-read the whole project to 'understand it first'; if you catch yourself \
             reading many files or thinking 'let me first read everything / understand the codebase', STOP \
             and write/edit now. NEVER read the project's TEST files (`test_*.py`/`*_test.py`) — they are \
             not your dependencies and tell you nothing you need; reading the test suite is wasted turns \
             (a worker just burned 13 reads on 6 test files and wrote nothing). Read ONE specific SOURCE \
             file only if you must call its exact API. Trust the manifest + dependency context; verify by \
             RUNNING (pytest), not by re-reading.\n\
             - STOP WHEN GREEN. The MOMENT your file's tests pass, call final_output and finish. Do NOT \
             re-run pytest more than ~2 times and do NOT keep tweaking an UNSPECIFIED detail (e.g. whether \
             multiple filters use AND vs OR): pick the sensible default, note it in one line, and STOP — a \
             worker once ran pytest 12 times agonizing over an unspecified detail while the suite was \
             already green. Perfect is the enemy of done; a green, finished task beats an endlessly-polished \
             one.\n\
             \n{pillars_block}{layout_block}{contracts_block}{context_block}"
        );
        // Live concurrency view: each task prints when it STARTS and FINISHES. Because dispatches
        // run concurrently, you see several "▸ run" lines before their "✓" — that IS the parallelism.
        let started = std::time::Instant::now();
        eprintln!(
            "  {} {} → {}",
            style("▸ run").cyan().bold(),
            style(&req.task_id).bold(),
            req.device_id
        );
        // Idle-based, not wall-clock: run_agent's watchdog aborts only if NO agent event arrives for
        // worker_timeout_secs (a genuinely stalled stream). A slow-but-PROGRESSING local model emits an
        // event every turn and runs to completion no matter the total time — wall-clock would wrongly
        // kill an honest 885s task. A stall surfaces as transient below → the scheduler re-routes it.
        // If the idle-model judge killed a prior attempt, lead with its corrective hint so this
        // re-dispatch heeds it (e.g. "you were over-reading/looping — WRITE now").
        let worker_user_text = match &req.prior_hint {
            Some(h) => format!(
                "SUPERVISOR NOTE — your previous attempt was stopped: {h}\n\nNow complete the task:\n{}",
                req.description
            ),
            None => req.description.clone(),
        };
        let outcome = self
            .run_agent_in(
                root.clone(),
                &req.model_id,
                system_prompt,
                worker_user_text,
                None,
                self.worker_max_turns,
                &self.worker_extensions,
                self.worker_timeout_secs,
                Some(&req.task_id),
            )
            .await;
        let secs = started.elapsed().as_secs_f64();
        match outcome {
            Ok(out) => {
                // Hallucinated-completion guard: a worker can call final_output ("done") WITHOUT ever
                // writing its owned file — seen repeatedly (a test-archive task; parser/shared-models
                // subtasks). Verify every owned file now exists and is non-empty; if not, retry. Use
                // ContentRetry (NOT Transient) so the next attempt is GUIDED — the worker is told as a
                // SUPERVISOR NOTE exactly which files it failed to write, instead of a blind re-roll that
                // tends to repeat the same omission until attempts exhaust.
                if !req.owned_files.is_empty() {
                    let cwd = root.clone();
                    let missing: Vec<String> = req
                        .owned_files
                        .iter()
                        .filter(|f| match cwd.join(f).metadata() {
                            // Missing -> always flag. Empty -> flag UNLESS it is a legitimately-empty marker
                            // (an empty `__init__.py` / `py.typed` is a correct, intentional file, not a
                            // flaky no-write); flagging those would wrongly tell the worker to "write it
                            // IN FULL".
                            Err(_) => true,
                            Ok(m) => {
                                m.len() == 0
                                    && !(f.ends_with("__init__.py") || f.ends_with("py.typed"))
                            }
                        })
                        .cloned()
                        .collect();
                    if !missing.is_empty() {
                        eprintln!(
                            "  {} {} on {} ({:.1}s) — claimed done but never wrote: {}",
                            style("✗").red().bold(),
                            style(&req.task_id).bold(),
                            req.device_id,
                            secs,
                            missing.join(", ")
                        );
                        return Err(DispatchError::ContentRetry(format!(
                            "You finished WITHOUT writing your owned file(s): {}. Your VERY FIRST action this \
                             attempt MUST be to `write` EACH of them IN FULL from your spec, then finish — do \
                             NOT explore, cat, or explain first.",
                            missing.join(", ")
                        )));
                    }
                }
                // GOOSE_SWARM_DONE_GATE: a worker is not "done" if an owned .py will not parse. Return a
                // ContentRetry carrying the exact error so the retry is GUIDED (the hint reaches the worker
                // as a SUPERVISOR NOTE) instead of a blind re-roll. Off by default; reuses py_syntax_error
                // (ast.parse, no __pycache__ pollution). The retry budget bounds it — a worker that cannot
                // fix the syntax in max_attempts fails the task rather than looping.
                let done_gate_on = std::env::var("GOOSE_SWARM_DONE_GATE")
                    .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
                    .unwrap_or(false);
                if done_gate_on {
                    let cwd = root.clone();
                    for f in &req.owned_files {
                        let path = cwd.join(f);
                        if path.is_file() {
                            if let Some(err) = syntax_error(&path).await {
                                eprintln!(
                                    "  {} {} on {}: syntax error in {f} — retry with the fix",
                                    style("✗").red().bold(),
                                    style(&req.task_id).bold(),
                                    req.device_id
                                );
                                return Err(DispatchError::ContentRetry(format!(
                                    "syntax error in {f}: {err} — FIX it before finishing"
                                )));
                            }
                        }
                    }
                }
                eprintln!(
                    "  {} {} on {} ({:.1}s)",
                    style("✓").green().bold(),
                    style(&req.task_id).bold(),
                    req.device_id,
                    secs
                );
                let output = if out.text.trim().is_empty() {
                    format!("(task {} completed)", req.task_id)
                } else {
                    out.text
                };
                Ok(TaskRunOutput {
                    output,
                    session_id: Some(out.session_id),
                    tool_calls: out.tool_calls,
                })
            }
            Err(e) => {
                let s = e.to_string();
                let transient = s.contains("stalled")
                    || s.contains("Model is unloaded")
                    || s.contains("Server error")
                    || s.contains("model_not_found")
                    || s.contains("Invalid model identifier")
                    || s.contains("is not loaded")
                    || s.contains("connection");
                eprintln!(
                    "  {} {} on {} ({:.1}s){}",
                    style(if transient { "↻" } else { "✗" }).red().bold(),
                    style(&req.task_id).bold(),
                    req.device_id,
                    secs,
                    if transient { " — will retry" } else { "" }
                );
                if transient {
                    // Best-effort re-warm before re-dispatch — only if model loading is allowed.
                    if self.allow_model_load
                        && (s.contains("Model is unloaded") || s.contains("connection"))
                    {
                        ensure_loaded(&req.model_id, 1);
                    }
                    Err(DispatchError::Transient(s))
                } else {
                    Err(DispatchError::Terminal(s))
                }
            }
        }
    }

    /// The twin WON: copy ONLY its owned files from its shadow into the real tree, then drop the shadow
    /// (TempDir cleanup). `copy_owned_files` is guarded so it can never write outside the real tree. A no-op
    /// if there is no shadow for `task_id` (e.g. the flag is off, or the twin already lost + was discarded).
    async fn promote_speculative(&self, task_id: &str) {
        let entry = self.spec_shadows.lock().unwrap().remove(task_id);
        if let Some((shadow, owned)) = entry {
            let real_root = std::env::current_dir().unwrap_or_else(|_| self.working_dir.clone());
            let n = copy_owned_files(shadow.path(), &real_root, &owned);
            eprintln!("speculative: promoted {n} owned file(s) from the winning twin of {task_id}");
            // `shadow` (TempDir) drops here -> the shadow workspace is removed from disk.
        }
    }
}

#[async_trait]
impl Replanner for GooseAgentDispatcher {
    async fn replan(&self, ctx: ReplanContext) -> Result<Vec<TaskSpec>> {
        let done = ctx
            .completed
            .iter()
            .map(|(id, out)| format!("- {id}: {}", out.chars().take(160).collect::<String>()))
            .collect::<Vec<_>>()
            .join("\n");
        let cap = ctx.idle_capacity.max(1);
        let system = format!(
            "You are the PLANNER continuing an in-progress local AI swarm. {cap} worker(s) just went IDLE while \
             other tasks finish and the goal is not fully done. Propose UP TO {cap} NEW INDEPENDENT subtasks that \
             add REAL value NOW — more tests, edge-case coverage, input validation, error handling, or hardening \
             on the COMPLETED work — they run in parallel on the idle workers. FUNCTIONALITY, TESTS, and \
             VALIDATION ONLY: do NOT propose README/docs, CI/workflow, packaging, LICENSE, or any \
             project-scaffolding subtask — they waste the run's tail for zero functional value. If nothing useful \
             remains, return an EMPTY subtasks list (do NOT invent make-work). Rules: every id MUST be new (never \
             reuse an existing id); depends_on may reference DONE ids but NEVER a failed id; files must not overlap \
             work still in progress. Give id/description/difficulty/model/depends_on/files, then call the \
             final_output tool."
        );
        let user = format!(
            "Goal: {}\n\nAlready created (do NOT reuse these ids): {}\n\nDone so far:\n{}\n\nFailed (do not depend on): {}\n\nStill running: {}",
            ctx.goal,
            ctx.existing_ids.join(", "),
            done,
            ctx.failed.join(", "),
            ctx.incomplete.join(", "),
        );
        let response = Some(Response {
            json_schema: Some(plan_schema()),
        });
        let out = match self
            .run_agent_timed(&self.planner_model, system, user, response, 10, &[])
            .await
        {
            Ok(o) => o,
            Err(_) => return Ok(Vec::new()),
        };
        let Some(fo) = out.final_output else {
            return Ok(Vec::new());
        };
        let mut specs = match goose_swarm::specs_from_plan_json(&fo) {
            Ok(s) => s,
            Err(_) => return Ok(Vec::new()),
        };
        let existing: std::collections::HashSet<&str> =
            ctx.existing_ids.iter().map(|s| s.as_str()).collect();
        specs.retain(|s| !existing.contains(s.id.as_str()));
        Ok(specs)
    }
}

/// Parse the ACTIVE parameter count (in billions) from a model id, for GOOSE_SWARM_ASK floor scaling. A MoE
/// id like `qwen3.6-35b-a3b` exposes ~3B ACTIVE (weaker than a 27B dense despite 35 total), so the `a<N>b`
/// active marker WINS over the leading dense `<N>b` size. Returns None if unparseable. HEURISTIC — fuzzy.
fn model_active_params_b(model_id: &str) -> Option<u32> {
    let id = model_id.to_lowercase();
    let tokens: Vec<&str> = id.split(|c: char| !c.is_ascii_alphanumeric()).collect();
    // 1) MoE active marker "a<N>b" takes precedence (the real compute size).
    for t in &tokens {
        if let Some(rest) = t.strip_prefix('a') {
            if let Some(num) = rest.strip_suffix('b') {
                if let Ok(n) = num.parse::<u32>() {
                    if (1..=2000).contains(&n) {
                        return Some(n);
                    }
                }
            }
        }
    }
    // 2) Mixtral-style "NxMb" dense-expert MoE: the per-expert size M is a rough ACTIVE proxy (only a couple
    // of experts fire per token), so read M, not the N×M total.
    for t in &tokens {
        if let Some((_, rest)) = t.split_once('x') {
            if let Some(num) = rest.strip_suffix('b') {
                if let Ok(n) = num.parse::<u32>() {
                    if (1..=2000).contains(&n) {
                        return Some(n);
                    }
                }
            }
        }
    }
    // 3) else the dense size "<N>b".
    for t in &tokens {
        if let Some(num) = t.strip_suffix('b') {
            if let Ok(n) = num.parse::<u32>() {
                if (1..=2000).contains(&n) {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// How much to RAISE the ask floor for a planner of `active_b` billion active params — weaker -> ask sooner.
/// HEURISTIC; small + bounded. None (unknown) gets a mild bump.
fn ask_floor_weak_bump(active_b: Option<u32>) -> u8 {
    match active_b {
        Some(n) if n >= 30 => 0, // strong dense (e.g. 30B+)
        Some(n) if n >= 13 => 5, // mid (e.g. 13-27B)
        Some(n) if n >= 7 => 10, // small dense (7-12B)
        Some(_) => 15,           // <7B active (e.g. an a3b MoE) -> ask much sooner
        None => 5,               // unknown id -> mild bump
    }
}

/// Schema for the GOOSE_SWARM_ASK clarifying-question generator: a flat list of interrogative strings.
fn clarify_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["questions"],
        "properties": {
            "questions": {
                "type": "array",
                "items": {"type": "string"}
            }
        }
    })
}

fn research_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["questions"],
        "properties": {
            "questions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "question", "kind"],
                    "properties": {
                        "id": {"type": "string"},
                        "question": {"type": "string"},
                        "kind": {"type": "string", "enum": ["library_docs", "web", "codebase"]}
                    }
                }
            }
        }
    })
}

fn plan_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["subtasks", "integration"],
        "properties": {
            "subtasks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "description", "difficulty", "model", "depends_on", "files"],
                    "properties": {
                        "id": {"type": "string"},
                        "description": {"type": "string"},
                        "difficulty": {"type": "string", "enum": ["easy", "hard"]},
                        "model": {"type": "string"},
                        "depends_on": {"type": "array", "items": {"type": "string"}},
                        "files": {"type": "array", "items": {"type": "string"}}
                    }
                }
            },
            "integration": {"type": "string"}
        }
    })
}

fn pillars_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["pillars"],
        "properties": {
            "pillars": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "goal"],
                    "properties": {
                        "id": {"type": "string"},
                        "goal": {"type": "string"},
                        "check": {"type": "string"}
                    }
                }
            }
        }
    })
}

/// A clear, solid-color phase header on stderr so a watcher understands what stage the run is in and
/// why the fleet is busy or the planner is reasoning alone.
fn phase_banner(label: &str, why: &str) {
    eprintln!(
        "\n{} {}  {}",
        style("▶").cyan().bold(),
        style(format!(" {label} ")).on_cyan().black().bold(),
        style(why).dim()
    );
}

/// Confidence-gated clarifying questions (GOOSE_SWARM_ASK_FLOOR). When the plan-confidence meter is below
/// the floor, the swarm asks the USER rather than guessing — local models are weak, so asking beats a
/// confident wrong decomposition. Interactive TTY -> cliclack prompts. Detached (no TTY: the autonomous
/// harness or an eval) -> write the questions to `.swarm/clarify-questions.json`, emit a `low_confidence_ask`
/// event, and BLOCK-poll for `.swarm/clarify-answers.json` (the harness answers AS the human) up to
/// `wait_secs`, then proceed. Returns a Q&A block to fold into the planner findings, or "" if unanswered.
async fn ask_clarifying_questions(
    questions: &[String],
    cwd: &Path,
    plan_conf: u8,
    wait_secs: u64,
    sink: &dyn EventSink,
) -> String {
    use std::io::IsTerminal;
    if questions.is_empty() {
        return String::new();
    }
    let dir = cwd.join(".swarm");
    let _ = std::fs::create_dir_all(&dir);
    let qpath = dir.join("clarify-questions.json");
    let apath = dir.join("clarify-answers.json");
    let _ = std::fs::remove_file(&apath); // never read a stale answer from a previous gate
    if let Err(e) = std::fs::write(
        &qpath,
        serde_json::to_string_pretty(&serde_json::json!({
            "plan_confidence": plan_conf,
            "questions": questions,
            "answer_file": ".swarm/clarify-answers.json",
            "how_to_answer": "Write a JSON array of answer strings (one per question, same order), or {\"answers\":[...]}, to answer_file. The swarm is BLOCKED on it and will re-plan with your answers.",
        }))
        .unwrap_or_default(),
    ) {
        eprintln!(
            "  warning: could not write clarify questions to {} ({e}) — the harness has nothing to answer",
            qpath.display()
        );
    }
    sink.write_value(serde_json::json!({
        "event": "low_confidence_ask",
        "plan_confidence": plan_conf,
        "questions": questions,
    }));

    let mut answers: Vec<String> = Vec::new();
    // INTERACTIVE only when BOTH stdin AND stdout are real terminals. A capture harness that pipes stdout
    // (or the autonomous loop / evals) is detached -> the file handshake, never a timeout-less cliclack
    // prompt that could hang forever on a PTY-backed child. GOOSE_SWARM_ASK_FILE=1 forces the file path.
    let force_file = std::env::var("GOOSE_SWARM_ASK_FILE")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
        .unwrap_or(false);
    let interactive =
        !force_file && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if interactive {
        eprintln!(
            "{}",
            style(format!(
                "Plan confidence {plan_conf}/100 — {} quick question(s) to get this right:",
                questions.len()
            ))
            .yellow()
            .bold()
        );
        for q in questions {
            let a: String = cliclack::input(q.as_str())
                .default_input("")
                .interact()
                .unwrap_or_default();
            answers.push(a);
        }
    } else {
        eprintln!(
            "{}",
            style(format!(
                "Plan confidence {plan_conf}/100 below floor — wrote {} question(s) to {}; BLOCKING up to {}s for answers in {} (the harness answers as the human) ...",
                questions.len(),
                qpath.display(),
                wait_secs,
                apath.display()
            ))
            .yellow()
        );
        let mut waited = 0u64;
        loop {
            if let Ok(s) = std::fs::read_to_string(&apath) {
                let parsed: Option<Vec<String>> =
                    serde_json::from_str::<Vec<String>>(&s).ok().or_else(|| {
                        serde_json::from_str::<serde_json::Value>(&s)
                            .ok()
                            .and_then(|val| {
                                val.get("answers").and_then(|a| a.as_array()).map(|arr| {
                                    arr.iter()
                                        .map(|x| x.as_str().unwrap_or("").to_string())
                                        .collect()
                                })
                            })
                    });
                if let Some(a) = parsed {
                    answers = a;
                    eprintln!(
                        "{}",
                        style("clarifications received — continuing with the answers").green()
                    );
                    break;
                }
            }
            if waited >= wait_secs {
                eprintln!(
                    "{}",
                    style("no answers within the wait window — proceeding with the current plan")
                        .yellow()
                );
                return String::new();
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            waited += 5;
        }
    }

    let mut block = String::from(
        "\n\nUSER CLARIFICATIONS (authoritative — they resolve ambiguity in the spec above; honor them):\n",
    );
    let mut any = false;
    for (i, q) in questions.iter().enumerate() {
        let a = answers.get(i).map(|s| s.trim()).unwrap_or("");
        if !a.is_empty() {
            block.push_str(&format!("Q: {q}\nA: {a}\n"));
            any = true;
        }
    }
    if any {
        sink.write_value(serde_json::json!({ "event": "low_confidence_answered" }));
        block
    } else {
        String::new()
    }
}

/// GOOSE_SWARM_COMPLETE_ROUNDS: the fix-round budget for the push-to-completion loop. Default 2; clamped
/// to [1,6] so a misconfigured value can never spin the fleet forever. Pure split-out for unit testing.
fn complete_rounds_from(v: Option<String>) -> u32 {
    v.and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(2)
        .clamp(1, 6)
}

fn complete_rounds() -> u32 {
    complete_rounds_from(std::env::var("GOOSE_SWARM_COMPLETE_ROUNDS").ok())
}

/// GOOSE_SWARM_COMPLETE_PARALLEL (default OFF): fan the push-to-completion FIX step across the fleet's
/// models — one fix agent per failing FILE, each writing only its own file (shadow isolation), instead of
/// one serial fix on a single node. Off => the serial v1 fix path runs verbatim.
fn complete_parallel() -> bool {
    std::env::var("GOOSE_SWARM_COMPLETE_PARALLEL")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
        .unwrap_or(false)
}

pub async fn run_swarm(opts: RunOpts) -> Result<()> {
    let mut cfg = load_config();
    // Auto-use what's loaded: the worker pool is derived from the models RESIDENT on the fleet
    // (`lms ps`), so the swarm runs on what's actually loaded — never spinning up the (possibly
    // stale) configured models over them. The configured pool is only a fallback for an empty fleet.
    let (fleet_pool, fleet_planner) = reconcile_pool_with_fleet(&cfg);
    let enabled: Vec<SwarmDevice> = if !fleet_pool.is_empty() {
        if let Some(p) = fleet_planner {
            cfg.planner_model = p;
        }
        eprintln!(
            "{}",
            style(format!(
                "auto-pool: {} resident model(s) from the fleet — using what's loaded, not spinning up anything",
                fleet_pool.len()
            ))
            .green()
            .bold()
        );
        fleet_pool
    } else if cfg.allow_model_load {
        eprintln!(
            "{}",
            style("fleet has no models loaded — bootstrapping the configured pool (allow_model_load=on)")
                .yellow()
        );
        cfg.devices.iter().filter(|d| d.enabled).cloned().collect()
    } else {
        return Err(anyhow!(
            "No models are loaded on the fleet (`lms ps` is empty or unavailable) and model loading is off.\n\
             Load your models in LM Studio, or enable loading via `goose swarm pool` (model-load)."
        ));
    };
    std::env::set_var("LMSTUDIO_HOST", &cfg.endpoint);
    if let Some(cap) = cfg.context_cap {
        std::env::set_var("GOOSE_LOCAL_CONTEXT_CAP", cap.to_string());
    }
    // Hard-cap any single tool result fed back to the weak model. Over-cap content spills to a temp file
    // ("response was larger… stored in /…/goose_mcp_responses/…"). CAUTION: set this ABOVE a normal source
    // file / pytest run — at 8000 a routine `cat store.py` (8.6KB) tripped the spill, the worker then catted
    // the temp file which ALSO re-tripped it, looping until the 900s timeout. 30000 (~7.5K tokens) clears
    // ordinary reads while still bounding a pathological dump. Respects an explicit env override.
    if std::env::var("GOOSE_MAX_TOOL_RESPONSE_SIZE").is_err() {
        let tcap = cfg.max_tool_response_chars.unwrap_or(30000);
        std::env::set_var("GOOSE_MAX_TOOL_RESPONSE_SIZE", tcap.to_string());
    }

    let json = opts.output_format == "json";
    let working_dir = std::env::current_dir()?;
    let worker_max_turns = opts.max_turns.unwrap_or(cfg.worker_max_turns);

    let run_id = format!("swarm-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S%3f"));
    // M5: a fresh run must NOT inherit stale .swarm/prereview findings from a previous run in this working
    // dir — they would be injected into THIS run's integrate-verify and describe code that no longer exists.
    let _ = std::fs::remove_dir_all(working_dir.join(".swarm").join("prereview"));
    let log_path: Option<PathBuf> = if opts.no_log {
        None
    } else {
        Some(opts.log_file.clone().unwrap_or_else(|| {
            working_dir
                .join(".swarm")
                .join(format!("run-{run_id}.jsonl"))
        }))
    };
    let sink: Arc<dyn EventSink> = match &log_path {
        Some(p) => match JsonlSink::new(p, run_id.clone()) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                eprintln!("(swarm log disabled: {e})");
                Arc::new(NullSink)
            }
        },
        None => Arc::new(NullSink),
    };

    // Progress goes to stderr so stdout carries only the report (clean in --output-format json).
    eprintln!(
        "{} working dir: {}",
        style("swarm").bold(),
        working_dir.display()
    );
    eprintln!(
        "pool: {}  planner {}",
        enabled
            .iter()
            .map(|d| format!("{}(w{})", d.id, d.weight))
            .collect::<Vec<_>>()
            .join(", "),
        style(&cfg.planner_model).green()
    );
    if let Some(p) = &log_path {
        eprintln!("log: {}", p.display());
    }

    sink.write_value(serde_json::json!({
        "event": "run_started",
        "prompt": opts.prompt,
        "planner_model": cfg.planner_model,
        "endpoint": cfg.endpoint,
        "working_dir": working_dir.display().to_string(),
        "max_turns": worker_max_turns,
        "max_attempts": cfg.max_attempts,
        "pool": enabled.iter().map(|d| serde_json::json!({
            "id": d.id, "model_id": d.model_id, "weight": d.weight, "instances": d.instances,
        })).collect::<Vec<_>>(),
    }));

    // Optionally pre-warm the planner + enabled worker models so remote JIT-load doesn't race.
    // Gated by allow_model_load — OFF by default, so the swarm never spins up models on its own.
    if cfg.allow_model_load {
        eprintln!("pre-warming models (idempotent) ...");
        ensure_loaded(&cfg.planner_model, 1);
        for d in &enabled {
            ensure_loaded(&d.model_id, d.instances);
        }
    } else {
        eprintln!(
            "{}",
            style("model loading off (allow_model_load=off) — using only resident models; enable via `goose swarm pool`")
                .yellow()
        );
    }

    // Map a device id to its configured speed_weight (substring match against the `speed_weights` map,
    // e.g. {"worksmacstudio":3,"mihai":2,"gabee":1}); default 1 = equal share.
    let speed_weight_for = |id: &str| -> u32 {
        cfg.speed_weights
            .iter()
            .find(|(pat, _)| id.contains(pat.as_str()))
            .map(|(_, w)| (*w).max(1))
            .unwrap_or(1)
    };
    let mut devices: Vec<DeviceCfg> = enabled
        .iter()
        .map(|d| DeviceCfg {
            id: d.id.clone(),
            model_id: d.model_id.clone(),
            weight: d.weight,
            enabled: true,
            speed_weight: speed_weight_for(&d.id),
        })
        .collect();
    // The planner model also pitches in as a worker after planning, so the smartest model isn't idle
    // (and hard subtasks can route to it). Skip if it's already a worker device.
    if cfg.planner_also_works && !devices.iter().any(|d| d.model_id == cfg.planner_model) {
        let w = cfg.planner_weight.max(1);
        devices.push(DeviceCfg {
            id: "planner".to_string(),
            model_id: cfg.planner_model.clone(),
            weight: w,
            enabled: true,
            speed_weight: speed_weight_for(&cfg.planner_model),
        });
        eprintln!(
            "planner also working: {} (weight {})",
            style(&cfg.planner_model).green(),
            w
        );
    }

    let mut ext_names = cfg.worker_extensions.clone();
    ext_names.extend(opts.mcp.iter().cloned());
    ext_names.sort();
    ext_names.dedup();
    let worker_extensions: Vec<ExtensionConfig> = ext_names
        .iter()
        .filter_map(|n| build_worker_extension(n))
        .collect();
    if !worker_extensions.is_empty() {
        eprintln!("worker MCP extensions: {}", ext_names.join(", "));
    }

    let dispatcher = Arc::new(
        GooseAgentDispatcher::new(
            working_dir.clone(),
            worker_max_turns,
            worker_extensions,
            cfg.planner_model.clone(),
            cfg.worker_timeout_secs,
            cfg.planner_timeout_secs,
            cfg.allow_model_load,
            SamplingParams {
                temperature: cfg.temperature,
                top_p: cfg.top_p,
                top_k: cfg.top_k,
                min_p: cfg.min_p,
                repeat_penalty: cfg.repeat_penalty,
            },
        )
        .await?,
    );

    // Parallel research-planning: scope independent research questions, run them across the fleet,
    // feed the findings into the planner. Best-effort — never blocks the run.
    let is_amendment = working_dir_has_sources(&working_dir);
    let do_research = match opts.research {
        Some(v) => v,
        None => match cfg.research_planning {
            ResearchPlanningMode::Off => false,
            ResearchPlanningMode::On => true,
            ResearchPlanningMode::Auto => is_amendment,
        },
    };
    let mut research_findings = String::new();
    // Per-phase wall-clock so every run SHOWS where time goes (research / planning / execute) — performance
    // must be MEASURED, not asserted: a phase that does not pay for its minutes is waste to find and cut.
    let t_start = std::time::Instant::now();
    if do_research {
        let research_exts: Arc<Vec<ExtensionConfig>> = Arc::new(
            ["context7", "web-search"]
                .into_iter()
                .filter_map(build_worker_extension)
                .collect(),
        );
        let worker_models: Vec<String> = devices.iter().map(|d| d.model_id.clone()).collect();
        let findings = if cfg.research_scouts {
            phase_banner(
                "SCOUT",
                "fixed-lens scouts investigate IN PARALLEL across the fleet — no serial scoping",
            );
            let lenses: Vec<&str> = select_lenses(is_amendment, cfg.max_research_questions)
                .iter()
                .map(|l| l.id)
                .collect();
            eprintln!(
                "  {} lens scout(s) → running across the fleet:",
                lenses.len()
            );
            sink.write_value(serde_json::json!({"event": "scouts_planned", "lenses": lenses}));
            dispatcher
                .run_scouts(
                    &opts.prompt,
                    is_amendment,
                    cfg.max_research_questions,
                    research_exts,
                    worker_models,
                    cfg.scout_budget_secs,
                )
                .await
        } else {
            phase_banner(
                "RESEARCH",
                "27B scopes questions ALONE, then the fleet researches them IN PARALLEL",
            );
            eprintln!("  scoping research questions on {} ...", cfg.planner_model);
            let questions = dispatcher
                .research_questions(
                    &cfg.planner_model,
                    &opts.prompt,
                    cfg.max_research_questions,
                    is_amendment,
                )
                .await
                .unwrap_or_default();
            sink.write_value(serde_json::json!({
                "event": "research_planned",
                "count": questions.len(),
                "questions": questions.iter().map(|q| serde_json::json!({"id": q.id, "kind": q.kind, "question": q.question})).collect::<Vec<_>>(),
            }));
            if questions.is_empty() {
                Vec::new()
            } else {
                eprintln!(
                    "  {} research question(s) → running across the fleet:",
                    questions.len()
                );
                dispatcher
                    .run_research(questions, research_exts, worker_models)
                    .await
            }
        };
        research_findings = findings
            .iter()
            .map(|f| format!("### [{}] {}\n{}", f.kind, f.question, f.findings))
            .collect::<Vec<_>>()
            .join("\n\n");
        sink.write_value(
            serde_json::json!({"event": "research_completed", "findings": findings.len()}),
        );
    }
    // Research is done (or was skipped — then this is ~t_start, research_min ~= 0).
    let t_research = std::time::Instant::now();

    // GOOSE_SWARM_ASK_FLOOR (1-100): when set, the swarm asks the USER clarifying questions if the plan-
    // confidence meter is below the floor, instead of committing to a low-confidence decomposition — local
    // models are weak, so asking beats guessing. Unset/0 = OFF = today's behavior exactly (eval/upstream
    // untouched). Setting it forces best_of_n>=2 so the calibrated cross-draft agreement signal is real
    // (it returns an inert neutral 60 for a single draft).
    let base_floor: Option<u8> = std::env::var("GOOSE_SWARM_ASK_FLOOR")
        .ok()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .filter(|f| *f > 0)
        .map(|f| f.min(100)); // documented 1-100; clamp so the weak-bump can never dip below the literal floor
                              // Inc3: with a floor set, RAISE the effective floor for a WEAKER planner (fewer ACTIVE params) so a weak
                              // local model asks the user SOONER — "ask more on weaker models". Default-ON when a floor is set;
                              // GOOSE_SWARM_ASK_SCALE=0 disables (then the user's literal floor is used). HEURISTIC (model-id -> active
                              // params is fuzzy); the bump is small + capped at 100.
    let ask_scale = base_floor.is_some()
        && std::env::var("GOOSE_SWARM_ASK_SCALE")
            .map(|v| {
                !matches!(
                    v.trim().to_lowercase().as_str(),
                    "0" | "off" | "false" | "no"
                )
            })
            .unwrap_or(true);
    let ask_floor: Option<u8> = base_floor.map(|f| {
        if ask_scale {
            let bump = ask_floor_weak_bump(model_active_params_b(&cfg.planner_model));
            let eff = ((u16::from(f)) + u16::from(bump)).min(100) as u8;
            if eff != f {
                eprintln!(
                    "  ask floor {f} -> {eff}/100 (+{bump} weak-planner bump for {})",
                    cfg.planner_model
                );
            }
            eff
        } else {
            f
        }
    });
    let ask_max_q: usize = std::env::var("GOOSE_SWARM_ASK_MAXQ")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(3)
        .max(1);
    let ask_wait_secs: u64 = std::env::var("GOOSE_SWARM_ASK_WAIT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(1800);
    // After the ASK handshake, re-plan from scratch (default) or reuse the first plan (answers still reach
    // workers via research_findings). Default-ON = today's behavior; the evidence-based default is set by A/B.
    let ask_replan = ask_replan_enabled(std::env::var("GOOSE_SWARM_ASK_REPLAN").ok());
    let best_of_n = {
        let base = opts.best_of_n.unwrap_or(cfg.best_of_n_skeletons);
        // Size the skeleton drafting to the FLEET so no worker node sits IDLE during the draft step (the user
        // flagged a 3rd node idling while only 2 of 3 drafted). Drafts run in parallel, so using all nodes
        // adds no wall-clock and yields a better best-of-N skeleton. Capped so a large fleet does not
        // over-draft. An explicit --best-of-n still wins (max with the fleet, never below the user's intent).
        let fleet = devices.len().clamp(1, 5);
        let sized = base.max(fleet);
        if ask_floor.is_some() {
            sized.max(2)
        } else {
            sized
        }
    };
    let cwd_for_ask = std::env::current_dir().unwrap_or_default();
    // The confidence meter only exists on the parallel (best-of-N) path; force it when a floor is set so
    // the gate is never silently inert (the solo planner returns no confidence).
    let use_parallel = cfg.parallel_planning || ask_floor.is_some();
    let mut asked = false;
    let (plan_json, dag) = loop {
        let (pj, plan_conf, uncertainties) = if use_parallel {
            phase_banner(
                "PLAN",
                "27B drafts the skeleton, then the fleet writes every subtask spec IN PARALLEL",
            );
            eprintln!("  architecting skeleton on {} ...", cfg.planner_model);
            let wm: Vec<String> = devices.iter().map(|d| d.model_id.clone()).collect();
            match dispatcher
                .parallel_plan(
                    &cfg.planner_model,
                    wm,
                    &opts.prompt,
                    plan_schema(),
                    devices.len(),
                    &research_findings,
                    best_of_n,
                    cfg.homogeneous_models,
                )
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("  parallel planning failed ({e}); falling back to the solo planner");
                    if ask_floor.is_some() {
                        eprintln!(
                            "  {} GOOSE_SWARM_ASK_FLOOR is inert on the solo fallback (no confidence signal)",
                            style("!").yellow()
                        );
                    }
                    dispatcher
                        .plan(
                            &cfg.planner_model,
                            &opts.prompt,
                            plan_schema(),
                            devices.len(),
                            &research_findings,
                        )
                        .await?
                }
            }
        } else {
            phase_banner(
                "PLAN",
                "27B builds the task DAG ALONE — workers idle while it reasons",
            );
            eprintln!(
                "  planning on {} (targeting {} workers) ...",
                cfg.planner_model,
                devices.len()
            );
            dispatcher
                .plan(
                    &cfg.planner_model,
                    &opts.prompt,
                    plan_schema(),
                    devices.len(),
                    &research_findings,
                )
                .await?
        };
        let dag = Dag::from_planner_json(&pj)
            .map_err(|e| anyhow!("invalid plan from planner: {e}\nplan was: {pj}"))?;
        eprintln!("  plan: {} subtask(s)", dag.tasks.len());
        // CONFIDENCE GATE: ask the user once when the meter is below the floor, then re-plan with the answers.
        if let (Some(floor), Some(conf)) = (ask_floor, plan_conf) {
            if conf < floor && !asked {
                asked = true;
                // Generate questions, cascading from best to fallback so a below-floor plan ALWAYS asks
                // (never proceeds on a default): (1) a dedicated LLM generator writes crisp interrogatives
                // from the spec + plan + uncertainties; (2) else split the model's raw uncertainties; (3)
                // else one generic high-value question.
                let mut questions = dispatcher
                    .clarify_questions(
                        &cfg.planner_model,
                        &opts.prompt,
                        &pj,
                        &uncertainties,
                        conf,
                        ask_max_q as u32,
                    )
                    .await;
                if questions.is_empty() {
                    questions = uncertainties
                        .split(';')
                        .map(|s| s.trim().to_string())
                        .filter(|s| s.len() >= 4)
                        .take(ask_max_q)
                        .collect();
                }
                if questions.is_empty() {
                    questions.push(format!(
                        "Plan confidence is only {conf}/100. What is the single most important constraint or acceptance criterion this MUST get right for the task: {}?",
                        opts.prompt
                    ));
                }
                // Always engage the handshake when below floor (the harness IS the human).
                let qa = ask_clarifying_questions(
                    &questions,
                    &cwd_for_ask,
                    conf,
                    ask_wait_secs,
                    sink.as_ref(),
                )
                .await;
                if !qa.is_empty() {
                    research_findings.push_str(&qa);
                    if ask_replan {
                        eprintln!(
                            "  {} re-planning with the user's clarifications",
                            style("↻").cyan()
                        );
                        continue;
                    }
                    eprintln!(
                        "  {} keeping this plan; clarifications injected into every worker via research findings (default: skips the ~15min re-plan — set GOOSE_SWARM_ASK_REPLAN=1 to re-plan instead)",
                        style("✓").green()
                    );
                }
            }
        }
        break (pj, dag);
    };

    // GOOSE_SWARM_CONTRACTS (2b): freeze signature-only module interfaces across the fleet before
    // EXECUTE, so every parallel worker builds against ONE agreed contract (kills cross-module drift).
    let contracts_on = std::env::var("GOOSE_SWARM_CONTRACTS")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
        .unwrap_or(false);
    // The contract stubs are PYTHON signature stubs — gate the whole phase to a Python target so a
    // non-Python (or mixed) tree never gets Python stubs injected into its worker prompts. The `.py`
    // module filter below already empties on a pure non-Python tree; this makes the skip explicit and
    // misfire-proof. Per-language (TS interface / Rust trait) stub generation is deferred. Python unchanged.
    if contracts_on && detect_language(&opts.prompt, &[]) == TargetLang::Python {
        let modules: Vec<TaskSpec> = dag
            .tasks
            .values()
            .map(|n| n.spec.clone())
            .filter(|s| {
                s.id != "integrate-verify" && s.owned_files.iter().any(|f| f.ends_with(".py"))
            })
            .collect();
        if !modules.is_empty() {
            phase_banner(
                "CONTRACTS",
                "freeze signature-only module interfaces across the fleet before EXECUTE",
            );
            let wm: Vec<String> = devices.iter().map(|d| d.model_id.clone()).collect();
            let cwd = std::env::current_dir().unwrap_or_default();
            let before: std::collections::HashSet<PathBuf> =
                collect_py_files(&cwd).into_iter().collect();
            let bundle = dispatcher
                .generate_contracts(modules, wm, &opts.prompt)
                .await;
            // The stub-gen workers must emit TEXT, but a weak model sometimes writes a `...`-body stub
            // file anyway. Remove any .py that appeared so EXECUTE starts from a clean tree — a leftover
            // stub would otherwise risk a lazy worker shipping it as "done".
            let stray: Vec<PathBuf> = collect_py_files(&cwd)
                .into_iter()
                .filter(|p| !before.contains(p))
                .collect();
            for p in &stray {
                let _ = std::fs::remove_file(p);
            }
            if !stray.is_empty() {
                eprintln!(
                    "  contracts: removed {} stray stub file(s) the stub-gen wrote (interfaces kept in-prompt)",
                    stray.len()
                );
            }
            if bundle.trim().is_empty() {
                eprintln!("  contracts: no stubs produced — skipping injection");
            } else {
                dispatcher.set_contracts(bundle);
                eprintln!("  contracts: frozen interfaces injected into every worker");
            }
        }
    }

    // GOOSE_SWARM_GOALS (part 1+3): distill the app's non-negotiable PILLARS from the spec + research + the
    // chosen plan and inject them into EVERY worker, so modules cohere to one north star through context
    // compaction. Post-plan (grounded in the real decomposition), before EXECUTE (reaches every worker).
    // Off -> never runs; the injection block is then an empty string ⇒ the worker prompt is byte-identical.
    if goals_enabled() {
        phase_banner(
            "PILLARS",
            "distill the app's non-negotiable acceptance criteria + inject them into every worker",
        );
        let pillars = dispatcher
            .distill_pillars(
                &cfg.planner_model,
                &opts.prompt,
                &research_findings,
                &plan_json,
            )
            .await;
        if pillars.pillars.is_empty() {
            eprintln!("  pillars: none distilled — skipping injection");
        } else {
            let dir = std::env::current_dir().unwrap_or_default().join(".swarm");
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(
                dir.join("pillars.json"),
                serde_json::to_string_pretty(&pillars).unwrap_or_default(),
            );
            sink.write_value(serde_json::json!({
                "event": "pillars",
                "count": pillars.pillars.len(),
                "pillars": pillars.pillars,
            }));
            eprintln!(
                "  pillars: {} distilled and injected into every worker",
                pillars.pillars.len()
            );
            dispatcher.set_pillars(render_pillars_block(&pillars));
        }
    }

    sink.write_value(serde_json::json!({
        "event": "plan_loaded",
        "task_count": dag.tasks.len(),
        "tasks": dag.tasks.values().map(|n| serde_json::json!({
            "id": n.spec.id,
            "deps": n.spec.deps,
            "files": n.spec.owned_files,
            "difficulty": format!("{:?}", n.spec.difficulty).to_lowercase(),
            "model": n.spec.preferred_model,
        })).collect::<Vec<_>>(),
        "raw_plan_json": plan_json,
    }));

    phase_banner(
        "EXECUTE",
        "subtasks run IN PARALLEL across the fleet; dynamic replan fills idle workers",
    );
    // Captured before `devices`/`dag`/`dispatcher` move into the scheduler — used only by the
    // GOOSE_SWARM_SMOKE corrective re-dispatch (one guided fix attempt if the smoke check fails).
    let smoke_fix_target = devices.first().map(|d| (d.id.clone(), d.model_id.clone()));
    let smoke_all_files: Vec<String> = dag
        .tasks
        .values()
        .flat_map(|n| n.spec.owned_files.clone())
        .collect();
    let smoke_fix_dispatcher = dispatcher.clone();
    // GOOSE_SWARM_COMPLETE_PARALLEL: the fleet's model ids, captured before the scheduler consumes
    // `devices`, so the completion fix step can fan one fix per failing file across all models.
    let fleet_models: Vec<String> = devices.iter().map(|d| d.model_id.clone()).collect();
    let mut scheduler = Scheduler::new(devices, cfg.max_attempts).with_sink(sink.clone());
    let replan_on = opts.dynamic_replan.unwrap_or(cfg.dynamic_replan);
    if replan_on && cfg.max_replans > 0 {
        eprintln!("dynamic replan: on (up to {} round(s))", cfg.max_replans);
        scheduler =
            scheduler.with_replanner(dispatcher.clone() as Arc<dyn Replanner>, cfg.max_replans);
    }
    // Idle-model judge: a node that would sit idle while tasks run inspects a busy worker and may kill +
    // re-dispatch a stuck one. On by default; GOOSE_SWARM_JUDGE=0 disables it.
    let judge_on = std::env::var("GOOSE_SWARM_JUDGE")
        .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "off" | "false" | "no"))
        .unwrap_or(true);
    if judge_on {
        eprintln!("idle-model judge: on (GOOSE_SWARM_JUDGE=0 to disable)");
        scheduler =
            scheduler.with_judge(dispatcher.clone() as Arc<dyn Judge>, JudgeConfig::default());
    }
    // M5: idle-node correctness PRE-REVIEW of completed tasks (findings feed integrate-verify). DEFAULT-ON
    // for the local fleet so a node never sleeps while completed work is unreviewed (it now runs CONCURRENTLY
    // with the judge, bounded by idle_capacity, instead of being starved by the single judge slot). Opt out
    // with GOOSE_SWARM_PREREVIEW=0.
    let prereview_on = std::env::var("GOOSE_SWARM_PREREVIEW")
        .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "off" | "false" | "no"))
        .unwrap_or(true);
    if prereview_on {
        eprintln!("idle-node pre-review: on (correctness-checks completed tasks)");
        scheduler = scheduler.with_pre_reviewer(dispatcher.clone() as Arc<dyn PreReviewer>);
    }
    // GOOSE_SWARM_SPECULATE (default-OFF, experimental): when a node would otherwise idle at a serial
    // chokepoint, race a TWIN of the longest-running in-flight task on the idle device (first-to-finish wins).
    // OFF until the Phase-2 dispatcher shadow-isolation is verified — with it off the scheduler is unchanged.
    let speculate_on = std::env::var("GOOSE_SWARM_SPECULATE")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
        .unwrap_or(false);
    if speculate_on {
        eprintln!("speculative execution: ON (idle nodes race the chokepoint — EXPERIMENTAL)");
        scheduler = scheduler.with_speculation();
    }
    // GOOSE_SWARM_REVIEW: snapshot the PRE-EXECUTE unwired findings so the post-run wire-fix only chases
    // modules THIS run left unwired — never a PRE-EXISTING intentional dead module (e.g. an amendment's
    // already-unwired duplicate, like byte-oracle's detector.py, which the wire-fix otherwise flails on).
    // Greenfield: the tree is empty here -> no findings -> review_before is empty -> no effect.
    let review_before: std::collections::HashSet<String> = if std::env::var("GOOSE_SWARM_REVIEW")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
        .unwrap_or(false)
    {
        run_ast_review(&std::env::current_dir().unwrap_or_default())
            .await
            .findings
            .into_iter()
            .collect()
    } else {
        std::collections::HashSet::new()
    };
    // Planning ends here (skeleton draft + verbalized confidence + any ASK/re-plan + detailing are all
    // behind us); the scheduler.run below IS the execute phase (workers + judge + integrate-verify).
    let t_plan = std::time::Instant::now();
    let report = scheduler
        .run(
            dag,
            dispatcher as Arc<dyn TaskDispatcher>,
            opts.prompt.clone(),
        )
        .await?;
    let t_exec = std::time::Instant::now();

    // GOOSE_SWARM_COMPLETE: push to REAL completion. VERIFY the built app by RUNNING it (reuse the smoke
    // oracle — pytest collect + `pytest -q` + the entry `--help`); if it is red, re-dispatch ONE bounded fix
    // against the distilled failure and RE-VERIFY — up to GOOSE_SWARM_COMPLETE_ROUNDS, capped by
    // GOOSE_SWARM_COMPLETE_CAP_SECS. Unlike GOOSE_SWARM_SMOKE (advisory, one-shot) the FINAL verdict is fed
    // into the run's exit code below, so a still-red app can no longer exit 0 and get delivered as "done".
    // Off by default => this block never runs and the exit path stays byte-identical.
    let complete_on = std::env::var("GOOSE_SWARM_COMPLETE")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
        .unwrap_or(false);
    let mut complete_failed = false;
    if complete_on {
        let rounds = complete_rounds();
        let cap_deadline = std::env::var("GOOSE_SWARM_COMPLETE_CAP_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&s| s > 0)
            .map(|s| std::time::Instant::now() + std::time::Duration::from_secs(s));
        let complete_lang = detect_language(&opts.prompt, &[]);
        let cwd = std::env::current_dir().unwrap_or_default();
        phase_banner(
            "COMPLETE",
            "verify the app by RUNNING it, fix-until-green (bounded), and refuse to ship a red app",
        );
        let mut final_passed = false;
        let mut last_findings: Vec<String> = Vec::new();
        // `rounds` fix attempts, each preceded by a verify, PLUS a final verify after the last fix so the
        // last fix is actually checked (0..=rounds => rounds+1 verifies, rounds fixes).
        for round in 0..=rounds {
            let verdict = run_smoke_gate(&cwd, complete_lang).await;
            sink.write_value(serde_json::json!({
                "event": "complete_verify",
                "round": round,
                "ran": verdict.ran,
                "passed": verdict.passed(),
                "findings": verdict.findings.len(),
            }));
            // The gate does not apply (no recognized build) OR a clean verify => done.
            if !verdict.ran || verdict.findings.is_empty() {
                final_passed = true;
                eprintln!(
                    "{}",
                    style(format!(
                        "complete: GREEN at round {round} — the built app runs and its checks pass"
                    ))
                    .green()
                );
                break;
            }
            last_findings = verdict.findings.clone();
            eprintln!(
                "{} round {round}: {} finding(s)",
                style("complete: RED").red().bold(),
                verdict.findings.len()
            );
            for f in &verdict.findings {
                eprintln!("  - {}", f.lines().next().unwrap_or(""));
            }
            // No fix budget left after the final verify, or the wall-clock cap has passed.
            if round == rounds {
                break;
            }
            if cap_deadline.is_some_and(|dl| std::time::Instant::now() >= dl) {
                eprintln!("complete: wall-clock cap reached — stopping the fix loop");
                break;
            }
            // FIX. Default: ONE serial fix on a single node (v1). GOOSE_SWARM_COMPLETE_PARALLEL: fan one fix
            // per failing FILE across the fleet's models — each agent writes only its own file (shadow
            // isolation), so two agents can never touch the same file; same-file findings collapse into one
            // group and serialize. Re-verify happens at the loop head next round.
            let Some((dev_id, model_id)) = smoke_fix_target.clone() else {
                break;
            };
            if complete_parallel() && !fleet_models.is_empty() {
                let (groups, unassigned) = group_findings_by_file(&verdict.findings);
                eprintln!(
                    "complete: fix round {round} — {} file-group(s) fanned across {} model(s), {} unassigned",
                    groups.len(),
                    fleet_models.len(),
                    unassigned.len()
                );
                if !groups.is_empty() {
                    let me = smoke_fix_dispatcher.clone();
                    let all_files = smoke_all_files.clone();
                    let dev = dev_id.clone();
                    let summaries =
                        fanout_over_fleet(fleet_models.clone(), groups, move |g, model| {
                            let me = me.clone();
                            let all_files = all_files.clone();
                            let dev = dev.clone();
                            async move {
                                let req = DispatchRequest {
                                    task_id: format!("complete-fix::{}", g.file),
                                    description: smoke_fix_description(&g.findings, complete_lang),
                                    device_id: dev,
                                    model_id: model,
                                    context_slice: String::new(),
                                    attempt: round,
                                    owned_files: vec![g.file.clone()],
                                    all_files,
                                    prior_hint: None,
                                    speculative: false,
                                };
                                match tokio::time::timeout(
                                    std::time::Duration::from_secs(1200),
                                    me.run(req),
                                )
                                .await
                                {
                                    Ok(Ok(o)) => format!(
                                        "{}: {}",
                                        g.file,
                                        o.output.lines().next().unwrap_or("fixed")
                                    ),
                                    _ => format!("{}: (fix timed out or errored)", g.file),
                                }
                            }
                        })
                        .await;
                    sink.write_value(serde_json::json!({
                        "event": "complete_fix_wave",
                        "round": round,
                        "shards": summaries.len(),
                        "unassigned": unassigned.len(),
                    }));
                }
                // A finding that names no file still gets one serial shot after the partitioned wave.
                if !unassigned.is_empty() {
                    let req = DispatchRequest {
                        task_id: "complete-fix-unassigned".to_string(),
                        description: smoke_fix_description(&unassigned, complete_lang),
                        device_id: dev_id,
                        model_id,
                        context_slice: String::new(),
                        attempt: round,
                        owned_files: vec![],
                        all_files: smoke_all_files.clone(),
                        prior_hint: None,
                        speculative: false,
                    };
                    let _ = smoke_fix_dispatcher.run(req).await;
                }
            } else {
                eprintln!("complete: fix round {round} against the distilled failure ...");
                let fix_req = DispatchRequest {
                    task_id: "complete-fix".to_string(),
                    description: smoke_fix_description(&verdict.findings, complete_lang),
                    device_id: dev_id,
                    model_id,
                    context_slice: String::new(),
                    attempt: round,
                    owned_files: vec![],
                    all_files: smoke_all_files.clone(),
                    prior_hint: None,
                    speculative: false,
                };
                let _ = smoke_fix_dispatcher.run(fix_req).await;
            }
        }
        complete_failed = !final_passed;
        sink.write_value(serde_json::json!({
            "event": "complete_result",
            "passed": final_passed,
            "remaining_findings": last_findings.len(),
        }));
        if !final_passed {
            eprintln!(
                "{}",
                style(format!(
                    "complete: STILL RED after {rounds} fix round(s) — the run will NOT report success ({} finding(s) remain)",
                    last_findings.len()
                ))
                .red()
                .bold()
            );
        }
    }

    // GOOSE_SWARM_SMOKE: deterministic end-to-end oracle on the produced tree (off by default —
    // GOOSE_SWARM_SMOKE=1). Emits a `smoke` event the eval reads; does not alter the run's exit code.
    // GOOSE_SWARM_COMPLETE (above) supersedes this standalone gate to avoid double-running the suite.
    let smoke_on = std::env::var("GOOSE_SWARM_SMOKE")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
        .unwrap_or(false);
    if smoke_on && !complete_on {
        let smoke_lang = detect_language(&opts.prompt, &[]);
        let smoke = run_smoke_gate(&std::env::current_dir().unwrap_or_default(), smoke_lang).await;
        let smoke_value = serde_json::to_value(&smoke).unwrap_or(serde_json::Value::Null);
        sink.write_value(serde_json::json!({
            "event": "smoke",
            "result": smoke_value,
        }));
        if !smoke.ran {
            eprintln!("smoke gate: skipped (no recognized build/entry in the produced tree)");
        } else if smoke.passed() {
            eprintln!(
                "{}",
                style("smoke gate: PASS (built + entry runs without crashing)").green()
            );
        } else {
            eprintln!(
                "{} ({} finding(s)):",
                style("smoke gate: FAIL").red().bold(),
                smoke.findings.len()
            );
            for f in &smoke.findings {
                eprintln!("  - {f}");
            }
            // Corrective re-dispatch: ONE guided fix attempt against the findings, then re-verify once.
            // Bounded to a single attempt (no loop) — the traceback IS the worker's instruction.
            if let Some((dev_id, model_id)) = smoke_fix_target.clone() {
                eprintln!("smoke gate: dispatching ONE corrective fix attempt ...");
                let fix_req = DispatchRequest {
                    task_id: "smoke-fix".to_string(),
                    description: smoke_fix_description(&smoke.findings, smoke_lang),
                    device_id: dev_id,
                    model_id,
                    context_slice: String::new(),
                    attempt: 0,
                    owned_files: vec![],
                    all_files: smoke_all_files.clone(),
                    prior_hint: None,
                    speculative: false,
                };
                let _ = smoke_fix_dispatcher.run(fix_req).await;
                let after =
                    run_smoke_gate(&std::env::current_dir().unwrap_or_default(), smoke_lang).await;
                let after_value = serde_json::to_value(&after).unwrap_or(serde_json::Value::Null);
                sink.write_value(serde_json::json!({
                    "event": "smoke_after_fix",
                    "result": after_value,
                }));
                if after.passed() {
                    eprintln!(
                        "{}",
                        style("smoke gate: corrective fix RESOLVED the findings").green()
                    );
                } else {
                    eprintln!(
                        "{} ({} finding(s) remain after one fix attempt)",
                        style("smoke gate: still failing").red().bold(),
                        after.findings.len()
                    );
                }
            }
        }
    }

    // GOOSE_SWARM_REVIEW: model-free AST wiring/drift review of the produced tree (off by default).
    // Advisory — emits a `review` event the eval reads; never blocks or fails the run.
    let review_on = std::env::var("GOOSE_SWARM_REVIEW")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "on" | "true" | "yes"))
        .unwrap_or(false);
    if review_on {
        let review = run_ast_review(&std::env::current_dir().unwrap_or_default()).await;
        let review_value = serde_json::to_value(&review).unwrap_or(serde_json::Value::Null);
        // Only act on findings THIS run introduced — exclude any that already held before EXECUTE (a
        // pre-existing intentional dead module). The `review` event still carries ALL findings for the eval.
        let new_findings: Vec<String> = review
            .findings
            .iter()
            .filter(|f| !review_before.contains(*f))
            .cloned()
            .collect();
        let pre_existing = review.findings.len().saturating_sub(new_findings.len());
        sink.write_value(serde_json::json!({
            "event": "review",
            "result": review_value,
            "new_findings": new_findings,
            "pre_existing_skipped": pre_existing,
        }));
        if !review.ran {
            eprintln!("AST review: skipped (no python in the produced tree)");
        } else if new_findings.is_empty() {
            let extra = if pre_existing > 0 {
                format!(" ({pre_existing} pre-existing skipped)")
            } else {
                String::new()
            };
            eprintln!(
                "{}",
                style(format!("AST review: clean (no NEW unwired modules){extra}")).green()
            );
        } else {
            let extra = if pre_existing > 0 {
                format!(", {pre_existing} pre-existing skipped")
            } else {
                String::new()
            };
            eprintln!(
                "{} ({} new finding(s){extra} — model-free, advisory):",
                style("AST review").yellow().bold(),
                new_findings.len()
            );
            for f in &new_findings {
                eprintln!("  - {f}");
            }
            // Corrective re-dispatch (mirrors the SMOKE autofix): ONE guided wire-fix that imports + uses
            // the NEWLY-unwired module(s), then re-reviews once. Bounded to a single attempt.
            if let Some((dev_id, model_id)) = smoke_fix_target.clone() {
                eprintln!("AST review: dispatching ONE corrective wire-fix attempt ...");
                let fix_req = DispatchRequest {
                    task_id: "wire-fix".to_string(),
                    description: ast_fix_description(&new_findings),
                    device_id: dev_id,
                    model_id,
                    context_slice: String::new(),
                    attempt: 0,
                    owned_files: vec![],
                    all_files: smoke_all_files.clone(),
                    prior_hint: None,
                    speculative: false,
                };
                let _ = smoke_fix_dispatcher.run(fix_req).await;
                let after = run_ast_review(&std::env::current_dir().unwrap_or_default()).await;
                let after_new: Vec<String> = after
                    .findings
                    .iter()
                    .filter(|f| !review_before.contains(*f))
                    .cloned()
                    .collect();
                let after_value = serde_json::to_value(&after).unwrap_or(serde_json::Value::Null);
                sink.write_value(serde_json::json!({
                    "event": "review_after_fix",
                    "result": after_value,
                    "new_findings": after_new,
                }));
                if after.ran && after_new.is_empty() {
                    eprintln!(
                        "{}",
                        style("AST review: wire-fix RESOLVED the unwired findings").green()
                    );
                } else {
                    eprintln!(
                        "{} ({} new finding(s) remain after one wire-fix)",
                        style("AST review: still unwired").yellow().bold(),
                        after_new.len()
                    );
                }
            }
        }
    }

    let report_value = serde_json::to_value(&report).unwrap_or(serde_json::Value::Null);
    // Phase wall-clock (minutes). research = scouts; planning = skeleton draft + verbalized + ASK/re-plan +
    // detailing (INCLUDES any human ASK-answer wait); execute = workers + judge + integrate-verify + review.
    let research_m = t_research.duration_since(t_start).as_secs_f64() / 60.0;
    let planning_m = t_plan.duration_since(t_research).as_secs_f64() / 60.0;
    let execute_m = t_exec.duration_since(t_plan).as_secs_f64() / 60.0;
    let total_m = t_exec.duration_since(t_start).as_secs_f64() / 60.0;
    let pct = |x: f64| {
        if total_m > 0.0 {
            (100.0 * x / total_m).round() as u32
        } else {
            0
        }
    };
    sink.write_value(serde_json::json!({
        "event": "run_finished",
        "report": report_value,
        "phases": {
            "research_min": (research_m * 10.0).round() / 10.0,
            "planning_min": (planning_m * 10.0).round() / 10.0,
            "execute_min": (execute_m * 10.0).round() / 10.0,
            "total_min": (total_m * 10.0).round() / 10.0,
        },
    }));

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        println!("\n{}", style("=== swarm report ===").bold());
        println!(
            "phases: research {:.1}m ({}%) | planning {:.1}m ({}%) | execute {:.1}m ({}%) | total {:.1}m",
            research_m,
            pct(research_m),
            planning_m,
            pct(planning_m),
            execute_m,
            pct(execute_m),
            total_m
        );
        println!("done   ({}): {}", report.done.len(), report.done.join(", "));
        let core_failed: Vec<&String> = report
            .failed
            .iter()
            .filter(|id| !report.bonus.contains(*id))
            .collect();
        let bonus_failed: Vec<&String> = report
            .failed
            .iter()
            .filter(|id| report.bonus.contains(*id))
            .collect();
        if !core_failed.is_empty() {
            println!(
                "{} ({}): {}",
                style("FAILED").red().bold(),
                core_failed.len(),
                core_failed
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !bonus_failed.is_empty() {
            println!(
                "{} ({}): {}  (opportunistic — did NOT fail the run)",
                style("bonus skipped").yellow(),
                bonus_failed.len(),
                bonus_failed
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!("dispatched per device: {:?}", report.dispatched_per_device);
        // Observed node speed (avg ms/task) — hard tasks are routed to the fastest free node.
        let mut speeds: Vec<(String, u64)> = report
            .per_device
            .iter()
            .filter(|(_, s)| s.dispatched > 0)
            .map(|(d, s)| (d.clone(), s.busy_ms / s.dispatched as u64))
            .collect();
        speeds.sort_by_key(|(_, ms)| *ms);
        if !speeds.is_empty() {
            let line = speeds
                .iter()
                .enumerate()
                .map(|(i, (d, ms))| {
                    if i == 0 {
                        format!("{d} {ms}ms (fastest)")
                    } else {
                        format!("{d} {ms}ms")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            println!("node speed (avg ms/task): {line}");
        }
        for id in &report.done {
            if let Some(r) = report.results.get(id) {
                let snippet: String = r.chars().take(280).collect();
                println!("\n--- {id} ---\n{snippet}");
            }
        }
    }

    // Run success is judged on the CORE plan only — a failed opportunistic/replanner (bonus) task
    // must not fail an otherwise-complete run.
    let core_failed = report
        .failed
        .iter()
        .filter(|id| !report.bonus.contains(*id))
        .count();
    // GOOSE_SWARM_COMPLETE: a still-red app (verify-by-running never went green within the fix budget) must
    // NOT report success, even if every planned subtask "completed" — this is the never-ship-broken gate.
    // When the flag is off, `complete_on && complete_failed` is false and the exit path is byte-identical.
    if complete_on && complete_failed {
        Err(anyhow!(
            "push-to-completion: the built app still fails its verify checks after the fix loop{}",
            if core_failed > 0 {
                format!(" ({core_failed} core subtask(s) also failed)")
            } else {
                String::new()
            }
        ))
    } else if core_failed == 0 {
        Ok(())
    } else {
        Err(anyhow!("{} core subtask(s) failed", core_failed))
    }
}
