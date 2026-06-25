//! `goose swarm run` — local multi-device swarm (goose-local-edition).
//!
//! The 27B planner emits a typed DAG; the goose-swarm weighted scheduler dispatches each task to a
//! device's LM Link model via [`GooseAgentDispatcher`], which inlines Goose's public Agent drive
//! sequence (no private APIs) and captures the typed `recipe__final_output` payload from the reply
//! stream. M1.2: hard-coded 2-device pool; the cliclack `goose swarm pool` menu is M2.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::StreamExt;
use goose::agents::{Agent, AgentConfig, AgentEvent, ExtensionConfig, GoosePlatform, SessionConfig};
use goose::config::permission::PermissionManager;
use goose::config::GooseMode;
use goose::conversation::message::{Message, MessageContent};
use goose::providers::base::Provider;
use goose::recipe::Response;
use goose::session::session_manager::SessionType;
use goose::session::SessionManager;
use goose_swarm::{Dag, DeviceCfg, DispatchError, DispatchRequest, Scheduler, TaskDispatcher};
use std::path::PathBuf;
use std::sync::Arc;

/// The public final-output tool name (stable across the agent loop). We read its argument from the
/// reply stream instead of touching the private `Agent::final_output_tool` field.
const FINAL_OUTPUT_TOOL: &str = "recipe__final_output";

/// Drives a Goose Agent per task over one shared lmstudio provider; the device is selected by the
/// per-task model id (LM Link routes it).
pub struct GooseAgentDispatcher {
    provider: Arc<dyn Provider>,
    session_manager: Arc<SessionManager>,
    permission_manager: Arc<PermissionManager>,
    working_dir: PathBuf,
    worker_max_turns: u32,
}

impl GooseAgentDispatcher {
    pub async fn new(working_dir: PathBuf, worker_max_turns: u32) -> Result<Self> {
        // One lmstudio provider; per-task model is set via update_provider.
        let provider = goose::providers::create("lmstudio", vec![]).await?;
        let session_root = std::env::temp_dir().join("goose-swarm-sessions");
        std::fs::create_dir_all(&session_root)?;
        let session_manager = Arc::new(SessionManager::new(session_root.clone()));
        let permission_manager = Arc::new(PermissionManager::new(session_root));
        Ok(Self {
            provider,
            session_manager,
            permission_manager,
            working_dir,
            worker_max_turns,
        })
    }

    /// Run one isolated agent bound to `model_id`. Returns (joined assistant text, final_output JSON
    /// if a response schema was set and the model called the final_output tool).
    async fn run_agent(
        &self,
        model_id: &str,
        system_prompt: String,
        user_text: String,
        response: Option<Response>,
        max_turns: u32,
    ) -> Result<(String, Option<String>)> {
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
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(AgentEvent::Message(msg)) => {
                    for content in &msg.content {
                        match content {
                            MessageContent::Text(t) => texts.push(t.text.clone()),
                            MessageContent::ToolRequest(req) => {
                                if let Ok(tc) = req.tool_call.as_ref() {
                                    if tc.name == FINAL_OUTPUT_TOOL {
                                        final_output =
                                            Some(serde_json::to_string(&tc.arguments).unwrap_or_default());
                                    }
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

        // Stream delivers incremental Text chunks; concatenate to reconstruct the message text.
        Ok((texts.concat(), final_output))
    }

    /// Run the planner on `planner_model` and return the typed plan JSON (the final_output payload).
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
            files (paths it owns; non-overlapping across parallel subtasks). Also produce an integration note.\n\
            Then call the final_output tool with the plan."
            .to_string();
        let response = Some(Response {
            json_schema: Some(plan_schema),
        });
        let (_text, final_output) = self
            .run_agent(planner_model, system, format!("Plan this task: {user_prompt}"), response, 15)
            .await?;
        final_output.ok_or_else(|| anyhow!("planner did not produce a final_output plan"))
    }
}

#[async_trait]
impl TaskDispatcher for GooseAgentDispatcher {
    async fn run(&self, req: DispatchRequest) -> Result<String, DispatchError> {
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
        match self
            .run_agent(&req.model_id, system_prompt, req.description.clone(), None, self.worker_max_turns)
            .await
        {
            Ok((text, _)) => Ok(if text.trim().is_empty() {
                format!("(task {} completed)", req.task_id)
            } else {
                text
            }),
            Err(e) => {
                let s = e.to_string();
                if s.contains("Model is unloaded")
                    || s.contains("Server error")
                    || s.contains("model_not_found")
                    || s.contains("connection")
                {
                    Err(DispatchError::Transient(s))
                } else {
                    Err(DispatchError::Terminal(s))
                }
            }
        }
    }
}

/// The typed plan schema the planner must satisfy (mirrors local-edition/recipes/planner.yaml).
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

/// M1.2 hard-coded device pool (M2 reads/edits this via `goose swarm pool`). Weights reflect
/// heterogeneous capacity; model ids must be unique (LM Link routes by id).
fn default_pool() -> Vec<DeviceCfg> {
    vec![
        DeviceCfg {
            id: "mac".to_string(),
            model_id: "qwen/qwen3.6-35b-a3b".to_string(),
            weight: 2,
            enabled: true,
        },
        DeviceCfg {
            id: "macbook".to_string(),
            model_id: "qwen3.6-35b-a3b-mtp-holo3-qwopus-qx86-hi-mlx".to_string(),
            weight: 1,
            enabled: true,
        },
    ]
}

pub async fn run_swarm(prompt: String) -> Result<()> {
    let working_dir = std::env::current_dir()?;
    println!("\x1b[1mswarm\x1b[0m working dir: {}", working_dir.display());

    let dispatcher = Arc::new(GooseAgentDispatcher::new(working_dir, 40).await?);
    let devices = default_pool();
    println!(
        "pool: {} device(s) — {}",
        devices.len(),
        devices
            .iter()
            .map(|d| format!("{}(w{})", d.id, d.weight))
            .collect::<Vec<_>>()
            .join(", ")
    );

    println!("planning on qwen/qwen3.6-27b ...");
    let plan_json = dispatcher
        .plan("qwen/qwen3.6-27b", &prompt, plan_schema())
        .await?;
    let dag = Dag::from_planner_json(&plan_json)
        .map_err(|e| anyhow!("invalid plan from planner: {e}\nplan was: {plan_json}"))?;
    println!("plan: {} subtask(s). dispatching ...", dag.tasks.len());

    let scheduler = Scheduler::new(devices, 3);
    let report = scheduler
        .run(dag, dispatcher as Arc<dyn TaskDispatcher>)
        .await?;

    println!("\n\x1b[1m=== swarm report ===\x1b[0m");
    println!("done   ({}): {}", report.done.len(), report.done.join(", "));
    if !report.failed.is_empty() {
        println!("FAILED ({}): {}", report.failed.len(), report.failed.join(", "));
    }
    println!("dispatched per device: {:?}", report.dispatched_per_device);
    for id in &report.done {
        if let Some(r) = report.results.get(id) {
            let snippet: String = r.chars().take(280).collect();
            println!("\n--- {id} ---\n{snippet}");
        }
    }
    if report.failed.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("{} subtask(s) failed", report.failed.len()))
    }
}
