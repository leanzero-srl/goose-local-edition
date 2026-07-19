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

/// #ai-session-names (GOOSE_SWARM_AI_NAME env, else `swarm.ai_session_name` in config; DEFAULT OFF —
/// conservative about fleet queue-contention). When on, a swarm-build session is titled by ONE cheap
/// local-planner call instead of the first-4-words truncation ("Build X — a").
fn ai_session_name_enabled() -> bool {
    if let Ok(v) = std::env::var("GOOSE_SWARM_AI_NAME") {
        return matches!(
            v.trim().to_lowercase().as_str(),
            "1" | "on" | "true" | "yes"
        );
    }
    crate::config::Config::global()
        .get_param::<serde_json::Value>("swarm")
        .ok()
        .and_then(|c| c.get("ai_session_name").and_then(|v| v.as_bool()))
        .unwrap_or(true)
}

/// Title the session with ONE local-planner call (thinking OFF via complete_fast, 25s timeout). Passes the
/// SAME session-title system+messages straight to the real lmstudio provider, so the planner emits a title and
/// the outer `generate_session_name` strips the reasoning block + picks the short title. Any error/timeout
/// bubbles up so the caller falls back to the truncation. Fires off the reply critical path (a detached spawn
/// in agent.rs), so it never slows build start.
async fn ai_session_title(
    system: &str,
    messages: &[Message],
) -> Result<MessageStream, ProviderError> {
    let planner = crate::config::Config::global()
        .get_param::<serde_json::Value>("swarm")
        .ok()
        .and_then(|c| {
            c.get("planner_model")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "qwen/qwen3.6-27b".to_string());
    let provider = crate::providers::create("lmstudio", vec![])
        .await
        .map_err(|e| ProviderError::ExecutionError(e.to_string()))?;
    let mc = crate::model_config::model_config_from_user_config("lmstudio", &planner)
        .map_err(|e| ProviderError::ExecutionError(e.to_string()))?;
    let (message, usage) = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        crate::model_config::complete_fast(
            provider.as_ref(),
            &mc,
            "swarm-name",
            system,
            messages,
            &[],
        ),
    )
    .await
    .map_err(|_| ProviderError::ExecutionError("session-title call timed out".into()))??;
    Ok(stream_from_single_message(message, usage))
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
            // #ai-session-names: title the session with a cheap local-planner call instead of the
            // first-4-words truncation ("Build X — a"). Gated (default OFF); on error/timeout it falls back to
            // the truncation below. Runs off the reply critical path (a detached spawn in agent.rs), so it
            // never slows build start.
            if ai_session_name_enabled() {
                if let Ok(s) = ai_session_title(system, messages).await {
                    return Ok(s);
                }
            }
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
        // End-to-end verification for UI-dispatched builds. The smoke gate RUNS the produced program
        // (e.g. `python3 -m <pkg>` / `pytest --collect-only`) and fires one corrective fix if it crashes,
        // so a build that produced a broken entry point (e.g. a CLI that raises on every command) is caught
        // before it is reported "done". This gate is off unless the env var is set or "assured" mode is on;
        // CLI runs set it explicitly, but the UI provider did not — so UI builds silently skipped end-to-end
        // verification and shipped broken CLIs. Default it on here; respect an explicit override.
        if std::env::var("GOOSE_SWARM_SMOKE").is_err() {
            cmd.env("GOOSE_SWARM_SMOKE", "1");
        }
        // Fleet utilization: without splitting, the architect's coarse near-serial plans (a big shared-types
        // root -> a couple modules -> a lone cli -> a lone integrate-verify) leave 1-2 of 3 fleet nodes idle
        // most of the wall-clock (observed: peak concurrency 2-3 but most dispatches at concurrency 1). The
        // split mechanism partitions an over-long, multi-file task into 2-4 independent children so several
        // workers run in parallel. It is off by default (CLI A/B runs set it; the UI provider did not, which
        // is why app-dispatched builds starved the fleet). Enable it here, and lower the too-long threshold
        // from the conservative 900s default to 300s: measured task durations are median 219s / p75 406s, so
        // 900s split almost nothing while 300s splits the fat multi-file tasks that actually cause the idle.
        if std::env::var("GOOSE_SWARM_SPLIT").is_err() {
            cmd.env("GOOSE_SWARM_SPLIT", "1");
        }
        if std::env::var("GOOSE_SWARM_SPLIT_SECS").is_err() {
            cmd.env("GOOSE_SWARM_SPLIT_SECS", "300");
        }
        // Quality: two built-but-unenabled gates that the CLI/assured path uses but the UI provider did not.
        // CONTRACTS freezes signature-only module interfaces before EXECUTE so parallel workers agree on the
        // shape (the dominant cross-module drift: bookclub ctx.obj, csvql row dict-vs-list, tmpl parser/renderer).
        // It is also the quality partner for SPLIT — more parallel children means more interfaces to agree on.
        // COMPLETE verifies the produced app by RUNNING it (language-aware: pytest/`-m` for Python, cargo
        // build+test+run for Rust) and fixes-until-green within a bounded round budget, refusing to ship a red
        // app — the "detect AND prevent" the advisory smoke gate lacked (kvstore empty main, wal no-persistence,
        // taskq won't-compile all shipped). Bounded so a doomed build can't spin: default 2 rounds + a hard cap.
        if std::env::var("GOOSE_SWARM_CONTRACTS").is_err() {
            cmd.env("GOOSE_SWARM_CONTRACTS", "1");
        }
        if std::env::var("GOOSE_SWARM_COMPLETE").is_err() {
            cmd.env("GOOSE_SWARM_COMPLETE", "1");
        }
        if std::env::var("GOOSE_SWARM_COMPLETE_CAP_SECS").is_err() {
            cmd.env("GOOSE_SWARM_COMPLETE_CAP_SECS", "1200");
        }
        // Convergence molding (steer the weak planner to the simplest canonical decomposition, role-normalize
        // agreement) is now a persisted swarm CONFIG tunable (`converge`, default ON) that the spawned
        // `goose swarm run` reads directly — so it's toggleable from the desktop settings, not forced here.
        // GOOSE_SWARM_CONVERGE env still overrides for scripted A/B.
        if let Some(dir) = &self.working_dir {
            cmd.current_dir(dir);
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        // TEE THE ENGINE'S STDERR TO DISK, AS IT STREAMS.
        //
        // `cmd.output()` below buffers stderr in memory and hands it back only when the child EXITS — and the
        // provider then surfaces it only `if !output.status.success()`, into one chat message. So when a run
        // dies, its last words go into a message nobody kept, and the build dir — the place anyone would
        // actually look — holds nothing.
        //
        // MEASURED 2026-07-17: three consecutive runs of the same arm died at three DIFFERENT phases (a
        // redraft, a pre_review, and right after research_completed). Every time the heartbeat froze on its
        // 5-second timer while `goose serve` stayed up, swap read 0.00M, and macOS filed no crash report. I
        // could not diagnose ANY of them, because there was nothing written down. Three dead runs, zero lines.
        //
        // The prime suspect is `kill_on_drop(true)` below: this child dies the moment the request future is
        // dropped, and a kill leaves no crash report and no panic — exactly what was observed. Whether that is
        // it or a panic in the run's own task, the FIRST requirement is that the engine's output survives the
        // process that produced it. Streamed, never accumulated; the run's own dir, beside its events.
        let stderr_log = self.working_dir.as_ref().map(|d| {
            let p = std::path::Path::new(d)
                .join(".swarm")
                .join("engine-stderr.log");
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            p
        });
        // A swarm run drives the whole fleet and can take minutes. When the user hits Stop in the
        // UI, the agent drops this request future; without kill_on_drop the child `goose swarm run`
        // is orphaned and keeps dispatching to the fleet. kill_on_drop makes Stop actually stop it.
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            ProviderError::RequestFailed(format!(
                "Failed to spawn '{} swarm run': {e}. Set SWARM_COMMAND to the goose binary path if it is \
                 not on PATH.",
                self.command
            ))
        })?;

        // Drain stderr into BOTH the tail (what the chat message needs) and the log file (what a post-mortem
        // needs), line by line, so a killed child still leaves everything it managed to say. This task holds
        // no reference to the child, so kill_on_drop still works exactly as before.
        let stderr_pipe = child.stderr.take();
        let log_path = stderr_log.clone();
        let stderr_task = tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufWriter};
            let mut collected = String::new();
            let Some(pipe) = stderr_pipe else {
                return collected;
            };
            let mut sink = match &log_path {
                Some(p) => tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .await
                    .ok()
                    .map(BufWriter::new),
                None => None,
            };
            if let Some(w) = sink.as_mut() {
                let header = format!("\n===== swarm run started {} =====\n", chrono::Utc::now());
                let _ = tokio::io::AsyncWriteExt::write_all(w, header.as_bytes()).await;
            }
            let mut lines = tokio::io::BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(w) = sink.as_mut() {
                    let _ = tokio::io::AsyncWriteExt::write_all(w, format!("{line}\n").as_bytes())
                        .await;
                    // Flush every line: an unflushed buffer loses precisely the last words before a kill,
                    // which are the only ones worth having.
                    let _ = tokio::io::AsyncWriteExt::flush(w).await;
                }
                collected.push_str(&line);
                collected.push('\n');
            }
            collected
        });

        // DRAIN STDOUT CONCURRENTLY WITH wait(), NOT AFTER IT.
        //
        // This is why `cmd.output()` reads both pipes at once, and getting it wrong deadlocks every run: the
        // OS pipe buffer is ~64KB, so a child that writes more than that to stdout BLOCKS on the write until
        // someone reads. If we sat in `child.wait()` first and only read stdout afterwards, the swarm's
        // --output-format json RunReport (easily past 64KB on a real build) would fill the pipe, the child
        // would block forever, and wait() would never return. Both pipes must be drained while the child runs.
        let stdout_pipe = child.stdout.take();
        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut so) = stdout_pipe {
                let _ = tokio::io::AsyncReadExt::read_to_end(&mut so, &mut buf).await;
            }
            buf
        });

        let status = child.wait().await.map_err(|e| {
            ProviderError::RequestFailed(format!("'{} swarm run' failed to run: {e}", self.command))
        })?;
        let stderr_all = stderr_task.await.unwrap_or_default();
        let stdout_buf = stdout_task.await.unwrap_or_default();
        let output = std::process::Output {
            status,
            stdout: stdout_buf,
            stderr: stderr_all.into_bytes(),
        };

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
