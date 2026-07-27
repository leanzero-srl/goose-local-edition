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

/// What the engine's OWN event log says about a run. Read after the child exits, so a failed run can be
/// described from its record rather than from whatever text happened to be last on stderr.
#[derive(Default)]
struct RunTrace {
    brief: String,
    phase: &'static str,
    planned: usize,
    done: usize,
    failed: usize,
    in_flight: Vec<String>,
    finished: bool,
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

    /// Parse the run's JSONL generically (goose does not depend on goose-swarm, and an unknown event must
    /// never break the summary). Pure over a `&str` so it is unit-testable without a run. LAST event wins:
    /// the engine emits `contracts` before `plan_loaded`, so no fixed phase order may be assumed.
    fn trace_from_log(log: &str) -> RunTrace {
        let mut t = RunTrace {
            phase: "starting up",
            ..Default::default()
        };
        let (mut dispatched, mut settled): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        for line in log.lines() {
            let Ok(e) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let id = e
                .get("task_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match e.get("event").and_then(Value::as_str).unwrap_or_default() {
                "run_started" => {
                    t.brief = e
                        .get("prompt")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                }
                "scouts_planned" => t.phase = "planning the research",
                "research_completed" | "contracts" => t.phase = "planning the build",
                "plan_loaded" => {
                    t.planned = e
                        .get("task_count")
                        .and_then(Value::as_u64)
                        .or_else(|| {
                            e.get("tasks")
                                .and_then(Value::as_array)
                                .map(|a| a.len() as u64)
                        })
                        .unwrap_or(0) as usize;
                    t.phase = "planning the build";
                }
                "task_dispatched" => {
                    dispatched.push(id);
                    t.phase = "building";
                }
                "task_completed" => {
                    settled.push(id);
                    match e.get("status").and_then(Value::as_str) {
                        Some("done") | None => t.done += 1,
                        _ => t.failed += 1,
                    }
                }
                "task_failed" => {
                    settled.push(id);
                    t.failed += 1;
                }
                "smoke" | "review" | "complete_result" => t.phase = "verifying the app",
                "run_finished" => {
                    t.finished = true;
                    t.phase = "finishing up";
                }
                _ => {}
            }
        }
        dispatched.retain(|d| !settled.contains(d) && !d.is_empty());
        dispatched.sort();
        dispatched.dedup();
        t.in_flight = dispatched;
        t
    }

    /// The failure message a PERSON reads. The old one pasted the last 1500 characters of the engine's
    /// stderr — but stderr is the human PROGRESS log (stdout is reserved for the report JSON), so the chat
    /// got `▸ run cli → local-mihai-…`, phase banners and internal env hints such as
    /// `GOOSE_SWARM_JUDGE=0 to disable` as the explanation for a dead build, usually starting mid-line
    /// because the slice cuts anywhere. It answered none of the four questions a user actually has.
    ///
    /// The run's own record answers all of them, and the full stderr is already teed to
    /// `.swarm/engine-stderr.log`, so nothing is lost by not re-pasting a slice of it here.
    fn failure_summary(&self, code: Option<i32>, stderr: &str) -> String {
        let spawn_dir = self
            .working_dir
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        // The breadcrumb is the only link from the dir we spawned in to where the run actually built —
        // resolve_app_root redirects out of $HOME into ~/goose-builds/<run-id>.
        let run_dir = std::fs::read_to_string(spawn_dir.join(".swarm").join("current-run.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| {
                let dir = v.get("dir").and_then(Value::as_str)?.to_string();
                let id = v.get("run_id").and_then(Value::as_str)?.to_string();
                Some((PathBuf::from(dir), id))
            });

        let (trace, log_line) = match &run_dir {
            Some((dir, id)) => {
                let log = dir.join(".swarm").join(format!("run-{id}.jsonl"));
                let t = std::fs::read_to_string(&log)
                    .map(|s| Self::trace_from_log(&s))
                    .unwrap_or_default();
                (t, format!("\nFull log: `{}`", log.display()))
            }
            None => (RunTrace::default(), String::new()),
        };

        // WHY it stopped. A signal (no exit code) is the Stop button or a kill; a non-zero code is the
        // engine returning an error. Never guess beyond what the status actually says.
        let why = match code {
            None => "It was stopped (interrupted or killed).",
            Some(c) => &format!("The engine exited with error code {c}."),
        };

        let mut out = String::from("**The build stopped before it finished.**\n\n");
        out.push_str(why);
        out.push_str(&format!("\nIt got as far as: {}.", trace.phase));
        if trace.planned > 0 {
            out.push_str(&format!(
                "\nTasks: {} of {} finished",
                trace.done, trace.planned
            ));
            if trace.failed > 0 {
                out.push_str(&format!(", {} failed", trace.failed));
            }
            out.push('.');
        }
        if !trace.in_flight.is_empty() {
            out.push_str(&format!(
                "\nStill running when it stopped: {}.",
                trace.in_flight.join(", ")
            ));
        }
        if let Some((dir, _)) = &run_dir {
            out.push_str(&format!("\n\nWhat it wrote is in `{}`.", dir.display()));
        }
        out.push_str(&log_line);
        // Only when the run left no record at all is stderr worth showing — and then just a short tail, so
        // a spawn failure (the engine never started) is still diagnosable.
        if run_dir.is_none() {
            let tail: String = stderr.lines().rev().take(8).collect::<Vec<_>>().join("\n");
            let tail: String = tail.lines().rev().collect::<Vec<_>>().join("\n");
            if !tail.trim().is_empty() {
                out.push_str(&format!(
                    "\n\nThe engine left no run record. Last output:\n\n```\n{tail}\n```"
                ));
            }
        }
        out
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

/// #ai-session-names (GOOSE_SWARM_AI_NAME env, else `swarm.ai_session_name` in config; DEFAULT ON — the
/// title call is a 25s detached spawn off the reply critical path, so the queue-contention concern is
/// moot and the ugly first-4-words truncation is not worth shipping). When on, a swarm-build session is titled by ONE cheap
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

/// The session-title call is fired at reply START (agent.rs, a DETACHED spawn) — which means it races the build
/// it is naming for the single PARALLEL:1 planner slot. The old 25s timeout LOST that race on every real build
/// (planning holds the slot for minutes), silently falling back to the "Build X — a" truncation. Because the
/// spawn is detached it blocks nothing, so we can afford to WAIT: a generous timeout lets the queued title
/// request get served between build generations (typically within a minute or two) and return the real title.
/// Override with GOOSE_SWARM_NAME_TIMEOUT_SECS; 0 disables the timer entirely (wait indefinitely for the fleet).
fn name_timeout_secs() -> u64 {
    std::env::var("GOOSE_SWARM_NAME_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(600)
}

/// Title the session with ONE local-planner call (thinking OFF via complete_fast). Passes the SAME session-title
/// system+messages straight to the real lmstudio provider, so the planner emits a title and the outer
/// `generate_session_name` strips the reasoning block + picks the short title. Any error/timeout bubbles up so
/// the caller falls back to the truncation.
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
    let call = crate::model_config::complete_fast(
        provider.as_ref(),
        &mc,
        "swarm-name",
        system,
        messages,
        &[],
    );
    let secs = name_timeout_secs();
    let (message, usage) = if secs == 0 {
        call.await?
    } else {
        tokio::time::timeout(std::time::Duration::from_secs(secs), call)
            .await
            .map_err(|_| ProviderError::ExecutionError("session-title call timed out".into()))??
    };
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
        // ELECTRON PARITY (2026-07-22): this block used to force SMOKE/SPLIT/SPLIT_SECS/CONTRACTS/COMPLETE/
        // COMPLETE_CAP_SECS onto the spawned engine because those levers had no config field and were
        // otherwise unreachable from the desktop (`open -n Goose.app` hands the app its own environment via
        // LaunchServices, so env set around the app is discarded).
        //
        // The side effect was that the desktop and the headless CLI ran DIFFERENT engines: these six were
        // always on for a UI build and always off for `goose swarm run`, so the same spec could not be
        // reproduced across the two surfaces and a headless measurement did not describe what users get.
        //
        // They are now real config fields defaulting to exactly these values, resolved by the engine through
        // one path (env > config > default). Desktop behaviour is unchanged; headless now matches it. Forcing
        // them here as well would re-break it, because env BEATS config — a user who turned one off in
        // settings would be silently overridden.
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
            self.failure_summary(output.status.code(), &stderr)
        };

        let message = Message::assistant().with_text(summary);
        Ok(stream_from_single_message(
            message,
            ProviderUsage::new(self.name.clone(), Usage::default()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Event shapes taken from a REAL killed run (~/goose-builds/swarm-.../run-*.jsonl): `plan_loaded`
    /// carries `task_count`, `task_completed` carries `status` + `task_id`.
    const REAL_SHAPE: &str = r#"{"event":"run_started","prompt":"Build a Python CLI expense tracker","working_dir":"/x"}
{"event":"scouts_planned","lenses":["a","b"]}
{"event":"research_completed"}
{"event":"contracts"}
{"event":"plan_loaded","task_count":12,"plan_confidence":88}
{"event":"task_dispatched","task_id":"store"}
{"event":"task_dispatched","task_id":"cli"}
{"event":"task_dispatched","task_id":"models"}
{"event":"task_completed","task_id":"cli","status":"done"}
{"event":"judge_verdict","task_id":"store"}
"#;

    #[test]
    fn trace_reads_how_far_the_run_actually_got() {
        let t = SwarmProvider::trace_from_log(REAL_SHAPE);
        assert_eq!(t.planned, 12);
        assert_eq!(t.done, 1);
        assert_eq!(t.failed, 0);
        assert_eq!(t.phase, "building");
        // Dispatched but never settled — what was in flight when it stopped.
        assert_eq!(t.in_flight, vec!["models".to_string(), "store".to_string()]);
        assert!(!t.finished);
        assert!(t.brief.contains("expense tracker"));
    }

    /// A garbage or partial line must never break the summary — the log is read while it is still being
    /// written, so the last line is routinely truncated.
    #[test]
    fn trace_tolerates_garbage_and_a_truncated_last_line() {
        let t = SwarmProvider::trace_from_log(
            "not json\n{\"event\":\"plan_loaded\",\"task_count\":3}\n{\"event\":\"task_comp",
        );
        assert_eq!(t.planned, 3);
        assert_eq!(t.done, 0);
    }

    #[test]
    fn a_failed_task_is_counted_as_failed_not_done() {
        let t = SwarmProvider::trace_from_log(
            "{\"event\":\"plan_loaded\",\"task_count\":2}\n\
             {\"event\":\"task_completed\",\"task_id\":\"a\",\"status\":\"failed\"}\n\
             {\"event\":\"task_completed\",\"task_id\":\"b\",\"status\":\"done\"}",
        );
        assert_eq!(t.done, 1);
        assert_eq!(t.failed, 1);
        assert!(t.in_flight.is_empty());
    }

    /// The whole point: the message must NOT be the engine's progress log. It answers what happened, how
    /// far it got, and where the files are — and never pastes phase banners or env-var hints.
    #[test]
    fn failure_summary_is_a_verdict_not_a_log_dump() {
        let p = SwarmProvider {
            name: "swarm".into(),
            command: "goose".into(),
            working_dir: Some(std::env::temp_dir().join("goose_no_such_run_dir")),
        };
        let noisy = "▶  EXECUTE   subtasks run IN PARALLEL across the fleet\n\
                     idle-model judge: on (GOOSE_SWARM_JUDGE=0 to disable)\n\
                     ▸ run cli → local-mihai-qwopus3.6-27b-coder-mt";
        let out = p.failure_summary(None, noisy);
        assert!(out.starts_with("**The build stopped before it finished.**"));
        assert!(out.contains("stopped (interrupted or killed)"));
        // No breadcrumb here, so a SHORT tail is allowed as the last resort — but never the old 1500-char
        // mid-line slice presented as the explanation.
        assert!(out.contains("left no run record"));
        assert!(!out.contains("GOOSE_SWARM_JUDGE=0 to disable\n▸ run cli") || out.len() < 900);
        let exited = p.failure_summary(Some(2), "");
        assert!(exited.contains("error code 2"));
    }
}
