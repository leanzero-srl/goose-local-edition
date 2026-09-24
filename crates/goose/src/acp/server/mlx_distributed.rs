//! ACP surface for the DISTRIBUTED MLX engine (`goose_sidecar::distributed`): status, dry-run
//! preflight, start, stop and config persistence under the `mlx_distributed` config key. Local to
//! this goosed — the Mac that is rank 0 supervises the run — so no request carries a mesh `nodeId`.
//!
//! The single engine's handlers are untouched except for two guards that make "one engine owns a
//! Mac at a time" hold in both directions: `distributedStart` refuses (code
//! `singleEngineMounted`) while the single engine is mounted, and the single engine's `mount`
//! refuses while the distributed engine owns the Mac (`refuse_single_mount_while_distributed`).

use super::*;
use crate::config::ConfigError;
use goose_sidecar::distributed::compaction::NodeCompaction;
use goose_sidecar::distributed::provision::{self, EnvSpec};
use goose_sidecar::distributed::{
    self, supervisor::Liveness, Backend, CheckVerdict, DistributedConfig, DistributedStatus,
    NodeConfig, PreflightReport, RankPlan, StartOutcome, StopReport,
};
use goose_sidecar::distributed::{NodeExec, SystemExec};
use goose_sidecar::engine::{expand_tilde, served_model_id};
use goose_sidecar::GIB;
use std::collections::BTreeMap;
use std::sync::Mutex as StdMutex;

use super::mlx_distributed_discover as discover;
use super::mlx_distributed_link as link;
use crate::providers::mlx_distributed_owner::{self as owner_record, OwnerRecord, PublishedEngine};
use goose_sidecar::distributed::link_control;

const MLX_DISTRIBUTED_CONFIG_KEY: &str = "mlx_distributed";

static LAST_DISTRIBUTED_STATE: StdMutex<String> = StdMutex::new(String::new());

/// The last (or running) provisioning of the nodes' goose-managed Python; read by `distributedStatus`.
static PROVISION: StdMutex<Option<MlxDistributedProvisionDto>> = StdMutex::new(None);

fn gib(bytes: u64) -> f64 {
    bytes as f64 / GIB as f64
}

fn config_from_dto(dto: MlxDistributedConfigDto) -> anyhow::Result<DistributedConfig> {
    let backend = match dto.backend.as_str() {
        "jaccl" => Backend::Jaccl,
        "ring" => Backend::Ring,
        other => anyhow::bail!("backend '{other}' is not \"jaccl\" or \"ring\""),
    };
    Ok(DistributedConfig {
        model_id: dto.model_id,
        backend,
        port: dto.port,
        coordinator_port: dto.coordinator_port,
        context: dto.context,
        slots: dto.slots,
        restart_on_failure: dto.restart_on_failure,
        hang_ratio_only: dto.hang_ratio_only,
        watchdog_warn_ratio: dto.watchdog_warn_ratio,
        watchdog_critical_ratio: dto.watchdog_critical_ratio,
        nodes: dto
            .nodes
            .into_iter()
            .map(|n| NodeConfig {
                name: n.name,
                ssh: n.ssh,
                tb_ip: n.tb_ip,
                tb_netmask: n.tb_netmask,
                tb_interface: n.tb_interface,
                tb_service: n.tb_service,
                rdma_device: n.rdma_device,
                python: n.python,
                pipeline_python: n.pipeline_python,
                model_dir: n.model_dir,
                // Absent is the documented default: ON (the DTO's own doc).
                free_memory_automatically: n.free_memory_automatically.unwrap_or(true),
            })
            .collect(),
    })
}

fn config_to_dto(config: DistributedConfig) -> MlxDistributedConfigDto {
    MlxDistributedConfigDto {
        model_id: config.model_id,
        backend: config.backend.as_str().to_string(),
        port: config.port,
        coordinator_port: config.coordinator_port,
        context: config.context,
        slots: config.slots,
        restart_on_failure: config.restart_on_failure,
        hang_ratio_only: config.hang_ratio_only,
        watchdog_warn_ratio: config.watchdog_warn_ratio,
        watchdog_critical_ratio: config.watchdog_critical_ratio,
        nodes: config
            .nodes
            .into_iter()
            .map(|n| MlxDistributedNodeConfigDto {
                name: n.name,
                ssh: n.ssh,
                tb_ip: n.tb_ip,
                tb_netmask: n.tb_netmask,
                tb_interface: n.tb_interface,
                tb_service: n.tb_service,
                rdma_device: n.rdma_device,
                python: n.python,
                pipeline_python: n.pipeline_python,
                model_dir: n.model_dir,
                free_memory_automatically: Some(n.free_memory_automatically),
            })
            .collect(),
    }
}

fn checks_to_dto(checks: Vec<distributed::Check>) -> Vec<MlxDistributedCheckDto> {
    checks
        .into_iter()
        .map(|c| MlxDistributedCheckDto {
            id: c.id,
            verdict: match c.verdict {
                CheckVerdict::Pass => "pass",
                CheckVerdict::Warn => "warn",
                CheckVerdict::Fail => "fail",
            }
            .to_string(),
            message: c.message,
        })
        .collect()
}

fn plan_to_dto(plan: RankPlan) -> MlxDistributedRankPlanDto {
    MlxDistributedRankPlanDto {
        layer_start: plan.layer_start,
        layer_end: plan.layer_end,
        shard_index: plan.shard_index,
        shard_count: plan.shard_count,
        weights_bytes: plan.weights_bytes,
        state_bytes: plan.state_bytes,
        workspace_bytes: plan.workspace_bytes,
        prompt_cache_bytes: plan.prompt_cache_bytes,
        planned_bytes: plan.planned_bytes,
        with_overhead_bytes: plan.with_overhead_bytes,
        budget_bytes: plan.budget_bytes,
        fits: plan.fits,
    }
}

fn preflight_to_dto(report: PreflightReport) -> MlxDistributedPreflightDto {
    MlxDistributedPreflightDto {
        ok: report.ok,
        ran_at_ms: report.ran_at_ms,
        backend: report.backend.as_str().to_string(),
        runner: report.runner.map(|r| r.as_str().to_string()),
        model_type: report.model_type,
        context_limit: report.context_limit,
        context_source: report.context_source,
        max_context_fits: report.max_context_fits,
        slots: report.slots,
        checks: checks_to_dto(report.checks),
        nodes: report
            .nodes
            .into_iter()
            .map(|n| MlxDistributedNodePreflightDto {
                name: n.name,
                rank: n.rank as u32,
                host: n.host,
                checks: checks_to_dto(n.checks),
                available_bytes: n.available_bytes,
                total_bytes: n.total_bytes,
                pressure: n.pressure,
                plan: n.plan.map(plan_to_dto),
                link_speed: n.link_speed,
                mlx_version: n.mlx_version,
                ceiling_bytes: n.ceiling_bytes,
                wired_limit_mb: n.wired_limit_mb,
                short_bytes: n.short_bytes,
                top_apps: n
                    .top_apps
                    .into_iter()
                    .map(|a| MlxDistributedAppMemoryDto {
                        name: a.name,
                        rss_bytes: a.rss_bytes,
                    })
                    .collect(),
            })
            .collect(),
        repairs: report.repairs,
    }
}

fn compaction_to_dto(record: NodeCompaction) -> MlxDistributedCompactionDto {
    let mut dto = MlxDistributedCompactionDto {
        node: record.node,
        at_ms: record.at_ms,
        trigger: record.trigger,
        ..Default::default()
    };
    match (record.report, record.refusal, record.error) {
        (Some(report), _, _) => {
            dto.outcome = "compacted".to_string();
            dto.message = report.summary();
            dto.total_bytes = Some(report.total_bytes);
            dto.before_available_bytes = Some(report.before_available_bytes);
            dto.peak_available_bytes = Some(report.peak_available_bytes);
            dto.settled_available_bytes = Some(report.settled_available_bytes);
            dto.gained_bytes = Some(report.gained_bytes);
            dto.end = Some(report.end.as_str().to_string());
            dto.settle_samples = Some(report.settle_samples as u32);
        }
        (None, Some(refusal), _) => {
            dto.outcome = "refused".to_string();
            dto.code = Some(refusal.code);
            dto.message = refusal.message;
        }
        (None, None, Some(error)) => {
            dto.outcome = "failed".to_string();
            dto.message = error;
        }
        (None, None, None) => {
            dto.outcome = "failed".to_string();
            dto.message = "the compaction recorded no outcome".to_string();
        }
    }
    dto
}

fn liveness_to_dto(liveness: Liveness) -> MlxDistributedLivenessDto {
    MlxDistributedLivenessDto {
        samples: liveness.samples as u32,
        median_ms: liveness.median_ms,
        bound_ms: liveness.bound_ms,
        silent_ms: liveness.silent_ms,
    }
}

fn status_to_dto(
    status: DistributedStatus,
    persisted: Option<DistributedConfig>,
) -> MlxDistributedStatusDto {
    MlxDistributedStatusDto {
        mode: status.mode().to_string(),
        state: status.state.as_str().to_string(),
        backend: status.backend.map(|b| b.as_str().to_string()),
        runner: status.runner.map(|r| r.as_str().to_string()),
        model_id: status.model_id,
        served_model_id: status.served_model_id,
        base_url: status.base_url,
        context_limit: status.context_limit,
        admission_open: status.admission_open,
        inflight: status.inflight,
        waiting: status.waiting,
        slots: status.slots,
        slots_in_use: status.slots_in_use,
        sequences_in_flight: status.sequences_in_flight,
        server_status_error: status.server_status_error,
        liveness: status.liveness.map(liveness_to_dto),
        nodes: status
            .nodes
            .into_iter()
            .map(|n| MlxDistributedNodeStatusDto {
                name: n.name,
                rank: n.rank as u32,
                role: n.role,
                host: n.host,
                state: n.state.as_str().to_string(),
                pid: n.pid,
                layer_start: n.layer_start,
                layer_end: n.layer_end,
                shard_index: n.shard_index,
                shard_count: n.shard_count,
                available_memory_gb: n.available_bytes.map(gib),
                total_memory_gb: n.total_bytes.map(gib),
                pressure: n.pressure,
                memory_error: n.memory_error,
                active_memory_gb: n.active_bytes.map(gib),
                peak_memory_gb: n.peak_bytes.map(gib),
                planned_memory_gb: n.planned_bytes.map(gib),
                memory_limit_gb: n.memory_limit_bytes.map(gib),
                wired_limit_gb: n.wired_limit_bytes.map(gib),
                cache_limit_gb: n.cache_limit_bytes.map(gib),
                kv_reserved_gb: n.kv_reserved_bytes.map(gib),
                kv_budget_gb: n.kv_budget_bytes.map(gib),
                link: MlxDistributedLinkDto {
                    backend: n.backend.as_str().to_string(),
                    tb_ip: n.tb_ip,
                    interface: n.tb_interface,
                    speed: n.link_speed,
                },
            })
            .collect(),
        last_preflight: status.last_preflight.map(preflight_to_dto),
        events: status
            .events
            .into_iter()
            .map(|e| MlxDistributedEventDto {
                at_ms: e.at_ms,
                kind: e.kind.as_str().to_string(),
                node: e.node,
                message: e.message,
            })
            .collect(),
        restarts: status.restarts,
        last_error: status.last_error,
        config: status.config.or(persisted).map(config_to_dto),
        provision: None,
        owner: None,
        hosting: None,
        allow_distributed_node: false,
        compactions: status
            .compactions
            .into_iter()
            .map(compaction_to_dto)
            .collect(),
    }
}

fn stop_to_dto(report: StopReport) -> MlxDistributedStopReportDto {
    MlxDistributedStopReportDto {
        steps: report.steps,
        verified: report.verified,
    }
}

pub(super) fn persisted_config() -> Result<Option<DistributedConfig>, agent_client_protocol::Error> {
    match Config::global().get_param::<DistributedConfig>(MLX_DISTRIBUTED_CONFIG_KEY) {
        Ok(config) => Ok(Some(config)),
        Err(ConfigError::NotFound(_)) => Ok(None),
        Err(e) => Err(e).internal_err_ctx("reading mlx_distributed config"),
    }
}

fn persist_config(config: &DistributedConfig) -> Result<(), agent_client_protocol::Error> {
    Config::global()
        .set_param(MLX_DISTRIBUTED_CONFIG_KEY, config)
        .internal_err_ctx("persisting mlx_distributed config")
}

/// The request's config, else the persisted one; validated, and never on the single engine's port.
fn resolve_config(
    dto: Option<MlxDistributedConfigDto>,
) -> Result<DistributedConfig, agent_client_protocol::Error> {
    let config = match dto {
        Some(dto) => config_from_dto(dto).invalid_params_err()?,
        None => persisted_config()?.ok_or_else(|| {
            agent_client_protocol::Error::invalid_params().data(
                "no distributed config: send `config`, or save one with distributedConfigUpdate",
            )
        })?,
    };
    config.validate().invalid_params_err()?;
    let single_port = super::mlx_engine::load_engine_settings()?.port;
    if config.port == single_port {
        return Err(agent_client_protocol::Error::invalid_params().data(format!(
            "port {single_port} is the single MLX engine's port; the distributed engine serves on its own"
        )));
    }
    Ok(config)
}

/// The single engine's mount path calls this: while the distributed engine owns the Mac, a mount
/// is refused with a named reason (never a silent stop of either engine).
pub(super) async fn refuse_single_mount_while_distributed(
) -> Result<(), agent_client_protocol::Error> {
    link::reclaim_orphaned_hosting().await;
    if let Some(refusal) = link::hosting_refusal() {
        return Err(agent_client_protocol::Error::invalid_params().data(refusal));
    }
    let status = distributed::global_manager().status();
    if status.state.owns_the_mac() {
        return Err(agent_client_protocol::Error::invalid_params().data(format!(
            "distributedEngineActive: the distributed MLX engine is {} with '{}' on this Mac; one \
             engine owns a Mac at a time — stop it before mounting",
            status.state.as_str(),
            status.model_id.as_deref().unwrap_or("<model not reported>")
        )));
    }
    if let OwnerRecord::Other(engine) = owner_record::read() {
        return Err(agent_client_protocol::Error::invalid_params().data(format!(
            "distributedEngineActive: {}; one engine owns a Mac at a time — stop it from that window \
             before mounting",
            owned_elsewhere(&engine)
        )));
    }
    Ok(())
}

const OWNED_BY_ANOTHER_WINDOW: &str = "ownedByAnotherWindow";

fn owned_elsewhere(engine: &PublishedEngine) -> String {
    format!(
        "the distributed MLX engine serving '{}' at {} is owned by another window (goosed pid {})",
        engine.served_model_id, engine.base_url, engine.pid
    )
}

/// The record's fact for THIS goosed's readers, with the engine's own answer on /v1/models as the
/// liveness measure — the way every goosed finds the single engine on its port.
async fn owner_dto(record: OwnerRecord) -> Option<MlxDistributedOwnerDto> {
    let with =
        |engine: PublishedEngine, state: &str, detail: Option<String>| MlxDistributedOwnerDto {
            state: state.to_string(),
            pid: Some(engine.pid),
            base_url: Some(engine.base_url),
            served_model_id: Some(engine.served_model_id),
            model_id: Some(engine.model_id),
            backend: Some(engine.backend),
            node_names: engine.node_names,
            detail,
        };
    match record {
        OwnerRecord::Absent | OwnerRecord::Mine(_) => None,
        OwnerRecord::Stale(engine) => Some(with(engine, "stale", None)),
        OwnerRecord::Unreadable { path, error } => Some(MlxDistributedOwnerDto {
            state: "unreadable".to_string(),
            detail: Some(format!("{}: {error}", path.display())),
            ..Default::default()
        }),
        OwnerRecord::Other(engine) => {
            let url = format!("{}/v1/models", engine.base_url);
            let answer = match reqwest::Client::new().get(&url).send().await {
                Ok(resp) => match resp.text().await {
                    Ok(body) => goose_sidecar::engine::parse_model_info(&body)
                        .map(|(served, _, _)| served)
                        .map_err(|e| format!("GET {url}: {e:#}")),
                    Err(e) => Err(format!("GET {url} body unreadable ({e})")),
                },
                Err(e) => Err(format!("GET {url} failed ({e})")),
            };
            Some(match answer {
                Ok(Some(served)) if served == engine.served_model_id => {
                    with(engine, "answering", None)
                }
                Ok(served) => {
                    let detail = format!(
                        "{url} lists {:?}, not the published '{}'",
                        served, engine.served_model_id
                    );
                    with(engine, "notAnswering", Some(detail))
                }
                Err(e) => with(engine, "notAnswering", Some(e)),
            })
        }
    }
}

/// `goose serve`'s exit path: stop a supervised distributed run (verified, per pid) so no 20 GB
/// rank outlives goosed on either Mac.
pub(super) async fn shutdown_distributed_engine() -> String {
    let hosted = goose_sidecar::distributed::link_host::shutdown().await;
    format!("{}; {hosted}", shutdown_supervised_engine().await)
}

async fn shutdown_supervised_engine() -> String {
    let manager = distributed::global_manager();
    let state = manager.status().state;
    if !state.owns_the_mac() {
        withdraw_owner_record();
        return format!("nothing supervised (state '{}')", state.as_str());
    }
    let report = manager.stop().await;
    withdraw_owner_record();
    format!(
        "{} ({}): {}",
        if report.verified {
            "stopped, verified"
        } else {
            "stopped, NOT verified"
        },
        state.as_str(),
        report.steps.join("; ")
    )
}

/// A failed withdraw leaves a record other windows would trust while this goosed lives — logged
/// loudly; it turns `stale` the moment this process exits.
fn withdraw_owner_record() {
    if let Err(e) = owner_record::withdraw_if_mine() {
        warn!(error = %e, path = %owner_record::record_path().display(), "withdrawing the distributed engine's owner record failed");
    }
}

async fn status_response(
) -> Result<MlxEngineDistributedStatusResponse, agent_client_protocol::Error> {
    let status = distributed::global_manager().status();
    if !status.state.owns_the_mac() {
        // This goosed's run is over (stopped, failed, never started): its record, if any, goes.
        withdraw_owner_record();
    }
    let owner = if status.state.owns_the_mac() {
        None
    } else {
        owner_dto(owner_record::read()).await
    };
    let persisted = if status.config.is_none() {
        persisted_config()?
    } else {
        None
    };
    let mut status = status_to_dto(status, persisted);
    status.provision = PROVISION.lock().unwrap().clone();
    status.owner = owner;
    status.hosting = link::hosting_dto();
    status.allow_distributed_node = link::distributed_node_allowed();
    Ok(MlxEngineDistributedStatusResponse { status })
}

/// The provisioning jobs a config asks for: every node's `python` (and `pipelinePython`) that is a
/// goose-managed env. An operator's own interpreter is reported `skipped`, never touched.
fn provision_jobs(
    config: &DistributedConfig,
) -> Vec<(MlxDistributedProvisionNodeDto, Option<EnvSpec>)> {
    let started_ms = goose_sidecar::distributed::preflight::now_ms();
    let mut jobs = Vec::new();
    for (rank, node) in config.nodes.iter().enumerate() {
        let pythons = std::iter::once(node.python.clone()).chain(node.pipeline_python.clone());
        for python in pythons {
            let spec = EnvSpec::managed_by(&python);
            let row = MlxDistributedProvisionNodeDto {
                rank: rank as u32,
                name: node.name.clone(),
                host: node.ssh.clone(),
                python: python.clone(),
                state: if spec.is_some() { "running" } else { "skipped" }.to_string(),
                step: None,
                detail: match &spec {
                    Some(spec) => format!("{} — {}", spec.name, spec.packages.join(" ")),
                    None => {
                        "the operator's own interpreter (Advanced) — goose does not provision it"
                            .to_string()
                    }
                },
                lines: Vec::new(),
                started_ms,
                finished_ms: spec.is_none().then_some(started_ms),
            };
            jobs.push((row, spec));
        }
    }
    jobs
}

fn update_provision(index: usize, f: impl FnOnce(&mut MlxDistributedProvisionNodeDto)) {
    let mut guard = PROVISION.lock().unwrap();
    let Some(run) = guard.as_mut() else { return };
    if let Some(row) = run.nodes.get_mut(index) {
        f(row);
    }
    if run.nodes.iter().all(|n| n.state != "running") {
        run.state = if run.nodes.iter().any(|n| n.state == "failed") {
            "failed"
        } else {
            "done"
        }
        .to_string();
        run.finished_ms
            .get_or_insert_with(goose_sidecar::distributed::preflight::now_ms);
    }
}

async fn run_provision_job(index: usize, host: Option<String>, spec: EnvSpec) {
    let on_line = |line: &str| {
        let progress = provision::parse_progress(line);
        update_provision(index, |row| {
            row.lines.push(line.to_string());
            if let Some(p) = progress {
                row.step = Some(p.step.clone());
                row.detail = p.detail;
            }
        });
    };
    // A LeanZero Link node builds the env itself from its own pins; ssh and this Mac run the script.
    let result = match link_control::link_peer(host.as_deref()) {
        Some(peer) => link_control::provision(peer, &spec, on_line).await,
        None => {
            let script = provision::provision_script(&spec);
            provision::run_streaming(host.as_deref(), &script, on_line).await
        }
    };
    update_provision(index, |row| {
        let finished = goose_sidecar::distributed::preflight::now_ms();
        row.finished_ms = Some(finished);
        match (&result, row.step.as_deref()) {
            (Ok(Some(0)), Some("done")) => row.state = "done".to_string(),
            (Ok(code), _) => {
                row.state = "failed".to_string();
                if row.step.as_deref() != Some("fail") {
                    row.detail = format!(
                        "the provisioning script exited {code:?} without finishing{}",
                        if code == &Some(255) {
                            " (ssh failed)"
                        } else {
                            ""
                        }
                    );
                }
            }
            (Err(e), _) => {
                row.state = "failed".to_string();
                row.detail = format!("{e:#}");
            }
        }
    });
}

impl GooseAcpAgent {
    pub(super) async fn on_mlx_engine_distributed_status(
        &self,
        _req: MlxEngineDistributedStatusRequest,
    ) -> Result<MlxEngineDistributedStatusResponse, agent_client_protocol::Error> {
        link::ensure_link_transport();
        super::mlx_engine::align_omlx_host_env();
        let response = status_response().await?;
        let entered_serving = {
            let mut last = LAST_DISTRIBUTED_STATE.lock().unwrap();
            let serving = matches!(response.status.state.as_str(), "ready" | "serving");
            let was_serving = matches!(last.as_str(), "ready" | "serving");
            *last = response.status.state.clone();
            serving && !was_serving
        };
        if entered_serving {
            // The omlx provider now reaches the distributed engine (OMLX_HOST follows it); its
            // model inventory must list the distributed model id before the next model switch.
            if let Err(e) = self
                .start_provider_inventory_refresh(&["omlx".to_string()])
                .await
            {
                warn!(error = ?e, "omlx inventory refresh after the distributed engine came up failed");
            }
        }
        Ok(response)
    }

    pub(super) async fn on_mlx_engine_distributed_preflight(
        &self,
        req: MlxEngineDistributedPreflightRequest,
    ) -> Result<MlxEngineDistributedPreflightResponse, agent_client_protocol::Error> {
        link::ensure_link_transport();
        let config = resolve_config(req.config)?;
        let report = distributed::global_manager()
            .preflight(&config, req.repair_link)
            .await
            .invalid_params_err()?;
        Ok(MlxEngineDistributedPreflightResponse {
            preflight: preflight_to_dto(report),
        })
    }

    pub(super) async fn on_mlx_engine_distributed_start(
        &self,
        req: MlxEngineDistributedStartRequest,
    ) -> Result<MlxEngineDistributedStartResponse, agent_client_protocol::Error> {
        link::ensure_link_transport();
        link::reclaim_orphaned_hosting().await;
        if let Some(refusal) = link::hosting_refusal() {
            return Ok(MlxEngineDistributedStartResponse {
                started: false,
                refusal: Some(MlxDistributedRefusalDto {
                    code: "hostingRank".to_string(),
                    message: refusal,
                }),
                preflight: None,
            });
        }
        if let OwnerRecord::Other(engine) = owner_record::read() {
            return Ok(MlxEngineDistributedStartResponse {
                started: false,
                refusal: Some(MlxDistributedRefusalDto {
                    code: OWNED_BY_ANOTHER_WINDOW.to_string(),
                    message: format!(
                        "{}; start and stop it from that window",
                        owned_elsewhere(&engine)
                    ),
                }),
                preflight: None,
            });
        }
        let given = req.config.is_some();
        let config = resolve_config(req.config)?;
        if given {
            persist_config(&config)?;
        }

        // One naming rule for both engines: the swarm node that names this Mac's MLX engine
        // (`mihai-mlx` → `mihai-qwen3.8-…`) must find the SAME id whichever engine owns the Mac.
        let served = served_model_id(
            &super::mlx_engine::load_engine_settings()?,
            &config.model_id,
        );
        let published = PublishedEngine {
            pid: std::process::id(),
            base_url: config.base_url(),
            served_model_id: served.clone(),
            model_id: config.model_id.clone(),
            backend: config.backend.as_str().to_string(),
            node_names: config.nodes.iter().map(|n| n.name.clone()).collect(),
        };
        let outcome = distributed::global_manager()
            .start(config, served)
            .await
            .invalid_params_err()?;
        if matches!(outcome, StartOutcome::Started { .. }) {
            // Every other goosed on this Mac (another window) finds the run through this record.
            if let Err(e) = owner_record::publish(&published) {
                warn!(error = %e, "publishing the distributed engine's owner record failed; other windows will not find it");
            }
        }
        super::mlx_engine::align_omlx_host_env();
        Ok(match outcome {
            StartOutcome::Started { preflight } => MlxEngineDistributedStartResponse {
                started: true,
                refusal: None,
                preflight: Some(preflight_to_dto(preflight)),
            },
            StartOutcome::Refused {
                code,
                message,
                preflight,
            } => MlxEngineDistributedStartResponse {
                started: false,
                refusal: Some(MlxDistributedRefusalDto {
                    code: code.as_str().to_string(),
                    message,
                }),
                preflight: preflight.map(preflight_to_dto),
            },
        })
    }

    pub(super) async fn on_mlx_engine_distributed_stop(
        &self,
        _req: MlxEngineDistributedStopRequest,
    ) -> Result<MlxEngineDistributedStopResponse, agent_client_protocol::Error> {
        link::ensure_link_transport();
        let manager = distributed::global_manager();
        if !manager.owns_the_mac() {
            // Without a run of its own, stop sweeps the configured nodes for goose ranks by marker
            // — which would kill another window's live run. Only its owner stops it.
            if let OwnerRecord::Other(engine) = owner_record::read() {
                return Err(agent_client_protocol::Error::invalid_params().data(format!(
                    "{OWNED_BY_ANOTHER_WINDOW}: {}; stop it from that window",
                    owned_elsewhere(&engine)
                )));
            }
        }
        if manager.status().config.is_none() {
            manager.set_config(persisted_config()?);
        }
        let report = manager.stop().await;
        withdraw_owner_record();
        super::mlx_engine::align_omlx_host_env();
        Ok(MlxEngineDistributedStopResponse {
            stop: stop_to_dto(report),
            status: status_response().await?.status,
        })
    }

    pub(super) async fn on_mlx_engine_distributed_make_room(
        &self,
        req: MlxEngineDistributedMakeRoomRequest,
    ) -> Result<MlxEngineDistributedMakeRoomResponse, agent_client_protocol::Error> {
        link::ensure_link_transport();
        // Another window's run owns this Mac's ranks: its loaded models are never pressured.
        if let OwnerRecord::Other(engine) = owner_record::read() {
            return Err(agent_client_protocol::Error::invalid_params().data(format!(
                "{OWNED_BY_ANOTHER_WINDOW}: {}; a node holding a loaded model is never compacted",
                owned_elsewhere(&engine)
            )));
        }
        let config = resolve_config(req.config)?;
        let record = distributed::global_manager()
            .make_room(&config, &req.node)
            .await
            .invalid_params_err()?;
        Ok(MlxEngineDistributedMakeRoomResponse {
            compaction: compaction_to_dto(record),
            status: status_response().await?.status,
        })
    }

    pub(super) async fn on_mlx_engine_distributed_peer_candidates(
        &self,
        _req: MlxEngineDistributedPeerCandidatesRequest,
    ) -> Result<MlxEngineDistributedPeerCandidatesResponse, agent_client_protocol::Error> {
        link::ensure_link_transport();
        let path = dirs::home_dir()
            .ok_or_else(|| {
                agent_client_protocol::Error::internal_error().data("no home directory")
            })?
            .join(".ssh/config");
        let source = path.display().to_string();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(MlxEngineDistributedPeerCandidatesResponse {
                    candidates: Vec::new(),
                    source: format!("{source} (absent)"),
                    link: link::link_discovery().await,
                })
            }
            Err(e) => return Err(e).internal_err_ctx("reading ~/.ssh/config"),
        };
        let exec = SystemExec;
        let candidates =
            futures::future::join_all(discover::ssh_config_hosts(&text).into_iter().map(|alias| {
                let exec = &exec;
                async move {
                    // A git host (bitbucket.org) accepts the key, ignores the command and exits 0
                    // with a banner — measured 2026-09-24. Only a shell that RAN the command
                    // prints the marker first.
                    let answer = exec
                        .run(Some(&alias), "echo GOOSE_PEER_SHELL; /bin/hostname -s")
                        .await;
                    let (answered, detail) = match answer {
                        Ok(out) if out.success() => {
                            match discover::peer_shell_answer(&out.stdout) {
                                Some(host) => (true, host),
                                None => (
                                    false,
                                    format!(
                                        "answered without running a shell command: {}",
                                        out.stdout.trim().chars().take(120).collect::<String>()
                                    ),
                                ),
                            }
                        }
                        Ok(out) => (false, out.stderr.trim().to_string()),
                        Err(e) => (false, format!("{e:#}")),
                    };
                    MlxDistributedPeerCandidateDto {
                        alias,
                        answered,
                        detail,
                    }
                }
            }))
            .await;
        Ok(MlxEngineDistributedPeerCandidatesResponse {
            candidates,
            source,
            link: link::link_discovery().await,
        })
    }

    pub(super) async fn on_mlx_engine_distributed_discover(
        &self,
        req: MlxEngineDistributedDiscoverRequest,
    ) -> Result<MlxEngineDistributedDiscoverResponse, agent_client_protocol::Error> {
        link::ensure_link_transport();
        let mut peers: Vec<String> = Vec::new();
        for peer in req.peers.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
            if !peers.iter().any(|p| p == peer) {
                peers.push(peer.to_string());
            }
        }
        if peers.is_empty() {
            return Err(agent_client_protocol::Error::invalid_params().data(
                "name at least one peer (a LeanZero Link Mac `link:<node>` or an ssh alias such \
                 as `workhorse`)",
            ));
        }
        let settings = super::mlx_engine::load_engine_settings()?;
        let goose_models_dir = expand_tilde(&settings.models_dir).display().to_string();
        let persisted = persisted_config()?;
        let mut roots: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();
        roots
            .entry(None)
            .or_default()
            .push(goose_models_dir.clone());
        for node in persisted.iter().flat_map(|c| c.nodes.iter()) {
            let parent = std::path::Path::new(&node.model_dir)
                .parent()
                .map(|p| p.display().to_string());
            if let Some(parent) = parent {
                roots.entry(node.ssh.clone()).or_default().push(parent);
            }
        }
        let context = discover::Context {
            single_port: settings.port,
            goose_models_dir,
            preferred_model: req
                .model_id
                .or_else(|| persisted.as_ref().map(|c| c.model_id.clone())),
        };
        let discovery = discover::discover(Arc::new(SystemExec), &peers, &roots, &context).await;
        Ok(MlxEngineDistributedDiscoverResponse { discovery })
    }

    pub(super) async fn on_mlx_engine_distributed_provision(
        &self,
        req: MlxEngineDistributedProvisionRequest,
    ) -> Result<MlxEngineDistributedProvisionResponse, agent_client_protocol::Error> {
        link::ensure_link_transport();
        let config = resolve_config(req.config)?;
        let jobs = provision_jobs(&config);
        let snapshot = {
            let mut guard = PROVISION.lock().unwrap();
            if guard.as_ref().is_some_and(|p| p.state == "running") {
                return Err(agent_client_protocol::Error::invalid_params().data(
                    "provisioning is already running; its progress is on distributedStatus",
                ));
            }
            let started_ms = goose_sidecar::distributed::preflight::now_ms();
            let all_skipped = jobs.iter().all(|(_, spec)| spec.is_none());
            let run = MlxDistributedProvisionDto {
                state: if all_skipped { "done" } else { "running" }.to_string(),
                started_ms,
                finished_ms: all_skipped.then_some(started_ms),
                nodes: jobs.iter().map(|(row, _)| row.clone()).collect(),
            };
            *guard = Some(run.clone());
            run
        };
        for (index, (row, spec)) in jobs.into_iter().enumerate() {
            if let Some(spec) = spec {
                tokio::spawn(run_provision_job(index, row.host, spec));
            }
        }
        Ok(MlxEngineDistributedProvisionResponse {
            provision: snapshot,
        })
    }

    pub(super) async fn on_mlx_engine_distributed_config_update(
        &self,
        req: MlxEngineDistributedConfigUpdateRequest,
    ) -> Result<MlxEngineDistributedConfigResponse, agent_client_protocol::Error> {
        let config = resolve_config(Some(req.config))?;
        persist_config(&config)?;
        distributed::global_manager().set_config(Some(config.clone()));
        Ok(MlxEngineDistributedConfigResponse {
            config: config_to_dto(config),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto() -> MlxDistributedConfigDto {
        MlxDistributedConfigDto {
            model_id: "org/model".to_string(),
            backend: "jaccl".to_string(),
            port: 8190,
            coordinator_port: 32323,
            context: None,
            slots: None,
            restart_on_failure: true,
            hang_ratio_only: false,
            watchdog_warn_ratio: None,
            watchdog_critical_ratio: None,
            nodes: vec![
                MlxDistributedNodeConfigDto {
                    name: "MacBook Pro".to_string(),
                    ssh: None,
                    tb_ip: "192.168.0.1".to_string(),
                    tb_netmask: "255.255.255.252".to_string(),
                    tb_interface: "en3".to_string(),
                    tb_service: "EXO Thunderbolt 3".to_string(),
                    rdma_device: "rdma_en3".to_string(),
                    python: "/v/bin/python".to_string(),
                    pipeline_python: None,
                    model_dir: "/m/a".to_string(),
                    free_memory_automatically: Some(true),
                },
                MlxDistributedNodeConfigDto {
                    name: "workhorse".to_string(),
                    ssh: Some("workhorse".to_string()),
                    tb_ip: "192.168.0.2".to_string(),
                    tb_netmask: "255.255.255.252".to_string(),
                    tb_interface: "en3".to_string(),
                    tb_service: "EXO Thunderbolt 2".to_string(),
                    rdma_device: "rdma_en3".to_string(),
                    python: "/v/bin/python".to_string(),
                    pipeline_python: Some("/f/bin/python".to_string()),
                    model_dir: "/m/b".to_string(),
                    free_memory_automatically: Some(false),
                },
            ],
        }
    }

    #[test]
    fn the_config_round_trips_through_the_wire_shape() {
        let config = config_from_dto(dto()).unwrap();
        config.validate().unwrap();
        assert_eq!(config_to_dto(config), dto());
        let wire = serde_json::to_value(dto()).unwrap();
        assert_eq!(wire["coordinatorPort"], 32323);
        assert_eq!(wire["nodes"][1]["tbService"], "EXO Thunderbolt 2");
        assert_eq!(wire["nodes"][1]["pipelinePython"], "/f/bin/python");
        assert_eq!(wire["nodes"][1]["freeMemoryAutomatically"], false);
        // A node saved before the switch existed reads as ON.
        let mut legacy = dto();
        legacy.nodes[0].free_memory_automatically = None;
        assert!(config_from_dto(legacy).unwrap().nodes[0].free_memory_automatically);
        assert!(
            wire["nodes"][0].get("ssh").is_none(),
            "absent ssh = this Mac"
        );
    }

    #[test]
    fn an_unknown_backend_is_refused_by_name() {
        let mut bad = dto();
        bad.backend = "mpi".to_string();
        let err = config_from_dto(bad).unwrap_err().to_string();
        assert!(err.contains("mpi"), "{err}");
    }

    #[tokio::test]
    async fn a_stopped_engine_reports_single_mode_and_the_persisted_config() {
        let status = status_to_dto(
            distributed::DistributedManager::default().status(),
            Some(config_from_dto(dto()).unwrap()),
        );
        assert_eq!(status.mode, "single");
        assert_eq!(status.state, "stopped");
        assert!(status.admission_open);
        assert_eq!(status.config.unwrap().model_id, "org/model");
        refuse_single_mount_while_distributed().await.unwrap();
    }
}
