mod benchmark;
mod commands;
mod desktop;
pub use desktop::DesktopBridge;

use super::cursors::BenchmarkLogSource;
use crate::session::work_catalog::{CatalogRecord, ExternalWork, WorkCatalog};
use leanzero_link::{
    controller::ControllerError,
    controller_wire::*,
    jobs::JobRegistry,
    load::{JobKind, JobState},
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

pub const LOG_PAGE_BYTES: u64 = 128 * 1024;

pub(crate) struct ExternalLogPage {
    pub chunk: leanzero_link::jobs::JobLogChunk,
    pub benchmark_source: Option<BenchmarkLogSource>,
}

pub struct WorkAdapters {
    pub jobs: Arc<JobRegistry>,
    pub config_dir: PathBuf,
    pub desktop: Option<DesktopBridge>,
    control: Mutex<()>,
    benchmark_note_supported: std::sync::RwLock<Option<String>>,
    benchmark_receipt_offsets: Mutex<std::collections::HashMap<benchmark::ReceiptScanKey, u64>>,
}

impl WorkAdapters {
    pub fn new(
        jobs: Arc<JobRegistry>,
        config_dir: PathBuf,
        desktop: Option<DesktopBridge>,
    ) -> Self {
        Self {
            jobs,
            config_dir,
            desktop,
            control: Mutex::new(()),
            benchmark_note_supported: std::sync::RwLock::new(None),
            benchmark_receipt_offsets: Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub async fn refresh(&self, catalog: &WorkCatalog) -> Result<(), ControllerError> {
        let mut shell_locators = Vec::new();
        for job in self
            .jobs
            .list()
            .into_iter()
            .filter(|job| job.brief.kind == JobKind::Shell)
        {
            shell_locators.push(job.brief.job_id.clone());
            let state = match job.brief.state {
                JobState::Queued => "idle",
                JobState::Running | JobState::Cancelling => "running",
                JobState::Done => "finished",
                JobState::Failed | JobState::Killed => "failed",
                JobState::Cancelled | JobState::Lost => "interrupted",
            };
            catalog
                .upsert_external(&ExternalWork {
                    log_bytes: self.jobs.log_length(&job.brief.job_id).map_err(error)?,
                    kind: "shell_job".into(),
                    locator: job.brief.job_id.clone(),
                    title: format!("Shell job {}", job.brief.job_id),
                    working_dir: String::new(),
                    state: state.into(),
                    run_key: (!job.brief.state.is_terminal())
                        .then(|| job.brief.started_at.to_rfc3339()),
                    updated_at: job.ended_at.unwrap_or(job.brief.started_at).to_rfc3339(),
                    event_kind: format!(
                        "shell_{}",
                        serde_json::to_value(job.brief.state)
                            .map_err(error)?
                            .as_str()
                            .ok_or_else(|| error("job state is not a string"))?
                    ),
                })
                .await
                .map_err(error)?;
        }
        catalog
            .remove_missing_external("shell_job", &shell_locators)
            .await
            .map_err(error)?;
        if let Some(registry) = read_json_optional(&self.config_dir.join("agent-work.json")).await?
        {
            let mut agent_locators = Vec::new();
            let agents = registry["agents"]
                .as_array()
                .ok_or_else(|| error("Agent Work registry has no agents array"))?;
            for agent in agents {
                let dir = expand_path(field(agent, "dir")?)?;
                agent_locators.push(dir.to_string_lossy().into_owned());
                let state_path = dir.join(".swarm/agent/state.json");
                let state = read_json_optional(&state_path).await?;
                let (title, status, run_key, updated_at, event_kind) = if let Some(state) = state {
                    let raw_status = field(&state, "status")?;
                    let active = agent_alive(&dir).await?;
                    let status = match raw_status {
                        "paused" if active => "paused",
                        "ticking" if active => "running",
                        "holding" if active => "waiting_input",
                        "idle" | "waiting" if active => "idle",
                        "stopped" => "finished",
                        _ => "interrupted",
                    };
                    (
                        field(&state, "title")?.to_owned(),
                        status.to_owned(),
                        if active {
                            Some(field(&state, "run_id")?.to_owned())
                        } else {
                            None
                        },
                        field(&state, "updated_at")?.to_owned(),
                        format!("agent_{raw_status}"),
                    )
                } else {
                    (
                        dir.file_name()
                            .ok_or_else(|| error("Agent Work directory has no name"))?
                            .to_string_lossy()
                            .into_owned(),
                        "idle".into(),
                        None,
                        field(agent, "addedAt")?.to_owned(),
                        "agent_not_started".into(),
                    )
                };
                catalog
                    .upsert_external(&ExternalWork {
                        log_bytes: file_length(&dir.join(".swarm/agent/engine.log")).await?,
                        kind: "agent_work".into(),
                        locator: dir.to_string_lossy().into_owned(),
                        title,
                        working_dir: dir.to_string_lossy().into_owned(),
                        state: status,
                        run_key,
                        updated_at,
                        event_kind,
                    })
                    .await
                    .map_err(error)?;
            }
            catalog
                .remove_missing_external("agent_work", &agent_locators)
                .await
                .map_err(error)?;
        }
        let benchmark = match &self.desktop {
            Some(desktop) => Some(desktop.call("benchmark.catalog", json!({})).await?),
            None => read_json_optional(&self.config_dir.join("benchmark/sessions.json")).await?,
        };
        if let Some(benchmark) = benchmark {
            let mut benchmark_locators = Vec::new();
            let mut note_supported = None;
            let sessions = benchmark["sessions"]
                .as_array()
                .ok_or_else(|| error("benchmark index has no sessions array"))?;
            for session in sessions {
                let started = field(session, "startedAt")?;
                let slot = session["slotDir"].as_str().unwrap_or("");
                let run_id = session["runId"].as_str();
                benchmark_locators.push(
                    serde_json::to_string(&json!({"slot_dir":slot,"started_at":started}))
                        .map_err(error)?,
                );
                let active = benchmark.get("active").filter(|active| {
                    run_id.is_some()
                        && active["runId"].as_str() == run_id
                        && active["startedAt"] == started
                });
                if active.is_some_and(|active| active["noteSupported"].as_bool() == Some(true)) {
                    note_supported = Some(
                        serde_json::to_string(&json!({"slot_dir":slot,"started_at":started}))
                            .map_err(error)?,
                    );
                }
                let state = match (active, session["outcome"].as_str()) {
                    (Some(_), _) => "running",
                    (_, Some("finished")) => "finished",
                    (_, Some("did_not_start" | "did_not_finish")) => "failed",
                    _ => "interrupted",
                };
                catalog
                    .upsert_external(&ExternalWork {
                        log_bytes: active.and_then(|active|active["logBytes"].as_u64()),
                        kind: "benchmark".into(),
                        locator: serde_json::to_string(
                            &json!({"slot_dir":slot,"started_at":started}),
                        )
                        .map_err(error)?,
                        title: format!("Benchmark {}", run_id.unwrap_or(started)),
                        working_dir: slot.into(),
                        state: state.into(),
                        run_key: active
                            .map(|_| {
                                serde_json::to_string(
                                    &json!({"run_id":run_id,"started_at":started,"activity_started_at":active.and_then(|active|active["activityStartedAt"].as_str())}),
                                )
                            })
                            .transpose()
                            .map_err(error)?,
                        updated_at: session["logActivityStartedAt"]
                            .as_str()
                            .or_else(|| session["endedAt"].as_str())
                            .unwrap_or(started)
                            .into(),
                        event_kind: active
                            .and_then(|a| a["phase"].as_str())
                            .unwrap_or(state)
                            .into(),
                    })
                    .await
                    .map_err(error)?;
            }
            catalog
                .remove_missing_external("benchmark", &benchmark_locators)
                .await
                .map_err(error)?;
            *self
                .benchmark_note_supported
                .write()
                .expect("benchmark capabilities poisoned") = note_supported;
        }
        Ok(())
    }

    pub fn capabilities(&self, record: &CatalogRecord) -> Vec<WorkAction> {
        if record.current_run_id.is_none() {
            return Vec::new();
        }
        match record.kind.as_str() {
            "shell_job" => vec![WorkAction::Stop],
            "agent_work" => vec![
                WorkAction::Note,
                WorkAction::Stop,
                WorkAction::Tick,
                if record.state == "paused" {
                    WorkAction::Resume
                } else {
                    WorkAction::Pause
                },
            ],
            "benchmark" if self.desktop.is_some() => {
                let mut actions = vec![WorkAction::Stop];
                if self
                    .benchmark_note_supported
                    .read()
                    .expect("benchmark capabilities poisoned")
                    .as_deref()
                    == Some(record.local_locator.as_str())
                {
                    actions.push(WorkAction::Note);
                }
                actions
            }
            _ => Vec::new(),
        }
    }

    pub(crate) async fn log_tail(
        &self,
        record: &CatalogRecord,
        end: Option<u64>,
        benchmark_source: Option<&BenchmarkLogSource>,
    ) -> Result<ExternalLogPage, ControllerError> {
        if record.kind == "shell_job" {
            return self
                .jobs
                .read_log_tail(&record.local_locator, end, LOG_PAGE_BYTES)
                .map_err(error)?
                .ok_or_else(|| error("shell job is no longer present"))
                .map(|chunk| ExternalLogPage {
                    chunk,
                    benchmark_source: None,
                });
        }
        if record.kind == "benchmark" {
            let desktop = self
                .desktop
                .as_ref()
                .ok_or_else(|| error("benchmark desktop owner is unavailable"))?;
            let mut params: Value = serde_json::from_str(&record.local_locator).map_err(error)?;
            if let Some(source) = benchmark_source {
                params["activity_started_at"] = json!(source.activity_started_at);
            }
            match end {
                Some(end) => {
                    params["from"] = json!(end.saturating_sub(LOG_PAGE_BYTES));
                    params["to"] = json!(end);
                }
                None => {
                    params["tail_bytes"] = json!(LOG_PAGE_BYTES);
                }
            }
            let result = desktop.call("benchmark.log", params).await?;
            let activity = result
                .get("activity_started_at")
                .ok_or_else(|| error("desktop returned no log source identity"))?;
            let source = BenchmarkLogSource {
                activity_started_at: serde_json::from_value(activity.clone()).map_err(error)?,
            };
            if benchmark_source.is_some_and(|expected| expected != &source) {
                return Err(error("desktop returned a different log source"));
            }
            let chunk: leanzero_link::jobs::JobLogChunk =
                serde_json::from_value(result).map_err(error)?;
            if chunk.next < chunk.from
                || chunk.next - chunk.from > LOG_PAGE_BYTES
                || end.is_some_and(|end| {
                    chunk.next > end || chunk.from < end.saturating_sub(LOG_PAGE_BYTES)
                })
            {
                return Err(error("desktop returned an invalid log interval"));
            }
            return Ok(ExternalLogPage {
                chunk,
                benchmark_source: Some(source),
            });
        }
        if record.kind != "agent_work" {
            return Err(super::unsupported(
                "this work has no readable text log adapter",
            ));
        }
        let path = PathBuf::from(&record.local_locator).join(".swarm/agent/engine.log");
        let done = record.current_run_id.is_none();
        tokio::task::spawn_blocking(move || {
            leanzero_link::jobs::read_log_tail_file(&path, end, LOG_PAGE_BYTES, done)
        })
        .await
        .map_err(error)?
        .map_err(error)
        .map(|chunk| ExternalLogPage {
            chunk,
            benchmark_source: None,
        })
    }
}

pub(super) fn error(error: impl std::fmt::Display) -> ControllerError {
    ControllerError::unavailable(error.to_string())
}
fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str, ControllerError> {
    value[name]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| error(format!("work metadata has no {name}")))
}
async fn file_length(path: &Path) -> Result<Option<u64>, ControllerError> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(error(e)),
    }
}
async fn read_json_optional(path: &Path) -> Result<Option<Value>, ControllerError> {
    match tokio::fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(error),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(error(e)),
    }
}
fn expand_path(path: &str) -> Result<PathBuf, ControllerError> {
    if let Some(tail) = path.strip_prefix("~/") {
        Ok(dirs::home_dir()
            .ok_or_else(|| error("home directory unavailable"))?
            .join(tail))
    } else {
        Ok(PathBuf::from(path))
    }
}
async fn agent_alive(dir: &Path) -> Result<bool, ControllerError> {
    let lock = match tokio::fs::read_to_string(dir.join(".swarm/agent/lock")).await {
        Ok(lock) => lock,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(error(e)),
    };
    let pid: i32 = lock.trim().parse().map_err(error)?;
    if pid <= 0 {
        return Err(error("invalid Agent Work PID"));
    }
    #[cfg(unix)]
    {
        Ok(unsafe { libc::kill(pid, 0) } == 0)
    }
    #[cfg(not(unix))]
    {
        Err(error("Agent Work liveness requires Unix"))
    }
}
