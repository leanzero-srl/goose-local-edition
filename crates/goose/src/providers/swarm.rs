//! The `swarm` provider — makes the local model swarm a selectable "model" in goose.
//!
//! The swarm is task ORCHESTRATION (a minutes-long, file-writing multi-agent build), not a per-turn chat
//! completion. So this provider does NOT pretend to be a chat model: selecting it turns a chat turn into a
//! "build run" — the user's message is the brief, and the provider spawns `goose swarm run` against the
//! session's working directory, then returns the run's fan-in/report as the assistant message.
//!
//! It mirrors [`crate::providers::claude_code::ClaudeCodeProvider`]: a subprocess provider that spawns the
//! `goose` CLI (avoiding the goose-cli → goose dependency cycle) and translates its `--output-format json`
//! RunReport into a [`Message`]. `manages_own_context() = true` — the swarm manages its own context.
//!
//! First increment: spawn-to-completion (await the run, summarize the report). Live fan-in streaming and the
//! cancellation/fleet-abort hardening are follow-ups (the cancellation path is the low-confidence seam —
//! aborting a run must also unload in-flight remote generations, which is not guaranteed yet).

use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use serde_json::Value;

use super::base::{
    stream_from_single_message, ConfigKey, MessageStream, Provider, ProviderDef, ProviderMetadata,
};
use crate::config::ExtensionConfig;
use crate::conversation::message::{Message, MessageContent};
use crate::providers::api_client::TlsConfig;
use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use rmcp::model::Tool;

const SWARM_PROVIDER_NAME: &str = "swarm";
const SWARM_DEFAULT_MODEL: &str = "swarm";
const SWARM_DOC_URL: &str = "https://leanzero.atlascrafted.com/portfolio/goose-local-edition";

/// A subprocess-backed swarm provider. Holds the `goose` binary path and the session working directory to
/// scaffold into (threaded via [`ProviderDef::from_env_with_working_dir`] — without it the swarm would write
/// files into an unpredictable directory).
pub struct SwarmProvider {
    name: String,
    /// Path/name of the `goose` CLI binary to spawn (resolved from `SWARM_COMMAND`, default `goose`).
    command: String,
    /// The directory a run scaffolds into. `None` → the goose process cwd.
    working_dir: Option<PathBuf>,
}

impl SwarmProvider {
    fn resolve_command() -> String {
        if let Ok(cmd) = std::env::var("SWARM_COMMAND") {
            return cmd;
        }
        // In the desktop the running process is `goosed`; the `goose` CLI is usually a sibling binary.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(sibling) = exe.parent().map(|d| d.join("goose")) {
                if sibling.is_file() {
                    return sibling.to_string_lossy().into_owned();
                }
            }
        }
        "goose".to_string()
    }

    /// The brief for a run = the text of the last user message.
    fn extract_brief(messages: &[Message]) -> String {
        for msg in messages.iter().rev() {
            let text: String = msg
                .content
                .iter()
                .filter_map(|c| match c {
                    MessageContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !text.trim().is_empty() {
                return text;
            }
        }
        String::new()
    }

    /// Format a `RunReport` JSON blob into a concise assistant summary. Parsed generically (goose does not
    /// depend on goose-swarm), so it degrades to the raw output if the shape is unexpected.
    fn summarize_report(stdout: &str) -> String {
        let Ok(report) = serde_json::from_str::<Value>(stdout.trim()) else {
            let tail: String = stdout.chars().rev().take(1200).collect::<String>();
            let tail: String = tail.chars().rev().collect();
            return format!("Swarm run finished. Output:\n\n{tail}");
        };
        let done = report
            .get("done")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0);
        let failed = report
            .get("failed")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0);
        let mut out = String::new();
        if failed == 0 {
            out.push_str(&format!(
                "**Swarm run complete** — {done} task(s) done, 0 failed.\n"
            ));
        } else {
            out.push_str(&format!(
                "**Swarm run finished with failures** — {done} done, {failed} FAILED.\n"
            ));
        }
        if let Some(per_device) = report
            .get("dispatched_per_device")
            .and_then(Value::as_object)
        {
            let line = per_device
                .iter()
                .map(|(d, n)| format!("{d} {n}"))
                .collect::<Vec<_>>()
                .join(", ");
            if !line.is_empty() {
                out.push_str(&format!("\nDispatched per node: {line}\n"));
            }
        }
        if failed > 0 {
            if let Some(f) = report.get("failed").and_then(Value::as_array) {
                let ids: Vec<String> = f
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                out.push_str(&format!("\nFailed: {}\n", ids.join(", ")));
            }
        }
        out
    }
}

impl goose_providers::base::ProviderDescriptor for SwarmProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            SWARM_PROVIDER_NAME,
            "Swarm LeanZero",
            "Runs your local model fleet (LM Studio / LM Link) as a multi-agent build. A message is a build \
             brief, not a chat turn — the swarm plans, fans out across your nodes, and writes files to this \
             project. Configure the fleet + tunables in the Swarm settings.",
            SWARM_DEFAULT_MODEL,
            vec![SWARM_DEFAULT_MODEL],
            SWARM_DOC_URL,
            vec![ConfigKey::new(
                "SWARM_COMMAND",
                true,
                false,
                Some("goose"),
                true,
            )],
        )
    }
}

impl ProviderDef for SwarmProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<ExtensionConfig>,
        _tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(async move {
            Ok(Self {
                name: SWARM_PROVIDER_NAME.to_string(),
                command: Self::resolve_command(),
                working_dir: None,
            })
        })
    }

    fn from_env_with_working_dir(
        _extensions: Vec<ExtensionConfig>,
        working_dir: PathBuf,
        _tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(async move {
            Ok(Self {
                name: SWARM_PROVIDER_NAME.to_string(),
                command: Self::resolve_command(),
                working_dir: Some(working_dir),
            })
        })
    }
}

#[async_trait]
impl Provider for SwarmProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn manages_own_context(&self) -> bool {
        true
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        // Incidental-completion guard: session titles, tool summaries, etc. must NOT spawn the fleet.
        if super::cli_common::is_session_description_request(system) {
            let (message, usage) = super::cli_common::generate_simple_session_description(
                &model_config.model_name,
                messages,
            )?;
            return Ok(stream_from_single_message(message, usage));
        }

        let brief = Self::extract_brief(messages);
        if brief.trim().is_empty() {
            let msg = Message::assistant().with_text(
                "Give the swarm a build brief (what to build) and it will run your fleet.",
            );
            return Ok(stream_from_single_message(
                msg,
                ProviderUsage::new(self.name.clone(), Usage::default()),
            ));
        }

        let mut cmd = tokio::process::Command::new(&self.command);
        cmd.arg("swarm")
            .arg("run")
            .arg(&brief)
            .arg("--output-format")
            .arg("json");
        if let Some(dir) = &self.working_dir {
            cmd.current_dir(dir);
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        // A swarm run drives the whole fleet and can take minutes. When the user hits Stop in the
        // UI, the agent drops this request future; without kill_on_drop the child `goose swarm run`
        // is orphaned and keeps dispatching to the fleet. kill_on_drop makes Stop actually stop it.
        cmd.kill_on_drop(true);

        let output = cmd.output().await.map_err(|e| {
            ProviderError::RequestFailed(format!(
                "Failed to spawn '{} swarm run': {e}. Set SWARM_COMMAND to the goose binary path if it is \
                 not on PATH.",
                self.command
            ))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let summary = if output.status.success() {
            Self::summarize_report(&stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let tail: String = stderr.chars().rev().take(1500).collect::<String>();
            let tail: String = tail.chars().rev().collect();
            format!("**Swarm run failed.**\n\n{tail}")
        };

        let message = Message::assistant().with_text(summary);
        Ok(stream_from_single_message(
            message,
            ProviderUsage::new(self.name.clone(), Usage::default()),
        ))
    }
}
