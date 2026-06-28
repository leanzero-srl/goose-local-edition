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
    deterministic_verdict, Dag, DeviceCfg, DispatchError, DispatchRequest, EventSink, Judge,
    JudgeConfig, JudgeInput, JudgeOutcome, JudgeRequest, NullSink, ReplanContext, Replanner,
    Scheduler, SwarmEvent, TaskDispatcher, TaskRunOutput, TaskSpec, ToolCallRecord, Verdict,
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(procs[0].parallel, Some(4), "reads the PARALLEL column as the device weight source");
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
        })
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
    async fn run_agent(
        &self,
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
                self.working_dir.clone(),
                "swarm-task".to_string(),
                SessionType::Hidden,
                GooseMode::default(),
            )
            .await?;
        let session_id = session.id.clone();

        let mut model_config =
            goose::model_config::model_config_from_user_config("lmstudio", model_id)?;
        if let Some(t) = self.sampling.temperature {
            model_config = model_config.with_temperature(Some(t));
        }
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
            let dir = std::env::current_dir()
                .unwrap_or_else(|_| self.working_dir.clone())
                .join(".swarm")
                .join("activity");
            let _ = std::fs::create_dir_all(&dir);
            dir.join(format!("{k}.json"))
        });
        if let Some(p) = &activity_file {
            let _ = std::fs::write(p, "{\"tool_calls\":0,\"errors\":0,\"recent\":[],\"last_text\":\"\"}");
        }
        // IDLE-based watchdog: kill the task only if NO agent event arrives for `idle_secs` (a genuinely
        // stalled stream), NOT on total wall-clock — a slow-but-progressing local model emits an event
        // every turn and must be allowed to finish. idle_secs == 0 disables the watchdog.
        let idle = std::time::Duration::from_secs(if idle_secs == 0 { 86_400 } else { idle_secs });
        loop {
            let ev = match tokio::time::timeout(idle, stream.next()).await {
                Ok(Some(ev)) => ev,
                Ok(None) => break,
                Err(_) => {
                    return Err(anyhow!(
                        "agent stalled — no progress for {idle_secs}s (no token/tool activity)"
                    ))
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
                    .map(|t| format!("{} {}", t.name, if t.ok == Some(false) { "ERR" } else { "ok" }))
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
        let mut handles = Vec::new();
        for (i, q) in questions.into_iter().enumerate() {
            let me = self.clone();
            let exts = research_extensions.clone();
            let model = worker_models[i % worker_models.len()].clone();
            handles.push(tokio::spawn(async move {
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
            }));
        }
        let mut out = Vec::new();
        for h in handles {
            if let Ok(f) = h.await {
                out.push(f);
            }
        }
        out
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
        let mut handles = Vec::new();
        for (i, lens) in select_lenses(is_amendment, max_lenses)
            .into_iter()
            .enumerate()
        {
            let me = self.clone();
            let exts = research_extensions.clone();
            let model = worker_models[i % worker_models.len()].clone();
            let prompt = user_prompt.to_string();
            handles.push(tokio::spawn(async move {
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
            }));
        }
        let mut out = Vec::new();
        for h in handles {
            if let Ok(f) = h.await {
                out.push(f);
            }
        }
        out
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
    ) -> Result<String> {
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
        let system = format!("You are the ARCHITECT on the smart model. Produce a PLAN SKELETON ONLY — do NOT write code. \
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
            DELIVER ONLY THE APP: decompose the program's actual FUNCTIONALITY — its logic modules, the runnable entry point, and its \
            tests, nothing else. Do NOT add project-scaffolding subtasks: NO CI/workflow config, LICENSE, README/docs, \
            pyproject/setup/packaging, .gitignore, or pre-commit hooks — UNLESS the request explicitly asks for them. They are not the \
            deliverable, they waste the slow fleet, and the weak model tends to claim such a file done without ever writing it.\n\
            DECIDE THE LAYOUT FIRST and pick ONE convention, applied to EVERY file — do NOT mix: EITHER a single package \
            directory `pkgname/` that holds ALL modules AND the cli (imports like `from pkgname.models import X`), with tests \
            under `tests/`; OR fully FLAT (every .py at the project root, imports like `from models import X`). NEVER put the cli \
            in a package while its modules sit at the root. Every subtask's `files` and every import MUST match the one chosen \
            layout exactly.\n\
            AMENDMENT — if the manifest below already lists project files, you are EDITING an existing app: every subtask that \
            changes existing behavior MUST own the EXACT existing path (e.g. `src/notes/models.py`), and imports MUST match the \
            real modules. NEVER invent a new filename (e.g. `note.py`) for a module that already exists (e.g. `models.py`) — that \
            file will never be written and the task fails. Create NEW files ONLY for genuinely new modules or tests.\n\
            If the request is a CLI / command-line tool (says 'CLI', 'command', 'command-line'), you MUST include a subtask that \
            writes the RUNNABLE ENTRY POINT — a `cli.py` (argparse or click) that wires the logic modules into actual commands \
            AND a `__main__.py` so `python3 -m <pkg> ...` runs it. The logic modules + tests ALONE are NOT a usable CLI; never \
            omit the entry point.\n\
            For each subtask provide: id (kebab-case), description (ONE short line — a fuller spec is written separately, keep \
            it terse here), difficulty (\"easy\"|\"hard\"), model (\"qwen/qwen3.6-27b\" if hard else \"qwen/qwen3.6-35b-a3b\"), \
            depends_on (list of ids; empty if independent), files (paths it owns; non-overlapping).\n\
            UNLESS the task is purely text, ALWAYS add a FINAL subtask id \"integrate-verify\" depending_on EVERY other subtask, \
            difficulty \"hard\": be EFFICIENT (do not re-read every file; rely on the test run). It RUNS `python3 -m pytest` \
            (NOT py_compile) and fixes EVERY failure until GREEN — INCLUDING a pre-existing test that now fails because this \
            change intentionally altered behavior (e.g. a new field appears in a serialized dict): in that case EDIT that \
            existing test to assert the new correct output. Do not stall — make the whole suite pass. Then runs the program's \
            main command ONCE to confirm it starts, and reports PASS/FAIL honestly. Its own files must NOT overlap the others. \
            Then call the final_output tool with the plan.");
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
        let skeleton = if n == 1 {
            candidates
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("architect produced no skeleton"))?
        } else {
            let mut best: Option<(i64, String)> = None;
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
                        if best.as_ref().map(|(b, _)| score > *b).unwrap_or(true) {
                            best = Some((score, c));
                        }
                    }
                    None => eprintln!("  · candidate {i}: invalid DAG — skipped"),
                }
            }
            match best {
                Some((score, json)) => {
                    eprintln!(
                        "  {} picked best skeleton (score {score})",
                        style("✓").green().bold()
                    );
                    json
                }
                None => return Err(anyhow!("no valid skeleton among {n} candidates")),
            }
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
                arr.push(serde_json::json!({
                    "id": "integrate-verify",
                    "description": "Integrate every module and VERIFY the whole program works end-to-end: run the test suite, then ACTUALLY RUN the entry point (python3 -m <package> --help AND one real command with real arguments from the shell) and FIX any runtime crash — a green test suite does NOT prove the CLI runs.",
                    "depends_on": ids,
                    "files": [],
                    "difficulty": "hard",
                    "model": "qwen/qwen3.6-27b"
                }));
                eprintln!("  · injected missing integrate-verify sink (architect omitted it)");
            }
        }
        let items: Vec<(usize, String, String)> = v
            .get("subtasks")
            .and_then(|s| s.as_array())
            .ok_or_else(|| anyhow!("skeleton has no subtasks array"))?
            .iter()
            .enumerate()
            .map(|(i, st)| {
                (
                    i,
                    st["id"].as_str().unwrap_or("").to_string(),
                    st["description"].as_str().unwrap_or("").to_string(),
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
        let mut handles = Vec::new();
        for (idx, id, brief) in items {
            let me = self.clone();
            let model = wm[idx % wm.len()].clone();
            let goal = goal.clone();
            let findings = findings.clone();
            handles.push(tokio::spawn(async move {
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
                    files it owns, edge cases to handle, and what its tests must check. Be concrete and self-contained, \
                    and BRIEF — about 150 words, no preamble. Output ONLY the spec prose; do NOT write code files or \
                    restate the whole project."
                    .to_string();
                let user = format!("Overall goal: {goal}\n\nThis subtask: [{id}] {brief}{fb}");
                // Bound each detailer so one slow model cannot drag out the PLAN phase — on
                // timeout/empty/error we fall back to the architect's brief line (still a valid spec).
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
            }));
        }
        for h in handles {
            if let Ok((idx, desc)) = h.await {
                v["subtasks"][idx]["description"] = serde_json::Value::String(desc);
            }
        }
        Ok(v.to_string())
    }

    pub async fn plan(
        &self,
        planner_model: &str,
        user_prompt: &str,
        plan_schema: serde_json::Value,
        worker_count: usize,
        research_findings: &str,
    ) -> Result<String> {
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
            files, RUNS `python3 -m pytest`, and fixes EVERY failure until GREEN — including a pre-existing test that now \
            fails because the change intentionally altered behavior (EDIT that existing test to assert the new output; do not \
            stall). Reports PASS/FAIL; its files must NOT overlap the others.\n\
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
        out.final_output
            .ok_or_else(|| anyhow!("planner did not produce a final_output plan"))
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

/// Parse the semantic judge's one-line `VERDICT|CONFIDENCE|hint` reply. Conservative: anything not a
/// clearly-flagged problem reads as OK, so a vague weak-model reply can never kill a healthy worker.
/// CONFIDENCE gates agency — the judge acts (kill + correct) only on a verdict it marks HIGH.
fn parse_judge_reply(s: &str) -> JudgeOutcome {
    let upper = s.to_uppercase();
    let verdict = if upper.contains("BROKEN_CODE") || upper.contains("BROKEN CODE") {
        Verdict::BrokenCode
    } else if upper.contains("LOOPING") {
        Verdict::Looping
    } else if upper.contains("SPEC_DRIFT") || upper.contains("SPEC DRIFT") {
        Verdict::SpecDrift
    } else {
        return JudgeOutcome::ok();
    };
    // The correction is the first segment after the verdict that is real text — not the HIGH/LOW token
    // (the model may emit `VERDICT|CONFIDENCE|hint` or the older `VERDICT|hint`).
    let hint = s
        .split('|')
        .skip(1)
        .map(|h| h.trim())
        .find(|h| {
            let u = h.to_uppercase();
            !h.is_empty() && u != "HIGH" && u != "LOW"
        })
        .map(|h| h.to_string())
        .unwrap_or_else(|| "Your output does not match the spec — correct it now.".to_string());
    // Confidence gates AGENCY: the judge acts (kill + re-dispatch with the correction) only when it marks
    // the verdict HIGH; an unsure/LOW verdict is logged (observed) but never kills. This lets the model's
    // own intelligence drive interventions while keeping a vague reply harmless. TUNABLE: drop the HIGH
    // mapping below intervene_confidence (0.8) to revert to advisory-only if it mis-fires live; raise the
    // agency further (or raise the cap) as the model proves it judges well.
    let confidence = if upper.contains("HIGH") { 0.85 } else { 0.5 };
    JudgeOutcome {
        verdict,
        confidence,
        hint,
    }
}

#[async_trait]
impl Judge for GooseAgentDispatcher {
    async fn judge(&self, req: JudgeRequest) -> JudgeOutcome {
        let cfg = JudgeConfig::default();
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
                if f.ends_with(".py") && !contents.trim().is_empty() {
                    if let Some(err) = py_syntax_error(&path).await {
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
        let user = format!(
            "GOAL: {goal}\n\nRUN STATE:\n  done:\n{done}\n  still running: {rem}\n  failed: {fail}\n\n\
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
            Ok(Ok(o)) => parse_judge_reply(&o.text),
            _ => JudgeOutcome::ok(),
        }
    }
}

#[async_trait]
impl TaskDispatcher for GooseAgentDispatcher {
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError> {
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
            let cwd = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
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
                 run is a FAILURE no matter how many unit tests pass. Report any missing file or runtime crash.\n\n"
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
                format!(
                    "YOU OWN — write EXACTLY these ABSOLUTE paths, and write NOTHING outside them. Their \
                     parent directories ALREADY EXIST (pre-created for you) — NEVER run `mkdir` at all (it \
                     just wastes turns):\n{owned}{multi_note}\n\
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
                            "## CURRENT content of {f}{note} — you are EDITING this file; do NOT `cat` it \
                             again, edit it from here:\n```\n{capped}\n```\n\n"
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
                if owned_set.contains(f) || !f.ends_with(".py") {
                    continue;
                }
                let base = std::path::Path::new(f)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if base.starts_with("test_") || base.ends_with("_test.py") || base == "conftest.py"
                {
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
        let system_prompt = format!(
            "You are a WORKER on a local AI swarm. Complete EXACTLY the task below using your tools, \
             in the current working directory. Write correct, minimal code; do nothing beyond the task. \
             When finished, briefly state what you produced.\n\
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
             \n{layout_block}{context_block}"
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
            .run_agent(
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
                // writing its owned file — a test-archive task did exactly this (0 writes, the file never
                // appeared, yet the task was marked done). Verify every owned file now exists and is
                // non-empty; if not, RETRY the task (Transient) instead of silently accepting the lie.
                if !req.owned_files.is_empty() {
                    let cwd = std::env::current_dir().unwrap_or_default();
                    let missing: Vec<String> = req
                        .owned_files
                        .iter()
                        .filter(|f| cwd.join(f).metadata().map(|m| m.len() == 0).unwrap_or(true))
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
                        return Err(DispatchError::Transient(format!(
                            "task {} returned without writing its owned file(s): {}",
                            req.task_id,
                            missing.join(", ")
                        )));
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

    let plan_json = if cfg.parallel_planning {
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
                opts.best_of_n.unwrap_or(cfg.best_of_n_skeletons),
                cfg.homogeneous_models,
            )
            .await
        {
            Ok(j) => j,
            Err(e) => {
                eprintln!("  parallel planning failed ({e}); falling back to the solo planner");
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
    let dag = Dag::from_planner_json(&plan_json)
        .map_err(|e| anyhow!("invalid plan from planner: {e}\nplan was: {plan_json}"))?;
    eprintln!("  plan: {} subtask(s)", dag.tasks.len());

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
        scheduler = scheduler.with_judge(dispatcher.clone() as Arc<dyn Judge>, JudgeConfig::default());
    }
    let report = scheduler
        .run(
            dag,
            dispatcher as Arc<dyn TaskDispatcher>,
            opts.prompt.clone(),
        )
        .await?;

    let report_value = serde_json::to_value(&report).unwrap_or(serde_json::Value::Null);
    sink.write_value(serde_json::json!({
        "event": "run_finished",
        "report": report_value,
    }));

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        println!("\n{}", style("=== swarm report ===").bold());
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
    if core_failed == 0 {
        Ok(())
    } else {
        Err(anyhow!("{} core subtask(s) failed", core_failed))
    }
}
