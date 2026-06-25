//! `goose swarm` — local multi-device swarm (goose-local-edition).
//!
//! `goose swarm run "<task>"` plans on the smart model then dispatches subtasks across the LM Link
//! device pool with the goose-swarm weighted work-queue scheduler. `goose swarm pool` manages the
//! pool (devices, weights, enable/disable) via an interactive menu, persisted in the Goose config.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use console::style;
use futures::StreamExt;
use goose::agents::{Agent, AgentConfig, AgentEvent, ExtensionConfig, GoosePlatform, SessionConfig};
use goose::config::permission::PermissionManager;
use goose::config::{Config, GooseMode};
use goose::conversation::message::{Message, MessageContent};
use goose::providers::base::Provider;
use goose::recipe::Response;
use goose::session::session_manager::SessionType;
use goose::session::SessionManager;
use goose_swarm::{
    Dag, DeviceCfg, DispatchError, DispatchRequest, EventSink, NullSink, Scheduler, SwarmEvent,
    TaskDispatcher, TaskRunOutput, ToolCallRecord,
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
fn default_worker_max_turns() -> u32 {
    40
}
fn default_max_attempts() -> u32 {
    3
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
                },
                SwarmDevice {
                    id: "macbook".to_string(),
                    model_id: "qwen3.6-35b-a3b-mtp-holo3-qwopus-qx86-hi-mlx".to_string(),
                    weight: 1,
                    enabled: true,
                    instances: 1,
                },
            ],
            worker_max_turns: default_worker_max_turns(),
            max_attempts: default_max_attempts(),
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
}

pub async fn handle_swarm(cmd: SwarmCommand) -> Result<()> {
    match cmd {
        SwarmCommand::Run {
            prompt,
            output_format,
            log_file,
            no_log,
            max_turns,
        } => {
            run_swarm(RunOpts {
                prompt,
                output_format,
                log_file,
                no_log,
                max_turns,
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
    println!(
        "\n{}  endpoint {}  planner {}  max-turns {}  max-attempts {}",
        style(" swarm pool ").on_cyan().black().bold(),
        style(&cfg.endpoint).cyan(),
        style(&cfg.planner_model).green(),
        style(cfg.worker_max_turns).cyan(),
        style(cfg.max_attempts).cyan()
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
            "  {state}  {:<10} weight {}  ×{} inst  {}",
            style(&d.id).bold(),
            style(d.weight).cyan().bold(),
            style(d.instances).cyan(),
            style(&d.model_id).dim()
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
            .item("probe", "Probe the live fleet", "lms ps + endpoint models")
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
            "probe" => probe_fleet(),
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

// ---------------------------------------------------------------------------------------------
// Dispatcher (M1.1) — drives one Goose agent per task over the shared lmstudio provider
// ---------------------------------------------------------------------------------------------

pub struct GooseAgentDispatcher {
    provider: Arc<dyn Provider>,
    session_manager: Arc<SessionManager>,
    permission_manager: Arc<PermissionManager>,
    working_dir: PathBuf,
    worker_max_turns: u32,
}

impl GooseAgentDispatcher {
    pub async fn new(working_dir: PathBuf, worker_max_turns: u32) -> Result<Self> {
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
        })
    }

    async fn run_agent(
        &self,
        model_id: &str,
        system_prompt: String,
        user_text: String,
        response: Option<Response>,
        max_turns: u32,
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

        let model_config =
            goose::model_config::model_config_from_user_config("lmstudio", model_id)?;
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
        while let Some(ev) = stream.next().await {
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
                                            serde_json::to_string(&tc.arguments).unwrap_or_default(),
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

    pub async fn plan(
        &self,
        planner_model: &str,
        user_prompt: &str,
        plan_schema: serde_json::Value,
    ) -> Result<String> {
        let system = "You are the PLANNER on the smart model. Produce a PLAN ONLY — do NOT write code.\n\
            Decompose the task into the smallest set of subtasks; maximize the INDEPENDENT set (no shared files, no ordering dependency).\n\
            For each subtask provide: id (kebab-case), description (a precise self-contained spec), difficulty (\"easy\"|\"hard\"), \
            model (\"qwen/qwen3.6-27b\" if hard else \"qwen/qwen3.6-35b-a3b\"), depends_on (list of ids; empty if independent), \
            files (paths it owns; non-overlapping across parallel subtasks).\n\
            UNLESS the task is purely text with nothing to integrate, ALWAYS add a FINAL subtask id \"integrate-verify\" \
            that depends_on EVERY other subtask, difficulty \"hard\", model \"qwen/qwen3.6-27b\": it integrates the produced \
            files, writes and RUNS tests (e.g. python3), and reports PASS/FAIL; its files must NOT overlap the others (e.g. a test file).\n\
            Also produce a short integration note. Then call the final_output tool with the plan."
            .to_string();
        let response = Some(Response {
            json_schema: Some(plan_schema),
        });
        let out = self
            .run_agent(
                planner_model,
                system,
                format!("Plan this task: {user_prompt}"),
                response,
                15,
            )
            .await?;
        out.final_output
            .ok_or_else(|| anyhow!("planner did not produce a final_output plan"))
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
        let system_prompt = format!(
            "You are a WORKER on a local AI swarm. Complete EXACTLY the task below using your tools, \
             in the current working directory. Write correct, minimal code; do nothing beyond the task. \
             When finished, briefly state what you produced.\n\n{context_block}"
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
        let outcome = self
            .run_agent(
                &req.model_id,
                system_prompt,
                req.description.clone(),
                None,
                self.worker_max_turns,
            )
            .await;
        let secs = started.elapsed().as_secs_f64();
        match outcome {
            Ok(out) => {
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
                let transient = s.contains("Model is unloaded")
                    || s.contains("Server error")
                    || s.contains("model_not_found")
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
                    // M1.3: best-effort re-warm (idempotent) before the scheduler re-dispatches.
                    if s.contains("Model is unloaded") || s.contains("connection") {
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

pub async fn run_swarm(opts: RunOpts) -> Result<()> {
    let cfg = load_config();
    let enabled: Vec<&SwarmDevice> = cfg.devices.iter().filter(|d| d.enabled).collect();
    if enabled.is_empty() {
        return Err(anyhow!(
            "no enabled devices in the swarm pool — run `goose swarm pool` to add some"
        ));
    }
    std::env::set_var("LMSTUDIO_HOST", &cfg.endpoint);

    let json = opts.output_format == "json";
    let working_dir = std::env::current_dir()?;
    let worker_max_turns = opts.max_turns.unwrap_or(cfg.worker_max_turns);

    let run_id = format!("swarm-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S%3f"));
    let log_path: Option<PathBuf> = if opts.no_log {
        None
    } else {
        Some(opts.log_file.clone().unwrap_or_else(|| {
            working_dir.join(".swarm").join(format!("run-{run_id}.jsonl"))
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

    // M1.3: pre-warm the planner + all enabled worker models so remote JIT-load doesn't race.
    eprintln!("pre-warming models (idempotent) ...");
    ensure_loaded(&cfg.planner_model, 1);
    for d in &enabled {
        ensure_loaded(&d.model_id, d.instances);
    }

    let devices: Vec<DeviceCfg> = enabled
        .iter()
        .map(|d| DeviceCfg {
            id: d.id.clone(),
            model_id: d.model_id.clone(),
            weight: d.weight,
            enabled: true,
        })
        .collect();

    let dispatcher =
        Arc::new(GooseAgentDispatcher::new(working_dir.clone(), worker_max_turns).await?);

    eprintln!("planning on {} ...", cfg.planner_model);
    let plan_json = dispatcher
        .plan(&cfg.planner_model, &opts.prompt, plan_schema())
        .await?;
    let dag = Dag::from_planner_json(&plan_json)
        .map_err(|e| anyhow!("invalid plan from planner: {e}\nplan was: {plan_json}"))?;
    eprintln!("plan: {} subtask(s). dispatching ...", dag.tasks.len());

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

    let scheduler = Scheduler::new(devices, cfg.max_attempts).with_sink(sink.clone());
    let report = scheduler
        .run(dag, dispatcher as Arc<dyn TaskDispatcher>)
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
        if !report.failed.is_empty() {
            println!(
                "{} ({}): {}",
                style("FAILED").red().bold(),
                report.failed.len(),
                report.failed.join(", ")
            );
        }
        println!("dispatched per device: {:?}", report.dispatched_per_device);
        for id in &report.done {
            if let Some(r) = report.results.get(id) {
                let snippet: String = r.chars().take(280).collect();
                println!("\n--- {id} ---\n{snippet}");
            }
        }
    }

    if report.failed.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("{} subtask(s) failed", report.failed.len()))
    }
}
